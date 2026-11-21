//! 68000 machine code turned back into source, in Motorola syntax as vasm and Limoniter's own assembler take it.
//! What the file is says how: an Amiga program (its hunks, each a SECTION, with every relocated long written as
//! the label it points at, so the source can be assembled into a working program again), an Atari ST program
//! (TOS's header, its text, data and BSS, and its relocation table), an Amiga boot block, an Atari ST boot sector,
//! a Mega Drive cartridge (followed from its vectors) or a Kickstart ROM; anything else is plain code at an
//! address that's asked for.
//!
//! The code is followed from where it starts, through every branch, jump and call, so what's never reached
//! stays data. Places used get labels. The hardware is named where it's touched: the Amiga's custom chips and
//! CIAs (also through a register set to $DFF000), and the ST's; so are the system's calls: the Amiga's libraries
//! (exec's from the start, others when their base is followed from OpenLibrary into a6, with the NDK's names
//! when it's installed) and TOS's GEMDOS, BIOS and XBIOS calls and Line-A. The source is then assembled again,
//! and any line that wouldn't give back its bytes becomes data, so it always assembles to what it came from.

use crate::asm::hunk::{self, Kind, Memory};
pub use crate::asm::m68k::Processor;
use std::collections::{BTreeMap, BTreeSet};

// ---------- What a program is ----------

/// The machine the code is for, which says what its hardware addresses and system calls are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Machine {
    Amiga,
    AtariSt,
    MegaDrive,
    Plain,
}

impl Machine {
    pub const ALL: [Machine; 4] = [Machine::Amiga, Machine::AtariSt, Machine::MegaDrive, Machine::Plain];

    pub fn name(self) -> &'static str {
        match self {
            Machine::Amiga => "Amiga",
            Machine::AtariSt => "Atari ST",
            Machine::MegaDrive => "Mega Drive",
            Machine::Plain => "None",
        }
    }
}

/// How the program is laid out, which says how its source is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// An Amiga executable: each hunk a SECTION, loaded wherever there's room.
    Hunks,
    /// An Atari ST program: text, data and BSS one after another from 0, moved by TOS when it's loaded.
    Tos,
    /// Code at an address of its own (ORG).
    Raw,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub kind: Kind,
    pub memory: Memory,
    /// Where it's taken to be while disassembling.
    pub base: u32,
    pub bytes: Vec<u8>,
    /// Zeros after the bytes: a BSS hunk, or TOS's BSS.
    pub bss: u32,
    /// The longs in it that hold addresses: their offsets, and the section each points into.
    pub relocations: BTreeMap<u32, usize>,
}

impl Section {
    fn end(&self) -> u32 {
        self.base + self.bytes.len() as u32 + self.bss
    }

    fn holds(&self, address: u32) -> bool {
        address >= self.base && address < self.end()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub machine: Machine,
    pub layout: Layout,
    pub sections: Vec<Section>,
    pub starts: Vec<u32>,
    /// The names the file gives places.
    pub names: BTreeMap<u32, String>,
    /// For an Atari ST program, where its text ends: only the text is code.
    pub text_end: Option<u32>,
    /// Longs that are addresses in code at its own address: a Mega Drive's vectors.
    pub pointers: BTreeSet<u32>,
    /// What it was found to be, to say.
    pub what: String,
    /// The processor it's for: the 68010's and 68020's instructions and addresses are read only for those that
    /// have them, and the source says MACHINE for any but the 68000.
    pub cpu: Processor,
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    b.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]]))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// A number with its thousands marked.
fn group(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Plain code: `bytes` at `origin`, followed from `starts`.
pub fn raw(bytes: &[u8], origin: u32, starts: &[u32], machine: Machine) -> Program {
    let section = Section { kind: Kind::Code, memory: Memory::Any, base: origin, bytes: bytes.to_vec(), bss: 0, relocations: BTreeMap::new() };
    Program {
        machine,
        layout: Layout::Raw,
        sections: vec![section],
        starts: starts.to_vec(),
        names: BTreeMap::new(),
        text_end: None,
        pointers: BTreeSet::new(),
        what: crate::i18n::tf!("{} bytes of 68000 code at ${origin:X}", group(bytes.len() as u64)),
        cpu: Processor::M68000,
    }
}

/// An Amiga executable, its hunks each at an address of its own, as Limoniter's assembler puts SECTIONs.
pub fn amiga_program(bytes: &[u8]) -> Result<Program, String> {
    let hunks = hunk::read_executable(bytes)?;
    if hunks.is_empty() {
        return Err(crate::i18n::tr("The program has no hunks.").into());
    }
    let mut sections = Vec::new();
    let mut names = BTreeMap::new();
    for (i, h) in hunks.iter().enumerate() {
        let base = hunk::section_start(i + 1, 0) as u32;
        let (bytes, bss) = if h.kind == Kind::Bss { (Vec::new(), h.bytes.len() as u32) } else { (h.bytes.clone(), 0) };
        let relocations = h.relocations.iter().map(|&(place, target)| (place, target as usize)).collect();
        for (name, value) in &h.symbols {
            names.insert(base + value, name.clone());
        }
        sections.push(Section { kind: h.kind, memory: h.memory, base, bytes, bss, relocations });
    }
    let parts: Vec<String> = sections
        .iter()
        .map(|s| {
            let kind = match s.kind {
                Kind::Code => "code",
                Kind::Data => "data",
                Kind::Bss => "BSS",
            };
            format!("{kind} {} bytes", group(u64::from(s.end() - s.base)))
        })
        .collect();
    let what = crate::i18n::tf!("An Amiga program: {} ({})", if sections.len() == 1 { "1 hunk".to_string() } else { format!("{} hunks", sections.len()) }, parts.join(", "));
    let starts = vec![sections[0].base];
    Ok(Program { machine: Machine::Amiga, layout: Layout::Hunks, sections, starts, names, text_end: None, pointers: BTreeSet::new(), what, cpu: Processor::M68000 })
}

/// An Atari ST program: $601A, its parts' sizes, the text and data, the symbols, and the table of the longs TOS
/// moves when it loads it, each counted from the one before.
pub fn tos_program(bytes: &[u8]) -> Result<Program, String> {
    if !crate::running::is_atari_program(bytes) {
        return Err(crate::i18n::tr("This isn't an Atari ST program: it doesn't start $601A with sizes that fit.").into());
    }
    let size = |at| be32(bytes, at).unwrap_or(0) as usize;
    let (text, data, bss, symbols) = (size(2), size(6), size(10), size(14));
    let absolute = be16(bytes, 26).unwrap_or(0) != 0;
    let image = bytes[28..28 + text + data].to_vec();
    let mut names = BTreeMap::new();
    // DRI symbols: 8 letters, a type and a value; text, data and BSS ones are places in the program.
    let table = bytes.get(28 + text + data..28 + text + data + symbols).unwrap_or(&[]);
    for entry in table.as_chunks::<14>().0 {
        let name: String = entry[..8].iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
        let kind = u16::from_be_bytes([entry[8], entry[9]]);
        let value = u32::from_be_bytes([entry[10], entry[11], entry[12], entry[13]]);
        if kind & 0x0700 != 0 && !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') {
            names.insert(value, name);
        }
    }
    let mut relocations = BTreeMap::new();
    let mut at = 28 + text + data + symbols;
    if !absolute && let Some(first) = be32(bytes, at).filter(|&f| f != 0) {
        let mut place = first;
        relocations.insert(place, 0);
        at += 4;
        while let Some(&step) = bytes.get(at) {
            at += 1;
            match step {
                0 => break,
                1 => place += 254,
                n => {
                    place += u32::from(n);
                    relocations.insert(place, 0);
                }
            }
        }
    }
    let what = crate::i18n::tf!("An Atari ST program: text {} bytes, data {}, BSS {}{}", group(text as u64), group(data as u64), group(bss as u64), if names.is_empty() { String::new() } else { format!(", {} symbols", names.len()) });
    let section = Section { kind: Kind::Code, memory: Memory::Any, base: 0, bytes: image, bss: bss as u32, relocations };
    Ok(Program { machine: Machine::AtariSt, layout: Layout::Tos, sections: vec![section], starts: vec![0], names, text_end: Some(text as u32), pointers: BTreeSet::new(), what, cpu: Processor::M68000 })
}

/// What a file holds, when it says: an Amiga program, an Atari ST program, an Amiga boot block, an ST boot
/// sector, a Mega Drive cartridge or a Kickstart ROM.
pub fn recognise(name: &str, bytes: &[u8]) -> Option<Program> {
    let ending = std::path::Path::new(name).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    if be32(bytes, 0) == Some(hunk::HUNK_HEADER) {
        return amiga_program(bytes).ok();
    }
    if crate::running::is_atari_program(bytes) {
        return tos_program(bytes).ok();
    }
    // An Amiga boot block: DOS, its checksum and root block, then code that's called at offset 12.
    if bytes.len() == 1024 && bytes.starts_with(b"DOS") {
        let mut program = raw(bytes, 0, &[12], Machine::Amiga);
        program.what = crate::i18n::tr("An Amiga boot block: its code from offset 12 (it's loaded wherever there's room)").into();
        return Some(program);
    }
    // An Atari ST boot sector: its words add up to $1234, and it starts with a branch over the disk's details.
    if bytes.len() == 512 && bytes.as_chunks::<2>().0.iter().fold(0u16, |sum, w| sum.wrapping_add(u16::from_be_bytes([w[0], w[1]]))) == 0x1234 {
        let mut program = raw(bytes, 0, &[0], Machine::AtariSt);
        program.what = crate::i18n::tr("An Atari ST boot sector: its code from the branch at the start").into();
        return Some(program);
    }
    // A Mega Drive cartridge: SEGA at $100, and its vectors at the start.
    if bytes.len() >= 0x200 && (bytes.get(0x100..0x104) == Some(b"SEGA") || bytes.get(0x101..0x105) == Some(b"SEGA") || matches!(ending.as_str(), "md" | "gen")) {
        let inside = |a: u32| (a as usize) < bytes.len() && a.is_multiple_of(2) && a >= 0x200;
        let mut starts = Vec::new();
        let mut pointers = BTreeSet::new();
        for vector in 1..64u32 {
            if let Some(a) = be32(bytes, vector as usize * 4).filter(|&a| inside(a)) {
                pointers.insert(vector * 4);
                if !starts.contains(&a) {
                    starts.push(a);
                }
            }
        }
        if let Some(sp) = be32(bytes, 0).filter(|&a| inside(a)) {
            let _ = sp;
            pointers.insert(0);
        }
        let mut program = raw(bytes, 0, &starts, Machine::MegaDrive);
        program.pointers = pointers;
        let title: String = bytes.get(0x150..0x180).unwrap_or(&[]).iter().map(|&b| b as char).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
        program.what = crate::i18n::tf!("A Mega Drive cartridge{}, followed from its vectors", if title.is_empty() { String::new() } else { format!(" ({title})") });
        return Some(program);
    }
    // A Kickstart ROM: $1111 (256K, at $FC0000) or $1114 (512K, at $F80000), then a jump to its start.
    if matches!(be16(bytes, 0), Some(0x1111 | 0x1114)) && matches!(bytes.len(), 0x40000 | 0x80000) {
        let origin = if bytes.len() == 0x40000 { 0xFC_0000 } else { 0xF8_0000 };
        let start = be32(bytes, 4).filter(|&a| a >= origin && a < origin + bytes.len() as u32).unwrap_or(origin + 8);
        let mut program = raw(bytes, origin, &[start], Machine::Amiga);
        program.what = crate::i18n::tf!("A Kickstart ROM of {}K at ${origin:X}, from its start at ${start:X}", bytes.len() / 1024);
        return Some(program);
    }
    None
}

// ---------- Instructions ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    B,
    W,
    L,
}

impl Size {
    fn suffix(self) -> &'static str {
        match self {
            Size::B => ".b",
            Size::W => ".w",
            Size::L => ".l",
        }
    }

    fn from_bits(bits: u16) -> Option<Size> {
        match bits {
            0 => Some(Size::B),
            1 => Some(Size::W),
            2 => Some(Size::L),
            _ => None,
        }
    }
}

/// An index register: its number, whether it's an address register, whether all 32 bits count, and its scale (0
/// to 3, ×1 to ×8: the 68020's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Xn {
    pub reg: u8,
    pub address: bool,
    pub long: bool,
    pub scale: u8,
}

impl Xn {
    fn from_word(w: u16) -> Xn {
        Xn { reg: (w >> 12 & 7) as u8, address: w & 0x8000 != 0, long: w & 0x0800 != 0, scale: (w >> 9 & 3) as u8 }
    }
}

/// A displacement in a 68020 full extension word: none, a word, or a long and where it is (it may be relocated).
/// From PC, the address it comes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disp {
    Null,
    Word(i64),
    Long(i64, u32),
}

/// Whether a 68020 address reads a pointer from memory, and whether its index is added before or after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Indirect {
    No,
    Pre,
    Post,
}

/// A 68020 address with a full extension word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FullEa {
    /// The base: An's number, or None for PC.
    pub base: Option<u8>,
    pub base_suppressed: bool,
    pub index: Xn,
    pub index_suppressed: bool,
    pub bd: Disp,
    pub indirect: Indirect,
    pub od: Disp,
}

