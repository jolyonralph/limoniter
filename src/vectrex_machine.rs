//! The Vectrex built into Limoniter: its 6809 at 1.5 MHz, its 1K of RAM, the 6522 VIA, the AY-3-8912 for its sound
//! and its buttons, and the vector screen, a cartridge in it. Its BIOS (the "executive" ROM, with Mine Storm) belongs
//! to its makers, so it isn't here: the person chooses their own file (see `micro_roms`). A cartridge that drives the
//! hardware itself runs without it, started where its header ends.
//!
//! The memory map and the VIA's wiring are the Vectrex Programmer's Manual's (transcribed at
//! roadsidethoughts.com/vectrex): the cartridge from $0000 to $7FFF; the RAM at $C800, 1K seen twice to $CFFF;
//! the VIA's 16 registers at $D000, seen again through to $DFFF (from $D800 the RAM is selected as well, as vecx and
//! MAME have it); the BIOS from $E000. On port B: PB0 enables the multiplexer and PB1-PB2 choose where the DAC goes
//! (Y, the zero reference, the brightness, or the sound) and which joystick axis the comparator looks at; PB3 and PB4
//! are the AY's BC1 and BDIR; PB5 is the comparator's answer; PB7 is RAMP. Port A is the DAC and the AY's data bus.
//! CA2 is ZERO and CB2 is BLANK. The AY's port A (its register 14) reads the four buttons of each controller, low
//! when pressed.

use crate::m6809_cpu::{Bus, Cpu, Lines};
use crate::running::{Machine, Runnable};
use crate::vectrex_screen::{Beam, Screen, Signals};
use crate::via6522::Via;
use aym::{AyMode, AySample, AymBackend, AymPrecise, SoundChip};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

/// The 6809's clock: the 6 MHz crystal over 4.
pub const CLOCK: f64 = 1_500_000.0;
/// The BIOS's own frame, timer 2 set to 30,000 cycles by Wait_Recal: 50 a second, as the screen is refreshed here.
pub const FRAME_CYCLES: u32 = 30_000;
/// The sound's rate when there's no speaker to follow (in the tests).
const TEST_RATE: u32 = 44_100;

/// The controllers: each one's joystick (across and up, -127 to 127) and its four buttons.
#[derive(Clone, Copy, Debug, Default)]
pub struct Controls {
    pub sticks: [[i8; 2]; 2],
    pub buttons: [[bool; 4]; 2],
}

impl Controls {
    /// The AY's register 14 as the buttons pull it: bits 0-3 the first controller's buttons 1 to 4, bits 4-7 the
    /// second's, each low while it's pressed.
    fn port(&self) -> u8 {
        let mut value = 0xFF;
        for (player, buttons) in self.buttons.iter().enumerate() {
            for (n, down) in buttons.iter().enumerate() {
                if *down {
                    value &= !(1 << (player * 4 + n));
                }
            }
        }
        value
    }

    /// The joystick axis the multiplexer chooses: 0 and 1 the first controller's across and up, 2 and 3 the second's.
    fn pot(&self, select: u8) -> i8 {
        self.sticks[usize::from(select >> 1)][usize::from(select & 1)]
    }
}

/// The AY-3-8912: its registers as the VIA sets them, and the chip making the sound (emulated by the aym crate, as
/// the built-in Spectrum 128's AY is).
struct Ay {
    regs: [u8; 16],
    selected: u8,
    chip: AymPrecise,
}

impl Ay {
    fn new(rate: u32) -> Ay {
        // The Vectrex's AY runs from the 6809's 1.5 MHz.
        let mut chip = AymPrecise::new(SoundChip::AY, AyMode::Mono, CLOCK as usize, rate as usize);
        chip.enable_dc_filter();
        Ay { regs: [0; 16], selected: 0, chip }
    }

    fn write(&mut self, value: u8) {
        let register = self.selected & 0x0F;
        self.regs[usize::from(register)] = value;
        // Registers 14 and 15 are its I/O ports, not sound.
        if register < 14 {
            self.chip.write_register(register, value);
        }
    }
}

/// Everything on the 6809's bus.
struct System {
    cart: Vec<u8>,
    bios: Vec<u8>,
    has_bios: bool,
    ram: [u8; 1024],
    via: Via,
    ay: Ay,
    beam: Beam,
    controls: Controls,
    /// The sample-and-holds the multiplexer sets from the DAC: Y, the zero reference, the brightness (0 to 127),
    /// and the sound the DAC plays.
    y_hold: i8,
    zero_hold: i8,
    z_hold: u8,
    dac_sound: i8,
    /// Writes to watched places, noted for the machine to judge when the instruction is over.
    touches: crate::watchpoints::Touches,
    /// RAM whose writes are stopped (the trainer's Block writes), and what the program wrote to the debug console address.
    blocked: BTreeSet<u16>,
    console: crate::debug_console::Console,
}

