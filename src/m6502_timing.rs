//! How long 6502 and 6510 instructions take, all 256 opcodes with the undocumented ones, for timing raster code:
//! the cycles each takes, one more where an indexed address crosses a page, and for a branch, one more taken and
//! another if it lands in a different page. Where the source says enough (a branch's target, an index on an
//! address whose low byte is 0), the extra is known; otherwise a line has a least and a most. A block of lines
//! adds up to a least and a most too.

use crate::asm::m6502::Mode;

/// What may add to an instruction's cycles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extra {
    None,
    /// One more when the indexed address is in the next page.
    Page,
    /// One more when taken, two when taken into another page.
    Branch,
}

/// One opcode: its name as ACME writes it, how it addresses, and its cycles. JAM's cycles are 0: it stops the
/// processor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Op {
    pub name: &'static str,
    pub mode: Mode,
    pub cycles: u8,
    pub extra: Extra,
}

use Extra::{Branch as B, None as N, Page as P};
use Mode::*;

const fn op(name: &'static str, mode: Mode, cycles: u8, extra: Extra) -> Op {
    Op { name, mode, cycles, extra }
}

const JAM: Op = op("jam", Implied, 0, N);

/// Every opcode, $00 to $FF, as the NMOS 6502 and the C64's 6510 run them.
pub const OPS: [Op; 256] = [
    // $00
    op("brk", Implied, 7, N), op("ora", IndirectX, 6, N), JAM, op("slo", IndirectX, 8, N),
    op("nop", ZeroPage, 3, N), op("ora", ZeroPage, 3, N), op("asl", ZeroPage, 5, N), op("slo", ZeroPage, 5, N),
    op("php", Implied, 3, N), op("ora", Immediate, 2, N), op("asl", Accumulator, 2, N), op("anc", Immediate, 2, N),
    op("nop", Absolute, 4, N), op("ora", Absolute, 4, N), op("asl", Absolute, 6, N), op("slo", Absolute, 6, N),
    // $10
    op("bpl", Relative, 2, B), op("ora", IndirectY, 5, P), JAM, op("slo", IndirectY, 8, N),
    op("nop", ZeroPageX, 4, N), op("ora", ZeroPageX, 4, N), op("asl", ZeroPageX, 6, N), op("slo", ZeroPageX, 6, N),
    op("clc", Implied, 2, N), op("ora", AbsoluteY, 4, P), op("nop", Implied, 2, N), op("slo", AbsoluteY, 7, N),
    op("nop", AbsoluteX, 4, P), op("ora", AbsoluteX, 4, P), op("asl", AbsoluteX, 7, N), op("slo", AbsoluteX, 7, N),
    // $20
    op("jsr", Absolute, 6, N), op("and", IndirectX, 6, N), JAM, op("rla", IndirectX, 8, N),
    op("bit", ZeroPage, 3, N), op("and", ZeroPage, 3, N), op("rol", ZeroPage, 5, N), op("rla", ZeroPage, 5, N),
    op("plp", Implied, 4, N), op("and", Immediate, 2, N), op("rol", Accumulator, 2, N), op("anc", Immediate, 2, N),
    op("bit", Absolute, 4, N), op("and", Absolute, 4, N), op("rol", Absolute, 6, N), op("rla", Absolute, 6, N),
    // $30
    op("bmi", Relative, 2, B), op("and", IndirectY, 5, P), JAM, op("rla", IndirectY, 8, N),
    op("nop", ZeroPageX, 4, N), op("and", ZeroPageX, 4, N), op("rol", ZeroPageX, 6, N), op("rla", ZeroPageX, 6, N),
    op("sec", Implied, 2, N), op("and", AbsoluteY, 4, P), op("nop", Implied, 2, N), op("rla", AbsoluteY, 7, N),
    op("nop", AbsoluteX, 4, P), op("and", AbsoluteX, 4, P), op("rol", AbsoluteX, 7, N), op("rla", AbsoluteX, 7, N),
    // $40
    op("rti", Implied, 6, N), op("eor", IndirectX, 6, N), JAM, op("sre", IndirectX, 8, N),
    op("nop", ZeroPage, 3, N), op("eor", ZeroPage, 3, N), op("lsr", ZeroPage, 5, N), op("sre", ZeroPage, 5, N),
    op("pha", Implied, 3, N), op("eor", Immediate, 2, N), op("lsr", Accumulator, 2, N), op("alr", Immediate, 2, N),
    op("jmp", Absolute, 3, N), op("eor", Absolute, 4, N), op("lsr", Absolute, 6, N), op("sre", Absolute, 6, N),
    // $50
    op("bvc", Relative, 2, B), op("eor", IndirectY, 5, P), JAM, op("sre", IndirectY, 8, N),
    op("nop", ZeroPageX, 4, N), op("eor", ZeroPageX, 4, N), op("lsr", ZeroPageX, 6, N), op("sre", ZeroPageX, 6, N),
    op("cli", Implied, 2, N), op("eor", AbsoluteY, 4, P), op("nop", Implied, 2, N), op("sre", AbsoluteY, 7, N),
    op("nop", AbsoluteX, 4, P), op("eor", AbsoluteX, 4, P), op("lsr", AbsoluteX, 7, N), op("sre", AbsoluteX, 7, N),
    // $60
    op("rts", Implied, 6, N), op("adc", IndirectX, 6, N), JAM, op("rra", IndirectX, 8, N),
    op("nop", ZeroPage, 3, N), op("adc", ZeroPage, 3, N), op("ror", ZeroPage, 5, N), op("rra", ZeroPage, 5, N),
    op("pla", Implied, 4, N), op("adc", Immediate, 2, N), op("ror", Accumulator, 2, N), op("arr", Immediate, 2, N),
    op("jmp", Indirect, 5, N), op("adc", Absolute, 4, N), op("ror", Absolute, 6, N), op("rra", Absolute, 6, N),
    // $70
    op("bvs", Relative, 2, B), op("adc", IndirectY, 5, P), JAM, op("rra", IndirectY, 8, N),
    op("nop", ZeroPageX, 4, N), op("adc", ZeroPageX, 4, N), op("ror", ZeroPageX, 6, N), op("rra", ZeroPageX, 6, N),
    op("sei", Implied, 2, N), op("adc", AbsoluteY, 4, P), op("nop", Implied, 2, N), op("rra", AbsoluteY, 7, N),
    op("nop", AbsoluteX, 4, P), op("adc", AbsoluteX, 4, P), op("ror", AbsoluteX, 7, N), op("rra", AbsoluteX, 7, N),
    // $80
    op("nop", Immediate, 2, N), op("sta", IndirectX, 6, N), op("nop", Immediate, 2, N), op("sax", IndirectX, 6, N),
    op("sty", ZeroPage, 3, N), op("sta", ZeroPage, 3, N), op("stx", ZeroPage, 3, N), op("sax", ZeroPage, 3, N),
    op("dey", Implied, 2, N), op("nop", Immediate, 2, N), op("txa", Implied, 2, N), op("ane", Immediate, 2, N),
    op("sty", Absolute, 4, N), op("sta", Absolute, 4, N), op("stx", Absolute, 4, N), op("sax", Absolute, 4, N),
    // $90
    op("bcc", Relative, 2, B), op("sta", IndirectY, 6, N), JAM, op("sha", IndirectY, 6, N),
    op("sty", ZeroPageX, 4, N), op("sta", ZeroPageX, 4, N), op("stx", ZeroPageY, 4, N), op("sax", ZeroPageY, 4, N),
    op("tya", Implied, 2, N), op("sta", AbsoluteY, 5, N), op("txs", Implied, 2, N), op("tas", AbsoluteY, 5, N),
    op("shy", AbsoluteX, 5, N), op("sta", AbsoluteX, 5, N), op("shx", AbsoluteY, 5, N), op("sha", AbsoluteY, 5, N),
    // $A0
    op("ldy", Immediate, 2, N), op("lda", IndirectX, 6, N), op("ldx", Immediate, 2, N), op("lax", IndirectX, 6, N),
    op("ldy", ZeroPage, 3, N), op("lda", ZeroPage, 3, N), op("ldx", ZeroPage, 3, N), op("lax", ZeroPage, 3, N),
    op("tay", Implied, 2, N), op("lda", Immediate, 2, N), op("tax", Implied, 2, N), op("lxa", Immediate, 2, N),
    op("ldy", Absolute, 4, N), op("lda", Absolute, 4, N), op("ldx", Absolute, 4, N), op("lax", Absolute, 4, N),
    // $B0
    op("bcs", Relative, 2, B), op("lda", IndirectY, 5, P), JAM, op("lax", IndirectY, 5, P),
    op("ldy", ZeroPageX, 4, N), op("lda", ZeroPageX, 4, N), op("ldx", ZeroPageY, 4, N), op("lax", ZeroPageY, 4, N),
    op("clv", Implied, 2, N), op("lda", AbsoluteY, 4, P), op("tsx", Implied, 2, N), op("las", AbsoluteY, 4, P),
    op("ldy", AbsoluteX, 4, P), op("lda", AbsoluteX, 4, P), op("ldx", AbsoluteY, 4, P), op("lax", AbsoluteY, 4, P),
    // $C0
    op("cpy", Immediate, 2, N), op("cmp", IndirectX, 6, N), op("nop", Immediate, 2, N), op("dcp", IndirectX, 8, N),
    op("cpy", ZeroPage, 3, N), op("cmp", ZeroPage, 3, N), op("dec", ZeroPage, 5, N), op("dcp", ZeroPage, 5, N),
    op("iny", Implied, 2, N), op("cmp", Immediate, 2, N), op("dex", Implied, 2, N), op("sbx", Immediate, 2, N),
    op("cpy", Absolute, 4, N), op("cmp", Absolute, 4, N), op("dec", Absolute, 6, N), op("dcp", Absolute, 6, N),
    // $D0
    op("bne", Relative, 2, B), op("cmp", IndirectY, 5, P), JAM, op("dcp", IndirectY, 8, N),
    op("nop", ZeroPageX, 4, N), op("cmp", ZeroPageX, 4, N), op("dec", ZeroPageX, 6, N), op("dcp", ZeroPageX, 6, N),
    op("cld", Implied, 2, N), op("cmp", AbsoluteY, 4, P), op("nop", Implied, 2, N), op("dcp", AbsoluteY, 7, N),
    op("nop", AbsoluteX, 4, P), op("cmp", AbsoluteX, 4, P), op("dec", AbsoluteX, 7, N), op("dcp", AbsoluteX, 7, N),
    // $E0
    op("cpx", Immediate, 2, N), op("sbc", IndirectX, 6, N), op("nop", Immediate, 2, N), op("isc", IndirectX, 8, N),
    op("cpx", ZeroPage, 3, N), op("sbc", ZeroPage, 3, N), op("inc", ZeroPage, 5, N), op("isc", ZeroPage, 5, N),
    op("inx", Implied, 2, N), op("sbc", Immediate, 2, N), op("nop", Implied, 2, N), op("sbc", Immediate, 2, N),
    op("cpx", Absolute, 4, N), op("sbc", Absolute, 4, N), op("inc", Absolute, 6, N), op("isc", Absolute, 6, N),
    // $F0
    op("beq", Relative, 2, B), op("sbc", IndirectY, 5, P), JAM, op("isc", IndirectY, 8, N),
    op("nop", ZeroPageX, 4, N), op("sbc", ZeroPageX, 4, N), op("inc", ZeroPageX, 6, N), op("isc", ZeroPageX, 6, N),
    op("sed", Implied, 2, N), op("sbc", AbsoluteY, 4, P), op("nop", Implied, 2, N), op("isc", AbsoluteY, 7, N),
    op("nop", AbsoluteX, 4, P), op("sbc", AbsoluteX, 4, P), op("inc", AbsoluteX, 7, N), op("isc", AbsoluteX, 7, N),
];

