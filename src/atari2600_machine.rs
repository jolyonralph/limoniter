//! The Atari 2600 built into Limoniter: its 6507 (the 6502 core, a bus access a cycle), the TIA (see
//! `atari2600_tia`), the RIOT's 128 bytes of RAM, its timer and its ports, and the cartridge with its bank
//! switching. The 2600 has no ROM of its own, so the cartridge is all it needs. The joysticks, fire buttons and
//! console switches come from the keyboard and the panel's buttons; its sound goes to the speakers.
//!
//! Bank switching: 2K, 4K, F8, F6 and F4 (with or without the Superchip's RAM), FA (CBS RAM Plus), E0 (Parker
//! Brothers), E7 (M Network) and 3F (Tigervision). FE (Activision's, switched by the stack) and DPC (Pitfall
//! II's chip) aren't built in yet; Stella and MAME run those.

use crate::atari2600::Scheme;
use crate::atari2600_gfx::Tv;
use crate::atari2600_tia::{Frame, Tia, LINE, WIDTH};
use crate::m6502_cpu::{Bus, Cpu};
use crate::running::{Machine, Runnable};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

/// The colour clocks a second: NTSC's and PAL's.
const NTSC_CLOCK: f64 = 3_579_545.0;
const PAL_CLOCK: f64 = 3_546_894.0;
/// A frame of more lines than this is PAL's (312 lines; NTSC's has 262).
const PAL_LINES: usize = 287;

/// The lines of a frame shown, counted from the line the vertical sync ends in: room above and below the 192
/// lines NTSC programs draw from line 37, or the 228 PAL ones draw from line 45.
pub fn window(tv: Tv) -> (usize, usize) {
    match tv {
        Tv::Ntsc => (19, 228),
        _ => (22, 274),
    }
}

/// The RIOT (6532): RAM, the timer and the two ports.
#[derive(Clone, Debug)]
struct Riot {
    ram: [u8; 128],
    timer: u8,
    /// The timer's interval as a shift (1, 8, 64 or 1024 cycles) and the prescaler counting down to its next step.
    shift: u8,
    prescaler: u16,
    /// Set when the timer passes 0; it then counts every cycle until INTIM is read or the timer written.
    flag: bool,
    wrapped: bool,
    pa7_flag: bool,
    pa7_rising: bool,
    swcha: u8,
    swacnt: u8,
    swchb: u8,
    swbcnt: u8,
}

impl Default for Riot {
    fn default() -> Riot {
        Riot { ram: [0; 128], timer: 0xFF, shift: 10, prescaler: 0, flag: false, wrapped: false, pa7_flag: false, pa7_rising: false, swcha: 0, swacnt: 0, swchb: 0, swbcnt: 0 }
    }
}

impl Riot {
    /// One processor cycle.
    fn tick(&mut self) {
        self.wrapped = false;
        let due = self.prescaler == 0;
        self.prescaler = if due { (1u16 << self.shift) - 1 } else { self.prescaler - 1 };
        if self.flag {
            self.timer = self.timer.wrapping_sub(1);
        } else if due {
            if self.timer == 0 {
                self.timer = 0xFF;
                self.flag = true;
                self.wrapped = true;
            } else {
                self.timer -= 1;
            }
        }
    }

    fn read(&mut self, address: u16, swcha_in: u8, swchb_in: u8) -> u8 {
        if address & 4 == 0 {
            match address & 3 {
                0 => (self.swcha & self.swacnt) | (swcha_in & !self.swacnt),
                1 => self.swacnt,
                2 => (self.swchb & self.swbcnt) | (swchb_in & !self.swbcnt),
                _ => self.swbcnt,
            }
        } else if address & 1 == 0 {
            // INTIM: reading it clears the timer's flag, so it goes back to its interval.
            if !self.wrapped {
                self.flag = false;
            }
            self.timer
        } else {
            let flags = u8::from(self.flag) << 7 | u8::from(self.pa7_flag) << 6;
            self.pa7_flag = false;
            flags
        }
    }