impl System {
    fn new(cart: Vec<u8>, bios: Option<Vec<u8>>, rate: u32) -> System {
        let has_bios = bios.is_some();
        let bios = bios.unwrap_or_else(|| stand_in_bios(&cart));
        // The RAM is left as it powers up: not cleared, but this is as good as anything.
        let ram = std::array::from_fn(|i| i as u8);
        System { cart, bios, has_bios, ram, via: Via::default(), ay: Ay::new(rate), beam: Beam::default(), controls: Controls::default(), y_hold: 0, zero_hold: 0, z_hold: 0, dac_sound: 0, touches: Default::default(), blocked: BTreeSet::new(), console: Default::default() }
    }

    /// What's read at an address, with nothing changed (for the debugger and its memory view).
    fn peek(&self, address: u16) -> u8 {
        match address {
            0x0000..=0x7FFF => self.cart_byte(address),
            0xC000..=0xDFFF if address & 0x0800 != 0 => self.ram[usize::from(address & 0x3FF)],
            0xC000..=0xDFFF if address & 0x1000 != 0 => {
                let mut via = self.via.clone();
                self.inputs(&mut via);
                via.read(address as u8 & 0x0F)
            }
            0xE000..=0xFFFF => self.bios[usize::from(address & 0x1FFF)],
            _ => 0xFF,
        }
    }

    /// A cartridge smaller than 32K is seen again through its space, its upper address lines not connected.
    fn cart_byte(&self, address: u16) -> u8 {
        if self.cart.is_empty() {
            return 0xFF;
        }
        let size = self.cart.len().next_power_of_two();
        self.cart.get(usize::from(address) % size).copied().unwrap_or(0xFF)
    }

    /// What's outside the VIA's input pins: the comparator's answer on PB5 (whether the joystick axis chosen is
    /// further than the DAC says, as vecx has it), and on port A, the AY's register when it's being read.
    fn inputs(&self, via: &mut Via) {
        let dac = via.port_a() as i8;
        let select = (via.port_b() >> 1) & 3;
        let compare = self.controls.pot(select) > dac;
        via.port_b_in = 0xDF | if compare { 0x20 } else { 0 };
        via.port_a_in = if via.orb & 0x18 == 0x08 {
            if self.ay.selected == 14 { self.controls.port() } else { self.ay.regs[usize::from(self.ay.selected & 0x0F)] }
        } else {
            0xFF
        };
    }

    /// After port A or B is written: the multiplexer sets the sample-and-hold it chooses, and the AY latches a
    /// register's number or takes a value, as BDIR and BC1 say.
    fn after_port_write(&mut self) {
        let mut via = std::mem::take(&mut self.via);
        self.inputs(&mut via);
        self.via = via;
        let (pb, dac) = (self.via.port_b(), self.via.port_a());
        if pb & 0x01 == 0 {
            let value = dac as i8;
            match (pb >> 1) & 3 {
                0 => self.y_hold = value,
                1 => self.zero_hold = value,
                2 => self.z_hold = value.max(0) as u8,
                _ => self.dac_sound = value,
            }
        }
        match pb & 0x18 {
            // BDIR and BC1 both high: the address latched, if it's one of the chip's 16.
            0x18 if dac & 0xF0 == 0 => self.ay.selected = dac,
            0x10 => self.ay.write(dac),
            _ => {}
        }
    }

    /// The beam's signals in this cycle.
    fn signals(&self) -> Signals {
        let pb = self.via.port_b();
        let x_in = i32::from(self.via.port_a() as i8);
        let zero = i32::from(self.zero_hold);
        Signals { zero: !self.via.ca2(), ramp: pb & 0x80 == 0, lit: self.via.cb2(), dx: x_in - zero, dy: i32::from(self.y_hold) - zero, z: self.z_hold }
    }

    /// Runs the VIA and the beam for a number of cycles.
    fn run(&mut self, cycles: u32) {
        for _ in 0..cycles {
            self.via.tick();
            let signals = self.signals();
            self.beam.tick(signals);
        }
    }
}