/// The least and most cycles an instruction can take, as far as can be told from its bytes and where it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Cycles {
    pub least: u32,
    pub most: u32,
}

impl Cycles {
    pub fn add(self, other: Cycles) -> Cycles {
        Cycles { least: self.least + other.least, most: self.most + other.most }
    }

    /// "4", or "4/5" where it may take one more.
    pub fn text(self) -> String {
        if self.least == self.most { self.least.to_string() } else { format!("{}/{}", self.least, self.most) }
    }

    /// "12" or "12-15", for a total.
    pub fn range(self) -> String {
        if self.least == self.most { self.least.to_string() } else { format!("{}\u{2013}{}", self.least, self.most) }
    }
}

/// An instruction's cycles: from its bytes, and its address, if known, for a branch's page. None for JAM, which
/// never finishes.
pub fn cycles(bytes: &[u8], address: Option<i64>) -> Option<Cycles> {
    let op = OPS[*bytes.first()? as usize];
    if op.cycles == 0 {
        return None;
    }
    let base = op.cycles as u32;
    let most = match op.extra {
        Extra::None => base,
        // An index added to an address whose low byte is 0 can't carry into the next page.
        Extra::Page if matches!(op.mode, AbsoluteX | AbsoluteY) && bytes.get(1) == Some(&0) => base,
        Extra::Page => base + 1,
        Extra::Branch => match (address, bytes.get(1)) {
            (Some(at), Some(offset)) => {
                let next = at + 2;
                let target = next + *offset as i8 as i64;
                if (next & 0xFF00) == (target & 0xFF00) { base + 1 } else { base + 2 }
            }
            _ => base + 2,
        },
    };
    Some(Cycles { least: base, most })
}

