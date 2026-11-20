//! The NMOS 6502 (and the 6507 the Atari 2600 has), a cycle at a time: every cycle of every instruction is one
//! read or write on the bus, the dummy reads and writes the chip makes among them, so a machine whose chips count
//! the processor's cycles (the 2600's TIA and RIOT) sees each access in the cycle the chip makes it. All 256
//! opcodes run, the undocumented ones as the NMOS chip runs them; the twelve that jam it stop it.
//!
//! How each opcode addresses its operand comes from the timing table (`m6502_timing::OPS`), so the two can't
//! disagree.

use crate::asm::m6502::Mode;

pub trait Bus {
    /// One cycle reading.
    fn read(&mut self, address: u16) -> u8;
    /// One cycle writing.
    fn write(&mut self, address: u16, value: u8);
}

#[derive(Default, Debug, Clone)]
pub struct Cpu {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub s: u8,
    pub pc: u16,
    pub c: bool,
    pub z: bool,
    pub i: bool,
    pub d: bool,
    pub v: bool,
    pub n: bool,
    /// Stopped by one of the opcodes that jam the chip, at the address after it; only a reset starts it again.
    pub jammed: bool,
}

/// What an instruction does, as the timing table names it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Adc, And, Asl, Bit, Branch(u8), Brk, Clc, Cld, Cli, Clv, Cmp, Cpx, Cpy, Dec, Dex, Dey, Eor, Inc, Inx, Iny, Jmp, Jsr, Lda, Ldx, Ldy, Lsr, Nop, Ora, Pha, Php, Pla, Plp, Rol, Ror, Rti, Rts, Sbc, Sec, Sed, Sei, Sta, Stx, Sty, Tax, Tay, Tsx, Txa, Txs, Tya,
    // The undocumented ones.
    Slo, Rla, Sre, Rra, Sax, Lax, Dcp, Isc, Anc, Alr, Arr, Ane, Lxa, Sbx, Las, Tas, Sha, Shx, Shy, Jam,
}

/// How an instruction touches its operand, which decides the dummy cycles it makes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
    Read,
    Write,
    Modify,
}

fn kind_of(op: u8) -> Kind {
    use Kind::*;
    match crate::m6502_timing::OPS[usize::from(op)].name {
        "adc" => Adc, "and" => And, "asl" => Asl, "bit" => Bit, "brk" => Brk, "clc" => Clc, "cld" => Cld, "cli" => Cli, "clv" => Clv,
        "cmp" => Cmp, "cpx" => Cpx, "cpy" => Cpy, "dec" => Dec, "dex" => Dex, "dey" => Dey, "eor" => Eor, "inc" => Inc, "inx" => Inx,
        "iny" => Iny, "jmp" => Jmp, "jsr" => Jsr, "lda" => Lda, "ldx" => Ldx, "ldy" => Ldy, "lsr" => Lsr, "nop" => Nop, "ora" => Ora,
        "pha" => Pha, "php" => Php, "pla" => Pla, "plp" => Plp, "rol" => Rol, "ror" => Ror, "rti" => Rti, "rts" => Rts, "sbc" => Sbc,
        "sec" => Sec, "sed" => Sed, "sei" => Sei, "sta" => Sta, "stx" => Stx, "sty" => Sty, "tax" => Tax, "tay" => Tay, "tsx" => Tsx,
        "txa" => Txa, "txs" => Txs, "tya" => Tya, "slo" => Slo, "rla" => Rla, "sre" => Sre, "rra" => Rra, "sax" => Sax, "lax" => Lax,
        "dcp" => Dcp, "isc" => Isc, "anc" => Anc, "alr" => Alr, "arr" => Arr, "ane" => Ane, "lxa" => Lxa, "sbx" => Sbx, "las" => Las,
        "tas" => Tas, "sha" => Sha, "shx" => Shx, "shy" => Shy,
        "bpl" | "bmi" | "bvc" | "bvs" | "bcc" | "bcs" | "bne" | "beq" => Branch(op),
        _ => Jam,
    }
}

