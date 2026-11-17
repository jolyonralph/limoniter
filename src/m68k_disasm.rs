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
        what: format!("{} bytes of 68000 code at ${origin:X}", group(bytes.len() as u64)),
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
    let what = format!("An Amiga program: {} ({})", if sections.len() == 1 { "1 hunk".to_string() } else { format!("{} hunks", sections.len()) }, parts.join(", "));
    let starts = vec![sections[0].base];
    Ok(Program { machine: Machine::Amiga, layout: Layout::Hunks, sections, starts, names, text_end: None, pointers: BTreeSet::new(), what })
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
    let what = format!("An Atari ST program: text {} bytes, data {}, BSS {}{}", group(text as u64), group(data as u64), group(bss as u64), if names.is_empty() { String::new() } else { format!(", {} symbols", names.len()) });
    let section = Section { kind: Kind::Code, memory: Memory::Any, base: 0, bytes: image, bss: bss as u32, relocations };
    Ok(Program { machine: Machine::AtariSt, layout: Layout::Tos, sections: vec![section], starts: vec![0], names, text_end: Some(text as u32), pointers: BTreeSet::new(), what })
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
        program.what = "An Amiga boot block: its code from offset 12 (it's loaded wherever there's room)".into();
        return Some(program);
    }
    // An Atari ST boot sector: its words add up to $1234, and it starts with a branch over the disk's details.
    if bytes.len() == 512 && bytes.as_chunks::<2>().0.iter().fold(0u16, |sum, w| sum.wrapping_add(u16::from_be_bytes([w[0], w[1]]))) == 0x1234 {
        let mut program = raw(bytes, 0, &[0], Machine::AtariSt);
        program.what = "An Atari ST boot sector: its code from the branch at the start".into();
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
        program.what = format!("A Mega Drive cartridge{}, followed from its vectors", if title.is_empty() { String::new() } else { format!(" ({title})") });
        return Some(program);
    }
    // A Kickstart ROM: $1111 (256K, at $FC0000) or $1114 (512K, at $F80000), then a jump to its start.
    if matches!(be16(bytes, 0), Some(0x1111 | 0x1114)) && matches!(bytes.len(), 0x40000 | 0x80000) {
        let origin = if bytes.len() == 0x40000 { 0xFC_0000 } else { 0xF8_0000 };
        let start = be32(bytes, 4).filter(|&a| a >= origin && a < origin + bytes.len() as u32).unwrap_or(origin + 8);
        let mut program = raw(bytes, origin, &[start], Machine::Amiga);
        program.what = format!("A Kickstart ROM of {}K at ${origin:X}, from its start at ${start:X}", bytes.len() / 1024);
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

/// An index register: its number, whether it's an address register, and whether all 32 bits count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Xn {
    pub reg: u8,
    pub address: bool,
    pub long: bool,
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
}

