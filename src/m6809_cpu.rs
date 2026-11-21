//! The Motorola 6809, an instruction at a time, for the built-in Vectrex: every 6809 instruction and addressing mode,
//! the four undocumented aliases of the memory instructions (as the chip runs them), interrupts (NMI, FIRQ and IRQ),
//! CWAI and SYNC. Each instruction's cycles come from `m6809_timing`, the table F9's cycle counts use, so the two
//! can't disagree; the machine runs its other chips for as many cycles as each instruction took.
//!
//! What each instruction does is Motorola's MC6809-MC6809E Programming Manual (M6809PM/AD), chapters 6 and 7 and
//! appendix A, and the MC6809 data sheet's interrupt timing (NMI and IRQ take 19 cycles to start, FIRQ 10).

use crate::m6809_timing::{Mode, cycles};

pub trait Bus {
    fn read(&mut self, address: u16) -> u8;
    fn write(&mut self, address: u16, value: u8);
}

/// The condition code register's flags.
pub const C: u8 = 0x01;
pub const V: u8 = 0x02;
pub const Z: u8 = 0x04;
pub const N: u8 = 0x08;
pub const I: u8 = 0x10;
pub const H: u8 = 0x20;
pub const F: u8 = 0x40;
pub const E: u8 = 0x80;

/// Where the interrupts' and reset's addresses are kept.
const SWI3_VECTOR: u16 = 0xFFF2;
const SWI2_VECTOR: u16 = 0xFFF4;
const FIRQ_VECTOR: u16 = 0xFFF6;
const IRQ_VECTOR: u16 = 0xFFF8;
const SWI_VECTOR: u16 = 0xFFFA;
const NMI_VECTOR: u16 = 0xFFFC;
const RESET_VECTOR: u16 = 0xFFFE;

/// What the processor is waiting for, if anything.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Wait {
    #[default]
    Running,
    /// After CWAI: everything is stacked, and the next interrupt not masked goes straight to its routine.
    Cwai,
    /// After SYNC: any interrupt line carries on, to its routine if it isn't masked or to the next instruction if
    /// it is.
    Sync,
}

/// The interrupt lines, as the machine's chips hold them (true is asserted).
#[derive(Clone, Copy, Debug, Default)]
pub struct Lines {
    pub irq: bool,
    pub firq: bool,
    pub nmi: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Cpu {
    pub a: u8,
    pub b: u8,
    pub x: u16,
    pub y: u16,
    pub u: u16,
    pub s: u16,
    pub pc: u16,
    pub dp: u8,
    pub cc: u8,
    pub wait: Wait,
    /// Where an opcode the 6809 doesn't have was met, which stops it here.
    pub illegal: Option<u16>,
    /// Cycles run since it was reset.
    pub cycles: u64,
    nmi_before: bool,
    /// This instruction's bytes as they were fetched (with its opcode as the timing table knows it), for its cycles.
    fetched: [u8; 5],
    fetched_len: usize,
    /// Cycles an instruction takes beyond its least: a long branch taken, or RTI unstacking everything.
    most: bool,
}

/// The memory instructions, by the low four bits of their opcodes ($00 direct, $40 on A, $50 on B, $60 indexed,
/// $70 extended). The undocumented $x1, $x2, $x5 and $xB act as NEG, NEG or COM, LSR and DEC, as on the chip.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rmw {
    Neg,
    NegCom,
    Com,
    Lsr,
    Ror,
    Asr,
    Asl,
    Rol,
    Dec,
    Inc,
    Tst,
    Jmp,
    Clr,
}

fn rmw_of(low: u8) -> (Rmw, u8) {
    // The second is the documented opcode whose cycles it takes.
    match low {
        0x0 => (Rmw::Neg, 0x0),
        0x1 => (Rmw::Neg, 0x0),
        0x2 => (Rmw::NegCom, 0x0),
        0x3 => (Rmw::Com, 0x3),
        0x4 | 0x5 => (Rmw::Lsr, 0x4),
        0x6 => (Rmw::Ror, 0x6),
        0x7 => (Rmw::Asr, 0x7),
        0x8 => (Rmw::Asl, 0x8),
        0x9 => (Rmw::Rol, 0x9),
        0xA | 0xB => (Rmw::Dec, 0xA),
        0xC => (Rmw::Inc, 0xC),
        0xD => (Rmw::Tst, 0xD),
        0xE => (Rmw::Jmp, 0xE),
        _ => (Rmw::Clr, 0xF),
    }
}