    fn peek(&self, address: u16, swcha_in: u8, swchb_in: u8) -> u8 {
        let mut copy = self.clone();
        copy.read(address, swcha_in, swchb_in)
    }

    fn write(&mut self, address: u16, value: u8) {
        if address & 4 == 0 {
            match address & 3 {
                0 => self.swcha = value,
                1 => self.swacnt = value,
                2 => self.swchb = value,
                _ => self.swbcnt = value,
            }
        } else if address & 0x10 != 0 {
            self.shift = [0, 3, 6, 10][usize::from(address & 3)];
            self.timer = value;
            // The first step comes the cycle after the write, then one an interval.
            self.prescaler = 0;
            self.flag = false;
            self.wrapped = false;
        } else {
            self.pa7_rising = address & 1 != 0;
        }
    }
}

/// The cartridge, its banks and any RAM it has.
#[derive(Clone, Debug)]
struct Cartridge {
    rom: Vec<u8>,
    scheme: Scheme,
    /// The 4K bank in view (F8, F6, F4, FA), or the 2K one in the lower half (3F, E7).
    bank: usize,
    /// E0's three switchable 1K slices.
    slices: [usize; 3],
    /// E7: whether its 1K of RAM is in the lower half, and which 256 bytes of its other RAM are in view.
    e7_ram: bool,
    e7_ram_bank: usize,
    ram: Vec<u8>,
}

impl Cartridge {
    fn new(rom: Vec<u8>, scheme: Scheme) -> Cartridge {
        let banks = rom.len() / 4096;
        let ram = match scheme {
            Scheme::F8Sc | Scheme::F6Sc | Scheme::F4Sc => 128,
            Scheme::Fa => 256,
            Scheme::E7 => 2048,
            _ => 0,
        };
        // The last bank first, where a cartridge's reset vector is looked for.
        let bank = match scheme {
            Scheme::F8 | Scheme::F8Sc | Scheme::F6 | Scheme::F6Sc | Scheme::F4 | Scheme::F4Sc | Scheme::Fa => banks.saturating_sub(1),
            _ => 0,
        };
        Cartridge { rom, scheme, bank, slices: [4, 5, 6], e7_ram: false, e7_ram_bank: 0, ram: vec![0; ram] }
    }

    /// Whether this kind of cartridge is built in.
    fn supported(scheme: Scheme) -> bool {
        !matches!(scheme, Scheme::Fe | Scheme::Dpc)
    }

    /// An access anywhere in the cartridge's 4K, read or write, which switches banks at the hotspots.
    fn touch(&mut self, address: u16) {
        let a = address & 0x0FFF;
        match self.scheme {
            Scheme::F8 | Scheme::F8Sc if (0xFF8..=0xFF9).contains(&a) => self.bank = usize::from(a - 0xFF8),
            Scheme::F6 | Scheme::F6Sc if (0xFF6..=0xFF9).contains(&a) => self.bank = usize::from(a - 0xFF6),
            Scheme::F4 | Scheme::F4Sc if (0xFF4..=0xFFB).contains(&a) => self.bank = usize::from(a - 0xFF4),
            Scheme::Fa if (0xFF8..=0xFFA).contains(&a) => self.bank = usize::from(a - 0xFF8),
            Scheme::E0 if (0xFE0..=0xFF7).contains(&a) => self.slices[usize::from((a - 0xFE0) / 8)] = usize::from(a & 7),
            Scheme::E7 if (0xFE0..=0xFE6).contains(&a) => {
                self.bank = usize::from(a - 0xFE0);
                self.e7_ram = false;
            }
            Scheme::E7 if a == 0xFE7 => self.e7_ram = true,
            Scheme::E7 if (0xFE8..=0xFEB).contains(&a) => self.e7_ram_bank = usize::from(a - 0xFE8),
            _ => {}
        }
    }

