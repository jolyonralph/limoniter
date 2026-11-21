//! How long 68000 and 68008 instructions take, for timing code on the Amiga, the Atari ST, the Mega Drive and the
//! Sinclair QL. Which processor the code is for is told from the source (MACHINE 68008) or the settings, as the
//! assembler is told it.
//!
//! The 68000's figures are Motorola's (the M68000 Family Programmer's Reference and the MC68000 User's Manual,
//! section 8): each instruction's clock cycles with the bus cycles it makes, read and written, every bus cycle a
//! word (or a byte) taking four clocks. The 68008 is a 68000 with an 8-bit data bus, so each byte takes a bus cycle
//! of its own: a word takes it eight clocks, not four, and a byte four, as on the 68000. Its figures here are
//! worked out from the 68000's that way (four clocks more for each word read or written), not copied from
//! Motorola's 68008 tables, which weren't to hand.
//!
//! None are shown for the 68010, whose instructions take different times again (and loop in a mode of their own),
//! nor for the 68020 and later: with their caches and pipelines Motorola gives only a best, a cached and a worst
//! case, and any one figure would be wrong.

use crate::asm::m68k::Processor;
use crate::asm::{Assembly, Placed};
use crate::m68k_disasm::{Insn, Op, Size};

/// What an instruction takes on the 68000: its clock cycles, its bus cycles read and written, and how many of
/// those move a byte rather than a word (which the 68008 does in one bus cycle, not two).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
struct Cost {
    cycles: u32,
    reads: u32,
    writes: u32,
    bytes: u32,
}

impl Cost {
    const fn new(cycles: u32, reads: u32, writes: u32) -> Cost {
        Cost { cycles, reads, writes, bytes: 0 }
    }

    fn add(self, other: Cost) -> Cost {
        Cost { cycles: self.cycles + other.cycles, reads: self.reads + other.reads, writes: self.writes + other.writes, bytes: self.bytes + other.bytes }
    }

    fn more(self, cycles: u32) -> Cost {
        Cost { cycles: self.cycles + cycles, ..self }
    }

    /// With its writes moving bytes, for a byte-sized result written to memory.
    fn byte_writes(self) -> Cost {
        Cost { bytes: self.bytes + self.writes, ..self }
    }

    /// Its clock cycles on a processor: on the 68008 each word read or written takes two bus cycles.
    fn on(self, cpu: Processor) -> u32 {
        match cpu {
            Processor::M68008 => self.cycles + 4 * (self.reads + self.writes - self.bytes.min(self.reads + self.writes)),
            _ => self.cycles,
        }
    }
}

/// What an instruction takes: the least and most cycles (a branch taken or not, a shift by a register, a
/// multiply), and whether the most is only a bound (a divide, which Motorola gives no least for).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    pub least: u32,
    pub most: u32,
    pub up_to: bool,
}

impl Timing {
    /// "4", "8/10" where it may take more, or "≤140" for a divide.
    pub fn text(self) -> String {
        if self.up_to {
            format!("\u{2264}{}", self.most)
        } else if self.least == self.most {
            self.least.to_string()
        } else {
            format!("{}/{}", self.least, self.most)
        }
    }
}

/// The address's own time, as Motorola's table 8-1 gives it: reading the operand there (none from a register or,
/// for a number, only its extension words).
fn ea(op: &Op, size: Size) -> Cost {
    let long = size == Size::L;
    let byte = (size == Size::B) as u32;
    let (word, longs) = match op {
        Op::D(_) | Op::A(_) => return Cost::default(),
        Op::Ind(_) | Op::Post(_) => ((4, 1), (8, 2)),
        Op::Pre(_) => ((6, 1), (10, 2)),
        Op::Disp(..) | Op::AbsW(_) | Op::Pc(_) | Op::PcW(_) => ((8, 2), (12, 3)),
        Op::Index(..) | Op::PcIndex(..) | Op::PcIndexB(..) => ((10, 2), (14, 3)),
        Op::AbsL(..) => ((12, 3), (16, 4)),
        Op::Imm(..) => return if long { Cost::new(8, 2, 0) } else { Cost::new(4, 1, 0) },
        _ => return Cost::default(),
    };
    let (cycles, reads) = if long { longs } else { word };
    Cost { cycles, reads, writes: 0, bytes: byte }
}