impl Cpu {
    /// Reset: interrupts masked, the direct page at 0, and on from the address at $FFFE.
    pub fn reset(&mut self, bus: &mut impl Bus) {
        self.cc = I | F;
        self.dp = 0;
        self.wait = Wait::Running;
        self.illegal = None;
        self.pc = read16(bus, RESET_VECTOR);
    }

    pub fn d(&self) -> u16 {
        u16::from_be_bytes([self.a, self.b])
    }

    pub fn set_d(&mut self, d: u16) {
        [self.a, self.b] = d.to_be_bytes();
    }

    /// Runs one instruction, or starts an interrupt, or waits a cycle; the cycles it took.
    pub fn step(&mut self, bus: &mut impl Bus, lines: Lines) -> u32 {
        let nmi = lines.nmi && !self.nmi_before;
        self.nmi_before = lines.nmi;
        if self.illegal.is_some() {
            return 1;
        }
        if nmi {
            return self.interrupt(bus, NMI_VECTOR, true, I | F, 19);
        }
        if lines.firq && self.cc & F == 0 {
            return self.interrupt(bus, FIRQ_VECTOR, false, I | F, 10);
        }
        if lines.irq && self.cc & I == 0 {
            return self.interrupt(bus, IRQ_VECTOR, true, I, 19);
        }
        match self.wait {
            Wait::Running => {}
            // A masked interrupt ends SYNC too, carrying on with the next instruction.
            Wait::Sync if lines.irq || lines.firq => self.wait = Wait::Running,
            _ => {
                self.cycles += 1;
                return 1;
            }
        }
        let start = self.pc;
        self.fetched_len = 0;
        self.most = false;
        if !self.execute(bus) {
            self.pc = start;
            self.illegal = Some(start);
            return 1;
        }
        let timing = cycles(&self.fetched[..self.fetched_len], Mode::Emulation).unwrap_or_default();
        let taken = if self.most { timing.most } else { timing.least };
        let taken = taken.max(1);
        self.cycles += u64::from(taken);
        taken
    }

    /// Starts an interrupt's routine: everything stacked (E set) or only PC and CC (E clear, for FIRQ), unless
    /// CWAI has stacked everything already.
    fn interrupt(&mut self, bus: &mut impl Bus, vector: u16, entire: bool, mask: u8, cycles: u32) -> u32 {
        let taken = if self.wait == Wait::Cwai {
            // Stacked already; it only fetches the address.
            7
        } else {
            if entire {
                self.cc |= E;
                self.push_all(bus);
            } else {
                self.cc &= !E;
                self.push16_s(bus, self.pc);
                self.push8_s(bus, self.cc);
            }
            cycles
        };
        self.wait = Wait::Running;
        self.cc |= mask;
        self.pc = read16(bus, vector);
        self.cycles += u64::from(taken);
        taken
    }

    fn push_all(&mut self, bus: &mut impl Bus) {
        self.push16_s(bus, self.pc);
        self.push16_s(bus, self.u);
        self.push16_s(bus, self.y);
        self.push16_s(bus, self.x);
        self.push8_s(bus, self.dp);
        self.push8_s(bus, self.b);
        self.push8_s(bus, self.a);
        self.push8_s(bus, self.cc);
    }

    fn push8_s(&mut self, bus: &mut impl Bus, value: u8) {
        self.s = self.s.wrapping_sub(1);
        bus.write(self.s, value);
    }

    fn push16_s(&mut self, bus: &mut impl Bus, value: u16) {
        let [hi, lo] = value.to_be_bytes();
        self.push8_s(bus, lo);
        self.push8_s(bus, hi);
    }

    fn pull8_s(&mut self, bus: &mut impl Bus) -> u8 {
        let value = bus.read(self.s);
        self.s = self.s.wrapping_add(1);
        value
    }

