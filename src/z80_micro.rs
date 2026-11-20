//! What the built-in ZX81 and Jupiter Ace share: a Z80 (rustzx-z80, as the sound effects player uses) driven an
//! instruction at a time by the machine's own hardware, which keeps the time in T-states, with breakpoints, single
//! steps, keys held down for typing, and running at the machine's own speed. Each machine's hardware (its memory,
//! ports, picture and keyboard) is in its own module: `zx81` and `jupiter_ace`.

use rustzx_z80::{Z80, Z80Bus};
use std::collections::{BTreeSet, VecDeque};
use std::time::Instant;

/// Both run at 3.25 MHz, and a frame is a fiftieth of a second.
pub const CLOCK: u64 = 3_250_000;
pub const FRAME_T: u64 = CLOCK / 50;

/// The whole picture, border and all, as the built-in Spectrum's.
pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 240;

/// A machine's hardware round the Z80.
pub trait Hardware: Z80Bus {
    /// T-states since it was switched on.
    fn now(&self) -> u64;
    /// Just before an instruction: the hardware sees what it needs of the processor, and may take the step itself
    /// (a tape routine caught, or a HALT waited out), saying so.
    fn before(&mut self, cpu: &mut Z80) -> bool;
    /// Just after one, with the refresh register as it was before.
    fn after(&mut self, cpu: &mut Z80, r_before: u8);
    /// Memory as the processor sees it, without taking any time.
    fn peek(&self, address: u16) -> u8;
    fn poke(&mut self, address: u16, value: u8);
    /// The picture as red, green, blue and alpha, `WIDTH` by `HEIGHT`.
    fn picture(&self) -> &[u8];
    /// A key on the keyboard's matrix, by its row (the address line that reads it, A8 to A15) and its bit.
    fn set_key(&mut self, row: usize, bit: u8, down: bool);
}

/// A key on the matrix: its row and bit.
pub type Key = (usize, u8);

/// Keys held down together for a few frames, then let go for a few, for a character typed.
struct Press {
    keys: Vec<Key>,
    frames: u32,
    /// Frames to wait with nothing held afterwards.
    gap: u32,
}

/// The processor and its hardware, and what the debugger and the keyboard ask of them.
pub struct Core<H: Hardware> {
    pub cpu: Z80,
    pub hw: H,
    pub paused: bool,
    /// Where it stopped, at a breakpoint or a step: the address of the instruction it will run next.
    pub stopped: Option<u16>,
    breakpoints: BTreeSet<u16>,
    /// Where a CALL being stepped over returns to.
    once: Option<u16>,
    /// Memory as it was at the last stop, for what's changed since to stand out.
    pub memory_before: Option<std::sync::Arc<Vec<u8>>>,
    last: Instant,
    owed: f64,
    typing: VecDeque<Press>,
    /// The keys the typing queue holds down now, and those held from the computer's keyboard.
    pressing: Vec<Key>,
    held: Vec<Key>,
}

impl<H: Hardware> Core<H> {
    pub fn new(cpu: Z80, hw: H) -> Core<H> {
        Core { cpu, hw, paused: false, stopped: None, breakpoints: BTreeSet::new(), once: None, memory_before: None, last: Instant::now(), owed: 0.0, typing: VecDeque::new(), pressing: Vec::new(), held: Vec::new() }
    }

    /// One instruction, with its hardware round it.
    pub fn step_once(&mut self) {
        if self.hw.before(&mut self.cpu) {
            return;
        }
        let r = self.cpu.regs.get_r();
        self.cpu.emulate(&mut self.hw);
        self.hw.after(&mut self.cpu, r);
    }

    /// Runs until `until` T-states, or a breakpoint. Whether it got there.
    pub fn run_to(&mut self, until: u64) -> bool {
        let watching = !self.breakpoints.is_empty() || self.once.is_some();
        while self.hw.now() < until {
            self.step_once();
            if watching {
                let pc = self.cpu.regs.get_pc();
                if self.once == Some(pc) || self.breakpoints.contains(&pc) {
                    self.once = None;
                    self.paused = true;
                    self.stopped = Some(pc);
                    return false;
                }
            }
        }
        true
    }

    /// Runs a fiftieth of a second, pressing and letting go of what's being typed; nothing while it's paused.
    pub fn frame(&mut self) {
        if self.paused {
            return;
        }
        self.type_tick();
        // To the next whole frame since it was switched on, so the keys change just as a frame's interrupt comes
        // (the Ace's), and never partway through the ROM reading the keyboard after it.
        let until = (self.hw.now() / FRAME_T + 1) * FRAME_T;
        self.run_to(until);
    }

    /// Runs the frames due since last time, at the machine's own speed, catching up a few frames at most; faster
    /// while it's being given text to type, so a program typed in doesn't take long.
    pub fn catch_up(&mut self) {
        let now = Instant::now();
        if self.paused {
            self.last = now;
            return;
        }
        let rate = if self.typing.is_empty() { 50.0 } else { 50.0 * 25.0 };
        self.owed = (self.owed + now.duration_since(self.last).as_secs_f64() * rate).min(if self.typing.is_empty() { 5.0 } else { 60.0 });
        self.last = now;
        while self.owed >= 1.0 && !self.paused {
            self.owed -= 1.0;
            self.frame();
        }
    }