/// The words for a line: "4/5 cycles (one more if the index crosses a page)".
pub fn explain(bytes: &[u8], address: Option<i64>) -> Option<String> {
    let op = OPS[*bytes.first()? as usize];
    let Some(c) = cycles(bytes, address) else {
        return Some("JAM: stops the processor".into());
    };
    let why = match op.extra {
        Extra::Page if c.most > c.least => " (one more if the index crosses a page)",
        Extra::Branch if c.most == c.least + 1 => " (one more when taken)",
        Extra::Branch if address.is_some() => " (2 more when taken: it lands in another page)",
        Extra::Branch => " (one more when taken, 2 into another page)",
        _ => "",
    };
    Some(format!("{} cycles{why}", c.text()))
}

/// The cycles of the lines from `first` to `last` of a file of an assembly: the least and the most, and how many
/// instructions there were. Lines of data (anything after a JAM, or bytes that aren't an instruction's length)
/// count only where they're whole instructions.
pub fn block(assembly: &crate::asm::Assembly, file: usize, first: usize, last: usize) -> (Cycles, usize) {
    let mut total = Cycles::default();
    let mut count = 0;
    for placed in assembly.lines.iter().filter(|p| p.file == file && p.line >= first && p.line <= last && !p.bytes.is_empty()) {
        if !is_instruction(&placed.bytes) {
            continue;
        }
        if let Some(c) = cycles(&placed.bytes, Some(placed.address)) {
            total = total.add(c);
            count += 1;
        }
    }
    (total, count)
}

