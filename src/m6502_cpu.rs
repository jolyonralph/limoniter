//! A 6502 for tests: every documented instruction, with its cycles (one more for a page crossed where the chip
//! takes one, and for a branch taken, two if it crosses a page). Memory and what's at each address are the bus's.

pub trait Bus {
    fn read(&mut self, address: u16) -> u8;
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
}

#[derive(Clone, Copy)]
enum Mode {
    Implied,
    Immediate,
    Zero,
    ZeroX,
    ZeroY,
    Absolute,
    AbsoluteX,
    AbsoluteY,
    IndirectX,
    IndirectY,
    Relative,
    Indirect,
}

impl Cpu {
    fn status(&self, b: bool) -> u8 {
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

    fn push(&mut self, bus: &mut impl Bus, v: u8) {
        bus.write(0x100 | u16::from(self.s), v);
        self.s = self.s.wrapping_sub(1);
    }

    fn pull(&mut self, bus: &mut impl Bus) -> u8 {
        self.s = self.s.wrapping_add(1);
        bus.read(0x100 | u16::from(self.s))
    }

    fn word(bus: &mut impl Bus, at: u16) -> u16 {
        u16::from(bus.read(at)) | u16::from(bus.read(at.wrapping_add(1))) << 8
    }

    fn zero_word(bus: &mut impl Bus, at: u8) -> u16 {
        u16::from(bus.read(u16::from(at))) | u16::from(bus.read(u16::from(at.wrapping_add(1)))) << 8
    }

    /// Where the operand is, and whether a page was crossed reaching it.
    fn address(&mut self, bus: &mut impl Bus, mode: Mode) -> (u16, bool) {
        let at = self.pc;
        let crossed = |base: u16, to: u16| base & 0xFF00 != to & 0xFF00;
        match mode {
            Mode::Implied | Mode::Relative => (0, false),
            Mode::Immediate => {
                self.pc = at.wrapping_add(1);
                (at, false)
            }
            Mode::Zero | Mode::ZeroX | Mode::ZeroY => {
                self.pc = at.wrapping_add(1);
                let base = bus.read(at);
                let index = match mode {
                    Mode::ZeroX => self.x,
                    Mode::ZeroY => self.y,
                    _ => 0,
                };
                (u16::from(base.wrapping_add(index)), false)
            }
            Mode::Absolute | Mode::AbsoluteX | Mode::AbsoluteY => {
                self.pc = at.wrapping_add(2);
                let base = Self::word(bus, at);
                let index = match mode {
                    Mode::AbsoluteX => self.x,
                    Mode::AbsoluteY => self.y,
                    _ => 0,
                };
                let to = base.wrapping_add(u16::from(index));
                (to, crossed(base, to))
            }
            Mode::IndirectX => {
                self.pc = at.wrapping_add(1);
                let zero = bus.read(at).wrapping_add(self.x);
                (Self::zero_word(bus, zero), false)
            }
            Mode::IndirectY => {
                self.pc = at.wrapping_add(1);
                let zero = bus.read(at);
                let base = Self::zero_word(bus, zero);
                let to = base.wrapping_add(u16::from(self.y));
                (to, crossed(base, to))
            }
            Mode::Indirect => {
                self.pc = at.wrapping_add(2);
                let pointer = Self::word(bus, at);
                // The chip's own mistake: the high byte comes from the same page.
                let high = (pointer & 0xFF00) | (pointer.wrapping_add(1) & 0x00FF);
                (u16::from(bus.read(pointer)) | u16::from(bus.read(high)) << 8, false)
            }
        }
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

    /// Runs one instruction; its cycles.
    pub fn step(&mut self, bus: &mut impl Bus) -> u32 {
        use Mode::*;
        let op = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        // The mode, the cycles, and whether a page crossed costs one more.
        let (mode, cycles, extra): (Mode, u32, bool) = match op {
            0x69 | 0x29 | 0xC9 | 0xE0 | 0xC0 | 0x49 | 0xA9 | 0xA2 | 0xA0 | 0x09 | 0xE9 => (Immediate, 2, false),
            0x65 | 0x25 | 0x24 | 0xC5 | 0xE4 | 0xC4 | 0x45 | 0xA5 | 0xA6 | 0xA4 | 0x05 | 0xE5 | 0x85 | 0x86 | 0x84 => (Zero, 3, false),
            0x06 | 0xC6 | 0xE6 | 0x46 | 0x26 | 0x66 => (Zero, 5, false),
            0x75 | 0x35 | 0xD5 | 0x55 | 0xB5 | 0xB4 | 0x15 | 0xF5 | 0x95 | 0x94 => (ZeroX, 4, false),
            0x16 | 0xD6 | 0xF6 | 0x56 | 0x36 | 0x76 => (ZeroX, 6, false),
            0xB6 | 0x96 => (ZeroY, 4, false),
            0x6D | 0x2D | 0x2C | 0xCD | 0xEC | 0xCC | 0x4D | 0xAD | 0xAE | 0xAC | 0x0D | 0xED | 0x8D | 0x8E | 0x8C => (Absolute, 4, false),
            0x0E | 0xCE | 0xEE | 0x4E | 0x2E | 0x6E => (Absolute, 6, false),
            0x4C => (Absolute, 3, false),
            0x20 => (Absolute, 6, false),
            0x7D | 0x3D | 0xDD | 0x5D | 0xBD | 0xBC | 0x1D | 0xFD => (AbsoluteX, 4, true),
            0x9D => (AbsoluteX, 5, false),
            0x1E | 0xDE | 0xFE | 0x5E | 0x3E | 0x7E => (AbsoluteX, 7, false),
            0x79 | 0x39 | 0xD9 | 0x59 | 0xB9 | 0xBE | 0x19 | 0xF9 => (AbsoluteY, 4, true),
            0x99 => (AbsoluteY, 5, false),
            0x61 | 0x21 | 0xC1 | 0x41 | 0xA1 | 0x01 | 0xE1 | 0x81 => (IndirectX, 6, false),
            0x71 | 0x31 | 0xD1 | 0x51 | 0xB1 | 0x11 | 0xF1 => (IndirectY, 5, true),
            0x91 => (IndirectY, 6, false),
            0x6C => (Indirect, 5, false),
            0x10 | 0x30 | 0x50 | 0x70 | 0x90 | 0xB0 | 0xD0 | 0xF0 => (Relative, 2, false),
            0x0A | 0x4A | 0x2A | 0x6A | 0x18 | 0x38 | 0x58 | 0x78 | 0xB8 | 0xD8 | 0xF8 | 0xCA | 0x88 | 0xE8 | 0xC8 | 0xEA | 0xAA | 0x8A | 0xA8 | 0x98 | 0xBA | 0x9A => (Implied, 2, false),
            0x48 | 0x08 => (Implied, 3, false),
            0x68 | 0x28 => (Implied, 4, false),
            0x40 | 0x60 => (Implied, 6, false),
            0x00 => (Implied, 7, false),
            _ => panic!("${op:02X} at ${:04X} isn't a documented 6502 instruction", self.pc.wrapping_sub(1)),
        };
        let (at, crossed) = self.address(bus, mode);
        let mut cycles = cycles + u32::from(extra && crossed);
        match op {
            // Loads, stores and transfers.
            0xA9 | 0xA5 | 0xB5 | 0xAD | 0xBD | 0xB9 | 0xA1 | 0xB1 => {
                let v = bus.read(at);
                self.a = self.nz(v);
            }
            0xA2 | 0xA6 | 0xB6 | 0xAE | 0xBE => {
                let v = bus.read(at);
                self.x = self.nz(v);
            }
            0xA0 | 0xA4 | 0xB4 | 0xAC | 0xBC => {
                let v = bus.read(at);
                self.y = self.nz(v);
            }
            0x85 | 0x95 | 0x8D | 0x9D | 0x99 | 0x81 | 0x91 => bus.write(at, self.a),
            0x86 | 0x96 | 0x8E => bus.write(at, self.x),
            0x84 | 0x94 | 0x8C => bus.write(at, self.y),
            0xAA => self.x = self.nz(self.a),
            0x8A => self.a = self.nz(self.x),
            0xA8 => self.y = self.nz(self.a),
            0x98 => self.a = self.nz(self.y),
            0xBA => self.x = self.nz(self.s),
            0x9A => self.s = self.x,
            // Arithmetic and logic.
            0x69 | 0x65 | 0x75 | 0x6D | 0x7D | 0x79 | 0x61 | 0x71 => {
                let v = bus.read(at);
                self.adc(v);
            }
            0xE9 | 0xE5 | 0xF5 | 0xED | 0xFD | 0xF9 | 0xE1 | 0xF1 => {
                let v = bus.read(at);
                self.sbc(v);
            }
            0x29 | 0x25 | 0x35 | 0x2D | 0x3D | 0x39 | 0x21 | 0x31 => {
                let v = bus.read(at);
                self.a = self.nz(self.a & v);
            }
            0x09 | 0x05 | 0x15 | 0x0D | 0x1D | 0x19 | 0x01 | 0x11 => {
                let v = bus.read(at);
                self.a = self.nz(self.a | v);
            }
            0x49 | 0x45 | 0x55 | 0x4D | 0x5D | 0x59 | 0x41 | 0x51 => {
                let v = bus.read(at);
                self.a = self.nz(self.a ^ v);
            }
            0xC9 | 0xC5 | 0xD5 | 0xCD | 0xDD | 0xD9 | 0xC1 | 0xD1 => {
                let v = bus.read(at);
                self.compare(self.a, v);
            }
            0xE0 | 0xE4 | 0xEC => {
                let v = bus.read(at);
                self.compare(self.x, v);
            }
            0xC0 | 0xC4 | 0xCC => {
                let v = bus.read(at);
                self.compare(self.y, v);
            }
            0x24 | 0x2C => {
                let v = bus.read(at);
                self.z = self.a & v == 0;
                self.n = v & 0x80 != 0;
                self.v = v & 0x40 != 0;
            }
            // Increments, decrements and shifts, of memory or A.
            0xE6 | 0xF6 | 0xEE | 0xFE | 0xC6 | 0xD6 | 0xCE | 0xDE | 0x06 | 0x16 | 0x0E | 0x1E | 0x46 | 0x56 | 0x4E | 0x5E | 0x26 | 0x36 | 0x2E | 0x3E | 0x66 | 0x76 | 0x6E | 0x7E => {
                let v = bus.read(at);
                let result = self.modify(op, v);
                bus.write(at, result);
            }
            0x0A | 0x4A | 0x2A | 0x6A => {
                self.a = self.modify(op, self.a);
            }
            0xE8 => self.x = self.nz(self.x.wrapping_add(1)),
            0xCA => self.x = self.nz(self.x.wrapping_sub(1)),
            0xC8 => self.y = self.nz(self.y.wrapping_add(1)),
            0x88 => self.y = self.nz(self.y.wrapping_sub(1)),
            // Flags.
            0x18 => self.c = false,
            0x38 => self.c = true,
            0x58 => self.i = false,
            0x78 => self.i = true,
            0xB8 => self.v = false,
            0xD8 => self.d = false,
            0xF8 => self.d = true,
            0xEA => {}
            // The stack.
            0x48 => self.push(bus, self.a),
            0x08 => {
                let p = self.status(true);
                self.push(bus, p);
            }
            0x68 => {
                let v = self.pull(bus);
                self.a = self.nz(v);
            }
            0x28 => {
                let p = self.pull(bus);
                self.set_status(p);
            }
            // Jumps.
            0x4C | 0x6C => self.pc = at,
            0x20 => {
                let back = self.pc.wrapping_sub(1);
                self.push(bus, (back >> 8) as u8);
                self.push(bus, back as u8);
                self.pc = at;
            }
            0x60 => {
                let low = self.pull(bus);
                let high = self.pull(bus);
                self.pc = (u16::from(low) | u16::from(high) << 8).wrapping_add(1);
            }
            0x40 => {
                let p = self.pull(bus);
                self.set_status(p);
                let low = self.pull(bus);
                let high = self.pull(bus);
                self.pc = u16::from(low) | u16::from(high) << 8;
            }
            0x00 => {
                let back = self.pc.wrapping_add(1);
                self.push(bus, (back >> 8) as u8);
                self.push(bus, back as u8);
                let p = self.status(true);
                self.push(bus, p);
                self.i = true;
                self.pc = Self::word(bus, 0xFFFE);
            }
            // Branches.
            _ => {
                let offset = bus.read(self.pc) as i8;
                self.pc = self.pc.wrapping_add(1);
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
                    let to = self.pc.wrapping_add(offset as u16);
                    cycles += if to & 0xFF00 != self.pc & 0xFF00 { 2 } else { 1 };
                    self.pc = to;
                }
            }
        }
        cycles
    }

    /// INC, DEC, ASL, LSR, ROL or ROR (the opcode's low bits pick which) of a value.
    fn modify(&mut self, op: u8, v: u8) -> u8 {
        let result = match op & 0xE0 {
            0xE0 => v.wrapping_add(1),
            0xC0 => v.wrapping_sub(1),
            0x00 => {
                self.c = v & 0x80 != 0;
                v << 1
            }
            0x40 => {
                self.c = v & 1 != 0;
                v >> 1
            }
            0x20 => {
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

    fn run(source: &str) -> (Cpu, Ram) {
        let assembly = crate::asm::assemble(&crate::asm::m6502::M6502, std::path::Path::new("t.a"), &format!("* = $1000\n{source}\n brk\n"), &|_| Err(crate::i18n::tr("no").into()));
        assert!(assembly.ok(), "{:?}", assembly.problems.first().map(|p| &p.message));
        let mut ram = Ram(vec![0; 0x10000]);
        let image = assembly.image(0x1000, assembly.highest().unwrap());
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
    }

    #[test]
    fn cycles_count_crossed_pages_and_taken_branches() {
        struct Ram2(Vec<u8>);
        impl Bus for Ram2 {
            fn read(&mut self, a: u16) -> u8 {
                self.0[usize::from(a)]
            }
            fn write(&mut self, a: u16, v: u8) {
                self.0[usize::from(a)] = v;
            }
        }
        let mut ram = Ram2(vec![0; 0x10000]);
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
    }
}