/// Each opcode's kind and addressing, worked out once.
fn table() -> &'static [(Kind, Mode); 256] {
    static TABLE: std::sync::OnceLock<[(Kind, Mode); 256]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| std::array::from_fn(|op| (kind_of(op as u8), crate::m6502_timing::OPS[op].mode)))
}

/// The bus, counting the cycles an instruction takes as it goes.
struct Counted<'a, B: Bus> {
    bus: &'a mut B,
    cycles: u32,
}

impl<B: Bus> Counted<'_, B> {
    fn read(&mut self, address: u16) -> u8 {
        self.cycles += 1;
        self.bus.read(address)
    }

    fn write(&mut self, address: u16, value: u8) {
        self.cycles += 1;
        self.bus.write(address, value);
    }
}

/// The value ANE and LXA mix into A: it varies from chip to chip; this is the commonest.
const MAGIC: u8 = 0xEE;

impl Cpu {
    pub fn status(&self, b: bool) -> u8 {
        u8::from(self.c) | u8::from(self.z) << 1 | u8::from(self.i) << 2 | u8::from(self.d) << 3 | u8::from(b) << 4 | 0x20 | u8::from(self.v) << 6 | u8::from(self.n) << 7
    }

    fn set_status(&mut self, p: u8) {
        self.c = p & 1 != 0;
        self.z = p & 2 != 0;
        self.i = p & 4 != 0;
        self.d = p & 8 != 0;
        self.v = p & 0x40 != 0;
        self.n = p & 0x80 != 0;
    }

    fn nz(&mut self, v: u8) -> u8 {
        self.z = v == 0;
        self.n = v & 0x80 != 0;
        v
    }

    /// A reset: the processor reads its reset vector and starts there, its interrupts off, as the chip does after
    /// the reset line is let go (with the stack pointer moved down three, for the pushes it pretends to make).
    pub fn reset(&mut self, bus: &mut impl Bus) {
        let mut bus = Counted { bus, cycles: 0 };
        let pc = self.pc;
        bus.read(pc);
        bus.read(pc);
        for _ in 0..3 {
            bus.read(0x100 | u16::from(self.s));
            self.s = self.s.wrapping_sub(1);
        }
        let low = bus.read(0xFFFC);
        let high = bus.read(0xFFFD);
        self.pc = u16::from_le_bytes([low, high]);
        self.i = true;
        self.jammed = false;
    }

    fn adc(&mut self, v: u8) {
        if self.d {
            let mut low = (self.a & 15) + (v & 15) + u8::from(self.c);
            let mut high = (self.a >> 4) + (v >> 4);
            if low > 9 {
                low += 6;
                high += 1;
            }
            let binary = self.a.wrapping_add(v).wrapping_add(u8::from(self.c));
            self.z = binary == 0;
            self.n = high & 8 != 0;
            self.v = (!(self.a ^ v) & (self.a ^ (high << 4)) & 0x80) != 0;
            if high > 9 {
                high += 6;
            }
            self.c = high > 15;
            self.a = (high << 4) | (low & 15);
        } else {
            let sum = u16::from(self.a) + u16::from(v) + u16::from(self.c);
            let result = sum as u8;
            self.v = (!(self.a ^ v) & (self.a ^ result) & 0x80) != 0;
            self.c = sum > 0xFF;
            self.a = self.nz(result);
        }
    }

    fn sbc(&mut self, v: u8) {
        if self.d {
            let borrow = i16::from(!self.c);
            let mut low = i16::from(self.a & 15) - i16::from(v & 15) - borrow;
            let mut high = i16::from(self.a >> 4) - i16::from(v >> 4);
            if low < 0 {
                low -= 6;
                high -= 1;
            }
            if high < 0 {
                high -= 6;
            }
            let binary = u16::from(self.a).wrapping_sub(u16::from(v)).wrapping_sub(borrow as u16);
            self.v = ((self.a ^ v) & (self.a ^ binary as u8) & 0x80) != 0;
            self.c = binary < 0x100;
            self.nz(binary as u8);
            self.a = ((high as u8) << 4) | (low as u8 & 15);
        } else {
            self.adc(!v);
        }
    }

    fn compare(&mut self, register: u8, v: u8) {
        self.c = register >= v;
        self.nz(register.wrapping_sub(v));
    }