/// The 68000's brief extension word, refusing the 68020's scale and full format.
fn brief(w: u16) -> Option<Xn> {
    (w & 0x0700 == 0).then_some(Xn { reg: (w >> 12 & 7) as u8, address: w & 0x8000 != 0, long: w & 0x0800 != 0 })
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
        6 => {
            let w = r.next()?;
            Op::Index(n, w as u8 as i8, brief(w)?)
        }
        _ => match reg {
            0 => Op::AbsW(r.next()? as i16 as i32 as u32),
            1 => {
                let at = r.here();
                let (hi, lo) = (r.next()?, r.next()?);
                Op::AbsL(u32::from(hi) << 16 | u32::from(lo), at)
            }
            2 => {
                let at = r.here();
                let d = r.next()? as i16;
                Op::Pc(at.wrapping_add(d as i32 as u32))
            }
            3 => {
                let at = r.here();
                let w = r.next()?;
                Op::PcIndex(at.wrapping_add(w as u8 as i8 as i32 as u32), brief(w)?)
            }
            4 => {
                let at = r.here();
                let value = match size {
                    // A byte's word must have nothing in its top half, or it wouldn't be written again the same.
                    Size::B => u32::from(r.next().filter(|w| *w <= 0xFF)?),
                    Size::W => u32::from(r.next()?),
                    Size::L => u32::from(r.next()?) << 16 | u32::from(r.next()?),
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
    let pc = matches!(op, Op::Pc(_) | Op::PcIndex(..));
    let imm = matches!(op, Op::Imm(..));
    let control = matches!(op, Op::Ind(_) | Op::Disp(..) | Op::Index(..) | Op::AbsW(_) | Op::AbsL(..) | Op::Pc(_) | Op::PcIndex(..));
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
    let mut r = Reader { word, pc, length: 0 };
    let op = r.next()?;
    let (mode, reg) = ((op >> 3) & 7, op & 7);
    let rx = ((op >> 9) & 7) as u8;
    let size_bits = (op >> 6) & 3;
    let named = |name: &str, size: Option<Size>| format!("{name}{}", size.map_or("", Size::suffix));
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
                            if !fits(&dst, Class::DataAlterable) {
                                return None;
                            }
                            (named(name, Some(size)), vec![src, dst])
                        }
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
                0x4E76 => ("trapv".into(), vec![]),
                _ if op & 0xFFF0 == 0x4E40 => ("trap".into(), vec![Op::Quick(i32::from(op & 15))]),
                _ if op & 0xFFF8 == 0x4E50 => ("link".into(), vec![Op::A(reg as u8), Op::Quick(i32::from(r.next()? as i16))]),
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
                _ if op & 0xFFC0 == 0x40C0 => {
                    let dst = ea(&mut r, mode, reg, Size::W)?;
                    if !fits(&dst, Class::DataAlterable) {
                        return None;
                    }
                    ("move.w".into(), vec![Op::Sr, dst])
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
                    if !fits(&dst, Class::DataAlterable) {
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
                _ if op & 0xF1C0 == 0x41C0 => {
                    let src = ea(&mut r, mode, reg, Size::L)?;
                    if !fits(&src, Class::Control) {
                        return None;
                    }
                    ("lea".into(), vec![src, Op::A(rx)])
                }
                _ if op & 0xF1C0 == 0x4180 => {
                    let src = ea(&mut r, mode, reg, Size::W)?;
                    if !fits(&src, Class::Data) {
                        return None;
                    }
                    ("chk.w".into(), vec![src, Op::D(rx)])
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
            if size_bits == 3 {
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
    format!("{}.{}", register(x.reg, x.address), if x.long { 'l' } else { 'w' })
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
        Op::Imm(v, Size::L, at) => format!("#{}", names.long(*v, *at)),
        Op::Imm(v, _, _) => format!("#{}", hex(*v)),
        Op::Quick(n) => format!("#{}", signed(*n)),
        Op::Sr => "sr".into(),
        Op::Ccr => "ccr".into(),
        Op::Usp => "usp".into(),
        Op::List(mask) => list(*mask),
        Op::Target(t) => names.place(*t),
    }
}

fn text(insn: &Insn, names: &dyn Names) -> String {
    let operands: Vec<String> = insn.ops.iter().map(|o| operand(o, names)).collect();
    if operands.is_empty() { insn.mnemonic.clone() } else { format!("{:<8}{}", insn.mnemonic, operands.join(",")) }
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
        decode(&word, address)
    }

    fn trace(&mut self, starts: Vec<u32>) {
        let mut queue = starts;
        while let Some(mut pc) = queue.pop() {
            while self.code_at(pc) && self.seen.get(&pc).is_none_or(|s| *s == Seen::Data) {
                let Some(insn) = self.decode(pc) else { break };
                let length = insn.length;
                // In a program that's moved when it's loaded, nothing relative to PC can leave its own section.
                if self.program.layout != Layout::Raw {
                    let (index, _) = self.section(pc).expect("code is in a section");
                    let away = insn.ops.iter().any(|op| match op {
                        Op::Target(t) | Op::Pc(t) | Op::PcIndex(t, _) => self.section(*t).is_none_or(|(i, _)| i != index),
                        _ => false,
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
                let mut follow = Vec::new();
                for op in &insn.ops {
                    let target = match op {
                        Op::Target(t) => Some(*t),
                        Op::Pc(t) if matches!(insn.flow, Flow::Jump | Flow::Call) => Some(*t),
                        Op::AbsL(v, at) if matches!(insn.flow, Flow::Jump | Flow::Call) => self.address_in(*v, *at),
                        Op::AbsW(v) if matches!(insn.flow, Flow::Jump | Flow::Call) && self.program.layout == Layout::Raw => self.section(*v).map(|_| *v),
                        // An address put in a long as an immediate, in a code hunk: an interrupt handler, say.
                        Op::Imm(v, Size::L, at) if self.program.layout != Layout::Raw => self.address_in(*v, *at).filter(|a| self.code_at(*a) && self.program.layout == Layout::Hunks),
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
                let carries_on = matches!(insn.flow, Flow::Next | Flow::Branch | Flow::Call);
                self.decoded.insert(pc, insn);
                if !carries_on {
                    break;
                }
                pc += length;
            }
        }
    }

    /// The instruction an address is inside, if it's inside one past its first word.
    fn start_of(&self, address: u32) -> u32 {
        let mut back = address & !1;
        while back > 0 && self.seen.get(&back) == Some(&Seen::Inside) {
            back -= 2;
        }
        if self.seen.get(&back) == Some(&Seen::Start) && address > back { back } else { address }
    }
}

/// Disassembles a program into source that assembles back to it.
pub fn disassemble(program: &Program) -> String {
    let lines = write(program);
    let lines = check(program, lines);
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
        match self.work.address_in(value, at) {
            Some(address) if self.work.labels.contains(&address) || self.work.labels.contains(&self.work.start_of(address)) => self.label(address),
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

/// A comment for an instruction: the hardware it touches, the system call it makes. `follow` is moved on.
fn comment(work: &Work, insn: &Insn, follow: &mut Follow) -> Option<String> {
    let machine = work.program.machine;
    let mut said: Vec<String> = Vec::new();
    let base = insn.mnemonic.split('.').next().unwrap_or("");
    // LEA and PEA take an address, not what's there: the custom chips' base isn't a register of its own.
    if matches!(base, "lea" | "pea") && matches!(insn.ops.first(), Some(Op::AbsL(0xDF_F000, _))) {
        said.push("the custom chips".into());
    }
    let named_ops = if matches!(base, "lea" | "pea") { &insn.ops[..0] } else { &insn.ops[..] };
    for op in named_ops {
        match op {
            Op::AbsW(a) => said.extend(hardware(machine, *a)),
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
                        said.push(format!("{name}, if a6 is exec"));
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

fn write(program: &Program) -> Vec<Line> {
    BASES.with(|b| b.borrow_mut().clear());
    let mut work = Work { program, seen: BTreeMap::new(), decoded: BTreeMap::new(), labels: BTreeSet::new(), entries: BTreeSet::new() };
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
                Op::Target(t) | Op::Pc(t) | Op::PcIndex(t, _) => used.push(*t),
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
    let namer = Namer { work: &work };
    let mut lines = vec![Line { text: format!("; {}.", program.what), range: None }, Line { text: "; Disassembled by Limoniter: it assembles with Limoniter's own assembler or vasm (Motorola syntax) to the same bytes.".into(), range: None }, Line { text: String::new(), range: None }];
    if program.layout == Layout::Raw {
        lines.push(Line { text: format!("        org     ${:X}", program.sections[0].base), range: None });
    }
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
                if let Some(said) = comment(&work, insn, &mut follow) {
                    text = format!("{text:<48}; {said}");
                }
                lines.push(Line { text, range: Some((at, insn.length)) });
                at += insn.length;
                continue;
            }
            // Data, up to the next label, instruction, relocated long, or the TOS data's start.
            let mut stop = at + 1;
            while stop < end && !work.labels.contains(&stop) && !work.decoded.contains_key(&stop) && Some(stop) != program.text_end {
                stop += 1;
            }
            lines.extend(data_lines(&work, &namer, at, stop, true));
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
    lines
}

/// Data from `from` up to `to`: relocated longs as dc.l and their label, text as text, zeros as dcb.b, the rest
/// eight bytes a line. With `strings` off, no text: for a line that didn't assemble back as it was.
fn data_lines(work: &Work, namer: &Namer, from: u32, to: u32, strings: bool) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut at = from;
    let printable = |b: u8| (0x20..0x7F).contains(&b) && b != b'"' && b != b'\\' && b != b';' && b != b'\'';
    while at < to {
        let relocated = work.relocation(at).is_some() || work.program.pointers.contains(&at);
        if relocated && at + 4 <= to {
            let value = (0..4).fold(0u32, |v, i| v << 8 | u32::from(work.byte(at + i).unwrap_or(0)));
            let name = if work.program.layout == Layout::Raw { namer.place(value) } else { namer.long(value, at) };
            lines.push(Line { text: format!("        dc.l    {name}"), range: Some((at, 4)) });
            at += 4;
            continue;
        }
        let next_relocation = (at + 1..to).find(|a| work.relocation(*a).is_some() || work.program.pointers.contains(a)).unwrap_or(to);
        let stop = next_relocation.min(to);
        // A run of zeros.
        let zeros = (at..stop).take_while(|a| work.byte(*a) == Some(0)).count() as u32;
        if zeros >= 16 {
            lines.push(Line { text: format!("        dcb.b   {zeros},0"), range: Some((at, zeros)) });
            at += zeros;
            continue;
        }
        // Text: four or more printable characters.
        let text_length = (at..stop).take_while(|a| work.byte(*a).is_some_and(printable)).count() as u32;
        if strings && text_length >= 4 {
            let length = text_length.min(60);
            let text: String = (at..at + length).filter_map(|a| work.byte(a)).map(|b| b as char).collect();
            lines.push(Line { text: format!("        dc.b    \"{text}\""), range: Some((at, length)) });
            at += length;
            continue;
        }
        let mut length = 0;
        while at + length < stop && length < 8 {
            let a = at + length;
            if length > 0 && strings && (a..stop).take_while(|x| work.byte(*x).is_some_and(printable)).count() >= 4 {
                break;
            }
            if length > 0 && (a..stop).take_while(|x| work.byte(*x) == Some(0)).count() >= 16 {
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

/// Assembles the source again, and makes any line that doesn't give back its bytes data, until it all does.
fn check(program: &Program, mut lines: Vec<Line>) -> Vec<Line> {
    let mut work = Work { program, seen: BTreeMap::new(), decoded: BTreeMap::new(), labels: BTreeSet::new(), entries: BTreeSet::new() };
    // The labels the source defines, so data written in place of a line can still name them.
    for line in &lines {
        if let Some(name) = line.text.strip_suffix(':')
            && let Some(address) = parse_label(program, name)
        {
            work.labels.insert(address);
        }
    }
    let path = std::path::Path::new("disassembly.s");
    let isa = &crate::asm::m68k::M68k;
    for _ in 0..8 {
        let text = lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("\n") + "\n";
        let assembly = match program.layout {
            Layout::Hunks => crate::asm::assemble_sections(isa, path, &text, &|_| Err(crate::i18n::tr("no").into()), 0, &crate::asm::Options { folders: Vec::new(), values: None }),
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
        let mut wrong: Vec<usize> = Vec::new();
        for (n, line) in lines.iter().enumerate() {
            let Some((start, length)) = line.range else { continue };
            let got = placed.get(&n).cloned().unwrap_or_default();
            if problem_lines.contains(&n) || got != expected(start, length) {
                wrong.push(n);
            }
        }
        if wrong.is_empty() {
            break;
        }
        let namer = Namer { work: &work };
        for n in wrong.into_iter().rev() {
            let (start, length) = lines[n].range.expect("a line with bytes");
            let was_data = lines[n].text.trim_start().starts_with("dc.");
            let mut replacement = data_lines(&work, &namer, start, start + length, !was_data);
            if replacement.len() == 1 && replacement[0].text == lines[n].text {
                // Data that still doesn't come back is written byte by byte, with no labels.
                let items: Vec<String> = expected(start, length).iter().map(|b| format!("${b:02X}")).collect();
                replacement = vec![Line { text: format!("        dc.b    {}", items.join(",")), range: None }];
            }
            if !was_data && let Some(first) = replacement.first_mut() {
                let was = lines[n].text.trim().split(';').next().unwrap_or("").trim().to_string();
                first.text = format!("{:<48}; {was}?", first.text);
            }
            lines.splice(n..=n, replacement);
        }
        work.labels.clear();
        for line in &lines {
            if let Some(name) = line.text.strip_suffix(':')
                && let Some(address) = parse_label(program, name)
            {
                work.labels.insert(address);
            }
        }
    }
    lines
}

/// The address a label made here names.
fn parse_label(program: &Program, name: &str) -> Option<u32> {
    if let Some((address, _)) = program.names.iter().find(|(_, n)| *n == name) {
        return Some(*address);
    }
    let rest = name.strip_prefix('L')?;
    match rest.split_once('_') {
        Some((hunk, offset)) => {
            let s = program.sections.get(hunk.parse::<usize>().ok()?)?;
            Some(s.base + u32::from_str_radix(offset, 16).ok()?)
        }
        None => {
            let n = u32::from_str_radix(rest, 16).ok()?;
            Some(if program.layout == Layout::Hunks { program.sections[0].base + n } else { n })
        }
    }
}

#[cfg(test)]
mod tests;