    /// Where a read of the cartridge comes from: the ROM's offset, or the RAM's (as `Err`).
    fn place(&self, address: u16) -> Result<usize, usize> {
        let a = usize::from(address & 0x0FFF);
        let size = self.rom.len();
        match self.scheme {
            Scheme::K2 => Ok(a & 0x7FF),
            Scheme::K4 => Ok(a),
            Scheme::F8Sc | Scheme::F6Sc | Scheme::F4Sc if a < 0x100 => Err(a & 0x7F),
            Scheme::Fa if a < 0x200 => Err(a & 0xFF),
            Scheme::F8 | Scheme::F8Sc | Scheme::F6 | Scheme::F6Sc | Scheme::F4 | Scheme::F4Sc | Scheme::Fa => Ok(self.bank * 4096 + a),
            Scheme::E0 => {
                let slice = a >> 10;
                let bank = if slice == 3 { 7 } else { self.slices[slice] };
                Ok(bank * 1024 + (a & 0x3FF))
            }
            Scheme::ThreeF => Ok(if a < 0x800 { (self.bank * 2048) % size + a } else { size - 2048 + (a & 0x7FF) }),
            Scheme::E7 => match a {
                0..=0x7FF if self.e7_ram => Err(a & 0x3FF),
                0..=0x7FF => Ok(self.bank * 2048 + a),
                0x800..=0x9FF => Err(1024 + self.e7_ram_bank * 256 + (a & 0xFF)),
                _ => Ok(0x3800 + (a & 0x7FF)),
            },
            Scheme::Fe | Scheme::Dpc => Ok(a % size),
        }
    }

    /// Whether a RAM address is the half written to: RAM on a cartridge has a port for writing and one for
    /// reading, since the cartridge slot has no line saying which.
    fn write_port(&self, address: u16) -> bool {
        let a = address & 0x0FFF;
        match self.scheme {
            Scheme::F8Sc | Scheme::F6Sc | Scheme::F4Sc => a < 0x80,
            Scheme::Fa => a < 0x100,
            Scheme::E7 if a < 0x800 => a < 0x400,
            Scheme::E7 => a < 0x900,
            _ => false,
        }
    }

    fn peek(&self, address: u16) -> u8 {
        match self.place(address) {
            Ok(offset) => self.rom.get(offset).copied().unwrap_or(0xFF),
            Err(offset) => self.ram.get(offset).copied().unwrap_or(0xFF),
        }
    }

    fn read(&mut self, address: u16, bus: u8) -> u8 {
        self.touch(address);
        match self.place(address) {
            Err(offset) if self.write_port(address) => {
                // Reading the port that writes writes whatever's on the bus.
                if let Some(cell) = self.ram.get_mut(offset) {
                    *cell = bus;
                }
                bus
            }
            _ => self.peek(address),
        }
    }

    fn write(&mut self, address: u16, value: u8) {
        self.touch(address);
        if let Err(offset) = self.place(address)
            && self.write_port(address)
            && let Some(cell) = self.ram.get_mut(offset)
        {
            *cell = value;
        }
    }

    /// A write to the TIA's addresses, which Tigervision's cartridges watch for their bank.
    fn saw_tia_write(&mut self, address: u16, value: u8) {
        if self.scheme == Scheme::ThreeF && address < 0x40 {
            self.bank = usize::from(value);
        }
    }

    fn bank_name(&self) -> String {
        match self.scheme {
            Scheme::K2 | Scheme::K4 => String::new(),
            Scheme::E0 => format!("{} {} {} 7", self.slices[0], self.slices[1], self.slices[2]),
            _ => self.bank.to_string(),
        }
    }
}

/// The joysticks and the console's switches.
#[derive(Clone, Copy, Debug)]
pub struct Controls {
    /// Each joystick's up, down, left, right.
    pub sticks: [[bool; 4]; 2],
    pub reset: bool,
    pub select: bool,
    /// The TV switch: colour (true) or black and white.
    pub colour: bool,
    /// Each player's difficulty: A (true, the harder) or B.
    pub difficulty_a: [bool; 2],
}