impl Bus for System {
    fn read(&mut self, address: u16) -> u8 {
        match address {
            0xC000..=0xDFFF if address & 0x0800 == 0 && address & 0x1000 != 0 => {
                let mut via = std::mem::take(&mut self.via);
                self.inputs(&mut via);
                let value = via.read(address as u8 & 0x0F);
                self.via = via;
                value
            }
            _ => self.peek(address),
        }
    }

    fn write(&mut self, address: u16, value: u8) {
        if address == crate::debug_console::VECTREX_ADDRESS {
            // Nothing is on the bus here: the debug console port, which a real Vectrex ignores.
            self.console.put(value);
        }
        if !(0xC000..=0xDFFF).contains(&address) {
            return;
        }
        if !self.blocked.is_empty() && self.blocked.contains(&watched(address)) {
            return;
        }
        if address & 0x0800 != 0 {
            self.ram[usize::from(address & 0x3FF)] = value;
        }
        if address & 0x1000 != 0 {
            let register = address as u8 & 0x0F;
            self.via.write(register, value);
            if matches!(register, 0x0 | 0x1 | 0x2 | 0x3 | 0xB | 0xF) {
                self.after_port_write();
            }
        }
        self.touches.note(crate::watchpoints::Access::Write, u32::from(watched(address)), value);
    }
}

/// The address a write watch is kept under: the RAM's first place for either of its copies.
fn watched(address: u16) -> u16 {
    if (0xC800..=0xCFFF).contains(&address) { 0xC800 | (address & 0x3FF) } else { address }
}

/// What's at $E000 without the BIOS: nothing but the 6809's vectors, the reset one to the cartridge's code after its
/// header, and the interrupts to the places in RAM the BIOS sends them to (so a cartridge's own handlers work as
/// they would with it).
fn stand_in_bios(cart: &[u8]) -> Vec<u8> {
    let mut rom = vec![0xFF; 0x2000];
    let start = crate::vectrex::header(cart).map_or(0, |(_, _, _, code)| code as u16);
    let vectors: [(u16, u16); 7] = [(0xFFF2, 0xCBF2), (0xFFF4, 0xCBF2), (0xFFF6, 0xCBF5), (0xFFF8, 0xCBF8), (0xFFFA, 0xCBFB), (0xFFFC, 0xCBFB), (0xFFFE, start)];
    for (at, to) in vectors {
        let at = usize::from(at & 0x1FFF);
        rom[at..at + 2].copy_from_slice(&to.to_be_bytes());
    }
    rom
}

pub struct Vectrex {
    pub cpu: Cpu,
    sys: System,
    screen: Screen,
    picture: Vec<u8>,
    picture_made: u64,
    /// Cycles run in this refresh.
    in_frame: u32,
    last: Instant,
    owed: f64,
    pub paused: bool,
    pub stopped: Option<u16>,
    pub stop_reason: Option<String>,
    breakpoints: BTreeSet<u16>,
    conditions: BTreeMap<u16, String>,
    labels: Option<std::sync::Arc<BTreeMap<String, i64>>>,
    once: Option<u16>,
    pub memory_before: Option<std::sync::Arc<Vec<u8>>>,
    speaker: Option<crate::speaker::Speaker>,
    pub muted: bool,
    rate: u32,
    /// How far into the next sound sample, and the high-pass filter's last input and output for the DAC's sound.
    phase: f64,
    dc: (f32, f32),
    pub frames: u64,
    /// The judge of watchpoints, hit rules with how often each was reached, and the instructions run since it
    /// last stopped.
    watcher: crate::watchpoints::Watcher,
    hit_rules: BTreeMap<u16, crate::watchpoints::HitRule>,
    pub hit_counts: BTreeMap<u16, u64>,
    pub instructions: u64,
    /// The last refresh's lines, for the tests.
    #[cfg(test)]
    pub last_segments: Vec<crate::vectrex_screen::Segment>,
    #[cfg(test)]
    pub heard: Vec<f32>,
}

impl Vectrex {
    /// Whether it can run this; if not, why not (to follow "The built-in Vectrex").
    pub fn takes(run: &Runnable) -> Result<(), String> {
        if run.machine != Machine::Vectrex {
            return Err(crate::i18n::tf!("runs only Vectrex cartridges, not {} programs.", run.machine.name()));
        }
        if run.bytes.len() > 0x8000 {
            return Err(crate::i18n::tr("doesn't switch banks yet, so it runs cartridges of up to 32K: run this one in MAME or ParaJVE.").into());
        }
        Ok(())
    }