    fn pull16_s(&mut self, bus: &mut impl Bus) -> u16 {
        let hi = self.pull8_s(bus);
        let lo = self.pull8_s(bus);
        u16::from_be_bytes([hi, lo])
    }

    fn fetch(&mut self, bus: &mut impl Bus) -> u8 {
        let byte = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        if self.fetched_len < self.fetched.len() {
            self.fetched[self.fetched_len] = byte;
            self.fetched_len += 1;
        }
        byte
    }

    fn fetch16(&mut self, bus: &mut impl Bus) -> u16 {
        let hi = self.fetch(bus);
        let lo = self.fetch(bus);
        u16::from_be_bytes([hi, lo])
    }

    /// Which register an indexed postbyte names (bits 5 and 6).
    fn index_register(&mut self, post: u8) -> &mut u16 {
        match (post >> 5) & 3 {
            0 => &mut self.x,
            1 => &mut self.y,
            2 => &mut self.u,
            _ => &mut self.s,
        }
    }

    /// An indexed address, from its postbyte on (Programming Manual table 2-2).
    fn indexed(&mut self, bus: &mut impl Bus) -> u16 {
        let post = self.fetch(bus);
        if post & 0x80 == 0 {
            // A 5-bit offset, -16 to 15.
            let offset = (((post & 0x1F) << 3) as i8 >> 3) as i16 as u16;
            return self.index_register(post).wrapping_add(offset);
        }
        let (a, b, d) = (self.a, self.b, self.d());
        let address = match post & 0x0F {
            0x0 => {
                let r = self.index_register(post);
                let at = *r;
                *r = at.wrapping_add(1);
                at
            }
            0x1 => {
                let r = self.index_register(post);
                let at = *r;
                *r = at.wrapping_add(2);
                at
            }
            0x2 => {
                let r = self.index_register(post);
                *r = r.wrapping_sub(1);
                *r
            }
            0x3 => {
                let r = self.index_register(post);
                *r = r.wrapping_sub(2);
                *r
            }
            0x4 => *self.index_register(post),
            0x5 => self.index_register(post).wrapping_add(b as i8 as i16 as u16),
            0x6 => self.index_register(post).wrapping_add(a as i8 as i16 as u16),
            0x8 => {
                let offset = self.fetch(bus) as i8 as i16 as u16;
                self.index_register(post).wrapping_add(offset)
            }
            0x9 => {
                let offset = self.fetch16(bus);
                self.index_register(post).wrapping_add(offset)
            }
            0xB => self.index_register(post).wrapping_add(d),
            0xC => {
                let offset = self.fetch(bus) as i8 as i16 as u16;
                self.pc.wrapping_add(offset)
            }
            0xD => {
                let offset = self.fetch16(bus);
                self.pc.wrapping_add(offset)
            }
            // [address], with the indirect bit; the other codes ($x7, $xA and $xE) mean nothing on a 6809, and
            // read as ,R.
            0xF => self.fetch16(bus),
            _ => *self.index_register(post),
        };
        if post & 0x10 != 0 { read16(bus, address) } else { address }
    }

    /// The effective address for an opcode's mode, by bits 4 and 5 of $80-$FF opcodes: direct, indexed or extended.
    fn address(&mut self, bus: &mut impl Bus, op: u8) -> u16 {
        match (op >> 4) & 3 {
            1 => u16::from_be_bytes([self.dp, self.fetch(bus)]),
            2 => self.indexed(bus),
            _ => self.fetch16(bus),
        }
    }

    /// An 8-bit operand: immediate, or read from where the mode says.
    fn operand8(&mut self, bus: &mut impl Bus, op: u8) -> u8 {
        if (op >> 4) & 3 == 0 {
            return self.fetch(bus);
        }
        let at = self.address(bus, op);
        bus.read(at)
    }

    fn operand16(&mut self, bus: &mut impl Bus, op: u8) -> u16 {
        if (op >> 4) & 3 == 0 {
            return self.fetch16(bus);
        }
        let at = self.address(bus, op);
        read16(bus, at)
    }

    fn set(&mut self, flag: u8, on: bool) {
        if on {
            self.cc |= flag;
        } else {
            self.cc &= !flag;
        }
    }