impl FullEa {
    /// Where it points, if it's from PC (and PC isn't suppressed).
    fn target(&self) -> Option<u32> {
        match (self.base, self.base_suppressed, self.bd) {
            (None, false, Disp::Word(t) | Disp::Long(t, _)) => Some(t as u32),
            _ => None,
        }
    }
}

/// An operand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    D(u8),
    A(u8),
    Ind(u8),
    Post(u8),
    Pre(u8),
    Disp(u8, i16),
    Index(u8, i8, Xn),
    /// A short address, sign-extended.
    AbsW(u32),
    /// A long address, and where its long is (it may be one that's relocated).
    AbsL(u32, u32),
    /// An address relative to PC, as the address it comes to.
    Pc(u32),
    PcIndex(u32, Xn),
    /// The same, written with their sizes (.W, .B), where a 68020's assembler would otherwise widen them.
    PcW(u32),
    PcIndexB(u32, Xn),
    /// A 68020 address with a full extension word.
    Full(Box<FullEa>),
    /// An immediate value, its size, and where it is.
    Imm(u32, Size, u32),
    /// A number in the instruction itself: ADDQ's, a shift's count, TRAP's, MOVEQ's, a bit number.
    Quick(i32),
    Sr,
    Ccr,
    Usp,
    /// MOVEM's registers: bit 0 D0 to bit 15 A7.
    List(u16),
    /// Where a branch goes.
    Target(u32),
    /// Two data registers, Dh:Dl.
    Pair(u8, u8),
    /// CAS2's two registers in brackets: 0 to 7 for D0 to D7, 8 to 15 for A0 to A7.
    RegPair(u8, u8),
    /// MOVEC's control register, by its name.
    Control(&'static str),
    /// A bit field: the address, then its offset and width as written ({4:8}, {d1:d2}).
    Field(Box<Op>, String),
}

impl Op {
    /// The address an operand relative to PC points at.
    fn pc_target(&self) -> Option<u32> {
        match self {
            Op::Pc(t) | Op::PcIndex(t, _) | Op::PcW(t) | Op::PcIndexB(t, _) => Some(*t),
            Op::Full(f) => f.target(),
            Op::Field(inner, _) => inner.pc_target(),
            _ => None,
        }
    }
}

/// What an instruction does to where the code goes next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Next,
    /// Conditional: it may go to its target or carry on.
    Branch,
    Jump,
    Call,
    /// It doesn't carry on: RTS, RTE, RTR, ILLEGAL.
    Return,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Insn {
    pub mnemonic: String,
    pub ops: Vec<Op>,
    pub length: u32,
    pub flow: Flow,
}

const CONDITIONS: [&str; 16] = ["t", "f", "hi", "ls", "cc", "cs", "ne", "eq", "vc", "vs", "pl", "mi", "ge", "lt", "gt", "le"];

struct Reader<'a> {
    word: &'a dyn Fn(u32) -> Option<u16>,
    pc: u32,
    length: u32,
    cpu: Processor,
}

impl Reader<'_> {
    fn next(&mut self) -> Option<u16> {
        let w = (self.word)(self.pc.wrapping_add(self.length))?;
        self.length += 2;
        Some(w)
    }

    fn here(&self) -> u32 {
        self.pc.wrapping_add(self.length)
    }

    fn long(&mut self) -> Option<u32> {
        Some(u32::from(self.next()?) << 16 | u32::from(self.next()?))
    }
}

/// An indexed address's extension: the 68000's brief word (with the 68020's scale on a 68020 or CPU32), or the
/// 68020's full one. `base` is An's number, or None for PC; `at` is where the extension word is.
fn indexed(r: &mut Reader, base: Option<u8>) -> Option<Op> {
    let at = r.here();
    let w = r.next()?;
    let wide = r.cpu.has_68020_or_cpu32();
    if w & 0x0100 == 0 {
        if w & 0x0600 != 0 && !wide {
            return None;
        }
        let x = Xn::from_word(w);
        let d = w as u8 as i8;
        return Some(match base {
            Some(n) => Op::Index(n, d, x),
            None => Op::PcIndex(at.wrapping_add(d as i32 as u32), x),
        });
    }
    if !wide || w & 0x0008 != 0 {
        return None;
    }
    let base_suppressed = w & 0x0080 != 0;
    let index_suppressed = w & 0x0040 != 0;
    let indirect_bits = w & 7;
    let indirect = match (index_suppressed, indirect_bits) {
        (_, 0) => Indirect::No,
        (_, 1..=3) => Indirect::Pre,
        (false, 5..=7) => Indirect::Post,
        _ => return None,
    };
    // The CPU32 has the full format, but not memory indirect addressing.
    if indirect != Indirect::No && !r.cpu.has_68020() {
        return None;
    }
    let pc = base.is_none();
    let displacement = |bits: u16, from_pc: bool, r: &mut Reader| -> Option<Disp> {
        let place = r.here();
        Some(match bits {
            1 => Disp::Null,
            2 => {
                let v = i64::from(r.next()? as i16);
                Disp::Word(if from_pc { i64::from(at) + v } else { v })
            }
            3 => {
                let v = i64::from(r.long()? as i32);
                Disp::Long(if from_pc { i64::from(at.wrapping_add(v as u32)) } else { v }, place)
            }
            _ => return None,
        })
    };
    let bd = displacement(w >> 4 & 3, pc, r)?;
    let od = if indirect == Indirect::No { Disp::Null } else { displacement(indirect_bits & 3, false, r)? };
    // With its base and index both there and no displacement or memory indirection, it can only be written as the
    // brief word's (An,Xn).
    if indirect == Indirect::No && !base_suppressed && !index_suppressed && bd == Disp::Null {
        return None;
    }
    Some(Op::Full(Box::new(FullEa { base, base_suppressed, index: Xn::from_word(w), index_suppressed, bd, indirect, od })))
}