    pub fn set_breakpoints(&mut self, addresses: impl IntoIterator<Item = u16>) {
        self.breakpoints = addresses.into_iter().collect();
    }

    /// Carries on from a stop.
    pub fn carry_on(&mut self) {
        // Off the breakpoint it's on first, so it doesn't stop there again at once.
        self.step_once();
        self.paused = false;
        self.stopped = None;
    }

    /// Runs one instruction, and stops.
    pub fn step(&mut self) {
        self.step_once();
        self.paused = true;
        self.stopped = Some(self.cpu.regs.get_pc());
    }

    /// Runs one instruction, or a whole CALL, RST or block instruction, stopping after it.
    pub fn step_over(&mut self) {
        let pc = self.cpu.regs.get_pc();
        let op = self.hw.peek(pc);
        let length = match op {
            0xCD | 0xC4 | 0xCC | 0xD4 | 0xDC | 0xE4 | 0xEC | 0xF4 | 0xFC => 3,
            0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => 1,
            0xED if matches!(self.hw.peek(pc.wrapping_add(1)), 0xB0 | 0xB1 | 0xB2 | 0xB3 | 0xB8 | 0xB9 | 0xBA | 0xBB) => 2,
            0x10 => 2,
            _ => 0,
        };
        if length == 0 {
            return self.step();
        }
        self.once = Some(pc.wrapping_add(length));
        self.step_once();
        self.paused = false;
        self.stopped = None;
        // A tenth of a second at most, so a routine that never comes back doesn't hold the window up.
        let until = self.hw.now() + FRAME_T * 5;
        if self.run_to(until) {
            self.once = None;
            self.paused = true;
            self.stopped = Some(self.cpu.regs.get_pc());
        }
    }

    /// The registers, as the debugger shows them.
    pub fn registers(&self) -> crate::zx_machine::Registers {
        let r = &self.cpu.regs;
        let word = |high: u8, low: u8| u16::from_le_bytes([low, high]);
        crate::zx_machine::Registers {
            af: r.get_af(),
            bc: r.get_bc(),
            de: r.get_de(),
            hl: r.get_hl(),
            ix: r.get_ix(),
            iy: r.get_iy(),
            sp: r.get_sp(),
            pc: r.get_pc(),
            af2: word(r.get_acc_alt(), r.get_flags_alt()),
            bc2: word(r.get_b_alt(), r.get_c_alt()),
            de2: word(r.get_d_alt(), r.get_e_alt()),
            hl2: word(r.get_h_alt(), r.get_l_alt()),
            i: r.get_i(),
            r: r.get_r(),
            iff: r.get_iff1(),
            im: match self.cpu.get_im() {
                rustzx_z80::IntMode::Im0 => 0,
                rustzx_z80::IntMode::Im1 => 1,
                rustzx_z80::IntMode::Im2 => 2,
            },
        }
    }

    /// All 64K as the processor sees it.
    pub fn memory(&self) -> Vec<u8> {
        (0..=0xFFFFu16).map(|a| self.hw.peek(a)).collect()
    }

    /// Keys typed, each held for `frames` frames with `gap` frames after with nothing held.
    pub fn type_keys(&mut self, keys: Vec<Key>, frames: u32, gap: u32) {
        self.typing.push_back(Press { keys, frames, gap });
    }

    /// Whether there's still typing to be done.
    pub fn typing(&self) -> bool {
        !self.typing.is_empty()
    }

    /// A key on the computer's keyboard pressed or let go.
    pub fn hold(&mut self, keys: &[Key], down: bool) {
        for key in keys {
            if down {
                if !self.held.contains(key) {
                    self.held.push(*key);
                }
            } else {
                self.held.retain(|k| k != key);
            }
        }
        self.apply_keys();
    }

    /// Lets go of every key held from the computer's keyboard.
    pub fn release_all(&mut self) {
        if !self.held.is_empty() {
            self.held.clear();
            self.apply_keys();
        }
    }

    fn type_tick(&mut self) {
        let Some(press) = self.typing.front_mut() else {
            if !self.pressing.is_empty() {
                self.pressing.clear();
                self.apply_keys();
            }
            return;
        };
        if press.frames > 0 {
            press.frames -= 1;
            self.pressing = press.keys.clone();
        } else if press.gap > 0 {
            press.gap -= 1;
            self.pressing.clear();
        } else {
            self.typing.pop_front();
            self.pressing.clear();
            return self.type_tick();
        }
        self.apply_keys();
    }

    /// The matrix as the keys held and the keys being typed make it.
    fn apply_keys(&mut self) {
        for row in 0..8 {
            for bit in 0..5 {
                let down = self.held.contains(&(row, bit)) || self.pressing.contains(&(row, bit));
                self.hw.set_key(row, bit, down);
            }
        }
    }
}

/// The standard CRC-32, by which ROMs are told apart.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// Paints a picture of pixels (0 paper, 1 ink) as red, green, blue and alpha.
pub fn paint(pixels: &[u8], paper: [u8; 4], ink: [u8; 4], rgba: &mut [u8]) {
    for (pixel, out) in pixels.iter().zip(rgba.as_chunks_mut::<4>().0) {
        *out = if *pixel != 0 { ink } else { paper };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_is_the_usual_one() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