    /// ASL, LSR, ROL or ROR of a value, setting the carry and N and Z.
    fn shift(&mut self, kind: Kind, v: u8) -> u8 {
        let result = match kind {
            Kind::Asl | Kind::Slo => {
                self.c = v & 0x80 != 0;
                v << 1
            }
            Kind::Lsr | Kind::Sre => {
                self.c = v & 1 != 0;
                v >> 1
            }
            Kind::Rol | Kind::Rla => {
                let carry = u8::from(self.c);
                self.c = v & 0x80 != 0;
                v << 1 | carry
            }
            _ => {
                let carry = u8::from(self.c) << 7;
                self.c = v & 1 != 0;
                v >> 1 | carry
            }
        };
        self.nz(result)
    }

    fn push(&mut self, bus: &mut Counted<impl Bus>, v: u8) {
        bus.write(0x100 | u16::from(self.s), v);
        self.s = self.s.wrapping_sub(1);
    }

    fn pull(&mut self, bus: &mut Counted<impl Bus>) -> u8 {
        self.s = self.s.wrapping_add(1);
        bus.read(0x100 | u16::from(self.s))
    }

    fn fetch(&mut self, bus: &mut Counted<impl Bus>) -> u8 {
        let v = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    /// Where the operand is, after the cycles the chip spends finding it (dummy reads among them). For an indexed
    /// address that crosses a page, a read reads the wrong page first only then; a write or a change always does.
    /// Also the high byte of the base address plus one, which SHA, SHX, SHY and TAS mix into what they store.
    fn address(&mut self, bus: &mut Counted<impl Bus>, mode: Mode, access: Access) -> (u16, u8) {
        let indexed = |bus: &mut Counted<_>, base: u16, index: u8| {
            let to = base.wrapping_add(u16::from(index));
            let wrong = (base & 0xFF00) | (to & 0x00FF);
            if wrong != to || access != Access::Read {
                bus.read(wrong);
            }
            (to, ((base >> 8) as u8).wrapping_add(1))
        };
        match mode {
            Mode::Immediate => {
                let at = self.pc;
                self.pc = self.pc.wrapping_add(1);
                (at, 0)
            }
            Mode::ZeroPage => (u16::from(self.fetch(bus)), 0),
            Mode::ZeroPageX | Mode::ZeroPageY => {
                let base = self.fetch(bus);
                bus.read(u16::from(base));
                let index = if mode == Mode::ZeroPageX { self.x } else { self.y };
                (u16::from(base.wrapping_add(index)), 0)
            }
            Mode::Absolute => {
                let low = self.fetch(bus);
                let high = self.fetch(bus);
                (u16::from_le_bytes([low, high]), high.wrapping_add(1))
            }
            Mode::AbsoluteX | Mode::AbsoluteY => {
                let low = self.fetch(bus);
                let high = self.fetch(bus);
                let index = if mode == Mode::AbsoluteX { self.x } else { self.y };
                indexed(bus, u16::from_le_bytes([low, high]), index)
            }
            Mode::IndirectX => {
                let pointer = self.fetch(bus);
                bus.read(u16::from(pointer));
                let pointer = pointer.wrapping_add(self.x);
                let low = bus.read(u16::from(pointer));
                let high = bus.read(u16::from(pointer.wrapping_add(1)));
                (u16::from_le_bytes([low, high]), high.wrapping_add(1))
            }
            Mode::IndirectY => {
                let pointer = self.fetch(bus);
                let low = bus.read(u16::from(pointer));
                let high = bus.read(u16::from(pointer.wrapping_add(1)));
                indexed(bus, u16::from_le_bytes([low, high]), self.y)
            }
            // Implied, the accumulator, branches and indirect jumps work out their own.
            _ => (0, 0),
        }
    }

    /// Runs one instruction, a bus access a cycle; how many cycles it took.
    pub fn step(&mut self, bus: &mut impl Bus) -> u32 {
        use Kind::*;
        let mut bus = Counted { bus, cycles: 0 };
        let bus = &mut bus;
        if self.jammed {
            // A jammed 6502 keeps the bus busy but does nothing more.
            bus.read(0xFFFF);
            return bus.cycles;
        }
        let op = self.fetch(bus);
        let (kind, mode) = table()[usize::from(op)];
        match kind {
            // Two cycles, the second reading the next byte and letting it go.
            Clc | Cld | Cli | Clv | Sec | Sed | Sei | Dex | Dey | Inx | Iny | Tax | Tay | Tsx | Txa | Txs | Tya => {
                bus.read(self.pc);
                match kind {
                    Clc => self.c = false,
                    Cld => self.d = false,
                    Cli => self.i = false,
                    Clv => self.v = false,
                    Sec => self.c = true,
                    Sed => self.d = true,
                    Sei => self.i = true,
                    Dex => self.x = self.nz(self.x.wrapping_sub(1)),
                    Dey => self.y = self.nz(self.y.wrapping_sub(1)),
                    Inx => self.x = self.nz(self.x.wrapping_add(1)),
                    Iny => self.y = self.nz(self.y.wrapping_add(1)),
                    Tax => self.x = self.nz(self.a),
                    Tay => self.y = self.nz(self.a),
                    Tsx => self.x = self.nz(self.s),
                    Txa => self.a = self.nz(self.x),
                    Txs => self.s = self.x,
                    _ => self.a = self.nz(self.y),
                }
            }
            Asl | Lsr | Rol | Ror if mode == Mode::Accumulator => {
                bus.read(self.pc);
                self.a = self.shift(kind, self.a);
            }
            Nop if mode == Mode::Implied => {
                bus.read(self.pc);
            }
            Branch(op) => {
                let offset = self.fetch(bus) as i8;
                let taken = match op {
                    0x10 => !self.n,
                    0x30 => self.n,
                    0x50 => !self.v,
                    0x70 => self.v,
                    0x90 => !self.c,
                    0xB0 => self.c,
                    0xD0 => !self.z,
                    _ => self.z,
                };
                if taken {
                    bus.read(self.pc);
                    let to = self.pc.wrapping_add(offset as u16);
                    if to & 0xFF00 != self.pc & 0xFF00 {
                        bus.read((self.pc & 0xFF00) | (to & 0x00FF));
                    }
                    self.pc = to;
                }
            }
            Brk => {
                self.fetch(bus);
                self.push(bus, (self.pc >> 8) as u8);
                self.push(bus, self.pc as u8);
                let p = self.status(true);
                self.push(bus, p);
                self.i = true;
                let low = bus.read(0xFFFE);
                let high = bus.read(0xFFFF);
                self.pc = u16::from_le_bytes([low, high]);
            }
            Jsr => {
                let low = self.fetch(bus);
                bus.read(0x100 | u16::from(self.s));
                self.push(bus, (self.pc >> 8) as u8);
                self.push(bus, self.pc as u8);
                let high = bus.read(self.pc);
                self.pc = u16::from_le_bytes([low, high]);
            }
            Rts => {
                bus.read(self.pc);
                bus.read(0x100 | u16::from(self.s));
                let low = self.pull(bus);
                let high = self.pull(bus);
                self.pc = u16::from_le_bytes([low, high]);
                self.fetch(bus);
            }
            Rti => {
                bus.read(self.pc);
                bus.read(0x100 | u16::from(self.s));
                let p = self.pull(bus);
                self.set_status(p);
                let low = self.pull(bus);
                let high = self.pull(bus);
                self.pc = u16::from_le_bytes([low, high]);
            }
            Pha | Php => {
                bus.read(self.pc);
                let v = if kind == Pha { self.a } else { self.status(true) };
                self.push(bus, v);
            }
            Pla | Plp => {
                bus.read(self.pc);
                bus.read(0x100 | u16::from(self.s));
                let v = self.pull(bus);
                if kind == Pla {
                    self.a = self.nz(v);
                } else {
                    self.set_status(v);
                }
            }
            Jmp if mode == Mode::Absolute => {
                let low = self.fetch(bus);
                let high = self.fetch(bus);
                self.pc = u16::from_le_bytes([low, high]);
            }
            Jmp => {
                let low = self.fetch(bus);
                let high = self.fetch(bus);
                let pointer = u16::from_le_bytes([low, high]);
                // The chip's own mistake: the high byte comes from the same page.
                let to_low = bus.read(pointer);
                let to_high = bus.read((pointer & 0xFF00) | (pointer.wrapping_add(1) & 0x00FF));
                self.pc = u16::from_le_bytes([to_low, to_high]);
            }
            Jam => {
                // It reads on, stuck: the bus sees a few more cycles, then nothing changes until a reset.
                bus.read(self.pc);
                self.jammed = true;
            }
            // Those that store a register.
            Sta | Stx | Sty | Sax | Sha | Shx | Shy | Tas => {
                let (at, high) = self.address(bus, mode, Access::Write);
                let (at, v) = match kind {
                    Sta => (at, self.a),
                    Stx => (at, self.x),
                    Sty => (at, self.y),
                    Sax => (at, self.a & self.x),
                    _ => {
                        // These AND what they store with the high byte of the address plus one; where the index
                        // crosses a page, that's the high byte the write goes to as well.
                        let v = match kind {
                            Sha => self.a & self.x & high,
                            Shx => self.x & high,
                            Shy => self.y & high,
                            _ => {
                                self.s = self.a & self.x;
                                self.s & high
                            }
                        };
                        let index = if mode == Mode::AbsoluteX { self.x } else { self.y };
                        let base_low = (at.wrapping_sub(u16::from(index))) & 0xFF;
                        let crossed = base_low + u16::from(index) > 0xFF;
                        (if crossed { (u16::from(v) << 8) | (at & 0xFF) } else { at }, v)
                    }
                };
                bus.write(at, v);
            }
            // Those that read and write back.
            Asl | Lsr | Rol | Ror | Inc | Dec | Slo | Rla | Sre | Rra | Dcp | Isc => {
                let (at, _) = self.address(bus, mode, Access::Modify);
                let v = bus.read(at);
                bus.write(at, v);
                let result = match kind {
                    Inc => self.nz(v.wrapping_add(1)),
                    Dec => self.nz(v.wrapping_sub(1)),
                    Dcp => {
                        let r = v.wrapping_sub(1);
                        self.compare(self.a, r);
                        r
                    }
                    Isc => {
                        let r = v.wrapping_add(1);
                        self.sbc(r);
                        r
                    }
                    _ => {
                        let r = self.shift(kind, v);
                        match kind {
                            Slo => self.a = self.nz(self.a | r),
                            Rla => self.a = self.nz(self.a & r),
                            Sre => self.a = self.nz(self.a ^ r),
                            Rra => self.adc(r),
                            _ => {}
                        }
                        r
                    }
                };
                bus.write(at, result);
            }
            // Those that read.
            _ => {
                let (at, _) = self.address(bus, mode, Access::Read);
                let v = bus.read(at);
                match kind {
                    Adc => self.adc(v),
                    Sbc => self.sbc(v),
                    And => self.a = self.nz(self.a & v),
                    Ora => self.a = self.nz(self.a | v),
                    Eor => self.a = self.nz(self.a ^ v),
                    Cmp => self.compare(self.a, v),
                    Cpx => self.compare(self.x, v),
                    Cpy => self.compare(self.y, v),
                    Bit => {
                        self.z = self.a & v == 0;
                        self.n = v & 0x80 != 0;
                        self.v = v & 0x40 != 0;
                    }
                    Lda => self.a = self.nz(v),
                    Ldx => self.x = self.nz(v),
                    Ldy => self.y = self.nz(v),
                    Lax => {
                        self.a = self.nz(v);
                        self.x = v;
                    }
                    Las => {
                        let r = v & self.s;
                        self.a = self.nz(r);
                        self.x = r;
                        self.s = r;
                    }
                    Anc => {
                        self.a = self.nz(self.a & v);
                        self.c = self.n;
                    }
                    Alr => {
                        let r = self.a & v;
                        self.c = r & 1 != 0;
                        self.a = self.nz(r >> 1);
                    }
                    Arr => self.arr(v),
                    Ane => self.a = self.nz((self.a | MAGIC) & self.x & v),
                    Lxa => {
                        let r = (self.a | MAGIC) & v;
                        self.a = self.nz(r);
                        self.x = r;
                    }
                    Sbx => {
                        let ax = self.a & self.x;
                        self.c = ax >= v;
                        self.x = self.nz(ax.wrapping_sub(v));
                    }
                    // The NOPs that read (and so can switch a cartridge's bank).
                    _ => {}
                }
            }
        }
        bus.cycles
    }

    /// ARR: AND, then ROR, with its own flags (and its own decimal correction).
    fn arr(&mut self, v: u8) {
        let t = self.a & v;
        let r = (t >> 1) | (u8::from(self.c) << 7);
        if self.d {
            self.n = self.c;
            self.z = r == 0;
            self.v = (t ^ r) & 0x40 != 0;
            let mut r = r;
            if (t & 0x0F) + (t & 0x01) > 5 {
                r = (r & 0xF0) | (r.wrapping_add(6) & 0x0F);
            }
            if u16::from(t & 0xF0) + u16::from(t & 0x10) > 0x50 {
                r = r.wrapping_add(0x60);
                self.c = true;
            } else {
                self.c = false;
            }
            self.a = r;
        } else {
            self.a = self.nz(r);
            self.c = r & 0x40 != 0;
            self.v = ((r >> 6) ^ (r >> 5)) & 1 != 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ram(Vec<u8>);
    impl Bus for Ram {
        fn read(&mut self, address: u16) -> u8 {
            self.0[usize::from(address)]
        }
        fn write(&mut self, address: u16, value: u8) {
            self.0[usize::from(address)] = value;
        }
    }

    /// Memory that remembers every access, for the order the chip makes them in.
    struct Traced {
        ram: Vec<u8>,
        log: Vec<(char, u16)>,
    }
    impl Bus for Traced {
        fn read(&mut self, address: u16) -> u8 {
            self.log.push(('r', address));
            self.ram[usize::from(address)]
        }
        fn write(&mut self, address: u16, value: u8) {
            self.log.push(('w', address));
            self.ram[usize::from(address)] = value;
        }
    }

    fn assemble(source: &str) -> Vec<u8> {
        let assembly = crate::asm::assemble(&crate::asm::m6502::M6502, std::path::Path::new("t.a"), &format!("* = $1000\n{source}\n brk\n"), &|_| Err(crate::i18n::tr("no").into()));
        assert!(assembly.ok(), "{:?}", assembly.problems.first().map(|p| &p.message));
        assembly.image(0x1000, assembly.highest().unwrap())
    }

    fn run(source: &str) -> (Cpu, Ram) {
        let image = assemble(source);
        let mut ram = Ram(vec![0; 0x10000]);
        ram.0[0x1000..0x1000 + image.len()].copy_from_slice(&image);
        let mut cpu = Cpu { pc: 0x1000, s: 0xFF, ..Cpu::default() };
        while ram.0[usize::from(cpu.pc)] != 0 {
            cpu.step(&mut ram);
        }
        (cpu, ram)
    }

    #[test]
    fn arithmetic_comes_out_as_the_chip_makes_it() {
        let (cpu, _) = run(" clc\n lda #$50\n adc #$50");
        assert_eq!((cpu.a, cpu.v, cpu.c, cpu.n), (0xA0, true, false, true));
        let (cpu, _) = run(" sec\n lda #$50\n sbc #$f0");
        assert_eq!((cpu.a, cpu.v, cpu.c), (0x60, false, false));
        let (cpu, _) = run(" sed\n clc\n lda #$19\n adc #$28");
        assert_eq!((cpu.a, cpu.c), (0x47, false));
        let (cpu, _) = run(" sed\n sec\n lda #$42\n sbc #$13");
        assert_eq!((cpu.a, cpu.c), (0x29, true));
        let (cpu, ram) = run(" ldx #3\n lda #9\n sta $10,x\n asl $13\n ror $13\n ror $13\n ldy $13");
        assert_eq!((ram.0[0x13], cpu.y, cpu.c), (0x04, 0x04, true), "9, doubled to 18, then halved twice with the carry going round");
    }

    #[test]
    fn subroutines_and_loops_go_where_they_should() {
        let (cpu, _) = run(" ldx #0\n ldy #10\nloop jsr add\n dey\n bne loop\n jmp done\nadd inx\n inx\n rts\ndone nop");
        assert_eq!(cpu.x, 20);
        let (cpu, _) = run(" lda #<target\n sta $20\n lda #>target\n sta $21\n jmp ($20)\n lda #1\ntarget lda #2");
        assert_eq!(cpu.a, 2);
        let (cpu, _) = run(" lda #$34\n pha\n lda #0\n pla");
        assert_eq!(cpu.a, 0x34);
        let (cpu, _) = run(" lda #>back\n pha\n lda #<back\n pha\n php\n rti\n lda #1\nback lda #7");
        assert_eq!(cpu.a, 7, "RTI goes to the address pulled, not one past it");
    }

    #[test]
    fn cycles_count_crossed_pages_and_taken_branches() {
        let mut ram = Ram(vec![0; 0x10000]);
        // LDA $10FF,X with X=1 crosses into $1100; LDA $1000,X doesn't.
        ram.0[0x200..0x206].copy_from_slice(&[0xBD, 0xFF, 0x10, 0xBD, 0x00, 0x10]);
        let mut cpu = Cpu { pc: 0x200, x: 1, ..Cpu::default() };
        assert_eq!(cpu.step(&mut ram), 5);
        assert_eq!(cpu.step(&mut ram), 4);
        // BNE taken within the page, and not taken.
        ram.0[0x300..0x304].copy_from_slice(&[0xD0, 0x00, 0xF0, 0x00]);
        let mut cpu = Cpu { pc: 0x300, z: false, ..Cpu::default() };
        assert_eq!(cpu.step(&mut ram), 3);
        assert_eq!(cpu.step(&mut ram), 2);
        // A branch taken back over a page's edge takes four.
        ram.0[0x400..0x402].copy_from_slice(&[0xD0, 0xF0]);
        let mut cpu = Cpu { pc: 0x400, z: false, ..Cpu::default() };
        assert_eq!(cpu.step(&mut ram), 4);
        assert_eq!(cpu.pc, 0x3F2);
    }

    /// Every opcode takes the cycles the timing table gives it: with no page crossed, and with one.
    #[test]
    fn every_opcode_takes_the_cycles_the_table_says() {
        use crate::m6502_timing::{Extra, OPS};
        for op in 0..=255u8 {
            let entry = OPS[usize::from(op)];
            if entry.cycles == 0 {
                let mut ram = Ram(vec![op; 0x10000]);
                let mut cpu = Cpu { pc: 0x200, ..Cpu::default() };
                cpu.step(&mut ram);
                assert!(cpu.jammed, "${op:02X} jams");
                continue;
            }
            for crossing in [false, true] {
                let mut ram = Ram(vec![0; 0x10000]);
                // The operand: zero page $F0, or $20F0. An index of $20 crosses a page from there, of 1 doesn't;
                // a branch's offset of -16 goes back over the page's start, of 0 doesn't.
                ram.0[0x200..0x203].copy_from_slice(&[op, if crossing || entry.extra != Extra::Branch { 0xF0 } else { 0x00 }, 0x20]);
                let index: u8 = if crossing { 0x20 } else { 0x01 };
                // (zp),Y's pointer at $F0, its low byte making Y cross or not; (zp,X)'s at $F0 + X.
                ram.0[0xF0] = if crossing { 0xF0 } else { 0x00 };
                ram.0[0xF1] = 0x20;
                let mut cpu = Cpu { pc: 0x200, x: index, y: index, s: 0xFD, ..Cpu::default() };
                // Each branch taken.
                cpu.n = op == 0x30;
                cpu.v = op == 0x70;
                cpu.c = op == 0xB0;
                cpu.z = op == 0xF0;
                let low = if entry.mode == Mode::IndirectY { ram.0[0xF0] } else { 0xF0 };
                let expected = u32::from(entry.cycles)
                    + match entry.extra {
                        Extra::None => 0,
                        Extra::Page => u32::from(u16::from(low) + u16::from(index) > 0xFF),
                        Extra::Branch => 1 + u32::from(crossing),
                    };
                assert_eq!(cpu.step(&mut ram), expected, "${op:02X} {} {:?} crossing {crossing}", entry.name, entry.mode);
            }
        }
    }

    #[test]
    fn each_cycle_touches_the_bus_as_the_chip_does() {
        let trace = |bytes: &[u8], x: u8| {
            let mut bus = Traced { ram: vec![0; 0x10000], log: Vec::new() };
            bus.ram[0x200..0x200 + bytes.len()].copy_from_slice(bytes);
            let mut cpu = Cpu { pc: 0x200, x, y: x, s: 0xFD, ..Cpu::default() };
            cpu.step(&mut bus);
            bus.log
        };
        // STA $10F0,X with X=$20: the wrong page read first, then the write.
        assert_eq!(trace(&[0x9D, 0xF0, 0x10], 0x20), vec![('r', 0x200), ('r', 0x201), ('r', 0x202), ('r', 0x1010), ('w', 0x1110)]);
        // LDA $1000,X without a page crossed reads once.
        assert_eq!(trace(&[0xBD, 0x00, 0x10], 0x01), vec![('r', 0x200), ('r', 0x201), ('r', 0x202), ('r', 0x1001)]);
        // INC $80: read, the old value written back, then the new.
        assert_eq!(trace(&[0xE6, 0x80], 0), vec![('r', 0x200), ('r', 0x201), ('r', 0x80), ('w', 0x80), ('w', 0x80)]);
        // JSR $1234: the stack read, both halves of the return pushed, then the high byte.
        assert_eq!(trace(&[0x20, 0x34, 0x12], 0), vec![('r', 0x200), ('r', 0x201), ('r', 0x1FD), ('w', 0x1FD), ('w', 0x1FC), ('r', 0x202)]);
        // STA $02,X: zero page wraps, after reading the base.
        assert_eq!(trace(&[0x95, 0xF0], 0x20), vec![('r', 0x200), ('r', 0x201), ('r', 0xF0), ('w', 0x10)]);
        // A NOP with an absolute address reads it: the way some cartridges switch banks.
        assert_eq!(trace(&[0x0C, 0xF8, 0x1F], 0), vec![('r', 0x200), ('r', 0x201), ('r', 0x202), ('r', 0x1FF8)]);
    }

    #[test]
    fn the_undocumented_opcodes_do_what_the_chip_does() {
        let (cpu, ram) = run(" lda #$f0\n ldx #$3c\n byte $87, $20"); // SAX $20
        assert_eq!((ram.0[0x20], cpu.a, cpu.x), (0x30, 0xF0, 0x3C));
        let (cpu, _) = run(" lda #$5a\n sta $21\n byte $a7, $21"); // LAX $21
        assert_eq!((cpu.a, cpu.x), (0x5A, 0x5A));
        let (cpu, ram) = run(" lda #$10\n sta $22\n lda #$0f\n byte $c7, $22"); // DCP $22
        assert_eq!((ram.0[0x22], cpu.z, cpu.c), (0x0F, true, true));
        let (cpu, ram) = run(" lda #$40\n sta $23\n lda #$10\n sec\n byte $e7, $23"); // ISC $23
        assert_eq!((ram.0[0x23], cpu.a), (0x41, 0xCF));
        let (cpu, ram) = run(" lda #$81\n sta $24\n lda #$01\n byte $07, $24"); // SLO $24
        assert_eq!((ram.0[0x24], cpu.a, cpu.c), (0x02, 0x03, true));
        let (cpu, _) = run(" lda #$ff\n ldx #$0f\n byte $cb, $05"); // SBX #5
        assert_eq!((cpu.x, cpu.c), (0x0A, true));
        let (cpu, _) = run(" lda #$ff\n byte $4b, $03"); // ALR #3
        assert_eq!((cpu.a, cpu.c), (0x01, true));
        let (cpu, _) = run(" lda #$80\n byte $0b, $ff"); // ANC #$FF
        assert_eq!((cpu.a, cpu.c, cpu.n), (0x80, true, true));
        let (cpu, _) = run(" sec\n lda #$ff\n byte $6b, $c0"); // ARR #$C0
        assert_eq!((cpu.a, cpu.c, cpu.v), (0xE0, true, false));
        let (cpu, _) = run(" lda #$0a\n sec\n byte $eb, $03"); // SBC #3, its other opcode
        assert_eq!(cpu.a, 0x07);
    }
}