impl Default for Controls {
    fn default() -> Controls {
        Controls { sticks: [[false; 4]; 2], reset: false, select: false, colour: true, difficulty_a: [false; 2] }
    }
}

impl Controls {
    /// SWCHA as the joysticks hold it: a bit low for each direction pushed, the left joystick in the high half.
    fn swcha(&self) -> u8 {
        let stick = |s: [bool; 4]| u8::from(!s[0]) | u8::from(!s[1]) << 1 | u8::from(!s[2]) << 2 | u8::from(!s[3]) << 3;
        stick(self.sticks[0]) << 4 | stick(self.sticks[1])
    }

    /// SWCHB as the switches hold it: Reset and Select low while pressed.
    fn swchb(&self) -> u8 {
        u8::from(!self.reset) | u8::from(!self.select) << 1 | u8::from(self.colour) << 3 | u8::from(self.difficulty_a[0]) << 6 | u8::from(self.difficulty_a[1]) << 7
    }
}

/// Everything on the 6507's bus, which moves the TIA three colour clocks and the RIOT one step each cycle.
struct System {
    tia: Tia,
    riot: Riot,
    cart: Cartridge,
    cycles: u64,
    /// What was last on the data bus, which the TIA's undriven bits read back.
    data_bus: u8,
    controls: Controls,
    /// Addresses whose writing stops it, and the write that did.
    watches: BTreeSet<u16>,
    wrote: Option<(u16, u8)>,
}

/// An address as the 6507 sees it: thirteen lines, so 8K mirrored through the 64K.
fn mirror(address: u16) -> u16 {
    address & 0x1FFF
}

/// RAM's addresses all one, for comparing watches: $80-$FF and its mirrors.
fn same_place(address: u16) -> u16 {
    let a = mirror(address);
    if a & 0x1080 == 0x80 && a & 0x200 == 0 { 0x80 | (a & 0x7F) } else { a }
}

impl System {
    fn cycle(&mut self) {
        self.cycles += 1;
        self.riot.tick();
        self.tia.clock();
        self.tia.clock();
        self.tia.clock();
    }

    /// The value at an address without anything read changing.
    fn peek(&self, address: u16) -> u8 {
        let a = mirror(address);
        if a & 0x1000 != 0 {
            self.cart.peek(a)
        } else if a & 0x80 == 0 {
            0
        } else if a & 0x200 == 0 {
            self.riot.ram[usize::from(a & 0x7F)]
        } else {
            self.riot.peek(a, self.controls.swcha(), self.controls.swchb())
        }
    }
}

impl Bus for System {
    fn read(&mut self, address: u16) -> u8 {
        // WSYNC holds the processor, as it reads, until the line ends.
        while self.tia.holding() {
            self.cycle();
        }
        self.cycle();
        let a = mirror(address);
        let value = if a & 0x1000 != 0 {
            self.cart.read(a, self.data_bus)
        } else if a & 0x80 == 0 {
            self.tia.read(a, self.data_bus)
        } else if a & 0x200 == 0 {
            self.riot.ram[usize::from(a & 0x7F)]
        } else {
            self.riot.read(a, self.controls.swcha(), self.controls.swchb())
        };
        self.data_bus = value;
        value
    }

    fn write(&mut self, address: u16, value: u8) {
        self.cycle();
        self.data_bus = value;
        let a = mirror(address);
        if a & 0x1000 != 0 {
            self.cart.write(a, value);
        } else if a & 0x80 == 0 {
            self.tia.write(a, value);
            self.cart.saw_tia_write(a, value);
        } else if a & 0x200 == 0 {
            self.riot.ram[usize::from(a & 0x7F)] = value;
        } else {
            self.riot.write(a, value);
        }
        if !self.watches.is_empty() && self.watches.contains(&same_place(a)) {
            self.wrote = Some((address, value));
        }
    }
}