    fn nz8(&mut self, value: u8) {
        self.set(N, value & 0x80 != 0);
        self.set(Z, value == 0);
    }

    fn nz16(&mut self, value: u16) {
        self.set(N, value & 0x8000 != 0);
        self.set(Z, value == 0);
    }

    /// N and Z from a value loaded, stored or worked out logically; V cleared.
    fn logical8(&mut self, value: u8) -> u8 {
        self.nz8(value);
        self.cc &= !V;
        value
    }

    fn logical16(&mut self, value: u16) -> u16 {
        self.nz16(value);
        self.cc &= !V;
        value
    }

    fn add8(&mut self, a: u8, b: u8, carry: bool) -> u8 {
        let sum = u16::from(a) + u16::from(b) + u16::from(carry);
        let result = sum as u8;
        self.set(H, (a ^ b ^ result) & 0x10 != 0);
        self.set(C, sum & 0x100 != 0);
        self.set(V, (a ^ result) & (b ^ result) & 0x80 != 0);
        self.nz8(result);
        result
    }

    fn sub8(&mut self, a: u8, b: u8, borrow: bool) -> u8 {
        let difference = u16::from(a).wrapping_sub(u16::from(b)).wrapping_sub(u16::from(borrow));
        let result = difference as u8;
        self.set(C, difference & 0x100 != 0);
        self.set(V, (a ^ b) & (a ^ result) & 0x80 != 0);
        self.nz8(result);
        result
    }

    fn add16(&mut self, a: u16, b: u16) -> u16 {
        let sum = u32::from(a) + u32::from(b);
        let result = sum as u16;
        self.set(C, sum & 0x1_0000 != 0);
        self.set(V, (a ^ result) & (b ^ result) & 0x8000 != 0);
        self.nz16(result);
        result
    }

    fn sub16(&mut self, a: u16, b: u16) -> u16 {
        let difference = u32::from(a).wrapping_sub(u32::from(b));
        let result = difference as u16;
        self.set(C, difference & 0x1_0000 != 0);
        self.set(V, (a ^ b) & (a ^ result) & 0x8000 != 0);
        self.nz16(result);
        result
    }

    /// One of the memory instructions on a value; what to write back (None for TST).
    fn rmw(&mut self, kind: Rmw, value: u8) -> Option<u8> {
        let carry = self.cc & C != 0;
        let result = match kind {
            Rmw::Neg => self.sub8(0, value, false),
            // $x2: NEG when C is clear and COM when it's set, as MAME's 6809 has it.
            Rmw::NegCom if carry => return self.rmw(Rmw::Com, value),
            Rmw::NegCom => self.sub8(0, value, false),
            Rmw::Com => {
                let result = self.logical8(!value);
                self.cc |= C;
                result
            }
            Rmw::Lsr => {
                self.set(C, value & 1 != 0);
                self.nz8(value >> 1);
                value >> 1
            }
            Rmw::Ror => {
                let result = (value >> 1) | (u8::from(carry) << 7);
                self.set(C, value & 1 != 0);
                self.nz8(result);
                result
            }
            Rmw::Asr => {
                let result = (value >> 1) | (value & 0x80);
                self.set(C, value & 1 != 0);
                self.nz8(result);
                result
            }
            Rmw::Asl => {
                let result = value << 1;
                self.set(C, value & 0x80 != 0);
                self.set(V, (value ^ result) & 0x80 != 0);
                self.nz8(result);
                result
            }
            Rmw::Rol => {
                let result = (value << 1) | u8::from(carry);
                self.set(C, value & 0x80 != 0);
                self.set(V, (value ^ (value << 1)) & 0x80 != 0);
                self.nz8(result);
                result
            }
            Rmw::Dec => {
                let result = value.wrapping_sub(1);
                self.set(V, value == 0x80);
                self.nz8(result);
                result
            }
            Rmw::Inc => {
                let result = value.wrapping_add(1);
                self.set(V, value == 0x7F);
                self.nz8(result);
                result
            }
            Rmw::Tst => {
                self.logical8(value);
                return None;
            }
            Rmw::Clr => {
                self.cc = (self.cc & !(N | V | C)) | Z;
                0
            }
            Rmw::Jmp => return None,
        };
        Some(result)
    }