fn ea(r: &mut Reader, mode: u16, reg: u16, size: Size) -> Option<Op> {
    let n = reg as u8;
    Some(match mode {
        0 => Op::D(n),
        1 => Op::A(n),
        2 => Op::Ind(n),
        3 => Op::Post(n),
        4 => Op::Pre(n),
        5 => Op::Disp(n, r.next()? as i16),
        6 => indexed(r, Some(n))?,
        _ => match reg {
            0 => Op::AbsW(r.next()? as i16 as i32 as u32),
            1 => {
                let at = r.here();
                Op::AbsL(r.long()?, at)
            }
            2 => {
                let at = r.here();
                let d = r.next()? as i16;
                Op::Pc(at.wrapping_add(d as i32 as u32))
            }
            3 => indexed(r, None)?,
            4 => {
                let at = r.here();
                let value = match size {
                    // A byte's word must have nothing in its top half, or it wouldn't be written again the same.
                    Size::B => u32::from(r.next().filter(|w| *w <= 0xFF)?),
                    Size::W => u32::from(r.next()?),
                    Size::L => r.long()?,
                };
                Op::Imm(value, size, at)
            }
            _ => return None,
        },
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Data,
    Control,
    Alterable,
    DataAlterable,
    MemoryAlterable,
    ControlAlterable,
}

fn fits(op: &Op, class: Class) -> bool {
    let (an, dn) = (matches!(op, Op::A(_)), matches!(op, Op::D(_)));
    let pc = op.pc_target().is_some() || matches!(op, Op::Full(f) if f.base.is_none());
    let imm = matches!(op, Op::Imm(..));
    let control = matches!(op, Op::Ind(_) | Op::Disp(..) | Op::Index(..) | Op::AbsW(_) | Op::AbsL(..) | Op::Full(_)) || pc;
    let alterable = !pc && !imm;
    match class {
        Class::Data => !an,
        Class::Control => control,
        Class::Alterable => alterable,
        Class::DataAlterable => !an && alterable,
        Class::MemoryAlterable => !an && !dn && alterable,
        Class::ControlAlterable => control && alterable,
    }
}

/// One instruction at `pc`, from its words; None if they aren't a 68000 instruction.
pub fn decode(word: &dyn Fn(u32) -> Option<u16>, pc: u32) -> Option<Insn> {
    decode_for(word, pc, Processor::M68000)
}

/// One instruction at `pc` for a processor of the family; None if they aren't one of its instructions, or are one
/// that can't be written so that it assembles back to the same words.
pub fn decode_for(word: &dyn Fn(u32) -> Option<u16>, pc: u32, cpu: Processor) -> Option<Insn> {
    let mut insn = decode_words(word, pc, cpu)?;
    // A 68020's assembler widens a PC-relative displacement that doesn't fit, reckoning it from just after the
    // instruction's own words, as vasm does: one that only fits from where it really is is written with its size.
    if cpu.has_68020_or_cpu32() && !matches!(insn.mnemonic.as_str(), "jmp" | "jsr") {
        let base = insn.mnemonic.split('.').next().unwrap_or("").to_string();
        let words = opcode_words(&base, &insn);
        let cpc = i64::from(pc) + 2 * words;
        for op in &mut insn.ops {
            let sized = match op {
                Op::Pc(t) if !(-0x8000..=0x7FFF).contains(&(i64::from(*t) - cpc)) => Some(Op::PcW(*t)),
                Op::PcIndex(t, x) if !(-0x80..=0x7F).contains(&(i64::from(*t) - cpc)) => Some(Op::PcIndexB(*t, *x)),
                _ => None,
            };
            if let Some(sized) = sized {
                *op = sized;
            }
            if let Op::Field(inner, _) = op {
                match inner.as_ref() {
                    Op::Pc(t) if !(-0x8000..=0x7FFF).contains(&(i64::from(*t) - cpc)) => **inner = Op::PcW(*t),
                    Op::PcIndex(t, x) if !(-0x80..=0x7F).contains(&(i64::from(*t) - cpc)) => **inner = Op::PcIndexB(*t, *x),
                    _ => {}
                }
            }
        }
    }
    Some(insn)
}

/// How many words an instruction has before its addresses' extension words, as the assembler counts them.
fn opcode_words(base: &str, insn: &Insn) -> i64 {
    let long_multiply = insn.mnemonic.ends_with(".l") && matches!(base, "mulu" | "muls" | "divu" | "divs");
    match base {
        "cas2" => 3,
        "movem" | "moves" | "cas" | "chk2" | "cmp2" | "callm" | "divsl" | "divul" | "bfchg" | "bfclr" | "bfexts" | "bfextu" | "bfffo" | "bfins" | "bfset" | "bftst" => 2,
        "btst" | "bchg" | "bclr" | "bset" if matches!(insn.ops.first(), Some(Op::Quick(_))) => 2,
        _ if long_multiply => 2,
        _ => 1,
    }
}

/// A bit field's offset and width as written: {4:8}, {d1:d2}, {0:32}.
fn field_text(w: u16) -> Option<String> {
    let offset = if w & 0x0800 != 0 {
        if w & 0x0600 != 0 {
            return None;
        }
        format!("d{}", w >> 6 & 7)
    } else {
        (w >> 6 & 31).to_string()
    };
    let width = if w & 0x0020 != 0 {
        if w & 0x0018 != 0 {
            return None;
        }
        format!("d{}", w & 7)
    } else {
        match w & 31 {
            0 => "32".to_string(),
            n => n.to_string(),
        }
    };
    Some(format!("{{{offset}:{width}}}"))
}

fn decode_words(word: &dyn Fn(u32) -> Option<u16>, pc: u32, cpu: Processor) -> Option<Insn> {
    let mut r = Reader { word, pc, length: 0, cpu };
    let op = r.next()?;
    let (mode, reg) = ((op >> 3) & 7, op & 7);
    let rx = ((op >> 9) & 7) as u8;
    let size_bits = (op >> 6) & 3;
    let named = |name: &str, size: Option<Size>| format!("{name}{}", size.map_or("", Size::suffix));
    let (m68010, m68020, wide) = (cpu.has_68010(), cpu.has_68020(), cpu.has_68020_or_cpu32());
    let general = |w: u16| if w & 0x8000 != 0 { Op::A((w >> 12 & 7) as u8) } else { Op::D((w >> 12 & 7) as u8) };
    let mut flow = Flow::Next;
    let (mnemonic, ops): (String, Vec<Op>) = match op >> 12 {
        0x0 => {
            if op & 0x0100 != 0 {
                if mode == 1 {
                    let d = r.next()? as i16;
                    let size = if op & 0x40 != 0 { Size::L } else { Size::W };
                    let memory = Op::Disp(reg as u8, d);
                    let ops = if op & 0x80 == 0 { vec![memory, Op::D(rx)] } else { vec![Op::D(rx), memory] };
                    (named("movep", Some(size)), ops)
                } else {
                    let kind = size_bits as usize;
                    let dst = ea(&mut r, mode, reg, Size::B)?;
                    if !fits(&dst, if kind == 0 { Class::Data } else { Class::DataAlterable }) {
                        return None;
                    }
                    (["btst", "bchg", "bclr", "bset"][kind].to_string(), vec![Op::D(rx), dst])
                }
            } else if size_bits == 3 && rx != 4 {
                // The 68020's CHK2, CMP2, CALLM, RTM, CAS and CAS2.
                match rx {
                    0..=2 if wide && mode >= 2 => {
                        let w = r.next().filter(|w| w & 0x07FF == 0)?;
                        let size = Size::from_bits(u16::from(rx))?;
                        let src = ea(&mut r, mode, reg, size)?;
                        if !fits(&src, Class::Control) {
                            return None;
                        }
                        (named(if w & 0x0800 != 0 { "chk2" } else { "cmp2" }, Some(size)), vec![src, general(w)])
                    }
                    3 if cpu == Processor::M68020 && mode <= 1 => ("rtm".into(), vec![if mode == 1 { Op::A(reg as u8) } else { Op::D(reg as u8) }]),
                    3 if cpu == Processor::M68020 => {
                        let n = r.next().filter(|n| *n <= 0xFF)?;
                        let target = ea(&mut r, mode, reg, Size::L)?;
                        if !fits(&target, Class::Control) {
                            return None;
                        }
                        // Its address is a module's descriptor, not code to follow.
                        ("callm".into(), vec![Op::Quick(i32::from(n)), target])
                    }
                    6 | 7 if m68020 && op & 0x3F == 0x3C => {
                        let (a, b) = (r.next()?, r.next()?);
                        if (a | b) & 0x0E38 != 0 {
                            return None;
                        }
                        let size = if rx == 7 { Size::L } else { Size::W };
                        let parts = |w: u16| ((w & 7) as u8, (w >> 6 & 7) as u8, (w >> 12) as u8);
                        let ((c1, u1, r1), (c2, u2, r2)) = (parts(a), parts(b));
                        (named("cas2", Some(size)), vec![Op::Pair(c1, c2), Op::Pair(u1, u2), Op::RegPair(r1, r2)])
                    }
                    5..=7 if m68020 => {
                        let w = r.next().filter(|w| w & 0xFE38 == 0)?;
                        let size = Size::from_bits(u16::from(rx) - 5)?;
                        let dst = ea(&mut r, mode, reg, size)?;
                        if !fits(&dst, Class::MemoryAlterable) {
                            return None;
                        }
                        (named("cas", Some(size)), vec![Op::D((w & 7) as u8), Op::D((w >> 6 & 7) as u8), dst])
                    }
                    _ => return None,
                }
            } else {
                match rx {
                    4 => {
                        let kind = size_bits as usize;
                        let n = r.next().filter(|n| *n <= 0xFF)?;
                        let dst = ea(&mut r, mode, reg, Size::B)?;
                        let ok = if kind == 0 { fits(&dst, Class::Data) && !matches!(dst, Op::Imm(..)) } else { fits(&dst, Class::DataAlterable) };
                        if !ok {
                            return None;
                        }
                        (["btst", "bchg", "bclr", "bset"][kind].to_string(), vec![Op::Quick(i32::from(n)), dst])
                    }
                    0..=3 | 5 | 6 => {
                        let name = ["ori", "andi", "subi", "addi", "", "eori", "cmpi"][rx as usize];
                        if matches!(rx, 0 | 1 | 5) && (op & 0xFF) == 0x3C {
                            let at = r.here();
                            let value = r.next().filter(|v| *v <= 0xFF)?;
                            (named(name, Some(Size::B)), vec![Op::Imm(u32::from(value), Size::B, at), Op::Ccr])
                        } else if matches!(rx, 0 | 1 | 5) && (op & 0xFF) == 0x7C {
                            let at = r.here();
                            let value = r.next()?;
                            (named(name, Some(Size::W)), vec![Op::Imm(u32::from(value), Size::W, at), Op::Sr])
                        } else {
                            let size = Size::from_bits(size_bits)?;
                            let src = ea(&mut r, 7, 4, size)?;
                            let dst = ea(&mut r, mode, reg, size)?;
                            // The 68020's CMPI may compare with PC-relative data.
                            let ok = fits(&dst, Class::DataAlterable) || (rx == 6 && wide && fits(&dst, Class::Data) && !matches!(dst, Op::Imm(..)));
                            if !ok {
                                return None;
                            }
                            (named(name, Some(size)), vec![src, dst])
                        }
                    }
                    // The 68010's MOVES.
                    7 if m68010 => {
                        let size = Size::from_bits(size_bits)?;
                        let w = r.next().filter(|w| w & 0x07FF == 0)?;
                        let place = ea(&mut r, mode, reg, size)?;
                        if !fits(&place, Class::MemoryAlterable) {
                            return None;
                        }
                        let ops = if w & 0x0800 != 0 { vec![general(w), place] } else { vec![place, general(w)] };
                        (named("moves", Some(size)), ops)
                    }
                    _ => return None,
                }
            }
        }
        0x1..=0x3 => {
            let size = match op >> 12 {
                1 => Size::B,
                3 => Size::W,
                _ => Size::L,
            };
            let src = ea(&mut r, mode, reg, size)?;
            if size == Size::B && matches!(src, Op::A(_)) {
                return None;
            }
            let (dmode, dreg) = ((op >> 6) & 7, (op >> 9) & 7);
            if dmode == 1 {
                if size == Size::B {
                    return None;
                }
                (named("movea", Some(size)), vec![src, Op::A(dreg as u8)])
            } else {
                let dst = ea(&mut r, dmode, dreg, size)?;
                if !fits(&dst, Class::DataAlterable) {
                    return None;
                }
                (named("move", Some(size)), vec![src, dst])
            }
        }
        0x4 => {
            match op {
                0x4AFC => {
                    flow = Flow::Return;
                    ("illegal".into(), vec![])
                }
                0x4E70 => ("reset".into(), vec![]),
                0x4E71 => ("nop".into(), vec![]),
                0x4E72 => {
                    let at = r.here();
                    ("stop".into(), vec![Op::Imm(u32::from(r.next()?), Size::W, at)])
                }
                0x4E73 | 0x4E75 | 0x4E77 => {
                    flow = Flow::Return;
                    ([("rte"), ("rts"), ("rtr")][usize::from((op & 7) / 2 - 1).min(2)].to_string(), vec![])
                }
                0x4E74 if m68010 => {
                    flow = Flow::Return;
                    // Its displacement is signed: RTD #-2.
                    ("rtd".into(), vec![Op::Quick(i32::from(r.next()? as i16))])
                }
                0x4E76 => ("trapv".into(), vec![]),
                0x4E7A | 0x4E7B if m68010 => {
                    let w = r.next()?;
                    let name = crate::asm::m68k::control_register(w & 0x0FFF, cpu)?;
                    let ops = if op & 1 == 0 { vec![Op::Control(name), general(w)] } else { vec![general(w), Op::Control(name)] };
                    ("movec".into(), ops)
                }
                _ if op & 0xFFF0 == 0x4E40 => ("trap".into(), vec![Op::Quick(i32::from(op & 15))]),
                _ if op & 0xFFF8 == 0x4E50 => ("link".into(), vec![Op::A(reg as u8), Op::Quick(i32::from(r.next()? as i16))]),
                _ if op & 0xFFF8 == 0x4808 && wide => ("link.l".into(), vec![Op::A(reg as u8), Op::Quick(r.long()? as i32)]),
                _ if op & 0xFFF8 == 0x4848 && m68010 => ("bkpt".into(), vec![Op::Quick(i32::from(reg))]),
                _ if op & 0xFFF8 == 0x49C0 && wide => ("extb.l".into(), vec![Op::D(reg as u8)]),
                _ if op & 0xFFF8 == 0x4E58 => ("unlk".into(), vec![Op::A(reg as u8)]),
                _ if op & 0xFFF8 == 0x4E60 => ("move.l".into(), vec![Op::A(reg as u8), Op::Usp]),
                _ if op & 0xFFF8 == 0x4E68 => ("move.l".into(), vec![Op::Usp, Op::A(reg as u8)]),
                _ if op & 0xFFC0 == 0x4E80 || op & 0xFFC0 == 0x4EC0 => {
                    let target = ea(&mut r, mode, reg, Size::L)?;
                    if !fits(&target, Class::Control) {
                        return None;
                    }
                    let jsr = op & 0x40 == 0;
                    flow = if jsr { Flow::Call } else { Flow::Jump };
                    ((if jsr { "jsr" } else { "jmp" }).to_string(), vec![target])
                }
                _ if op & 0xFFC0 == 0x40C0 || (op & 0xFFC0 == 0x42C0 && m68010) => {
                    let dst = ea(&mut r, mode, reg, Size::W)?;
                    if !fits(&dst, Class::DataAlterable) {
                        return None;
                    }
                    ("move.w".into(), vec![if op & 0x0200 == 0 { Op::Sr } else { Op::Ccr }, dst])
                }
                _ if op & 0xFFC0 == 0x44C0 || op & 0xFFC0 == 0x46C0 => {
                    let src = ea(&mut r, mode, reg, Size::W)?;
                    if !fits(&src, Class::Data) {
                        return None;
                    }
                    ("move.w".into(), vec![src, if op & 0x0200 == 0 { Op::Ccr } else { Op::Sr }])
                }
                _ if op & 0xF900 == 0x4000 && size_bits != 3 => {
                    // NEGX, CLR, NEG, NOT; TST is 4A00.
                    let size = Size::from_bits(size_bits)?;
                    let dst = ea(&mut r, mode, reg, size)?;
                    if !fits(&dst, Class::DataAlterable) {
                        return None;
                    }
                    let name = ["negx", "clr", "neg", "not"][usize::from((op >> 9) & 3)];
                    (named(name, Some(size)), vec![dst])
                }
                _ if op & 0xFF00 == 0x4A00 && size_bits != 3 => {
                    let size = Size::from_bits(size_bits)?;
                    let dst = ea(&mut r, mode, reg, size)?;
                    // The 68020 may test an address register (.W or .L), PC-relative data or a number.
                    let ok = fits(&dst, Class::DataAlterable) || (wide && (fits(&dst, Class::Data) || (matches!(dst, Op::A(_)) && size != Size::B)));
                    if !ok {
                        return None;
                    }
                    (named("tst", Some(size)), vec![dst])
                }
                _ if op & 0xFFC0 == 0x4AC0 => {
                    let dst = ea(&mut r, mode, reg, Size::B)?;
                    if !fits(&dst, Class::DataAlterable) {
                        return None;
                    }
                    ("tas".into(), vec![dst])
                }
                _ if op & 0xFFC0 == 0x4800 => {
                    let dst = ea(&mut r, mode, reg, Size::B)?;
                    if !fits(&dst, Class::DataAlterable) {
                        return None;
                    }
                    ("nbcd".into(), vec![dst])
                }
                _ if op & 0xFFF8 == 0x4840 => ("swap".into(), vec![Op::D(reg as u8)]),
                _ if op & 0xFFC0 == 0x4840 => {
                    let src = ea(&mut r, mode, reg, Size::L)?;
                    if !fits(&src, Class::Control) {
                        return None;
                    }
                    ("pea".into(), vec![src])
                }
                _ if op & 0xFFF8 == 0x4880 => ("ext.w".into(), vec![Op::D(reg as u8)]),
                _ if op & 0xFFF8 == 0x48C0 => ("ext.l".into(), vec![Op::D(reg as u8)]),
                _ if op & 0xFB80 == 0x4880 => {
                    let size = if op & 0x40 != 0 { Size::L } else { Size::W };
                    let mask = r.next().filter(|m| *m != 0)?;
                    let place = ea(&mut r, mode, reg, size)?;
                    if op & 0x0400 == 0 {
                        // Registers to memory: to -(An) the mask runs the other way.
                        match place {
                            Op::Pre(_) => (named("movem", Some(size)), vec![Op::List(mask.reverse_bits()), place]),
                            _ if fits(&place, Class::ControlAlterable) => (named("movem", Some(size)), vec![Op::List(mask), place]),
                            _ => return None,
                        }
                    } else if matches!(place, Op::Post(_)) || fits(&place, Class::Control) {
                        (named("movem", Some(size)), vec![place, Op::List(mask)])
                    } else {
                        return None;
                    }
                }
                // The 68020's long multiplies and divides.
                _ if op & 0xFF80 == 0x4C00 && wide => {
                    let w = r.next().filter(|w| w & 0x83F8 == 0)?;
                    let src = ea(&mut r, mode, reg, Size::L)?;
                    if !fits(&src, Class::Data) {
                        return None;
                    }
                    let signed = w & 0x0800 != 0;
                    let (low, high) = ((w >> 12 & 7) as u8, (w & 7) as u8);
                    let wide64 = w & 0x0400 != 0;
                    let (name, dst) = if op & 0x40 == 0 {
                        match (wide64, low == high) {
                            (true, _) => (if signed { "muls.l" } else { "mulu.l" }, Op::Pair(high, low)),
                            (false, true) => (if signed { "muls.l" } else { "mulu.l" }, Op::D(low)),
                            (false, false) => return None,
                        }
                    } else {
                        match (wide64, low == high) {
                            (true, _) => (if signed { "divs.l" } else { "divu.l" }, Op::Pair(high, low)),
                            (false, true) => (if signed { "divs.l" } else { "divu.l" }, Op::D(low)),
                            (false, false) => (if signed { "divsl.l" } else { "divul.l" }, Op::Pair(high, low)),
                        }
                    };
                    (name.to_string(), vec![src, dst])
                }
                _ if op & 0xF1C0 == 0x41C0 => {
                    let src = ea(&mut r, mode, reg, Size::L)?;
                    if !fits(&src, Class::Control) {
                        return None;
                    }
                    ("lea".into(), vec![src, Op::A(rx)])
                }
                _ if op & 0xF1C0 == 0x4180 || (op & 0xF1C0 == 0x4100 && m68020) => {
                    let size = if op & 0x0080 != 0 { Size::W } else { Size::L };
                    let src = ea(&mut r, mode, reg, size)?;
                    if !fits(&src, Class::Data) {
                        return None;
                    }
                    (named("chk", Some(size)), vec![src, Op::D(rx)])
                }
                _ => return None,
            }
        }
        0x5 => {
            let cc = usize::from((op >> 8) & 15);
            if size_bits == 3 {
                if mode == 1 {
                    let at = r.here();
                    let d = r.next()? as i16;
                    let target = at.wrapping_add(d as i32 as u32);
                    if target % 2 == 1 {
                        return None;
                    }
                    flow = Flow::Branch;
                    (if cc == 1 { "dbra".to_string() } else { format!("db{}", CONDITIONS[cc]) }, vec![Op::D(reg as u8), Op::Target(target)])
                } else if mode == 7 && (2..=4).contains(&reg) {
                    // The 68020's TRAPcc, with a word or a long the handler may read, or none.
                    if !wide {
                        return None;
                    }
                    let at = r.here();
                    match reg {
                        2 => (format!("trap{}.w", CONDITIONS[cc]), vec![Op::Imm(u32::from(r.next()?), Size::W, at)]),
                        3 => (format!("trap{}.l", CONDITIONS[cc]), vec![Op::Imm(r.long()?, Size::L, at)]),
                        _ => (format!("trap{}", CONDITIONS[cc]), vec![]),
                    }
                } else {
                    let dst = ea(&mut r, mode, reg, Size::B)?;
                    if !fits(&dst, Class::DataAlterable) {
                        return None;
                    }
                    (format!("s{}", CONDITIONS[cc]), vec![dst])
                }
            } else {
                let size = Size::from_bits(size_bits)?;
                let dst = ea(&mut r, mode, reg, size)?;
                if !fits(&dst, Class::Alterable) || (size == Size::B && matches!(dst, Op::A(_))) {
                    return None;
                }
                let n = if rx == 0 { 8 } else { i32::from(rx) };
                (named(if op & 0x100 == 0 { "addq" } else { "subq" }, Some(size)), vec![Op::Quick(n), dst])
            }
        }
        0x6 => {
            let cc = usize::from((op >> 8) & 15);
            let at = pc.wrapping_add(2);
            let (target, size) = match op as u8 {
                0 => (at.wrapping_add(r.next()? as i16 as i32 as u32), ".w"),
                0xFF if wide => (at.wrapping_add(r.long()?), ".l"),
                0xFF => return None,
                d => (at.wrapping_add(d as i8 as i32 as u32), ".s"),
            };
            if target % 2 == 1 {
                return None;
            }
            flow = match cc {
                0 => Flow::Jump,
                1 => Flow::Call,
                _ => Flow::Branch,
            };
            let name = match cc {
                0 => "bra".to_string(),
                1 => "bsr".to_string(),
                _ => format!("b{}", CONDITIONS[cc]),
            };
            (format!("{name}{size}"), vec![Op::Target(target)])
        }
        0x7 => {
            if op & 0x100 != 0 {
                return None;
            }
            ("moveq".into(), vec![Op::Quick(i32::from(op as u8 as i8)), Op::D(rx)])
        }
        0x8 | 0xC => {
            let or = op >> 12 == 0x8;
            let opmode = (op >> 6) & 7;
            match opmode {
                3 | 7 => {
                    let src = ea(&mut r, mode, reg, Size::W)?;
                    if !fits(&src, Class::Data) {
                        return None;
                    }
                    let name = match (or, opmode) {
                        (true, 3) => "divu.w",
                        (true, _) => "divs.w",
                        (false, 3) => "mulu.w",
                        _ => "muls.w",
                    };
                    (name.to_string(), vec![src, Op::D(rx)])
                }
                4 if mode <= 1 => {
                    let name = if or { "sbcd" } else { "abcd" };
                    let ops = if mode == 0 { vec![Op::D(reg as u8), Op::D(rx)] } else { vec![Op::Pre(reg as u8), Op::Pre(rx)] };
                    (name.to_string(), ops)
                }
                // The 68020's PACK and UNPK.
                5 | 6 if or && mode <= 1 && m68020 => {
                    let at = r.here();
                    let adjust = r.next()?;
                    let ops = if mode == 0 { vec![Op::D(reg as u8), Op::D(rx)] } else { vec![Op::Pre(reg as u8), Op::Pre(rx)] };
                    let mut ops = ops;
                    ops.push(Op::Imm(u32::from(adjust), Size::W, at));
                    ((if opmode == 5 { "pack" } else { "unpk" }).to_string(), ops)
                }
                5 if !or && mode == 0 => ("exg".into(), vec![Op::D(rx), Op::D(reg as u8)]),
                5 if !or && mode == 1 => ("exg".into(), vec![Op::A(rx), Op::A(reg as u8)]),
                6 if !or && mode == 1 => ("exg".into(), vec![Op::D(rx), Op::A(reg as u8)]),
                0..=2 => {
                    let size = Size::from_bits(opmode)?;
                    let src = ea(&mut r, mode, reg, size)?;
                    if !fits(&src, Class::Data) {
                        return None;
                    }
                    (named(if or { "or" } else { "and" }, Some(size)), vec![src, Op::D(rx)])
                }
                _ => {
                    let size = Size::from_bits(opmode - 4)?;
                    let dst = ea(&mut r, mode, reg, size)?;
                    if !fits(&dst, Class::MemoryAlterable) {
                        return None;
                    }
                    (named(if or { "or" } else { "and" }, Some(size)), vec![Op::D(rx), dst])
                }
            }
        }
        0x9 | 0xD => {
            let name = if op >> 12 == 0x9 { "sub" } else { "add" };
            let opmode = (op >> 6) & 7;
            match opmode {
                3 | 7 => {
                    let size = if opmode == 3 { Size::W } else { Size::L };
                    let src = ea(&mut r, mode, reg, size)?;
                    (named(&format!("{name}a"), Some(size)), vec![src, Op::A(rx)])
                }
                4..=6 if mode <= 1 => {
                    let size = Size::from_bits(opmode - 4)?;
                    let ops = if mode == 0 { vec![Op::D(reg as u8), Op::D(rx)] } else { vec![Op::Pre(reg as u8), Op::Pre(rx)] };
                    (named(&format!("{name}x"), Some(size)), ops)
                }
                0..=2 => {
                    let size = Size::from_bits(opmode)?;
                    let src = ea(&mut r, mode, reg, size)?;
                    if size == Size::B && matches!(src, Op::A(_)) {
                        return None;
                    }
                    (named(name, Some(size)), vec![src, Op::D(rx)])
                }
                _ => {
                    let size = Size::from_bits(opmode - 4)?;
                    let dst = ea(&mut r, mode, reg, size)?;
                    if !fits(&dst, Class::MemoryAlterable) {
                        return None;
                    }
                    (named(name, Some(size)), vec![Op::D(rx), dst])
                }
            }
        }
        0xB => {
            let opmode = (op >> 6) & 7;
            match opmode {
                3 | 7 => {
                    let size = if opmode == 3 { Size::W } else { Size::L };
                    let src = ea(&mut r, mode, reg, size)?;
                    (named("cmpa", Some(size)), vec![src, Op::A(rx)])
                }
                0..=2 => {
                    let size = Size::from_bits(opmode)?;
                    let src = ea(&mut r, mode, reg, size)?;
                    if size == Size::B && matches!(src, Op::A(_)) {
                        return None;
                    }
                    (named("cmp", Some(size)), vec![src, Op::D(rx)])
                }
                _ if mode == 1 => (named("cmpm", Size::from_bits(opmode - 4)), vec![Op::Post(reg as u8), Op::Post(rx)]),
                _ => {
                    let size = Size::from_bits(opmode - 4)?;
                    let dst = ea(&mut r, mode, reg, size)?;
                    if !fits(&dst, Class::DataAlterable) {
                        return None;
                    }
                    (named("eor", Some(size)), vec![Op::D(rx), dst])
                }
            }
        }
        0xE => {
            const KINDS: [&str; 4] = ["as", "ls", "rox", "ro"];
            let left = op & 0x100 != 0;
            if size_bits == 3 && op & 0x0800 != 0 {
                // The 68020's bit fields.
                if !m68020 {
                    return None;
                }
                const FIELDS: [&str; 8] = ["bftst", "bfextu", "bfchg", "bfexts", "bfclr", "bfffo", "bfset", "bfins"];
                let kind = usize::from(op >> 8 & 7);
                let w = r.next().filter(|w| w & 0x8000 == 0)?;
                let field = field_text(w)?;
                let place = ea(&mut r, mode, reg, Size::B)?;
                let writes = matches!(kind, 2 | 4 | 6 | 7);
                let ok = matches!(place, Op::D(_)) || fits(&place, if writes { Class::ControlAlterable } else { Class::Control });
                let register = Op::D((w >> 12 & 7) as u8);
                if !ok || (matches!(kind, 0 | 2 | 4 | 6) && w & 0x7000 != 0) {
                    return None;
                }
                let place = Op::Field(Box::new(place), field);
                let ops = match kind {
                    1 | 3 | 5 => vec![place, register],
                    7 => vec![register, place],
                    _ => vec![place],
                };
                (FIELDS[kind].to_string(), ops)
            } else if size_bits == 3 {
                if op & 0x0800 != 0 {
                    return None;
                }
                let dst = ea(&mut r, mode, reg, Size::W)?;
                if !fits(&dst, Class::MemoryAlterable) {
                    return None;
                }
                let name = format!("{}{}.w", KINDS[usize::from((op >> 9) & 3)], if left { 'l' } else { 'r' });
                (name, vec![dst])
            } else {
                let size = Size::from_bits(size_bits)?;
                let count = if op & 0x20 != 0 { Op::D(rx) } else { Op::Quick(if rx == 0 { 8 } else { i32::from(rx) }) };
                let name = format!("{}{}", KINDS[usize::from((op >> 3) & 3)], if left { 'l' } else { 'r' });
                (named(&name, Some(size)), vec![count, Op::D(reg as u8)])
            }
        }
        _ => return None,
    };
    Some(Insn { mnemonic, ops, length: r.length, flow })
}

// ---------- Writing it ----------

fn hex(n: u32) -> String {
    if n <= 9 { n.to_string() } else { format!("${n:X}") }
}

fn signed(n: i32) -> String {
    match n {
        -9..=9 => n.to_string(),
        n if n < 0 => format!("-${:X}", n.unsigned_abs()),
        n => format!("${n:X}"),
    }
}

fn register(n: u8, address: bool) -> String {
    match (address, n) {
        (true, 7) => "sp".into(),
        (true, n) => format!("a{n}"),
        (false, n) => format!("d{n}"),
    }
}

fn index(x: &Xn) -> String {
    let scale = if x.scale == 0 { String::new() } else { format!("*{}", 1 << x.scale) };
    format!("{}.{}{scale}", register(x.reg, x.address), if x.long { 'l' } else { 'w' })
}

/// MOVEM's list as d0-d3/a0/a5-a6.
fn list(mask: u16) -> String {
    let mut parts = Vec::new();
    for (bank, address) in [(0, false), (8, true)] {
        let mut n = 0;
        while n < 8 {
            if mask & (1 << (bank + n)) == 0 {
                n += 1;
                continue;
            }
            let start = n;
            while n + 1 < 8 && mask & (1 << (bank + n + 1)) != 0 {
                n += 1;
            }
            let first = if address { format!("a{start}") } else { format!("d{start}") };
            parts.push(if n == start { first } else { format!("{first}-{}{n}", if address { 'a' } else { 'd' }) });
            n += 1;
        }
    }
    parts.join("/")
}

/// What turns addresses into names, while the source is written.
trait Names {
    /// A long at `at` holding `value`: a label if it's an address in the program.
    fn long(&self, value: u32, at: u32) -> String;
    /// An address used relative to PC or branched to.
    fn place(&self, address: u32) -> String;
    /// A short address: a label (with .w) if it's in the program, otherwise as it is.
    fn short(&self, address: u32) -> String;
}

/// A 68020 address with a full extension word, written with every size given, so that it assembles back to the
/// same words: ([bd.w,a0,d1.l*4],od.l) and the rest.
fn full_text(f: &FullEa, names: &dyn Names) -> String {
    let pc = f.base.is_none();
    let displacement = |d: &Disp, from_pc: bool| -> Option<String> {
        match *d {
            Disp::Null => None,
            Disp::Word(v) if from_pc => Some(format!("{}.w", names.place(v as u32))),
            Disp::Word(v) => Some(format!("{}.w", signed(v as i32))),
            Disp::Long(v, _) if from_pc => Some(format!("{}.l", names.place(v as u32))),
            Disp::Long(v, at) => {
                let name = names.long(v as u32, at);
                Some(format!("{}.l", if name.starts_with('$') { signed(v as i32) } else { name }))
            }
        }
    };
    let base = match (f.base, f.base_suppressed) {
        (Some(n), false) => register(n, true),
        (Some(n), true) => format!("za{n}"),
        (None, false) => "pc".into(),
        (None, true) => "zpc".into(),
    };
    // A suppressed index is written when it isn't ZD0.W, or when nothing else would make it the full word's.
    let plain = Xn { reg: 0, address: false, long: false, scale: 0 };
    let forced = f.indirect == Indirect::No && !f.base_suppressed;
    let x = if f.index_suppressed {
        (f.index != plain || forced).then(|| format!("z{}", index(&f.index).replace("sp.", "a7.")))
    } else {
        Some(index(&f.index))
    };
    let bd = displacement(&f.bd, pc);
    let od = displacement(&f.od, false);
    let join = |parts: Vec<Option<String>>| parts.into_iter().flatten().collect::<Vec<_>>().join(",");
    match f.indirect {
        Indirect::No => format!("({})", join(vec![bd, Some(base), x])),
        Indirect::Pre => format!("([{}]{})", join(vec![bd, Some(base), x]), od.map(|o| format!(",{o}")).unwrap_or_default()),
        Indirect::Post => format!("([{}],{})", join(vec![bd, Some(base)]), join(vec![x, od])),
    }
}

fn operand(op: &Op, names: &dyn Names) -> String {
    match op {
        Op::D(n) => register(*n, false),
        Op::A(n) => register(*n, true),
        Op::Ind(n) => format!("({})", register(*n, true)),
        Op::Post(n) => format!("({})+", register(*n, true)),
        Op::Pre(n) => format!("-({})", register(*n, true)),
        // Offsets below a register are written in decimal, as library calls (-552(a6)) and stack frames are.
        Op::Disp(n, d) if *d < 0 => format!("{d}({})", register(*n, true)),
        Op::Disp(n, d) => format!("{}({})", signed(i32::from(*d)), register(*n, true)),
        Op::Index(n, d, x) => format!("{}({},{})", signed(i32::from(*d)), register(*n, true), index(x)),
        Op::AbsW(a) => names.short(*a),
        Op::AbsL(a, at) => names.long(*a, *at),
        Op::Pc(t) => format!("{}(pc)", names.place(*t)),
        Op::PcIndex(t, x) => format!("{}(pc,{})", names.place(*t), index(x)),
        Op::PcW(t) => format!("({}.w,pc)", names.place(*t)),
        Op::PcIndexB(t, x) => format!("({}.b,pc,{})", names.place(*t), index(x)),
        Op::Full(f) => full_text(f, names),
        Op::Imm(v, Size::L, at) => format!("#{}", names.long(*v, *at)),
        Op::Imm(v, _, _) => format!("#{}", hex(*v)),
        Op::Quick(n) => format!("#{}", signed(*n)),
        Op::Sr => "sr".into(),
        Op::Ccr => "ccr".into(),
        Op::Usp => "usp".into(),
        Op::List(mask) => list(*mask),
        Op::Target(t) => names.place(*t),
        Op::Pair(high, low) => format!("d{high}:d{low}"),
        Op::RegPair(a, b) => format!("({}):({})", register(a & 7, *a >= 8).replace("sp", "a7"), register(b & 7, *b >= 8).replace("sp", "a7")),
        Op::Control(name) => (*name).into(),
        Op::Field(inner, field) => format!("{}{field}", operand(inner, names)),
    }
}

fn text(insn: &Insn, names: &dyn Names) -> String {
    let operands: Vec<String> = insn.ops.iter().map(|o| operand(o, names)).collect();
    match (operands.is_empty(), insn.mnemonic.len() >= 8) {
        (true, _) => insn.mnemonic.clone(),
        (false, false) => format!("{:<8}{}", insn.mnemonic, operands.join(",")),
        (false, true) => format!("{} {}", insn.mnemonic, operands.join(",")),
    }
}

// ---------- The system's calls ----------

/// exec.library's functions, by their offsets: the base in a6 at a program's start is always exec's.
const EXEC: &[(i32, &str)] = &[
    (-30, "Supervisor"), (-42, "Schedule"), (-48, "Reschedule"), (-54, "Switch"), (-60, "Dispatch"), (-66, "Exception"), (-72, "InitCode"), (-78, "InitStruct"), (-84, "MakeLibrary"), (-90, "MakeFunctions"), (-96, "FindResident"), (-102, "InitResident"), (-108, "Alert"), (-114, "Debug"), (-120, "Disable"), (-126, "Enable"), (-132, "Forbid"), (-138, "Permit"), (-144, "SetSR"), (-150, "SuperState"), (-156, "UserState"), (-162, "SetIntVector"), (-168, "AddIntServer"), (-174, "RemIntServer"), (-180, "Cause"), (-186, "Allocate"), (-192, "Deallocate"), (-198, "AllocMem"), (-204, "AllocAbs"), (-210, "FreeMem"), (-216, "AvailMem"), (-222, "AllocEntry"), (-228, "FreeEntry"), (-234, "Insert"), (-240, "AddHead"), (-246, "AddTail"), (-252, "Remove"), (-258, "RemHead"), (-264, "RemTail"), (-270, "Enqueue"), (-276, "FindName"), (-282, "AddTask"), (-288, "RemTask"), (-294, "FindTask"), (-300, "SetTaskPri"), (-306, "SetSignal"), (-312, "SetExcept"), (-318, "Wait"), (-324, "Signal"), (-330, "AllocSignal"), (-336, "FreeSignal"), (-342, "AllocTrap"), (-348, "FreeTrap"), (-354, "AddPort"), (-360, "RemPort"), (-366, "PutMsg"), (-372, "GetMsg"), (-378, "ReplyMsg"), (-384, "WaitPort"), (-390, "FindPort"), (-396, "AddLibrary"), (-402, "RemLibrary"), (-408, "OldOpenLibrary"), (-414, "CloseLibrary"), (-420, "SetFunction"), (-426, "SumLibrary"), (-432, "AddDevice"), (-438, "RemDevice"), (-444, "OpenDevice"), (-450, "CloseDevice"), (-456, "DoIO"), (-462, "SendIO"), (-468, "CheckIO"), (-474, "WaitIO"), (-480, "AbortIO"), (-486, "AddResource"), (-492, "RemResource"), (-498, "OpenResource"), (-510, "RawMayGetChar"), (-516, "RawPutChar"), (-522, "RawDoFmt"), (-528, "GetCC"), (-534, "TypeOfMem"), (-540, "Procure"), (-546, "Vacate"), (-552, "OpenLibrary"), (-558, "InitSemaphore"), (-564, "ObtainSemaphore"), (-570, "ReleaseSemaphore"), (-576, "AttemptSemaphore"), (-582, "ObtainSemaphoreList"), (-588, "ReleaseSemaphoreList"), (-594, "FindSemaphore"), (-600, "AddSemaphore"), (-606, "RemSemaphore"), (-612, "SumKickData"), (-618, "AddMemList"), (-624, "CopyMem"), (-630, "CopyMemQuick"), (-636, "CacheClearU"), (-642, "CacheClearE"), (-648, "CacheControl"), (-654, "CreateIORequest"), (-660, "DeleteIORequest"), (-666, "CreateMsgPort"), (-672, "DeleteMsgPort"), (-678, "ObtainSemaphoreShared"), (-684, "AllocVec"), (-690, "FreeVec"), (-696, "CreatePrivatePool"), (-702, "DeletePrivatePool"), (-708, "AllocPooled"), (-714, "FreePooled"), (-720, "AttemptSemaphoreShared"), (-726, "ColdReboot"), (-732, "StackSwap"),
];

/// The most used functions of dos, graphics and intuition, for when the NDK isn't installed.
const DOS: &[(i32, &str)] = &[
    (-30, "Open"), (-36, "Close"), (-42, "Read"), (-48, "Write"), (-54, "Input"), (-60, "Output"), (-66, "Seek"), (-72, "DeleteFile"), (-78, "Rename"), (-84, "Lock"), (-90, "UnLock"), (-96, "DupLock"), (-102, "Examine"), (-108, "ExNext"), (-114, "Info"), (-120, "CreateDir"), (-126, "CurrentDir"), (-132, "IoErr"), (-138, "CreateProc"), (-144, "Exit"), (-150, "LoadSeg"), (-156, "UnLoadSeg"), (-174, "DeviceProc"), (-180, "SetComment"), (-186, "SetProtection"), (-192, "DateStamp"), (-198, "Delay"), (-204, "WaitForChar"), (-210, "ParentDir"), (-216, "IsInteractive"), (-222, "Execute"), (-342, "FPuts"), (-606, "SystemTagList"), (-798, "ReadArgs"), (-858, "FreeArgs"), (-948, "PutStr"), (-954, "VPrintf"),
];

const GRAPHICS: &[(i32, &str)] = &[(-60, "Text"), (-192, "LoadRGB4"), (-222, "LoadView"), (-228, "WaitBlit"), (-234, "SetRast"), (-240, "Move"), (-246, "Draw"), (-270, "WaitTOF"), (-306, "RectFill"), (-342, "SetAPen"), (-456, "OwnBlitter"), (-462, "DisownBlitter")];

const INTUITION: &[(i32, &str)] = &[(-66, "CloseScreen"), (-72, "CloseWindow"), (-78, "CloseWorkBench"), (-96, "DisplayBeep"), (-198, "OpenScreen"), (-204, "OpenWindow"), (-210, "OpenWorkBench"), (-348, "AutoRequest")];

/// A library's function by its offset: from the NDK when it's installed, otherwise from the lists here.
fn library_function(library: &str, offset: i32) -> Option<String> {
    let short = library.trim_end_matches(".library");
    if let Some(ndk) = crate::amiga_ndk::current()
        && let Some(f) = ndk.functions.values().find(|f| f.offset == offset && f.library.trim_end_matches(".library").eq_ignore_ascii_case(short) && !f.private)
    {
        return Some(f.name.clone());
    }
    let table = match short {
        "exec" => EXEC,
        "dos" => DOS,
        "graphics" => GRAPHICS,
        "intuition" => INTUITION,
        _ => return None,
    };
    table.iter().find(|(o, _)| *o == offset).map(|(_, n)| n.to_string())
}

/// What's followed through the code to name the system's calls and the hardware: the library whose base is in
/// a6, library bases kept in memory, the name of the library about to be opened, the address registers set to
/// the custom chips' base, and the function number last pushed for TOS.
#[derive(Clone, Default)]
struct Follow {
    a6: Option<String>,
    opened: Option<String>,
    result: Option<String>,
    custom: [bool; 8],
    pushed: Option<u32>,
}

// ---------- The whole ----------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Seen {
    Data,
    Start,
    Inside,
}

/// A line of source and the bytes it stands for, so a line that doesn't assemble back can be made data.
#[derive(Clone, Debug)]
struct Line {
    text: String,
    range: Option<(u32, u32)>,
}

struct Work<'a> {
    program: &'a Program,
    seen: BTreeMap<u32, Seen>,
    decoded: BTreeMap<u32, Insn>,
    labels: BTreeSet<u32>,
    /// Places called, which start a routine: what's followed is forgotten there.
    entries: BTreeSet<u32>,
    /// Data that can't be split by a label, by where each piece starts and its length: relocated longs, and
    /// copper instructions. A place inside one is named from its start, as one inside an instruction is.
    units: BTreeMap<u32, u32>,
    /// The copper instructions found, by where each starts: written as dc.w pairs.
    copper: BTreeSet<u32>,
}