/// Writing a MOVE's destination, as Motorola's tables 8-2 and 8-3 give it (-(An) takes no longer to write).
fn write(op: &Op, size: Size) -> Cost {
    let long = size == Size::L;
    let byte = (size == Size::B) as u32;
    let ((cycles, reads), writes) = match op {
        Op::D(_) | Op::A(_) => return Cost::default(),
        Op::Ind(_) | Op::Post(_) | Op::Pre(_) => (if long { (8, 0) } else { (4, 0) }, 1),
        Op::Disp(..) | Op::AbsW(_) => (if long { (12, 1) } else { (8, 1) }, 1),
        Op::Index(..) => (if long { (14, 1) } else { (10, 1) }, 1),
        Op::AbsL(..) => (if long { (16, 2) } else { (12, 2) }, 1),
        _ => return Cost::default(),
    };
    Cost { cycles, reads, writes: if long { 2 } else { writes }, bytes: byte }
}

/// JMP, JSR, LEA and PEA by their address, as Motorola's table 8-10 gives them.
fn control(name: &str, op: &Op) -> Option<Cost> {
    let column = match op {
        Op::Ind(_) => 0,
        Op::Disp(..) | Op::Pc(_) | Op::PcW(_) => 1,
        Op::Index(..) | Op::PcIndex(..) | Op::PcIndexB(..) => 2,
        Op::AbsW(_) => 3,
        Op::AbsL(..) => 4,
        _ => return None,
    };
    let table: [(u32, u32, u32); 5] = match name {
        "jmp" => [(8, 2, 0), (10, 2, 0), (14, 3, 0), (10, 2, 0), (12, 3, 0)],
        "jsr" => [(16, 2, 2), (18, 2, 2), (22, 2, 2), (18, 2, 2), (20, 3, 2)],
        "lea" => [(4, 1, 0), (8, 2, 0), (12, 2, 0), (8, 2, 0), (12, 3, 0)],
        _ => [(12, 1, 2), (16, 2, 2), (20, 2, 2), (16, 2, 2), (20, 3, 2)],
    };
    let (c, r, w) = table[column];
    Some(Cost::new(c, r, w))
}

/// MOVEM's time, as Motorola's table 8-10 gives it: `n` registers moved.
fn movem(to_memory: bool, op: &Op, size: Size, n: u32) -> Option<Cost> {
    let per = if size == Size::L { 8 } else { 4 };
    let words = if size == Size::L { 2 * n } else { n };
    let (base, reads) = match (to_memory, op) {
        (false, Op::Ind(_) | Op::Post(_)) => (12, 3),
        (false, Op::Disp(..) | Op::AbsW(_) | Op::Pc(_) | Op::PcW(_)) => (16, 4),
        (false, Op::Index(..) | Op::PcIndex(..) | Op::PcIndexB(..)) => (18, 4),
        (false, Op::AbsL(..)) => (20, 5),
        (true, Op::Ind(_) | Op::Pre(_)) => (8, 2),
        (true, Op::Disp(..) | Op::AbsW(_)) => (12, 3),
        (true, Op::Index(..)) => (14, 3),
        (true, Op::AbsL(..)) => (16, 4),
        _ => return None,
    };
    Some(if to_memory { Cost::new(base + per * n, reads, words) } else { Cost::new(base + per * n, reads + words, 0) })
}

/// Scc's conditions.
const CONDITIONS: [&str; 18] = ["t", "f", "hi", "ls", "cc", "hs", "cs", "lo", "ne", "eq", "vc", "vs", "pl", "mi", "ge", "lt", "gt", "le"];

fn size_of(mnemonic: &str) -> Size {
    match mnemonic.rsplit_once('.').map(|(_, s)| s) {
        Some("b" | "s") => Size::B,
        Some("l") => Size::L,
        _ => Size::W,
    }
}

fn memory(op: &Op) -> bool {
    !matches!(op, Op::D(_) | Op::A(_) | Op::Imm(..) | Op::Quick(_))
}