    /// Whether a branch's condition holds, by the low four bits of its opcode.
    fn condition(&self, code: u8) -> bool {
        let (c, v, z, n) = (self.cc & C != 0, self.cc & V != 0, self.cc & Z != 0, self.cc & N != 0);
        match code & 0x0F {
            0x0 => true,
            0x1 => false,
            0x2 => !(c || z),
            0x3 => c || z,
            0x4 => !c,
            0x5 => c,
            0x6 => !z,
            0x7 => z,
            0x8 => !v,
            0x9 => v,
            0xA => !n,
            0xB => n,
            0xC => n == v,
            0xD => n != v,
            0xE => !z && n == v,
            _ => z || n != v,
        }
    }

    /// A register as TFR and EXG number them; 8-bit ones read with $FF above them, as the chip gives them to a
    /// 16-bit register.
    fn register(&self, code: u8) -> (u16, bool) {
        match code {
            0x0 => (self.d(), true),
            0x1 => (self.x, true),
            0x2 => (self.y, true),
            0x3 => (self.u, true),
            0x4 => (self.s, true),
            0x5 => (self.pc, true),
            0x8 => (0xFF00 | u16::from(self.a), false),
            0x9 => (0xFF00 | u16::from(self.b), false),
            0xA => (0xFF00 | u16::from(self.cc), false),
            0xB => (0xFF00 | u16::from(self.dp), false),
            _ => (0xFFFF, true),
        }
    }

    fn set_register(&mut self, code: u8, value: u16) {
        match code {
            0x0 => self.set_d(value),
            0x1 => self.x = value,
            0x2 => self.y = value,
            0x3 => self.u = value,
            0x4 => self.s = value,
            0x5 => self.pc = value,
            0x8 => self.a = value as u8,
            0x9 => self.b = value as u8,
            0xA => self.cc = value as u8,
            0xB => self.dp = value as u8,
            _ => {}
        }
    }

    /// PSHS or PSHU (`user`): the registers in the postbyte, PC first.
    fn push(&mut self, bus: &mut impl Bus, mask: u8, user: bool) {
        let other = if user { self.s } else { self.u };
        let mut sp = if user { self.u } else { self.s };
        let mut put = |value: u8| {
            sp = sp.wrapping_sub(1);
            bus.write(sp, value);
        };
        for (bit, value) in [(0x80, self.pc), (0x40, other), (0x20, self.y), (0x10, self.x)] {
            if mask & bit != 0 {
                let [hi, lo] = value.to_be_bytes();
                put(lo);
                put(hi);
            }
        }
        for (bit, value) in [(0x08, self.dp), (0x04, self.b), (0x02, self.a), (0x01, self.cc)] {
            if mask & bit != 0 {
                put(value);
            }
        }
        if user {
            self.u = sp;
        } else {
            self.s = sp;
        }
    }

    /// PULS or PULU: CC first, PC last.
    fn pull(&mut self, bus: &mut impl Bus, mask: u8, user: bool) {
        let mut sp = if user { self.u } else { self.s };
        let mut take = || {
            let value = bus.read(sp);
            sp = sp.wrapping_add(1);
            value
        };
        let mut bytes = [0u8; 12];
        let mut taken = 0;
        let mut word = |bytes: &mut [u8; 12], taken: &mut usize, count: usize| {
            for _ in 0..count {
                bytes[*taken] = take();
                *taken += 1;
            }
        };
        for (bit, count) in [(0x01, 1), (0x02, 1), (0x04, 1), (0x08, 1), (0x10, 2), (0x20, 2), (0x40, 2), (0x80, 2)] {
            if mask & bit != 0 {
                word(&mut bytes, &mut taken, count);
            }
        }
        let mut at = 0;
        let mut next8 = || {
            at += 1;
            bytes[at - 1]
        };
        if mask & 0x01 != 0 {
            self.cc = next8();
        }
        if mask & 0x02 != 0 {
            self.a = next8();
        }
        if mask & 0x04 != 0 {
            self.b = next8();
        }
        if mask & 0x08 != 0 {
            self.dp = next8();
        }
        let mut next16 = || u16::from_be_bytes([next8(), next8()]);
        if mask & 0x10 != 0 {
            self.x = next16();
        }
        if mask & 0x20 != 0 {
            self.y = next16();
        }
        if mask & 0x40 != 0 {
            let value = next16();
            if user {
                self.s = value;
            } else {
                self.u = value;
            }
        }
        if mask & 0x80 != 0 {
            self.pc = next16();
        }
        if user {
            self.u = sp;
        } else {
            self.s = sp;
        }
    }