    /// The cartridge in, with the BIOS if there is one, switched on.
    pub fn start(run: &Runnable, bios: Option<Vec<u8>>, speaker: Option<crate::speaker::Speaker>) -> Result<Vectrex, String> {
        Self::takes(run).map_err(|why| crate::i18n::tf!("The built-in Vectrex {why}"))?;
        Ok(Self::with_cartridge(run.bytes.to_vec(), bios, speaker))
    }

    pub fn with_cartridge(cart: Vec<u8>, bios: Option<Vec<u8>>, speaker: Option<crate::speaker::Speaker>) -> Vectrex {
        let rate = speaker.as_ref().map_or(TEST_RATE, |s| s.rate);
        let has_bios = bios.is_some();
        let mut sys = System::new(cart, bios, rate);
        let mut cpu = Cpu::default();
        cpu.reset(&mut sys);
        if !has_bios {
            // Where the BIOS leaves the stack for a cartridge.
            cpu.s = 0xCBEA;
        }
        Vectrex {
            cpu,
            sys,
            screen: Screen::default(),
            picture: Vec::new(),
            picture_made: u64::MAX,
            in_frame: 0,
            last: Instant::now(),
            owed: 0.0,
            paused: false,
            stopped: None,
            stop_reason: None,
            breakpoints: BTreeSet::new(),
            conditions: BTreeMap::new(),
            labels: None,
            once: None,
            memory_before: None,
            speaker,
            muted: false,
            rate,
            phase: 0.0,
            dc: (0.0, 0.0),
            frames: 0,
            watcher: Default::default(),
            hit_rules: BTreeMap::new(),
            hit_counts: BTreeMap::new(),
            instructions: 0,
            #[cfg(test)]
            last_segments: Vec::new(),
            #[cfg(test)]
            heard: Vec::new(),
        }
    }

    pub fn has_bios(&self) -> bool {
        self.sys.has_bios
    }

    /// Runs the refreshes due since last time, at the Vectrex's own speed, catching up at most a few.
    pub fn catch_up(&mut self) -> Result<(), String> {
        let now = Instant::now();
        if self.paused {
            self.last = now;
            if let Some(speaker) = &self.speaker {
                speaker.clear();
            }
            return Ok(());
        }
        self.owed = (self.owed + now.duration_since(self.last).as_secs_f64() * CLOCK / f64::from(FRAME_CYCLES)).min(5.0);
        self.last = now;
        while self.owed >= 1.0 && !self.paused {
            self.owed -= 1.0;
            self.frame()?;
        }
        Ok(())
    }

    /// Runs a fiftieth of a second, or until a breakpoint or a problem stops it.
    pub fn frame(&mut self) -> Result<(), String> {
        let until = self.frames + 1;
        while !self.paused && self.frames < until {
            self.instruction(true)?;
        }
        Ok(())
    }

    /// Runs one instruction, stopping first at a breakpoint (if `breaks`) unless it's just carried on from it; the
    /// rest of the machine runs for as long as it took, and a refresh ends when its time is up.
    fn instruction(&mut self, breaks: bool) -> Result<(), String> {
        let pc = self.cpu.pc;
        if breaks && self.once == Some(pc) {
            self.once = None;
            self.stop(None);
            return Ok(());
        }
        if breaks && self.stopped.take() != Some(pc) && self.breakpoints.contains(&pc) {
            let stop = match self.conditions.get(&pc) {
                Some(condition) => match self.check(condition) {
                    Ok(0) => None,
                    Ok(_) => Some(Some(crate::i18n::tf!("Stopped because {condition}"))),
                    Err(problem) => Some(Some(crate::i18n::tf!("The condition {condition} couldn't be worked out: {problem}"))),
                },
                None => Some(None),
            };
            let stop = match (stop, self.hit_rules.get(&pc).copied()) {
                (Some(reason), Some(rule)) => {
                    let count = self.hit_counts.entry(pc).or_default();
                    rule.reached(count).then(|| {
                        let count = *count;
                        reason.or_else(|| Some(crate::i18n::tf!("Stopped on its hit number {count}")))
                    })
                }
                (stop, _) => stop,
            };
            if let Some(reason) = stop {
                self.stop(reason);
                return Ok(());
            }
        }
        self.stopped = None;
        if !self.sys.has_bios && pc >= 0xE000 {
            self.stop(None);
            let name = crate::vectrex::name_at(pc).map(|n| format!(" ({n})")).unwrap_or_default();
            return Err(crate::i18n::tf!("The cartridge called the BIOS at ${pc:04X}{name}, which isn't here: choose the Vectrex's BIOS file to run it."));
        }
        let lines = Lines { irq: self.sys.via.irq(), ..Lines::default() };
        let taken = self.cpu.step(&mut self.sys, lines);
        if let Some(at) = self.cpu.illegal {
            self.stop(None);
            return Err(crate::i18n::tf!("The 6809 met ${:02X} at ${at:04X}, which isn't one of its instructions.", self.sys.peek(at)));
        }
        self.sys.run(taken);
        self.sound(taken);
        self.in_frame += taken;
        if self.in_frame >= FRAME_CYCLES {
            self.in_frame -= FRAME_CYCLES;
            self.refresh();
        }
        self.instructions += 1;
        let touched = self.sys.touches.take();
        if !touched.is_empty() {
            let sys = &self.sys;
            let peek = |a: u32| sys.peek(a as u16);
            if let Some(hit) = touched.iter().find_map(|(what, address, value)| self.watcher.judge(*what, *address, *value, &peek)) {
                self.stop(Some(hit.reason(Some(u32::from(pc)), 4)));
            }
        }
        Ok(())
    }