impl Work<'_> {
    fn section(&self, address: u32) -> Option<(usize, &Section)> {
        self.program.sections.iter().enumerate().find(|(_, s)| s.holds(address))
    }

    fn byte(&self, address: u32) -> Option<u8> {
        let (_, s) = self.section(address)?;
        Some(s.bytes.get((address - s.base) as usize).copied().unwrap_or(0))
    }

    fn word(&self, address: u32) -> Option<u16> {
        if address % 2 == 1 {
            return None;
        }
        let (_, s) = self.section(address)?;
        let at = (address - s.base) as usize;
        be16(&s.bytes, at)
    }

    /// The relocation at a long, if it holds one: the section its address is in.
    fn relocation(&self, at: u32) -> Option<usize> {
        let (_, s) = self.section(at)?;
        s.relocations.get(&(at - s.base)).copied()
    }

    /// What a long holds as an address in the program, if it's one: moved to where its section is, if it's
    /// relocated, or as it is in code at its own address when it falls inside it.
    fn address_in(&self, value: u32, at: u32) -> Option<u32> {
        match self.program.layout {
            Layout::Raw => self.section(value).map(|_| value),
            _ => self.relocation(at).and_then(|target| self.program.sections.get(target)).map(|s| s.base.wrapping_add(value)),
        }
    }

    /// Whether code can be at an address: in a code hunk, an ST program's text, or anywhere in plain code.
    fn code_at(&self, address: u32) -> bool {
        let Some((_, s)) = self.section(address) else { return false };
        address.is_multiple_of(2)
            && address < s.base + s.bytes.len() as u32
            && match self.program.layout {
                Layout::Hunks => s.kind == Kind::Code,
                Layout::Tos => address < self.program.text_end.unwrap_or(0),
                Layout::Raw => true,
            }
    }

    fn decode(&self, address: u32) -> Option<Insn> {
        let word = |a: u32| self.word(a);
        decode_for(&word, address, self.program.cpu)
    }

    fn trace(&mut self, starts: Vec<u32>) {
        let mut queue = starts;
        while let Some(mut pc) = queue.pop() {
            // The address registers known to hold a place in the program along this run of code, for JSR (An).
            let mut held: [Option<u32>; 8] = [None; 8];
            while self.code_at(pc) && self.seen.get(&pc).is_none_or(|s| *s == Seen::Data) {
                // A zero long is ORI.B #0,D0, which no program runs: it's data reached as if it were code (a
                // cleared buffer or bitplane), and the code stops before it. Only a start that's given is taken
                // as it is.
                if self.word(pc) == Some(0) && self.word(pc + 2) == Some(0) && !self.program.starts.contains(&pc) {
                    break;
                }
                let Some(insn) = self.decode(pc) else { break };
                let length = insn.length;
                // In a program that's moved when it's loaded, nothing relative to PC can leave its own section.
                if self.program.layout != Layout::Raw {
                    let (index, _) = self.section(pc).expect("code is in a section");
                    let away = insn.ops.iter().any(|op| match op {
                        Op::Target(t) => self.section(*t).is_none_or(|(i, _)| i != index),
                        _ => op.pc_target().is_some_and(|t| self.section(t).is_none_or(|(i, _)| i != index)),
                    });
                    if away {
                        break;
                    }
                }
                // An instruction running off its section, or into one already found, is data.
                let (_, s) = self.section(pc).expect("code is in a section");
                if pc + length > s.base + s.bytes.len() as u32 || (2..length).step_by(2).any(|i| self.seen.get(&(pc + i)).is_some_and(|s| *s != Seen::Data)) {
                    break;
                }
                // A long that's relocated must be whole within an operand: one that starts mid-instruction
                // at an odd place can't be written.
                self.seen.insert(pc, Seen::Start);
                for i in (2..length).step_by(2) {
                    self.seen.insert(pc + i, Seen::Inside);
                }
                let jumps = matches!(insn.flow, Flow::Jump | Flow::Call);
                let mut follow = Vec::new();
                for op in &insn.ops {
                    let target = match op {
                        Op::Target(t) => Some(*t),
                        Op::Pc(t) | Op::PcW(t) if jumps => Some(*t),
                        Op::AbsL(v, at) if jumps => self.address_in(*v, *at),
                        Op::AbsW(v) if jumps && self.program.layout == Layout::Raw => self.section(*v).map(|_| *v),
                        // A place jumped to through the address register it was put in.
                        Op::Ind(n) if jumps => held[usize::from(*n)],
                        // An address put in a long as an immediate is only code when it's put in an exception
                        // vector, as an interrupt handler's is: put anywhere else (a custom chip register, a
                        // data register, a library's structure) it's usually data, a copper list or a bitplane.
                        Op::Imm(v, Size::L, at) if self.is_vector_store(&insn) => self.address_in(*v, *at),
                        _ => None,
                    };
                    if let Some(t) = target.filter(|t| self.code_at(*t)) {
                        follow.push(t);
                        if insn.flow == Flow::Call {
                            self.entries.insert(t);
                        }
                    }
                }
                for t in follow {
                    self.labels.insert(t);
                    queue.push(t);
                }
                self.hold(&insn, &mut held);
                let carries_on = matches!(insn.flow, Flow::Next | Flow::Branch | Flow::Call);
                self.decoded.insert(pc, insn);
                if !carries_on {
                    break;
                }
                pc += length;
            }
        }
    }

    /// Whether an instruction puts a long in one of the exception vectors in low memory: MOVE.L #handler,$6C.W.
    fn is_vector_store(&self, insn: &Insn) -> bool {
        let vector = |a: u32| (8..0x400).contains(&a) && a.is_multiple_of(4);
        insn.mnemonic == "move.l"
            && match insn.ops.get(1) {
                Some(Op::AbsW(a)) => vector(*a),
                Some(Op::AbsL(a, at)) => self.relocation(*at).is_none() && vector(*a),
                _ => false,
            }
    }

    /// What an instruction leaves in the address registers, for a JSR (An) or JMP (An) after it: a place put
    /// there by LEA or MOVE.L #, or nothing known once it's changed any other way.
    fn hold(&self, insn: &Insn, held: &mut [Option<u32>; 8]) {
        let base = insn.mnemonic.split('.').next().unwrap_or("");
        let place = match insn.ops.first() {
            Some(Op::AbsL(v, at)) if base == "lea" => self.address_in(*v, *at),
            Some(Op::Pc(t)) if base == "lea" => Some(*t),
            Some(Op::Imm(v, Size::L, at)) if matches!(insn.mnemonic.as_str(), "move.l" | "movea.l") => self.address_in(*v, *at),
            _ => None,
        };
        if insn.flow == Flow::Call || matches!(insn.ops.last(), Some(Op::List(_))) {
            *held = [None; 8];
        }
        for op in &insn.ops {
            if let Op::Post(n) | Op::Pre(n) = op {
                held[usize::from(*n)] = None;
            }
        }
        if base == "exg"
            && let Some(Op::A(n)) = insn.ops.first()
        {
            held[usize::from(*n)] = None;
        }
        if let Some(Op::A(n)) = insn.ops.last() {
            held[usize::from(*n)] = place;
        }
    }

    /// The copper lists, found where they can be told with confidence: a place put in COP1LC or COP2LC, or in
    /// graphics.library's copper list pointers (as a list is shown through the system), and any labelled data
    /// that reads as copper instructions all the way to the list's end, $FFFF,$FFFE.
    fn find_copper(&mut self) {
        if self.program.machine != Machine::Amiga {
            return;
        }
        for place in self.copper_installed() {
            self.take_copper(place, false);
        }
        let labelled: Vec<u32> = self.labels.iter().copied().collect();
        for place in labelled {
            self.take_copper(place, true);
        }
    }

    /// Copper instructions from a place, taken as a list if they are one. A MOVE has to be to a register the
    /// copper can set ($020 to $1FE), and the list stops at code, a relocated long or another list. One found
    /// only by how it reads (`strict`) has to name only known registers and end with $FFFF,$FFFE.
    fn take_copper(&mut self, from: u32, strict: bool) {
        let Some((_, s)) = self.section(from) else { return };
        let end = s.base + s.bytes.len() as u32;
        if !from.is_multiple_of(2) {
            return;
        }
        let mut at = from;
        let (mut pairs, mut moves, mut ended) = (Vec::new(), 0, false);
        while at + 4 <= end && pairs.len() < 8192 {
            let taken = (at..at + 4).any(|a| self.seen.contains_key(&a)) || (at.saturating_sub(3)..at + 4).any(|a| self.relocation(a).is_some()) || (at.saturating_sub(2)..at + 4).any(|a| self.copper.contains(&a));
            let (Some(first), Some(second)) = (self.word(at), self.word(at + 2)) else { break };
            if taken {
                break;
            }
            if first & 1 == 0 {
                if !(0x20..=0x1FE).contains(&first) || strict && crate::amiga_hw::custom(u32::from(first)).is_none() {
                    break;
                }
                moves += 1;
            }
            pairs.push(at);
            at += 4;
            if (first, second) == (0xFFFF, 0xFFFE) {
                ended = true;
                break;
            }
        }
        if pairs.is_empty() || strict && (!ended || moves == 0) {
            return;
        }
        for p in pairs {
            self.copper.insert(p);
            self.units.insert(p, 4);
        }
    }

    /// The places the code puts in the copper's list pointers: COP1LC and COP2LC, by their address or from a
    /// register set to the custom chips' base, and graphics.library's copinit, LOFlist and SHFlist ($26, $32 and
    /// $36 from its base); put there as an immediate, or from a register just loaded with one.
    fn copper_installed(&self) -> Vec<u32> {
        #[derive(Clone, Copy)]
        enum Base {
            Custom(i64),
            Graphics(i64),
        }
        let pointer = |offset: i64, base: Option<Base>| match base {
            Some(Base::Custom(o)) => matches!(o + offset, 0x80 | 0x84),
            Some(Base::Graphics(o)) => matches!(o + offset, 0x26 | 0x32 | 0x36),
            None => false,
        };
        let mut found = Vec::new();
        let mut follow = Follow::default();
        let mut bases: [Option<Base>; 8] = [None; 8];
        // The places held in D0-D7 and A0-A7.
        let mut places: [Option<u32>; 16] = [None; 16];
        for (&pc, insn) in &self.decoded {
            if self.entries.contains(&pc) {
                follow = Follow::default();
                bases = [None; 8];
                places = [None; 16];
            }
            // What's followed for the comments says which library's base is in a6.
            let _ = comment(self, insn, &mut follow);
            let base = insn.mnemonic.split('.').next().unwrap_or("");
            if insn.mnemonic == "move.l"
                && let [from, to] = insn.ops.as_slice()
            {
                let stored = match to {
                    Op::AbsL(a, at) => self.relocation(*at).is_none() && matches!(a & 0xFF_FFFF, 0xDF_F080 | 0xDF_F084),
                    Op::Disp(r, d) => pointer(i64::from(*d), bases[usize::from(*r)]),
                    Op::Ind(r) => pointer(0, bases[usize::from(*r)]),
                    _ => false,
                };
                let place = match from {
                    Op::Imm(v, Size::L, at) => self.address_in(*v, *at),
                    Op::D(n) => places[usize::from(*n)],
                    Op::A(n) => places[8 + usize::from(*n)],
                    _ => None,
                };
                if stored && let Some(place) = place {
                    found.push(place);
                }
            }
            // What the registers hold after it.
            let custom = matches!(insn.ops.first(), Some(Op::AbsL(0xDF_F000, at) | Op::Imm(0xDF_F000, Size::L, at)) if self.relocation(*at).is_none());
            let place = match insn.ops.first() {
                Some(Op::AbsL(v, at)) if base == "lea" => self.address_in(*v, *at),
                Some(Op::Pc(t)) if base == "lea" => Some(*t),
                Some(Op::Imm(v, Size::L, at)) if matches!(base, "move" | "movea") => self.address_in(*v, *at),
                _ => None,
            };
            let step = match (base, insn.ops.as_slice()) {
                ("adda" | "add", [Op::Imm(v, Size::L, at), Op::A(_)]) if self.relocation(*at).is_none() => Some(i64::from(*v as i32)),
                ("adda" | "add", [Op::Imm(v, Size::W, _), Op::A(_)]) => Some(i64::from(*v as u16 as i16)),
                ("addq", [Op::Quick(n), Op::A(_)]) => Some(i64::from(*n)),
                ("lea", [Op::Disp(r, d), Op::A(n)]) if r == n => Some(i64::from(*d)),
                _ => None,
            };
            match insn.ops.last() {
                Some(Op::A(n)) => {
                    let n = usize::from(*n);
                    bases[n] = match step {
                        Some(by) => bases[n].map(|b| match b {
                            Base::Custom(o) => Base::Custom(o + by),
                            Base::Graphics(o) => Base::Graphics(o + by),
                        }),
                        None if custom && matches!(base, "lea" | "move" | "movea") => Some(Base::Custom(0)),
                        None if n == 6 && matches!(base, "move" | "movea") && follow.a6.as_deref() == Some("graphics.library") => Some(Base::Graphics(0)),
                        None => None,
                    };
                    places[8 + n] = place;
                }
                Some(Op::D(n)) => places[usize::from(*n)] = place,
                Some(Op::List(_)) => places = [None; 16],
                _ => {}
            }
            if insn.flow == Flow::Call {
                places = [None; 16];
            }
        }
        found
    }

    /// The instruction an address is inside, if it's inside one past its first word; or the relocated long or
    /// copper instruction it's inside.
    fn start_of(&self, address: u32) -> u32 {
        let mut back = address & !1;
        while back > 0 && self.seen.get(&back) == Some(&Seen::Inside) {
            back -= 2;
        }
        if self.seen.get(&back) == Some(&Seen::Start) && address > back {
            return back;
        }
        match self.units.range(..address).next_back() {
            Some((&start, &length)) if address < start + length => start,
            _ => address,
        }
    }
}