    /// Runs the instruction at PC; false if it isn't one a 6809 has.
    fn execute(&mut self, bus: &mut impl Bus) -> bool {
        let op = self.fetch(bus);
        match op {
            0x10 => return self.page2(bus),
            0x11 => return self.page3(bus),
            // The memory instructions: direct, on A, on B, indexed and extended.
            0x00..=0x0F | 0x40..=0x7F => {
                let (kind, timed_as) = rmw_of(op & 0x0F);
                // There's no JMP on A or B ($4E and $5E), which the chip runs as CLR.
                let kind = if kind == Rmw::Jmp && matches!(op >> 4, 4 | 5) { Rmw::Clr } else { kind };
                let timed_as = if matches!(op, 0x4E | 0x5E) { 0x0F } else { timed_as };
                self.fetched[0] = (op & 0xF0) | timed_as;
                match op >> 4 {
                    0x4 => {
                        if let Some(result) = self.rmw(kind, self.a) {
                            self.a = result;
                        }
                    }
                    0x5 => {
                        if let Some(result) = self.rmw(kind, self.b) {
                            self.b = result;
                        }
                    }
                    page => {
                        let at = match page {
                            0x0 => u16::from_be_bytes([self.dp, self.fetch(bus)]),
                            0x6 => self.indexed(bus),
                            _ => self.fetch16(bus),
                        };
                        if kind == Rmw::Jmp {
                            self.pc = at;
                        } else if kind == Rmw::Clr {
                            // CLR reads before it writes, as the chip does.
                            let _ = bus.read(at);
                            self.rmw(kind, 0);
                            bus.write(at, 0);
                        } else {
                            let value = bus.read(at);
                            if let Some(result) = self.rmw(kind, value) {
                                bus.write(at, result);
                            }
                        }
                    }
                }
            }
            0x12 => {}
            0x13 => self.wait = Wait::Sync,
            0x16 => {
                let offset = self.fetch16(bus);
                self.pc = self.pc.wrapping_add(offset);
            }
            0x17 => {
                let offset = self.fetch16(bus);
                self.push16_s(bus, self.pc);
                self.pc = self.pc.wrapping_add(offset);
            }
            0x19 => {
                // DAA: the sum of two BCD bytes put right (Programming Manual, DAA).
                let (a, carry, half) = (self.a, self.cc & C != 0, self.cc & H != 0);
                let (low, high) = (a & 0x0F, a >> 4);
                let mut correction = 0u8;
                if low > 9 || half {
                    correction |= 0x06;
                }
                if high > 9 || carry || (high > 8 && low > 9) {
                    correction |= 0x60;
                }
                let sum = u16::from(a) + u16::from(correction);
                self.a = sum as u8;
                self.set(C, carry || sum > 0xFF);
                self.nz8(self.a);
                self.cc &= !V;
            }
            0x1A => {
                let value = self.fetch(bus);
                self.cc |= value;
            }
            0x1C => {
                let value = self.fetch(bus);
                self.cc &= value;
            }
            0x1D => {
                self.a = if self.b & 0x80 != 0 { 0xFF } else { 0 };
                self.nz16(self.d());
            }
            0x1E | 0x1F => {
                let post = self.fetch(bus);
                let (from, to) = (post >> 4, post & 0x0F);
                let (source, _) = self.register(from);
                if op == 0x1E {
                    let (dest, _) = self.register(to);
                    self.set_register(to, source);
                    self.set_register(from, dest);
                } else {
                    self.set_register(to, source);
                }
            }
            0x20..=0x2F => {
                let offset = self.fetch(bus) as i8 as i16 as u16;
                if self.condition(op) {
                    self.pc = self.pc.wrapping_add(offset);
                }
            }
            0x30..=0x33 => {
                let at = self.indexed(bus);
                match op {
                    0x30 => {
                        self.x = at;
                        self.set(Z, at == 0);
                    }
                    0x31 => {
                        self.y = at;
                        self.set(Z, at == 0);
                    }
                    0x32 => self.s = at,
                    _ => self.u = at,
                }
            }
            0x34..=0x37 => {
                let mask = self.fetch(bus);
                let user = op >= 0x36;
                if op & 1 == 0 {
                    self.push(bus, mask, user);
                } else {
                    self.pull(bus, mask, user);
                }
            }
            0x39 => self.pc = self.pull16_s(bus),
            0x3A => self.x = self.x.wrapping_add(u16::from(self.b)),
            0x3B => {
                self.cc = self.pull8_s(bus);
                if self.cc & E != 0 {
                    self.most = true;
                    self.a = self.pull8_s(bus);
                    self.b = self.pull8_s(bus);
                    self.dp = self.pull8_s(bus);
                    self.x = self.pull16_s(bus);
                    self.y = self.pull16_s(bus);
                    self.u = self.pull16_s(bus);
                }
                self.pc = self.pull16_s(bus);
            }
            0x3C => {
                let value = self.fetch(bus);
                self.cc = (self.cc & value) | E;
                self.push_all(bus);
                self.wait = Wait::Cwai;
            }
            0x3D => {
                let product = u16::from(self.a) * u16::from(self.b);
                self.set_d(product);
                self.set(Z, product == 0);
                self.set(C, product & 0x80 != 0);
            }
            0x3F => self.software_interrupt(bus, SWI_VECTOR, true),
            0x8D => {
                let offset = self.fetch(bus) as i8 as i16 as u16;
                self.push16_s(bus, self.pc);
                self.pc = self.pc.wrapping_add(offset);
            }
            // JSR direct, indexed and extended.
            0x9D | 0xAD | 0xBD => {
                let at = self.address(bus, op);
                self.push16_s(bus, self.pc);
                self.pc = at;
            }
            // There's no store immediate.
            0x87 | 0x8F | 0xC7 | 0xCD | 0xCF => return false,
            0x80..=0xFF => self.accumulator(bus, op),
            _ => return false,
        }
        true
    }