/// The built-in Atari 2600, running.
pub struct Atari2600 {
    cpu: Cpu,
    sys: System,
    /// The last frame finished, and the TV it was.
    shown: Frame,
    pub tv: Tv,
    picture: Vec<u8>,
    /// When frames were last caught up to, and how many are owed since.
    last: Instant,
    owed: f64,
    pub paused: bool,
    /// Where the program stopped, at a breakpoint or a step: the address of the instruction to run next.
    pub stopped: Option<u16>,
    /// Why, when it's more than a breakpoint: a write it was watching for, or a condition.
    pub stop_reason: Option<String>,
    breakpoints: BTreeSet<u16>,
    conditions: BTreeMap<u16, String>,
    labels: Option<std::sync::Arc<BTreeMap<String, i64>>>,
    /// The address a Step over runs to.
    once: Option<u16>,
    /// Memory as it was at the last stop, for what's changed since to stand out.
    pub memory_before: Option<std::sync::Arc<Vec<u8>>>,
    speaker: Option<crate::speaker::Speaker>,
    pub muted: bool,
    /// The sound's resampling to the speaker's rate: how far into the next sample, its sum and count so far, and
    /// the high-pass filter's last input and output.
    phase: f64,
    sum: f32,
    count: u32,
    dc: (f32, f32),
    /// How many frames have run.
    pub frames: u64,
    /// The sound's levels as the TIA made them (two a line), for the tests.
    #[cfg(test)]
    pub heard: Vec<f32>,
}

impl Atari2600 {
    /// Whether it can run this; if not, why not (to follow "The built-in Atari 2600").
    pub fn takes(run: &Runnable) -> Result<(), String> {
        if run.machine != Machine::Atari2600 {
            return Err(crate::i18n::tf!("runs only Atari 2600 cartridges, not {} programs.", run.machine.name()));
        }
        match crate::atari2600::detect(&run.bytes) {
            Some(scheme) if Cartridge::supported(scheme) => Ok(()),
            Some(scheme) => Err(crate::i18n::tf!("doesn't switch banks the {} way yet: run it in Stella or MAME.", scheme.name())),
            None => Err(crate::i18n::tr("can't tell how this cartridge switches its banks.").into()),
        }
    }

    pub fn start(run: &Runnable, speaker: Option<crate::speaker::Speaker>) -> Result<Atari2600, String> {
        Self::takes(run).map_err(|why| crate::i18n::tf!("The built-in Atari 2600 {why}"))?;
        let scheme = crate::atari2600::detect(&run.bytes).unwrap_or(Scheme::K4);
        Ok(Self::with_cartridge(run.bytes.to_vec(), scheme, speaker))
    }

    fn with_cartridge(rom: Vec<u8>, scheme: Scheme, speaker: Option<crate::speaker::Speaker>) -> Atari2600 {
        let mut sys = System { tia: Tia::default(), riot: Riot::default(), cart: Cartridge::new(rom, scheme), cycles: 0, data_bus: 0, controls: Controls::default(), watches: BTreeSet::new(), wrote: None };
        let mut cpu = Cpu { s: 0xFD, ..Cpu::default() };
        cpu.reset(&mut sys);
        Atari2600 {
            cpu,
            sys,
            shown: Frame::default(),
            tv: Tv::Ntsc,
            picture: Vec::new(),
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
            phase: 0.0,
            sum: 0.0,
            count: 0,
            dc: (0.0, 0.0),
            frames: 0,
            #[cfg(test)]
            heard: Vec::new(),
        }
    }

    /// A cartridge's bytes, as a cartridge of whatever kind they are; for the tests.
    #[cfg(test)]
    pub fn from_rom(rom: &[u8]) -> Atari2600 {
        let scheme = crate::atari2600::detect(rom).expect("a cartridge's size");
        Self::with_cartridge(rom.to_vec(), scheme, None)
    }