/// Disassembles a program into source that assembles back to it.
pub fn disassemble(program: &Program) -> String {
    let work = analyse(program);
    let lines = write(&work);
    let lines = check(&work, lines);
    lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n") + "\n"
}

fn label_name(program: &Program, address: u32) -> String {
    if let Some(name) = program.names.get(&address) {
        return name.clone();
    }
    match program.layout {
        Layout::Hunks if program.sections.len() > 1 => {
            let (i, s) = program.sections.iter().enumerate().find(|(_, s)| s.holds(address) || s.end() == address).unwrap_or((0, &program.sections[0]));
            format!("L{i}_{:06X}", address - s.base)
        }
        Layout::Hunks => format!("L{:06X}", address - program.sections[0].base),
        _ if address > 0xFF_FFFF => format!("L{address:08X}"),
        _ => format!("L{address:06X}"),
    }
}

struct Namer<'a> {
    work: &'a Work<'a>,
}

impl Namer<'_> {
    fn label(&self, address: u32) -> String {
        let program = self.work.program;
        if self.work.labels.contains(&address) {
            return label_name(program, address);
        }
        let start = self.work.start_of(address);
        if start != address && self.work.labels.contains(&start) {
            return format!("{}+{}", label_name(program, start), address - start);
        }
        // An address just past a section's end, as a loop's end often is.
        if let Some((_, s)) = program.sections.iter().enumerate().find(|(_, s)| s.end() == address) {
            let _ = s;
            return label_name(program, address);
        }
        format!("${address:X}")
    }
}