    /// The $80-$FF instructions on A, B, D, X and U.
    fn accumulator(&mut self, bus: &mut impl Bus, op: u8) {
        let on_b = op >= 0xC0;
        let low = op & 0x0F;
        let acc = if on_b { self.b } else { self.a };
        let put = |cpu: &mut Cpu, value: u8| {
            if on_b {
                cpu.b = value;
            } else {
                cpu.a = value;
            }
        };
        match low {
            0x0 => {
                let value = self.operand8(bus, op);
                let result = self.sub8(acc, value, false);
                put(self, result);
            }
            0x1 => {
                let value = self.operand8(bus, op);
                self.sub8(acc, value, false);
            }
            0x2 => {
                let value = self.operand8(bus, op);
                let result = self.sub8(acc, value, self.cc & C != 0);
                put(self, result);
            }
            0x3 => {
                let value = self.operand16(bus, op);
                let result = if on_b { self.add16(self.d(), value) } else { self.sub16(self.d(), value) };
                self.set_d(result);
            }
            0x4 => {
                let value = self.operand8(bus, op);
                let result = self.logical8(acc & value);
                put(self, result);
            }
            0x5 => {
                let value = self.operand8(bus, op);
                self.logical8(acc & value);
            }
            0x6 => {
                let value = self.operand8(bus, op);
                let result = self.logical8(value);
                put(self, result);
            }
            0x7 => {
                let at = self.address(bus, op);
                self.logical8(acc);
                bus.write(at, acc);
            }
            0x8 => {
                let value = self.operand8(bus, op);
                let result = self.logical8(acc ^ value);
                put(self, result);
            }
            0x9 => {
                let value = self.operand8(bus, op);
                let result = self.add8(acc, value, self.cc & C != 0);
                put(self, result);
            }
            0xA => {
                let value = self.operand8(bus, op);
                let result = self.logical8(acc | value);
                put(self, result);
            }
            0xB => {
                let value = self.operand8(bus, op);
                let result = self.add8(acc, value, false);
                put(self, result);
            }
            0xC => {
                let value = self.operand16(bus, op);
                if on_b {
                    let result = self.logical16(value);
                    self.set_d(result);
                } else {
                    self.sub16(self.x, value);
                }
            }
            0xD => {
                // STD (JSR and BSR are taken before this).
                let at = self.address(bus, op);
                let d = self.logical16(self.d());
                write16(bus, at, d);
            }
            0xE => {
                let value = self.operand16(bus, op);
                let value = self.logical16(value);
                if on_b {
                    self.u = value;
                } else {
                    self.x = value;
                }
            }
            _ => {
                let at = self.address(bus, op);
                let value = self.logical16(if on_b { self.u } else { self.x });
                write16(bus, at, value);
            }
        }
    }