/// An instruction's time on the 68000: the quickest case and the slowest (a branch taken or not), and whether the
/// slowest is only a bound.
fn cost(insn: &Insn) -> Option<(Cost, Cost, bool)> {
    let name = insn.mnemonic.split('.').next().unwrap_or("");
    let size = size_of(&insn.mnemonic);
    let long = size == Size::L;
    let byte = size == Size::B;
    let ops = insn.ops.as_slice();
    let one = |c: Cost| Some((c, c, false));
    let rmw = |base: Cost, dst: &Op| {
        let c = base.add(ea(dst, size));
        if byte { c.byte_writes() } else { c }
    };
    let is_d = |op: &Op| matches!(op, Op::D(_));
    match (name, ops) {
        ("nop", []) | ("trapv", []) => one(Cost::new(4, 1, 0)),
        ("rts", []) => one(Cost::new(16, 4, 0)),
        ("rte", []) | ("rtr", []) => one(Cost::new(20, 5, 0)),
        ("reset", []) => one(Cost::new(132, 1, 0)),
        ("stop", [_]) => one(Cost::new(4, 0, 0)),
        ("illegal", []) | ("trap", [_]) => one(Cost::new(34, 4, 3)),
        ("moveq", _) | ("swap", _) | ("ext", _) => one(Cost::new(4, 1, 0)),
        ("exg", _) => one(Cost::new(6, 1, 0)),
        ("link", _) => one(Cost::new(16, 2, 2)),
        ("unlk", _) => one(Cost::new(12, 3, 0)),
        ("move", [Op::Usp, _] | [_, Op::Usp]) => one(Cost::new(4, 1, 0)),
        ("move", [Op::Sr, dst]) if is_d(dst) => one(Cost::new(6, 1, 0)),
        ("move", [Op::Sr, dst]) => one(Cost::new(8, 1, 1).add(ea(dst, Size::W))),
        ("move", [src, Op::Sr | Op::Ccr]) => one(Cost::new(12, 1, 0).add(ea(src, Size::W))),
        ("move" | "movea", [src, dst]) => {
            let c = Cost::new(4, 1, 0).add(ea(src, size)).add(write(dst, size));
            one(c)
        }
        ("movem", [Op::List(mask), place]) => one(movem(true, place, size, mask.count_ones())?),
        ("movem", [place, Op::List(mask)]) => one(movem(false, place, size, mask.count_ones())?),
        ("movep", [Op::D(_), _]) => one(if long { Cost { bytes: 4, ..Cost::new(24, 2, 4) } } else { Cost { bytes: 2, ..Cost::new(16, 2, 2) } }),
        ("movep", [_, Op::D(_)]) => one(if long { Cost { bytes: 4, ..Cost::new(24, 6, 0) } } else { Cost { bytes: 2, ..Cost::new(16, 4, 0) } }),
        ("jmp" | "jsr" | "lea" | "pea", [place, ..]) => one(control(name, place)?),
        // To a data register: .B and .W 4, .L 6 (8 from a register or a number).
        ("add" | "sub" | "and" | "or", [src, Op::D(_)]) => {
            let base = if long { if matches!(src, Op::D(_) | Op::A(_) | Op::Imm(..)) { 8 } else { 6 } } else { 4 };
            one(Cost::new(base, 1, 0).add(ea(src, size)))
        }
        ("add" | "sub" | "and" | "or" | "eor", [Op::D(_), dst]) if memory(dst) => one(rmw(if long { Cost::new(12, 1, 2) } else { Cost::new(8, 1, 1) }, dst)),
        ("eor", [Op::D(_), Op::D(_)]) => one(Cost::new(if long { 8 } else { 4 }, 1, 0)),
        ("adda" | "suba", [src, _]) => {
            let base = if long { if matches!(src, Op::D(_) | Op::A(_) | Op::Imm(..)) { 8 } else { 6 } } else { 8 };
            one(Cost::new(base, 1, 0).add(ea(src, size)))
        }
        ("cmp", [src, _]) => one(Cost::new(if long { 6 } else { 4 }, 1, 0).add(ea(src, size))),
        ("cmpa", [src, _]) => one(Cost::new(6, 1, 0).add(ea(src, size))),
        ("andi" | "ori" | "eori", [_, Op::Ccr | Op::Sr]) => one(Cost::new(20, 3, 0)),
        ("addi" | "subi" | "andi" | "ori" | "eori", [_, dst]) => one(match (is_d(dst), long) {
            (true, false) => Cost::new(8, 2, 0),
            (true, true) => Cost::new(16, 3, 0),
            (false, false) => rmw(Cost::new(12, 2, 1), dst),
            (false, true) => rmw(Cost::new(20, 3, 2), dst),
        }),
        ("cmpi", [_, dst]) => one(match (is_d(dst), long) {
            (true, false) => Cost::new(8, 2, 0),
            (true, true) => Cost::new(14, 3, 0),
            (false, false) => Cost::new(8, 2, 0).add(ea(dst, size)),
            (false, true) => Cost::new(12, 3, 0).add(ea(dst, size)),
        }),
        ("addq" | "subq", [_, dst]) => one(match (dst, long) {
            (Op::D(_), false) => Cost::new(4, 1, 0),
            (Op::D(_) | Op::A(_), true) | (Op::A(_), false) => Cost::new(8, 1, 0),
            (_, false) => rmw(Cost::new(8, 1, 1), dst),
            (_, true) => rmw(Cost::new(12, 1, 2), dst),
        }),
        ("clr" | "neg" | "negx" | "not", [dst]) => one(match (is_d(dst), long) {
            (true, false) => Cost::new(4, 1, 0),
            (true, true) => Cost::new(6, 1, 0),
            (false, false) => rmw(Cost::new(8, 1, 1), dst),
            (false, true) => rmw(Cost::new(12, 1, 2), dst),
        }),
        ("tst", [dst]) => one(Cost::new(4, 1, 0).add(ea(dst, size))),
        ("nbcd", [dst]) => one(if is_d(dst) { Cost::new(6, 1, 0) } else { Cost::new(8, 1, 1).add(ea(dst, Size::B)).byte_writes() }),
        ("tas", [dst]) => one(if is_d(dst) { Cost::new(4, 1, 0) } else { Cost::new(10, 1, 1).add(ea(dst, Size::B)).byte_writes() }),
        (s, [dst]) if s.strip_prefix('s').is_some_and(|cc| CONDITIONS.contains(&cc)) => {
            if is_d(dst) {
                // 4 when the condition fails (the byte cleared), 6 when it holds (set).
                Some((Cost::new(4, 1, 0), Cost::new(6, 1, 0), false))
            } else {
                one(Cost::new(8, 1, 1).add(ea(dst, Size::B)).byte_writes())
            }
        }
        // Shifts and rotates: 2 cycles a place; by a register, 0 to 63 places.
        ("asl" | "asr" | "lsl" | "lsr" | "rol" | "ror" | "roxl" | "roxr", [count, Op::D(_)]) => {
            let base = if long { 8 } else { 6 };
            match count {
                Op::Quick(n) => one(Cost::new(base + 2 * *n as u32, 1, 0)),
                _ => Some((Cost::new(base, 1, 0), Cost::new(base + 2 * 63, 1, 0), false)),
            }
        }
        ("asl" | "asr" | "lsl" | "lsr" | "rol" | "ror" | "roxl" | "roxr", [dst]) => one(Cost::new(8, 1, 1).add(ea(dst, Size::W))),
        // Bit operations: by a register, or by a number (a word more to fetch); in a data register on all 32
        // bits, in memory on a byte. Motorola gives the most for a data register.
        ("btst" | "bchg" | "bclr" | "bset", [number, dst]) => {
            let fixed = matches!(number, Op::Quick(_));
            let c = match (name, is_d(dst)) {
                ("btst", true) => Cost::new(6, 1, 0),
                ("btst", false) => Cost::new(4, 1, 0).add(ea(dst, Size::B)),
                ("bclr", true) => Cost::new(10, 1, 0),
                (_, true) => Cost::new(8, 1, 0),
                (_, false) => Cost::new(8, 1, 1).add(ea(dst, Size::B)).byte_writes(),
            };
            one(if fixed { c.add(Cost::new(4, 1, 0)) } else { c })
        }
        ("bra", _) => one(Cost::new(10, 2, 0)),
        ("bsr", _) => one(Cost::new(18, 2, 2)),
        (b, [Op::Target(_)]) if b.starts_with('b') => {
            // Not taken, then taken: a short branch not taken is quicker, a word one slower.
            let not_taken = if insn.mnemonic.ends_with(".s") { Cost::new(8, 1, 0) } else { Cost::new(12, 2, 0) };
            let taken = Cost::new(10, 2, 0);
            Some(if not_taken.cycles < taken.cycles { (not_taken, taken, false) } else { (taken, not_taken, false) })
        }
        // Looping 10; the condition true 12; the count run out 14.
        (d, [_, Op::Target(_)]) if d.starts_with("db") => Some((Cost::new(10, 2, 0), Cost::new(14, 3, 0), false)),
        ("addx" | "subx", [Op::D(_), _]) => one(Cost::new(if long { 8 } else { 4 }, 1, 0)),
        ("addx" | "subx", _) => one(if long { Cost::new(30, 5, 2) } else if byte { Cost { bytes: 3, ..Cost::new(18, 3, 1) } } else { Cost::new(18, 3, 1) }),
        ("abcd" | "sbcd", [Op::D(_), _]) => one(Cost::new(6, 1, 0)),
        ("abcd" | "sbcd", _) => one(Cost { bytes: 3, ..Cost::new(18, 3, 1) }),
        ("cmpm", _) => one(if long { Cost::new(20, 5, 0) } else if byte { Cost { bytes: 2, ..Cost::new(12, 3, 0) } } else { Cost::new(12, 3, 0) }),
        // CHK when it doesn't trap.
        ("chk", [src, _]) => one(Cost::new(10, 1, 0).add(ea(src, Size::W))),
        // MULU 38 + 2 a bit set in the source; MULS 38 + 2 for each change between neighbouring bits (with a 0
        // below the lowest); a number's are counted.
        ("mulu" | "muls", [src, _]) => {
            let base = Cost::new(38, 1, 0).add(ea(src, Size::W));
            match src {
                Op::Imm(v, ..) => {
                    let v = *v & 0xFFFF;
                    let n = if name == "mulu" { v.count_ones() } else { (((v << 1) ^ v) & 0xFFFF).count_ones() };
                    one(base.more(2 * n))
                }
                _ => Some((base, base.more(32), false)),
            }
        }
        // A divide's most, which Motorola gives; it can take up to a tenth less.
        ("divu" | "divs", [src, _]) => {
            let most = if name == "divu" { 140 } else { 158 };
            let c = Cost::new(most, 1, 0).add(ea(src, Size::W));
            Some((c, c, true))
        }
        _ => None,
    }
}