    fn refresh(&mut self) {
        let segments = self.sys.beam.take();
        self.screen.refresh(&segments);
        #[cfg(test)]
        {
            self.last_segments = segments;
        }
        self.frames += 1;
    }

    /// The sound for the cycles just run: the AY, and the DAC when the multiplexer sends it to the speaker.
    fn sound(&mut self, cycles: u32) {
        let speaking = self.speaker.is_some() && !self.muted;
        if !speaking && !cfg!(test) {
            return;
        }
        self.phase += f64::from(cycles) * f64::from(self.rate) / CLOCK;
        while self.phase >= 1.0 {
            self.phase -= 1.0;
            let ay = self.sys.ay.chip.next_sample().left.to_f32();
            let dac = f32::from(self.sys.dac_sound) / 128.0;
            let (last_in, last_out) = self.dc;
            let dac_out = dac - last_in + 0.995 * last_out;
            self.dc = (dac, dac_out);
            let out = (ay * 0.6 + dac_out * 0.4).clamp(-1.0, 1.0);
            #[cfg(test)]
            self.heard.push(out);
            if let Some(speaker) = self.speaker.as_ref().filter(|_| !self.muted) {
                speaker.push(out, out);
            }
        }
    }

    fn stop(&mut self, reason: Option<String>) {
        self.paused = true;
        self.stopped = Some(self.cpu.pc);
        self.stop_reason = reason;
    }

    /// A breakpoint's condition worked out: the registers by name, labels, and memory.
    fn check(&self, condition: &str) -> Result<i64, String> {
        let names = |name: &str| self.register(name);
        let peek = |a: u32| self.sys.peek(a as u16);
        let wide = crate::debug_panel::Wide { names: &names, peek: &peek };
        crate::debug_panel::eval_wide(condition, &wide, self.labels.as_deref())
    }

    fn register(&self, name: &str) -> Option<i64> {
        let cpu = &self.cpu;
        Some(i64::from(match name.to_ascii_uppercase().as_str() {
            "A" => u16::from(cpu.a),
            "B" => u16::from(cpu.b),
            "D" => cpu.d(),
            "X" => cpu.x,
            "Y" => cpu.y,
            "U" => cpu.u,
            "S" | "SP" => cpu.s,
            "PC" => cpu.pc,
            "DP" => u16::from(cpu.dp),
            "CC" => u16::from(cpu.cc),
            _ => return None,
        }))
    }

    pub fn set_breakpoints(&mut self, addresses: impl IntoIterator<Item = u16>) {
        self.breakpoints = addresses.into_iter().collect();
    }

    pub fn set_conditions(&mut self, conditions: BTreeMap<u16, String>, labels: Option<std::sync::Arc<BTreeMap<String, i64>>>) {
        self.conditions = conditions;
        self.labels = labels;
    }

    /// The watchpoints. The 6809's reads can't be told from its dummy reads and instruction fetches without a
    /// slower core, so only writes are watched. The RAM's copies are one place.
    pub fn set_watchpoints(&mut self, list: Vec<crate::watchpoints::Resolved>) {
        self.watcher.big_endian = true;
        let list: Vec<_> = list.into_iter().filter(|w| w.access == crate::watchpoints::Access::Write).map(|w| crate::watchpoints::Resolved { start: u32::from(watched(w.start as u16)), end: u32::from(watched(w.end as u16)), ..w }).collect();
        self.sys.touches.set(&list);
        let sys = &self.sys;
        self.watcher.set(list, &|a| sys.peek(a as u16));
    }