    fn software_interrupt(&mut self, bus: &mut impl Bus, vector: u16, mask: bool) {
        self.cc |= E;
        self.push_all(bus);
        if mask {
            self.cc |= I | F;
        }
        self.pc = read16(bus, vector);
    }

    /// The instructions after $10: long branches, SWI2, and D, Y and S's own.
    fn page2(&mut self, bus: &mut impl Bus) -> bool {
        let op = self.fetch(bus);
        match op {
            0x21..=0x2F => {
                let offset = self.fetch16(bus);
                if self.condition(op) {
                    self.most = true;
                    self.pc = self.pc.wrapping_add(offset);
                }
            }
            0x3F => self.software_interrupt(bus, SWI2_VECTOR, false),
            0x83 | 0x93 | 0xA3 | 0xB3 => {
                let value = self.operand16(bus, op);
                self.sub16(self.d(), value);
            }
            0x8C | 0x9C | 0xAC | 0xBC => {
                let value = self.operand16(bus, op);
                self.sub16(self.y, value);
            }
            0x8E | 0x9E | 0xAE | 0xBE => {
                let value = self.operand16(bus, op);
                self.y = self.logical16(value);
            }
            0x9F | 0xAF | 0xBF => {
                let at = self.address(bus, op);
                let value = self.logical16(self.y);
                write16(bus, at, value);
            }
            0xCE | 0xDE | 0xEE | 0xFE => {
                let value = self.operand16(bus, op);
                self.s = self.logical16(value);
            }
            0xDF | 0xEF | 0xFF => {
                let at = self.address(bus, op);
                let value = self.logical16(self.s);
                write16(bus, at, value);
            }
            _ => return false,
        }
        true
    }

    /// The instructions after $11: SWI3, and comparing U and S.
    fn page3(&mut self, bus: &mut impl Bus) -> bool {
        let op = self.fetch(bus);
        match op {
            0x3F => self.software_interrupt(bus, SWI3_VECTOR, false),
            0x83 | 0x93 | 0xA3 | 0xB3 => {
                let value = self.operand16(bus, op);
                self.sub16(self.u, value);
            }
            0x8C | 0x9C | 0xAC | 0xBC => {
                let value = self.operand16(bus, op);
                self.sub16(self.s, value);
            }
            _ => return false,
        }
        true
    }

    /// The flags as letters, set or not, for the debugger: EFHINZVC.
    pub fn flags(&self) -> String {
        "EFHINZVC".chars().enumerate().map(|(i, name)| if self.cc & (0x80 >> i) != 0 { name } else { '-' }).collect()
    }
}

pub fn read16(bus: &mut impl Bus, address: u16) -> u16 {
    let hi = bus.read(address);
    let lo = bus.read(address.wrapping_add(1));
    u16::from_be_bytes([hi, lo])
}

fn write16(bus: &mut impl Bus, address: u16, value: u16) {
    let [hi, lo] = value.to_be_bytes();
    bus.write(address, hi);
    bus.write(address.wrapping_add(1), lo);
}

#[cfg(test)]
#[path = "m6809_cpu/tests.rs"]
mod tests;