    /// The frames per second it runs at, by how many lines its frames have.
    fn frames_per_second(&self) -> f64 {
        let lines = if self.shown.lines >= 200 { self.shown.lines } else { 262 };
        let clock = if self.tv == Tv::Ntsc { NTSC_CLOCK } else { PAL_CLOCK };
        clock / (lines as f64 * f64::from(LINE))
    }

    /// Runs the frames due since last time, at the 2600's own speed, catching up at most a few frames.
    pub fn catch_up(&mut self) -> Result<(), String> {
        let now = Instant::now();
        if self.paused {
            self.last = now;
            if let Some(speaker) = &self.speaker {
                speaker.clear();
            }
            return Ok(());
        }
        self.owed = (self.owed + now.duration_since(self.last).as_secs_f64() * self.frames_per_second()).min(5.0);
        self.last = now;
        while self.owed >= 1.0 && !self.paused {
            self.owed -= 1.0;
            self.frame()?;
        }
        Ok(())
    }

    /// Runs one frame: until the TIA finishes one, a breakpoint stops it, or two frames' time has passed.
    pub fn frame(&mut self) -> Result<(), String> {
        let until = self.sys.cycles + 2 * 312 * 76;
        let mut result = Ok(());
        while !self.paused {
            if let Err(problem) = self.instruction(true) {
                result = Err(problem);
                break;
            }
            if let Some(frame) = self.sys.tia.take_frame() {
                self.show(frame);
                break;
            }
            if self.sys.cycles >= until {
                // No vertical sync: what's been drawn is shown as it is.
                let (lines, _) = self.sys.tia.frame_so_far();
                if !lines.is_empty() {
                    let frame = Frame { pixels: lines.to_vec(), lines: lines.len() / WIDTH };
                    self.show(frame);
                }
                break;
            }
        }
        self.sound();
        result
    }

    fn show(&mut self, frame: Frame) {
        self.frames += 1;
        if frame.lines >= 200 {
            self.tv = if frame.lines > PAL_LINES { Tv::Pal } else { Tv::Ntsc };
        }
        self.shown = frame;
    }

    /// Runs one instruction, stopping first at a breakpoint (if `breaks`) unless it's just carried on from it.
    fn instruction(&mut self, breaks: bool) -> Result<(), String> {
        let pc = self.cpu.pc;
        if breaks && self.once == Some(pc) {
            self.once = None;
            self.stop(None);
            return Ok(());
        }
        if breaks && self.stopped.take() != Some(pc) && self.breakpoints.contains(&mirror(pc)) {
            let stop = match self.conditions.get(&mirror(pc)) {
                Some(condition) => match self.check(condition) {
                    Ok(0) => None,
                    Ok(_) => Some(Some(crate::i18n::tf!("Stopped because {condition}"))),
                    Err(problem) => Some(Some(crate::i18n::tf!("The condition {condition} couldn't be worked out: {problem}"))),
                },
                None => Some(None),
            };
            if let Some(reason) = stop {
                self.stop(reason);
                return Ok(());
            }
        }
        self.stopped = None;
        self.cpu.step(&mut self.sys);
        if self.cpu.jammed {
            self.paused = true;
            self.stopped = Some(self.cpu.pc);
            return Err(crate::i18n::tf!("The 6507 has jammed: the instruction at ${:04X} stops the processor.", self.cpu.pc.wrapping_sub(1)));
        }
        if let Some((address, value)) = self.sys.wrote.take() {
            self.stop(Some(crate::i18n::tf!("Stopped after writing {value} (${value:02X}) to ${address:04X}")));
        }
        Ok(())
    }

    fn stop(&mut self, reason: Option<String>) {
        self.paused = true;
        self.stopped = Some(self.cpu.pc);
        self.stop_reason = reason;
    }