/// An instruction's time on a processor, from its bytes, if they're one whole 68000 instruction and the processor
/// is one whose times can be given: the 68000 or the 68008.
pub fn cycles(bytes: &[u8], cpu: Processor) -> Option<Timing> {
    if !matches!(cpu, Processor::M68000 | Processor::M68008) || bytes.len() < 2 || bytes.len() % 2 == 1 {
        return None;
    }
    let word = |a: u32| bytes.get(a as usize..a as usize + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let insn = crate::m68k_disasm::decode(&word, 0)?;
    if insn.length as usize != bytes.len() {
        return None;
    }
    let (least, most, up_to) = cost(&insn)?;
    Some(Timing { least: least.on(cpu), most: most.on(cpu), up_to })
}

/// The processor each of an assembly's lines is for, as MACHINE (or the settings) chose it.
pub fn processors(assembly: &Assembly) -> Vec<Processor> {
    let mut changes = assembly.settings.iter().filter(|(_, key, _)| *key == crate::asm::m68k::SETTING).peekable();
    let mut cpu = Processor::M68000;
    let mut out = Vec::with_capacity(assembly.lines.len());
    for index in 0..assembly.lines.len() {
        while let Some((_, _, value)) = changes.next_if(|(at, _, _)| *at <= index) {
            cpu = Processor::from_id(*value).unwrap_or_default();
        }
        out.push(cpu);
    }
    out
}

/// A line's instruction, the processor it's for and its cycles, if the line is one whole instruction whose time
/// can be given.
pub fn line(assembly: &Assembly, file: usize, line: usize) -> Option<(&Placed, Processor, Timing)> {
    let index = assembly.lines.iter().position(|p| p.file == file && p.line == line && !p.bytes.is_empty())?;
    let placed = &assembly.lines[index];
    let cpu = *processors(assembly).get(index)?;
    Some((placed, cpu, cycles(&placed.bytes, cpu)?))
}

/// The words for a line's cycles: "8 cycles", or "16 cycles on the 68008".
pub fn words(timing: Timing, cpu: Processor) -> String {
    let text = timing.text();
    match cpu {
        Processor::M68008 => crate::i18n::tf!("{text} cycles on the 68008"),
        _ => crate::i18n::tf!("{text} cycles"),
    }
}

/// Each line's cycles between `first` and `last`, to show at the right of 68000 and 68008 source.
pub fn line_notes(intel: &crate::code_intel::CodeIntel, first: usize, last: usize) -> std::collections::HashMap<usize, String> {
    let Some(assembly) = &intel.assembly else {
        return std::collections::HashMap::new();
    };
    let cpus = processors(assembly);
    assembly
        .lines
        .iter()
        .zip(cpus)
        .filter(|(p, _)| p.file == intel.file && p.line >= first && p.line <= last && !p.bytes.is_empty())
        .filter_map(|(p, cpu)| cycles(&p.bytes, cpu).map(|c| (p.line, c.text())))
        .collect()
}

#[cfg(test)]
mod tests;