    /// How often the breakpoint at an address has been reached, if it has a hit rule.
    pub fn hit_count(&self, address: u16) -> Option<u64> {
        self.hit_counts.get(&address).copied()
    }

    pub fn set_hit_rules(&mut self, rules: BTreeMap<u16, crate::watchpoints::HitRule>) {
        self.hit_rules = rules;
    }

    pub fn carry_on(&mut self) {
        self.instructions = 0;
        self.stop_reason = None;
        self.paused = false;
    }

    /// Runs one instruction, and stops again.
    pub fn step(&mut self) -> Result<(), String> {
        self.instructions = 0;
        self.paused = false;
        self.stopped = None;
        self.stop_reason = None;
        let result = self.instruction(false);
        self.stop(None);
        result
    }

    /// Runs one instruction, or a whole JSR, BSR or LBSR and what it calls, and stops again.
    pub fn step_over(&mut self) -> Result<(), String> {
        let pc = self.cpu.pc;
        if !matches!(self.sys.peek(pc), 0x17 | 0x8D | 0x9D | 0xAD | 0xBD) {
            return self.step();
        }
        let length = crate::disasm::m6809(&|i| self.sys.peek(pc.wrapping_add(i as u16)), pc, false).length;
        self.once = Some(pc.wrapping_add(length as u16));
        self.stop_reason = None;
        self.paused = false;
        self.instruction(false)?;
        for _ in 0..50 * 5 {
            self.frame()?;
            if self.paused {
                return Ok(());
            }
        }
        self.once = None;
        self.stop(None);
        Ok(())
    }

    pub fn stop_words(&self) -> crate::remote_debug::Stop {
        let cpu = &self.cpu;
        let words = vec![
            format!("A {:02X}", cpu.a),
            format!("B {:02X}", cpu.b),
            format!("X {:04X}", cpu.x),
            format!("Y {:04X}", cpu.y),
            format!("U {:04X}", cpu.u),
            format!("S {:04X}", cpu.s),
            format!("PC {:04X}", cpu.pc),
            format!("DP {:02X}", cpu.dp),
            format!("CC {}", cpu.flags()),
            crate::i18n::tf!("Beam at {}, {}", self.sys.beam.x, self.sys.beam.y),
        ];
        crate::remote_debug::Stop { pc: cpu.pc, z80: None, words, ..Default::default() }
    }

    /// Writes to RAM ($C800 to $CFFF), for the trainer; anything else is let go.
    pub fn poke(&mut self, address: u16, values: &[u8]) {
        for (i, value) in values.iter().enumerate() {
            let a = address.wrapping_add(i as u16);
            if (0xC800..=0xCFFF).contains(&a) {
                self.sys.ram[usize::from(a & 0x3FF)] = *value;
            }
        }
    }

    /// The RAM addresses whose writes are stopped, for the trainer.
    pub fn set_blocked(&mut self, addresses: impl IntoIterator<Item = u16>) {
        self.sys.blocked = addresses.into_iter().map(watched).collect();
    }

    /// The lines the program has written to the debug console address since last asked.
    pub fn take_console(&mut self) -> Vec<String> {
        self.sys.console.take()
    }

    /// All 64K as the 6809 sees it, for the debugger.
    pub fn memory(&self) -> Vec<u8> {
        (0..=0xFFFFu16).map(|a| self.sys.peek(a)).collect()
    }

    pub fn has_speaker(&self) -> bool {
        self.speaker.is_some()
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        if let Some(speaker) = &self.speaker {
            speaker.clear();
        }
    }

    pub fn controls(&mut self) -> &mut Controls {
        &mut self.sys.controls
    }

    /// Lets go of the joysticks and buttons, as when the screen loses the keyboard.
    pub fn release_all(&mut self) {
        self.sys.controls = Controls::default();
    }

    /// The screen as it was at the last refresh, as RGBA, `vectrex_screen::WIDTH` by `HEIGHT`.
    pub fn picture(&mut self) -> &[u8] {
        if self.picture_made != self.screen.refreshes {
            self.screen.picture(&mut self.picture);
            self.picture_made = self.screen.refreshes;
        }
        &self.picture
    }

    #[cfg(test)]
    pub fn ay_registers(&self) -> [u8; 16] {
        self.sys.ay.regs
    }
}

#[cfg(test)]
#[path = "vectrex_machine/tests.rs"]
mod tests;