    /// A breakpoint's condition worked out: the registers by name (A, X, Y, S, P and PC), labels, and memory.
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
            "X" => u16::from(cpu.x),
            "Y" => u16::from(cpu.y),
            "S" | "SP" => u16::from(cpu.s),
            "P" => u16::from(cpu.status(false)),
            "PC" => cpu.pc,
            _ => return None,
        }))
    }

    /// The frame's sound, to the speaker at its own rate, through a high-pass filter as a TV's would be.
    fn sound(&mut self) {
        let samples = std::mem::take(&mut self.sys.tia.samples);
        #[cfg(test)]
        self.heard.extend_from_slice(&samples);
        let Some(speaker) = self.speaker.as_ref().filter(|_| !self.muted) else {
            return;
        };
        let clock = if self.tv == Tv::Ntsc { NTSC_CLOCK } else { PAL_CLOCK };
        let step = f64::from(speaker.rate) / (clock / 114.0);
        for level in samples {
            self.sum += level;
            self.count += 1;
            self.phase += step;
            while self.phase >= 1.0 {
                self.phase -= 1.0;
                let x = self.sum / self.count.max(1) as f32;
                let (last_in, last_out) = self.dc;
                let out = x - last_in + 0.995 * last_out;
                self.dc = (x, out);
                speaker.push(out * 0.8, out * 0.8);
                self.sum = 0.0;
                self.count = 0;
            }
        }
    }

    pub fn set_breakpoints(&mut self, addresses: impl IntoIterator<Item = u16>) {
        self.breakpoints = addresses.into_iter().map(mirror).collect();
    }

    pub fn set_conditions(&mut self, conditions: BTreeMap<u16, String>, labels: Option<std::sync::Arc<BTreeMap<String, i64>>>) {
        self.conditions = conditions.into_iter().map(|(a, c)| (mirror(a), c)).collect();
        self.labels = labels;
    }

    pub fn set_write_watches(&mut self, addresses: impl IntoIterator<Item = u16>) {
        self.sys.watches = addresses.into_iter().map(same_place).collect();
    }

    /// Carries on from where it stopped.
    pub fn carry_on(&mut self) {
        self.stop_reason = None;
        self.paused = false;
    }

    /// Runs one instruction, and stops again.
    pub fn step(&mut self) -> Result<(), String> {
        self.paused = false;
        self.stopped = None;
        self.stop_reason = None;
        let result = self.instruction(false);
        self.stop(None);
        self.after_step();
        result
    }

    /// Runs one instruction, or a whole JSR and what it calls, and stops again.
    pub fn step_over(&mut self) -> Result<(), String> {
        let pc = self.cpu.pc;
        if self.sys.peek(pc) != 0x20 {
            return self.step();
        }
        self.once = Some(pc.wrapping_add(3));
        self.stop_reason = None;
        self.paused = false;
        // Off the breakpoint it's on, then on until it's back.
        self.instruction(false)?;
        for _ in 0..50 {
            self.frame()?;
            if self.paused {
                return Ok(());
            }
        }
        self.once = None;
        Ok(())
    }

    /// After a step, a frame finished on the way is shown.
    fn after_step(&mut self) {
        if let Some(frame) = self.sys.tia.take_frame() {
            self.show(frame);
        }
        self.sound();
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

    pub fn set_fire(&mut self, player: usize, pressed: bool) {
        self.sys.tia.set_fire(player, pressed);
    }

    /// Lets go of the joysticks and fire buttons, as when the screen loses the keyboard.
    pub fn release_all(&mut self) {
        self.sys.controls.sticks = [[false; 4]; 2];
        self.sys.tia.set_fire(0, false);
        self.sys.tia.set_fire(1, false);
    }

    /// The last frame's size: 160 pixels across, and the lines shown of the TV it is.
    pub fn size(&self) -> [usize; 2] {
        [WIDTH, window(self.tv).1]
    }

    /// The colour value at each pixel of the window shown, from the last frame; black where it drew nothing. While
    /// it's stopped, the lines of this frame drawn so far are shown over the last, as far as the beam has got.
    pub fn values(&self) -> Vec<u8> {
        let (top, height) = window(self.tv);
        let mut out = vec![0; WIDTH * height];
        let (so_far, line) = self.sys.tia.frame_so_far();
        let mut now = Vec::new();
        let pixels = if self.paused {
            now.extend_from_slice(&self.shown.pixels);
            now.resize(now.len().max(so_far.len() + WIDTH), 0);
            now[..so_far.len()].copy_from_slice(so_far);
            let x = usize::from(self.sys.tia.hctr.saturating_sub(68)).min(WIDTH);
            now[so_far.len()..so_far.len() + x].copy_from_slice(&line[..x]);
            &now
        } else {
            &self.shown.pixels
        };
        for row in 0..height {
            let from = (top + row) * WIDTH;
            if from + WIDTH <= pixels.len() {
                out[row * WIDTH..(row + 1) * WIDTH].copy_from_slice(&pixels[from..from + WIDTH]);
            }
        }
        out
    }

    /// The last frame, as red, green, blue and alpha; in black and white when the TV switch says so.
    pub fn picture(&mut self) -> &[u8] {
        let tv = self.tv;
        let colour = self.sys.controls.colour;
        self.picture = self
            .values()
            .into_iter()
            .flat_map(|value| {
                let [r, g, b] = tv.rgb(if colour { value } else { value & 0x0E });
                [r, g, b, 255]
            })
            .collect();
        &self.picture
    }

    /// The whole of the last frame finished, every line of it: for the tests.
    #[cfg(test)]
    pub fn last_frame(&self) -> &Frame {
        &self.shown
    }

    /// Runs until the processor reaches an address, or a number of cycles pass; whether it got there.
    #[cfg(test)]
    pub fn run_to(&mut self, address: u16, most_cycles: u64) -> bool {
        let until = self.sys.cycles + most_cycles;
        while self.sys.cycles < until {
            if mirror(self.cpu.pc) == mirror(address) {
                return true;
            }
            self.cpu.step(&mut self.sys);
            if let Some(frame) = self.sys.tia.take_frame() {
                self.show(frame);
            }
        }
        false
    }

    pub fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    /// The processor's cycles since it started.
    #[cfg(test)]
    pub fn cycles(&self) -> u64 {
        self.sys.cycles
    }

    /// The beam: the line of the frame and the cycle of the line.
    pub fn beam(&self) -> (usize, u16) {
        (self.sys.tia.frame_so_far().0.len() / WIDTH, self.sys.tia.hctr / 3)
    }

    #[cfg(test)]
    pub fn peek(&self, address: u16) -> u8 {
        self.sys.peek(address)
    }

    /// All 64K as the processor sees it (its 8K, mirrored), as it is now.
    pub fn memory(&self) -> Vec<u8> {
        (0..=0xFFFFu16).map(|a| self.sys.peek(a)).collect()
    }

    /// Where it stopped, with the 6507's registers and the beam in words, for the debugger.
    pub fn stop_words(&self) -> crate::remote_debug::Stop {
        let cpu = &self.cpu;
        let flags = cpu.status(false);
        let flag_names: String = [(7, 'N'), (6, 'V'), (3, 'D'), (2, 'I'), (1, 'Z'), (0, 'C')].iter().map(|(bit, name)| if flags & (1 << bit) != 0 { *name } else { '-' }).collect();
        let (line, cycle) = self.beam();
        let mut words = vec![format!("A {:02X}", cpu.a), format!("X {:02X}", cpu.x), format!("Y {:02X}", cpu.y), format!("SP {:02X}", cpu.s), format!("PC {:04X}", cpu.pc), format!("Flags {flag_names}")];
        words.push(crate::i18n::tf!("Line {line}, cycle {cycle}"));
        let bank = self.sys.cart.bank_name();
        if !bank.is_empty() {
            words.push(crate::i18n::tf!("Bank {bank}"));
        }
        crate::remote_debug::Stop { pc: cpu.pc, z80: None, words, ..Default::default() }
    }
}

#[cfg(test)]
#[path = "atari2600_machine/tests.rs"]
mod tests;