/// How many bytes an opcode's instruction has.
pub fn length(op: u8) -> usize {
    match OPS[op as usize].mode {
        Implied | Accumulator => 1,
        Immediate | ZeroPage | ZeroPageX | ZeroPageY | IndirectX | IndirectY | Relative => 2,
        Absolute | AbsoluteX | AbsoluteY | Indirect => 3,
    }
}

/// Whether bytes a line made are one instruction (not a !byte table that happens to start with an opcode).
pub fn is_instruction(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes.len() == length(bytes[0])
}

thread_local! {
    static SHOWN: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Whether each line of 6502 source shows its cycles at the right (Tools, Commodore 64).
pub fn shown() -> bool {
    SHOWN.get()
}

pub fn show(on: bool) {
    SHOWN.set(on);
}

/// Each line's cycles, for the lines from `first` to `last` of a document of 6502 source, from the program as
/// last assembled, while the text is as it was then. A line of several instructions shows their sum.
pub fn line_notes(intel: &crate::code_intel::CodeIntel, version: u64, first: usize, last: usize) -> std::collections::HashMap<usize, String> {
    let mut sums: std::collections::HashMap<usize, Cycles> = std::collections::HashMap::new();
    if !shown() || intel.cpu != Some(crate::assemblers::Cpu::M6502) || intel.version != version {
        return std::collections::HashMap::new();
    }
    let Some(assembly) = &intel.assembly else {
        return std::collections::HashMap::new();
    };
    for placed in assembly.lines.iter().filter(|p| p.file == intel.file && p.line >= first && p.line <= last && is_instruction(&p.bytes)) {
        if let Some(c) = cycles(&placed.bytes, Some(placed.address)) {
            let sum = sums.entry(placed.line).or_default();
            *sum = sum.add(c);
        }
    }
    sums.into_iter().map(|(line, c)| (line, c.text())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asm::{Isa, m6502};
    use std::path::Path;

    /// The table checked against an independent one: cycles and page or branch extra for each opcode, as the
    /// 6502 instruction set reference at masswerk.at lists them (its documented and "illegal" opcode tables; the
    /// twelve JAMs it gives no cycles for). Each entry is opcode, cycles, and + for a page-crossing extra or b for
    /// a branch.
    const REFERENCE: &str = include_str!("asm/testdata/m6502_cycles.txt");

    #[test]
    fn every_opcode_takes_the_cycles_the_reference_gives() {
        let mut checked = 0;
        for line in REFERENCE.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let opcode = u8::from_str_radix(fields[0], 16).unwrap();
            let op = OPS[opcode as usize];
            if fields[1] == "jam" {
                assert_eq!(op, JAM, "${opcode:02X}");
                checked += 1;
                continue;
            }
            let want: u8 = fields[1].parse().unwrap();
            let extra = match fields.get(2) {
                Some(&"+") => Extra::Page,
                Some(&"b") => Extra::Branch,
                _ => Extra::None,
            };
            assert_eq!((op.cycles, op.extra), (want, extra), "${opcode:02X} {}", op.name);
            checked += 1;
        }
        assert_eq!(checked, 256);
    }

    #[test]
    fn the_table_agrees_with_the_assembler() {
        // Each opcode the assembler makes is the instruction and mode the table says.
        for opcode in 0..=255u8 {
            if let Some((name, mode)) = m6502::decode(opcode) {
                let op = OPS[opcode as usize];
                assert_eq!(op.mode, mode, "${opcode:02X}");
                if !matches!(opcode, 0xEB) {
                    assert_eq!(op.name, name, "${opcode:02X}");
                }
            }
        }
    }

    fn assemble(text: &str) -> crate::asm::Assembly {
        let a = crate::asm::assemble(&m6502::M6502, Path::new("t.a"), text, &|p| Err(p.display().to_string()));
        assert!(a.ok(), "{:?}", a.problems);
        a
    }

    #[test]
    fn page_crossing_and_branches_are_counted_where_they_can_be_known() {
        let a = assemble("*=$10f0\n lda $1200,x\n lda $1201,x\n lda ($fb),y\n sta $1201,x\n- bne -\n*=$12fc\n bne +\n nop\n nop\n+ nop\n");
        let at = |n: usize| {
            let p = a.lines.iter().find(|p| p.line == n && !p.bytes.is_empty()).unwrap();
            cycles(&p.bytes, Some(p.address)).unwrap().text()
        };
        assert_eq!(at(1), "4");
        assert_eq!(at(2), "4/5");
        assert_eq!(at(3), "5/6");
        assert_eq!(at(4), "5");
        assert_eq!(at(5), "2/3");
        // At $12FC a branch's next instruction is at $12FE, and its target, $1300, is in the next page.
        assert_eq!(at(7), "2/4");
        assert_eq!(cycles(&[0x02], None), None);
        assert_eq!(explain(&[0xD0, 0x00], None).unwrap(), "2/4 cycles (one more when taken, 2 into another page)");
    }

    #[test]
    fn a_block_adds_up() {
        // A raster wait loop's body, a delay loop and the line's writes.
        let a = assemble("*=$c000\n ldx #9\n- dex\n bne -\n lda $d012\n sta $d020\n inc $d019\n");
        let (sum, count) = block(&a, 0, 1, 6);
        assert_eq!(count, 6);
        assert_eq!(sum, Cycles { least: 2 + 2 + 2 + 4 + 4 + 6, most: 2 + 2 + 3 + 4 + 4 + 6 });
        assert_eq!(sum.range(), "20\u{2013}21");
        let _ = Isa::name(&m6502::M6502);
    }
}