impl Names for Namer<'_> {
    fn long(&self, value: u32, at: u32) -> String {
        let work = self.work;
        match work.address_in(value, at) {
            Some(address) if work.labels.contains(&address) || work.labels.contains(&work.start_of(address)) => self.label(address),
            // A relocated long is never written as a number, or it wouldn't be moved when the program's loaded:
            // one pointing outside its section is counted from the section's start.
            Some(_) if work.program.layout != Layout::Raw => {
                let target = work.relocation(at).and_then(|t| work.program.sections.get(t)).expect("a relocated long");
                let offset = value as i32;
                format!("{}{}{}", label_name(work.program, target.base), if offset < 0 { "-" } else { "+" }, hex(offset.unsigned_abs()))
            }
            _ => format!("${value:X}"),
        }
    }

    fn place(&self, address: u32) -> String {
        if self.work.labels.contains(&address) || self.work.labels.contains(&self.work.start_of(address)) { self.label(address) } else { format!("${address:X}") }
    }

    fn short(&self, address: u32) -> String {
        if self.work.program.layout == Layout::Raw && self.work.labels.contains(&address) {
            return format!("{}.w", self.label(address));
        }
        if address >= 0xFFFF_8000 { format!("${address:X}.w") } else { format!("{}.w", hex(address)) }
    }
}

/// What a hardware address is, on the program's machine.
fn hardware(machine: Machine, address: u32) -> Option<String> {
    match machine {
        Machine::Amiga => crate::amiga_hw::at(address).filter(|_| address & 0xFF_0000 == 0xDF_0000 || address & 0xFF_0000 == 0xBF_0000).map(|r| r.name.clone()),
        Machine::AtariSt => crate::st_hw::at(address & 0xFF_FFFF).filter(|_| crate::st_hw::is_hardware(address & 0xFF_FFFF)).map(|r| r.name.to_string()),
        _ => None,
    }
}

/// The exception vector at an address in low memory, on the Amiga and ST: $6C is the level 3 interrupt's.
fn vector(machine: Machine, address: u32) -> Option<String> {
    if !matches!(machine, Machine::Amiga | Machine::AtariSt) || !address.is_multiple_of(4) {
        return None;
    }
    let said: String = match address {
        0x08 => crate::i18n::tr("the bus error vector").into(),
        0x0C => crate::i18n::tr("the address error vector").into(),
        0x10 => crate::i18n::tr("the illegal instruction vector").into(),
        0x14 => crate::i18n::tr("the division by zero vector").into(),
        0x20 => crate::i18n::tr("the privilege violation vector").into(),
        0x24 => crate::i18n::tr("the trace vector").into(),
        0x64..=0x7C => {
            let level = (address - 0x60) / 4;
            crate::i18n::tf!("the level {level} interrupt vector")
        }
        0x80..=0xBC => {
            let number = (address - 0x80) / 4;
            crate::i18n::tf!("the TRAP #{number} vector")
        }
        _ => return None,
    };
    Some(said)
}

/// A comment for an instruction: the hardware it touches, the system call it makes. `follow` is moved on.
fn comment(work: &Work, insn: &Insn, follow: &mut Follow) -> Option<String> {
    let machine = work.program.machine;
    let mut said: Vec<String> = Vec::new();
    let base = insn.mnemonic.split('.').next().unwrap_or("");
    // LEA and PEA take an address, not what's there: the custom chips' base isn't a register of its own.
    if matches!(base, "lea" | "pea") && matches!(insn.ops.first(), Some(Op::AbsL(0xDF_F000, _))) {
        said.push(crate::i18n::tr("the custom chips").into());
    }
    let named_ops = if matches!(base, "lea" | "pea") { &insn.ops[..0] } else { &insn.ops[..] };
    for op in named_ops {
        match op {
            Op::AbsW(a) => said.extend(hardware(machine, *a).or_else(|| vector(machine, *a))),
            Op::AbsL(a, at) if work.address_in(*a, *at).is_none() || work.program.layout == Layout::Raw => said.extend(hardware(machine, *a)),
            Op::Disp(r, d) if machine == Machine::Amiga && follow.custom[usize::from(*r)] && (0..0x200).contains(d) => said.extend(crate::amiga_hw::custom(*d as u32).map(|r| r.name.clone())),
            _ => {}
        }
    }
    match (machine, base, insn.ops.as_slice()) {
        // Amiga: library calls through a6.
        (Machine::Amiga, "jsr" | "jmp", [Op::Disp(6, d)]) if *d < 0 => {
            let offset = i32::from(*d);
            match &follow.a6 {
                Some(library) => {
                    if let Some(name) = library_function(library, offset) {
                        said.push(format!("{} {name}", library.trim_end_matches(".library")));
                        if library == "exec.library" && (offset == -552 || offset == -408) {
                            follow.result = follow.opened.take();
                        }
                    }
                }
                None => {
                    if let Some(name) = library_function("exec.library", offset) {
                        said.push(crate::i18n::tf!("{name}, if a6 is exec"));
                    }
                }
            }
        }
        (Machine::Amiga, "move" | "movea", [Op::AbsW(4), Op::A(6)]) => follow.a6 = Some("exec.library".into()),
        (Machine::Amiga, "move" | "movea", [Op::AbsL(4, _), Op::A(6)]) => follow.a6 = Some("exec.library".into()),
        (Machine::Amiga, "lea", [place @ (Op::Pc(_) | Op::AbsL(..)), Op::A(1)]) => {
            let address = match place {
                Op::Pc(t) => Some(*t),
                Op::AbsL(v, at) => work.address_in(*v, *at),
                _ => None,
            };
            let name: Option<String> = address.and_then(|a| {
                let text: String = (0..40).map_while(|i| work.byte(a + i).filter(|b| *b != 0)).map(|b| b as char).collect();
                (text.ends_with(".library") || text.ends_with(".device") || text.ends_with(".resource")).then_some(text)
            });
            if let Some(name) = name {
                follow.opened = Some(name);
            }
        }
        (Machine::Amiga, "move" | "movea", [Op::D(0), Op::A(6)]) => follow.a6 = follow.result.clone(),
        (Machine::Amiga, "move" | "movea", [Op::D(0), place]) if follow.result.is_some() && !matches!(place, Op::D(_) | Op::A(_)) => {
            if let Some(address) = stored_at(work, place) {
                BASES.with(|b| b.borrow_mut().insert(address, follow.result.clone().unwrap_or_default()));
            }
        }
        (Machine::Amiga, "move" | "movea", [place, Op::A(6)]) => {
            follow.a6 = stored_at(work, place).and_then(|a| BASES.with(|b| b.borrow().get(&a).cloned()));
        }
        (Machine::Amiga, "lea" | "movea" | "move", [Op::AbsL(0xDF_F000, _) | Op::Imm(0xDF_F000, Size::L, _), Op::A(r)]) => follow.custom[usize::from(*r)] = true,
        // TOS: the function number pushed last, then the trap.
        (Machine::AtariSt, "move", [Op::Imm(n, _, _), Op::Pre(7)]) => follow.pushed = Some(*n),
        (Machine::AtariSt, "trap", [Op::Quick(n)]) => {
            if let Some(trap) = crate::st_tos::Trap::from_number(i64::from(*n)) {
                match follow.pushed.and_then(|f| crate::st_tos::function(trap, i64::from(f))) {
                    Some(f) => said.push(format!("{} {}", trap.name(), f.name)),
                    None => said.push(trap.name().to_string()),
                }
            }
            follow.pushed = None;
        }
        _ => {}
    }
    // An address register set to anything else isn't the custom chips' base any more.
    if let Some(Op::A(r)) = insn.ops.last()
        && !matches!(insn.ops.first(), Some(Op::AbsL(0xDF_F000, _) | Op::Imm(0xDF_F000, Size::L, _)))
        && base != "cmpa"
    {
        follow.custom[usize::from(*r)] = false;
        if *r == 6 && !matches!((base, insn.ops.first()), ("move" | "movea", Some(_))) {
            follow.a6 = None;
        }
    }
    said.dedup();
    (!said.is_empty()).then(|| said.join(", "))
}

thread_local! {
    /// Library bases kept in memory, found while writing the source: the address, and the library.
    static BASES: std::cell::RefCell<BTreeMap<u32, String>> = const { std::cell::RefCell::new(BTreeMap::new()) };
}

/// The address an operand stores to or reads from, when it's a fixed one.
fn stored_at(work: &Work, place: &Op) -> Option<u32> {
    match place {
        Op::Pc(t) => Some(*t),
        Op::AbsL(v, at) => work.address_in(*v, *at),
        Op::AbsW(v) => Some(*v),
        _ => None,
    }
}

/// Follows the code from where it starts, finds the copper lists, and gives a label to every place that's used.
fn analyse(program: &Program) -> Work<'_> {
    BASES.with(|b| b.borrow_mut().clear());
    let mut work = Work { program, seen: BTreeMap::new(), decoded: BTreeMap::new(), labels: BTreeSet::new(), entries: BTreeSet::new(), units: BTreeMap::new(), copper: BTreeSet::new() };
    let starts: Vec<u32> = program.starts.iter().copied().chain(program.names.keys().copied().filter(|a| work.code_at(*a) && program.layout != Layout::Tos)).collect();
    for s in &starts {
        work.labels.insert(*s);
    }
    work.trace(starts);
    // Addresses kept in data that are in code are routines reached some other way: through tables, as handlers.
    let mut tried: BTreeSet<u32> = BTreeSet::new();
    loop {
        let mut more = Vec::new();
        for (i, s) in program.sections.iter().enumerate() {
            for (&offset, &target) in &s.relocations {
                let at = s.base + offset;
                if work.seen.contains_key(&at) {
                    continue;
                }
                let data_here = match program.layout {
                    Layout::Hunks => s.kind != Kind::Code,
                    Layout::Tos => at >= program.text_end.unwrap_or(0),
                    Layout::Raw => false,
                };
                let value = be32(&s.bytes, offset as usize).unwrap_or(0);
                if let Some(address) = program.sections.get(target).map(|t| t.base.wrapping_add(value))
                    && data_here
                    && work.code_at(address)
                    && !work.seen.contains_key(&address)
                {
                    more.push(address);
                }
                let _ = i;
            }
        }
        for &p in &program.pointers {
            if let Some(a) = be32(&program.sections[0].bytes, (p - program.sections[0].base) as usize).filter(|a| work.code_at(*a) && !work.seen.contains_key(a)) {
                more.push(a);
            }
        }
        more.sort_unstable();
        more.dedup();
        // One that isn't code after all is only tried once.
        more.retain(|a| tried.insert(*a));
        if more.is_empty() {
            break;
        }
        for a in &more {
            work.labels.insert(*a);
        }
        work.trace(more);
    }
    // Every address used gets a label: where it falls inside an instruction, that instruction does.
    let mut used = Vec::new();
    for insn in work.decoded.values() {
        for op in &insn.ops {
            match op {
                Op::Target(t) => used.push(*t),
                _ if op.pc_target().is_some() => used.extend(op.pc_target()),
                Op::AbsL(v, at) | Op::Imm(v, Size::L, at) => used.extend(work.address_in(*v, *at).filter(|_| program.layout != Layout::Raw || matches!(op, Op::AbsL(..)))),
                Op::AbsW(v) if program.layout == Layout::Raw => used.extend(work.section(*v).map(|_| *v)),
                _ => {}
            }
        }
    }
    for s in &program.sections {
        for (&offset, &target) in &s.relocations {
            if let (Some(value), Some(t)) = (be32(&s.bytes, offset as usize), program.sections.get(target)) {
                used.push(t.base.wrapping_add(value));
            }
        }
        for &p in &program.pointers {
            if let Some(value) = be32(&s.bytes, (p - s.base) as usize) {
                used.push(value);
            }
        }
    }
    for address in used {
        let inside = program.sections.iter().any(|s| s.holds(address) || s.end() == address);
        if inside {
            let start = work.start_of(address);
            work.labels.insert(start);
        }
    }
    for address in program.names.keys() {
        if program.sections.iter().any(|s| s.holds(*address)) {
            work.labels.insert(*address);
        }
    }
    work.find_copper();
    // A relocated long in data is written whole, as a label: a place inside it is named from its start.
    for s in &program.sections {
        for &offset in s.relocations.keys() {
            let at = s.base + offset;
            if !(at..at + 4).any(|a| work.seen.contains_key(&a)) && work.units.range(at.saturating_sub(3)..at + 4).next().is_none() {
                work.units.insert(at, 4);
            }
        }
    }
    // A label can only be written where a line starts: one inside an instruction, a relocated long or a copper
    // instruction is its start's, and the place is named from there (L0026F4+2).
    let inside: Vec<u32> = work.labels.iter().copied().filter(|&l| work.start_of(l) != l).collect();
    for l in inside {
        work.labels.remove(&l);
        let start = work.start_of(l);
        work.labels.insert(start);
    }
    // A relocated long pointing outside its section is counted from the section's start, which needs a label.
    for s in &program.sections {
        for (&offset, &target) in &s.relocations {
            if let (Some(value), Some(t)) = (be32(&s.bytes, offset as usize), program.sections.get(target)) {
                let address = t.base.wrapping_add(value);
                if !work.labels.contains(&address) && !work.labels.contains(&work.start_of(address)) {
                    work.labels.insert(t.base);
                }
            }
        }
    }
    work
}

fn write(work: &Work) -> Vec<Line> {
    BASES.with(|b| b.borrow_mut().clear());
    let program = work.program;
    let namer = Namer { work };
    let mut lines = vec![Line { text: format!("; {}.", program.what), range: None }, Line { text: crate::i18n::tr("; Disassembled by Limoniter: it assembles with Limoniter's own assembler or vasm (Motorola syntax) to the same bytes.").into(), range: None }, Line { text: String::new(), range: None }];
    // Any processor but the 68000 is chosen as vasm and Devpac choose it.
    if program.cpu != Processor::M68000 {
        lines.push(Line { text: format!("        machine {}", program.cpu.name().to_ascii_lowercase()), range: None });
    }
    // vasm would otherwise make some instructions shorter (JSR to BSR.W, say), and the bytes wouldn't be the same.
    lines.push(Line { text: format!("{:<48}; {}", "        opt     o-", crate::i18n::tr("nothing optimised, for vasm")), range: None });
    if program.layout == Layout::Raw {
        lines.push(Line { text: format!("        org     ${:X}", program.sections[0].base), range: None });
    }
    // The custom chips' registers the copper lists set are named, and defined here once they're known.
    let equates_at = lines.len();
    let mut equates: BTreeMap<String, u16> = BTreeMap::new();
    let mut follow = Follow::default();
    for (i, s) in program.sections.iter().enumerate() {
        if program.layout == Layout::Hunks {
            let kind = match s.kind {
                Kind::Code => "code",
                Kind::Data => "data",
                Kind::Bss => "bss",
            };
            let memory = match s.memory {
                Memory::Chip => "_c",
                Memory::Fast => "_f",
                Memory::Any => "",
            };
            lines.push(Line { text: String::new(), range: None });
            lines.push(Line { text: format!("        section hunk{i},{kind}{memory}"), range: None });
        }
        let end = s.base + s.bytes.len() as u32;
        let mut at = s.base;
        while at < end {
            if work.labels.contains(&at) {
                if work.entries.contains(&at) {
                    follow = Follow::default();
                    lines.push(Line { text: String::new(), range: None });
                }
                lines.push(Line { text: format!("{}:", label_name(program, at)), range: None });
            }
            if program.layout == Layout::Tos && Some(at) == program.text_end && at != s.base {
                lines.push(Line { text: "; The data".into(), range: None });
            }
            if let Some(insn) = work.decoded.get(&at) {
                let mut text = format!("        {}", text(insn, &namer));
                if let Some(said) = comment(work, insn, &mut follow) {
                    text = format!("{text:<48}; {said}");
                }
                lines.push(Line { text, range: Some((at, insn.length)) });
                at += insn.length;
                continue;
            }
            if work.copper.contains(&at) {
                lines.push(Line { text: copper_text(work, at, &mut equates), range: Some((at, 4)) });
                at += 4;
                continue;
            }
            // Data, up to the next label, instruction, copper list, or the TOS data's start.
            let mut stop = at + 1;
            while stop < end && !work.labels.contains(&stop) && !work.decoded.contains_key(&stop) && !work.copper.contains(&stop) && Some(stop) != program.text_end {
                stop += 1;
            }
            lines.extend(data_lines(work, &namer, at, stop, Style::Text));
            at = stop;
        }
        // BSS: room, with any labels in it.
        if s.bss > 0 {
            let mut at = end;
            if program.layout == Layout::Tos {
                lines.push(Line { text: "; The BSS".into(), range: None });
            }
            while at < s.end() {
                if work.labels.contains(&at) {
                    lines.push(Line { text: format!("{}:", label_name(program, at)), range: None });
                }
                let next = (at + 1..s.end()).find(|a| work.labels.contains(a)).unwrap_or(s.end());
                lines.push(Line { text: format!("        ds.b    {}", next - at), range: None });
                at = next;
            }
        }
        if work.labels.contains(&s.end()) && program.sections.get(i + 1).is_none_or(|n| n.base != s.end()) {
            lines.push(Line { text: format!("{}:", label_name(program, s.end())), range: None });
        }
    }
    if !equates.is_empty() {
        let mut named = vec![Line { text: crate::i18n::tr("; The custom chips' registers the copper lists set, from $DFF000.").into(), range: None }];
        named.extend(equates.iter().map(|(name, offset)| Line { text: format!("{name:<16}equ     ${offset:03X}"), range: None }));
        lines.splice(equates_at..equates_at, named);
    }
    lines
}

/// A copper instruction as its two words: a MOVE with its register's name, a WAIT or SKIP with where the beam is
/// waited for, and the list's end.
fn copper_text(work: &Work, at: u32, equates: &mut BTreeMap<String, u16>) -> String {
    let (first, second) = (work.word(at).unwrap_or(0), work.word(at + 2).unwrap_or(0));
    let words = |said: String| format!("{:<48}; {said}", format!("        dc.w    ${first:04X},${second:04X}"));
    match crate::amiga_hw::Copper::decode(first, second) {
        crate::amiga_hw::Copper::Move { register, value } => {
            // A name the program gives a place of its own is left as a number.
            let name = crate::amiga_hw::custom(u32::from(register)).map(|r| r.name.clone()).filter(|n| n.bytes().all(|b| b.is_ascii_alphanumeric()) && !work.program.names.values().any(|m| m == n));
            match name {
                Some(name) => {
                    let text = format!("        dc.w    {name},${value:04X}");
                    equates.insert(name, register);
                    text
                }
                None => format!("        dc.w    ${first:04X},${second:04X}"),
            }
        }
        crate::amiga_hw::Copper::Wait { line, x, skip: false, .. } => words(crate::i18n::tf!("wait for line ${line:02X}, x ${x:02X}")),
        crate::amiga_hw::Copper::Wait { line, x, skip: true, .. } => words(crate::i18n::tf!("skip the next if the beam is past line ${line:02X}, x ${x:02X}")),
        crate::amiga_hw::Copper::End => words(crate::i18n::tr("the end of the copper list").into()),
    }
}

/// How data is written: with text and runs of zeros, without text (for an instruction that didn't assemble back),
/// or every byte as a number (for data that didn't).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Text,
    NoText,
    Bytes,
}

/// Whether a byte can be written in a string as it is.
fn printable(b: u8) -> bool {
    (0x20..0x7F).contains(&b) && b != b'"' && b != b'\\' && b != b';' && b != b'\''
}

/// How long the text at a place is, if it's text: four or more printable characters, at least half of them letters
/// and spaces, and all but one in twenty letters, digits, spaces and the punctuation text uses. A table's bytes are often
/// printable too, but they're mostly symbols ("!&)**+,/11/*%!!#"), and those are written as numbers.
fn text_at(work: &Work, at: u32, stop: u32) -> u32 {
    let bytes: Vec<u8> = (at..stop).map_while(|a| work.byte(a).filter(|b| printable(*b))).take(60).collect();
    if bytes.len() < 4 {
        return 0;
    }
    let letters = bytes.iter().filter(|b| b.is_ascii_alphabetic() || **b == b' ').count();
    let texty = bytes.iter().filter(|b| b.is_ascii_alphanumeric() || b" .,!?:-()".contains(b)).count();
    if letters * 2 >= bytes.len() && texty * 20 >= bytes.len() * 19 { bytes.len() as u32 } else { 0 }
}

/// Data from `from` up to `to`: relocated longs as dc.l and their label, text as text, zeros as dcb.b, the rest
/// eight bytes a line.
fn data_lines(work: &Work, namer: &Namer, from: u32, to: u32, style: Style) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut at = from;
    let strings = style == Style::Text;
    let zero_runs = style != Style::Bytes;
    while at < to {
        let relocated = work.relocation(at).is_some() || work.program.pointers.contains(&at);
        if relocated && at + 4 <= to {
            let value = (0..4).fold(0u32, |v, i| v << 8 | u32::from(work.byte(at + i).unwrap_or(0)));
            let name = if work.program.layout == Layout::Raw { namer.place(value) } else { namer.long(value, at) };
            lines.push(Line { text: format!("        dc.l    {name}"), range: Some((at, 4)) });
            at += 4;
            continue;
        }
        // Up to the next relocated long, looked up rather than searched for: data can run for hundreds of K.
        let relocation = work.section(at).and_then(|(_, s)| s.relocations.range(at + 1 - s.base..).next().map(|(&o, _)| s.base + o));
        let pointer = work.program.pointers.range(at + 1..).next().copied();
        let stop = [relocation, pointer].into_iter().flatten().fold(to, u32::min);
        // A run of zeros.
        let zeros = (at..stop).take_while(|a| work.byte(*a) == Some(0)).count() as u32;
        if zero_runs && zeros >= 16 {
            lines.push(Line { text: format!("        dcb.b   {zeros},0"), range: Some((at, zeros)) });
            at += zeros;
            continue;
        }
        let text_length = if strings { text_at(work, at, stop) } else { 0 };
        if text_length > 0 {
            let text: String = (at..at + text_length).filter_map(|a| work.byte(a)).map(|b| b as char).collect();
            lines.push(Line { text: format!("        dc.b    \"{text}\""), range: Some((at, text_length)) });
            at += text_length;
            continue;
        }
        let mut length = 0;
        while at + length < stop && length < 8 {
            let a = at + length;
            if length > 0 && strings && text_at(work, a, stop) > 0 {
                break;
            }
            if length > 0 && zero_runs && (a..stop).take_while(|x| work.byte(*x) == Some(0)).count() >= 16 {
                break;
            }
            length += 1;
        }
        let items: Vec<String> = (at..at + length).map(|a| format!("${:02X}", work.byte(a).unwrap_or(0))).collect();
        lines.push(Line { text: format!("        dc.b    {}", items.join(",")), range: Some((at, length)) });
        at += length;
    }
    lines
}

/// Assembles the source again, and makes any line that doesn't give back its bytes data, until it all does. What
/// made lines wrong is mended first, and the source assembled again: a line that doesn't assemble, or comes to a
/// different length, moves every label after it, and the lines using those are only wrong because of it. A
/// relocated long is never made a number.
fn check(work: &Work, mut lines: Vec<Line>) -> Vec<Line> {
    let program = work.program;
    let namer = Namer { work };
    let path = std::path::Path::new("disassembly.s");
    let isa = &crate::asm::m68k::M68k;
    for _ in 0..16 {
        let text = lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n") + "\n";
        let assembly = match program.layout {
            Layout::Hunks => crate::asm::assemble_sections(isa, path, &text, &|_| Err(crate::i18n::tr("no").into()), 0, &crate::asm::Options { folders: Vec::new(), values: None, ..Default::default() }),
            _ => crate::asm::assemble(isa, path, &text, &|_| Err(crate::i18n::tr("no").into())),
        };
        // Where the assembler put each hunk, for the relocated longs' values.
        let starts: Vec<u32> = match program.layout {
            Layout::Hunks => (0..program.sections.len()).map(|i| assembly.sections.iter().find(|s| s.name == format!("hunk{i}")).map_or(0, |s| s.start as u32)).collect(),
            _ => program.sections.iter().map(|s| s.base).collect(),
        };
        let expected = |start: u32, length: u32| -> Vec<u8> {
            let Some((_, s)) = program.sections.iter().enumerate().find(|(_, s)| s.holds(start)) else { return Vec::new() };
            let mut bytes: Vec<u8> = (start..start + length).map(|a| s.bytes.get((a - s.base) as usize).copied().unwrap_or(0)).collect();
            for (&offset, &target) in s.relocations.range(start.saturating_sub(s.base).saturating_sub(3)..start - s.base + length) {
                let place = s.base + offset;
                if place < start || place + 4 > start + length {
                    continue;
                }
                let at = (place - start) as usize;
                let value = u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]).wrapping_add(starts.get(target).copied().unwrap_or(0));
                bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
            }
            bytes
        };
        let mut placed: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
        for p in &assembly.lines {
            if p.file == 0 {
                placed.entry(p.line).or_default().extend_from_slice(&p.bytes);
            }
        }
        let problem_lines: BTreeSet<usize> = assembly.problems.iter().map(|p| p.line).collect();
        // The lines that didn't assemble, those that came to the wrong length, and those with the wrong bytes.
        let mut wrong: [Vec<usize>; 3] = Default::default();
        for (n, line) in lines.iter().enumerate() {
            let Some((start, length)) = line.range else { continue };
            let got = placed.get(&n).map_or(&[][..], Vec::as_slice);
            if problem_lines.contains(&n) {
                wrong[0].push(n);
            } else if got.len() != length as usize {
                wrong[1].push(n);
            } else if got != expected(start, length).as_slice() {
                wrong[2].push(n);
            }
        }
        // Each line mended: an instruction as data, data as numbers; a line that's numbers already stays.
        let mended = |n: usize| -> Option<Vec<Line>> {
            let (start, length) = lines[n].range.expect("a line with bytes");
            let trimmed = lines[n].text.trim_start();
            let was_data = trimmed.starts_with("dc.") || trimmed.starts_with("dcb.");
            let mut replacement = data_lines(work, &namer, start, start + length, if was_data { Style::Bytes } else { Style::NoText });
            if replacement.len() == 1 && replacement[0].text == lines[n].text {
                return None;
            }
            if !was_data && let Some(first) = replacement.first_mut() {
                let was = lines[n].text.trim().split(';').next().unwrap_or("").trim().to_string();
                first.text = format!("{:<48}; {was}?", first.text);
            }
            Some(replacement)
        };
        let Some(mends) = wrong.iter().map(|ns| ns.iter().filter_map(|&n| mended(n).map(|r| (n, r))).collect::<Vec<_>>()).find(|m| !m.is_empty()) else { break };
        for (n, replacement) in mends.into_iter().rev() {
            lines.splice(n..=n, replacement);
        }
    }
    lines
}

#[cfg(test)]
mod tests;
