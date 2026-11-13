//! Turning machine code back into source: the Z80 (with the Next's instructions) and the 6502. The code is traced
//! from where it starts, through every jump and call, so what's never run is kept as data; addresses jumped to,
//! called and used get labels; and the source is assembled again with Limoniter's own assembler, any line that
//! wouldn't give back the same bytes becoming DB, so the result always assembles to exactly the bytes it came from.

use crate::assemblers::Cpu;
use std::collections::{BTreeMap, BTreeSet};

/// One instruction: its length, how it's written (with {addr} where an address goes), the address if there is
/// one, and where it can go next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub length: usize,
    pub text: String,
    /// An address it names: jumped to, called, or read or written.
    pub address: Option<u16>,
    /// Whether it carries on to the next instruction (not an unconditional jump or return).
    pub falls_through: bool,
    /// Where it may jump or call.
    pub target: Option<u16>,
}

fn hex8(n: u8) -> String {
    format!("${n:02X}")
}

// ---------- Z80 ----------

const R: [&str; 8] = ["b", "c", "d", "e", "h", "l", "(hl)", "a"];
const RP: [&str; 4] = ["bc", "de", "hl", "sp"];
const RP2: [&str; 4] = ["bc", "de", "hl", "af"];
const CC: [&str; 8] = ["nz", "z", "nc", "c", "po", "pe", "p", "m"];
const ALU: [&str; 8] = ["add a,", "adc a,", "sub ", "sbc a,", "and ", "xor ", "or ", "cp "];
const ROT: [&str; 8] = ["rlc", "rrc", "rl", "rr", "sla", "sra", "sll", "srl"];

/// A Z80 instruction at `pc`, from `at(0)` on.
pub fn z80(at: &dyn Fn(usize) -> u8, pc: u16) -> Decoded {
    let op = at(0);
    match op {
        0xCB => {
            let o = at(1);
            let (x, y, z) = (o >> 6, (o >> 3) & 7, o & 7);
            let text = match x {
                0 => format!("{} {}", ROT[y as usize], R[z as usize]),
                1 => format!("bit {y},{}", R[z as usize]),
                2 => format!("res {y},{}", R[z as usize]),
                _ => format!("set {y},{}", R[z as usize]),
            };
            plain(2, text)
        }
        0xED => z80_ed(at, pc),
        0xDD | 0xFD => z80_index(at, pc, if op == 0xDD { "ix" } else { "iy" }),
        _ => z80_plain(at, pc, op, None),
    }
}

fn plain(length: usize, text: String) -> Decoded {
    Decoded { length, text, address: None, falls_through: true, target: None }
}

fn word(at: &dyn Fn(usize) -> u8, from: usize) -> u16 {
    u16::from_le_bytes([at(from), at(from + 1)])
}

/// An unprefixed instruction, or with `index` (ix or iy, and its displacement's place) one after DD or FD.
fn z80_plain(at: &dyn Fn(usize) -> u8, pc: u16, op: u8, index: Option<(&str, usize)>) -> Decoded {
    let (x, y, z, p, q) = (op >> 6, (op >> 3) & 7, op & 7, (op >> 4) & 3, (op >> 3) & 1);
    // With IX or IY: HL becomes it, H and L its halves, and (HL) (IX+d), which takes a byte.
    let skip = index.map_or(1, |_| 2);
    let disp = |at: &dyn Fn(usize) -> u8| index.map(|(reg, _)| {
        let d = at(2) as i8;
        if d < 0 { format!("({reg}-{})", -i16::from(d)) } else { format!("({reg}+{d})") }
    });
    let uses_memory = |r: u8| r == 6;
    let reg8 = |r: u8, memory_here: bool| -> String {
        match (index, r) {
            (Some(_), 6) => disp(at).unwrap_or_default(),
            (Some((reg, _)), 4) if !memory_here => format!("{reg}h"),
            (Some((reg, _)), 5) if !memory_here => format!("{reg}l"),
            _ => R[r as usize].to_string(),
        }
    };
    let rp = |n: u8, list: &[&str; 4]| -> String {
        match (index, n) {
            (Some((reg, _)), 2) => reg.to_string(),
            _ => list[n as usize].to_string(),
        }
    };
    // Where an immediate byte or word is: after the opcode, and after the displacement for (IX+d).
    let extra = |r_uses_memory: bool| skip + usize::from(index.is_some() && r_uses_memory);
    let relative = |from: usize| pc.wrapping_add(from as u16 + 1).wrapping_add(at(from) as i8 as u16);
    match x {
        0 => match z {
            0 => match y {
                0 => plain(skip, "nop".into()),
                1 => plain(skip, "ex af,af'".into()),
                2 => {
                    let t = relative(skip);
                    Decoded { length: skip + 1, text: "djnz {addr}".into(), address: Some(t), falls_through: true, target: Some(t) }
                }
                3 => {
                    let t = relative(skip);
                    Decoded { length: skip + 1, text: "jr {addr}".into(), address: Some(t), falls_through: false, target: Some(t) }
                }
                _ => {
                    let t = relative(skip);
                    Decoded { length: skip + 1, text: format!("jr {},{{addr}}", CC[(y - 4) as usize]), address: Some(t), falls_through: true, target: Some(t) }
                }
            },
            1 if q == 0 => {
                let nn = word(at, skip);
                Decoded { length: skip + 2, text: format!("ld {},{{addr}}", rp(p, &RP)), address: Some(nn), falls_through: true, target: None }
            }
            1 => plain(skip, format!("add {},{}", rp(2, &RP), rp(p, &RP))),
            2 => match (q, p) {
                (0, 0) => plain(skip, "ld (bc),a".into()),
                (0, 1) => plain(skip, "ld (de),a".into()),
                (0, 2) => Decoded { length: skip + 2, text: format!("ld ({{addr}}),{}", rp(2, &RP)), address: Some(word(at, skip)), falls_through: true, target: None },
                (0, _) => Decoded { length: skip + 2, text: "ld ({addr}),a".into(), address: Some(word(at, skip)), falls_through: true, target: None },
                (_, 0) => plain(skip, "ld a,(bc)".into()),
                (_, 1) => plain(skip, "ld a,(de)".into()),
                (_, 2) => Decoded { length: skip + 2, text: format!("ld {},({{addr}})", rp(2, &RP)), address: Some(word(at, skip)), falls_through: true, target: None },
                _ => Decoded { length: skip + 2, text: "ld a,({addr})".into(), address: Some(word(at, skip)), falls_through: true, target: None },
            },
            3 => plain(skip, format!("{} {}", if q == 0 { "inc" } else { "dec" }, rp(p, &RP))),
            4 => plain(extra(uses_memory(y)), format!("inc {}", reg8(y, false))),
            5 => plain(extra(uses_memory(y)), format!("dec {}", reg8(y, false))),
            6 => {
                let from = extra(uses_memory(y));
                plain(from + 1, format!("ld {},{}", reg8(y, false), hex8(at(from))))
            }
            _ => plain(skip, ["rlca", "rrca", "rla", "rra", "daa", "cpl", "scf", "ccf"][y as usize].into()),
        },
        1 if y == 6 && z == 6 => plain(skip, "halt".into()),
        1 => {
            // With (IX+d) on one side, H and L stay H and L on the other.
            let memory_here = uses_memory(y) || uses_memory(z);
            plain(extra(memory_here), format!("ld {},{}", reg8(y, memory_here), reg8(z, memory_here)))
        }
        2 => plain(extra(uses_memory(z)), format!("{}{}", ALU[y as usize], reg8(z, false))),
        _ => match z {
            0 => Decoded { length: skip, text: format!("ret {}", CC[y as usize]), address: None, falls_through: true, target: None },
            1 => match (q, p) {
                (0, _) => plain(skip, format!("pop {}", rp(p, &RP2))),
                (_, 0) => Decoded { length: skip, text: "ret".into(), address: None, falls_through: false, target: None },
                (_, 1) => plain(skip, "exx".into()),
                (_, 2) => Decoded { length: skip, text: format!("jp ({})", rp(2, &RP)), address: None, falls_through: false, target: None },
                _ => plain(skip, format!("ld sp,{}", rp(2, &RP))),
            },
            2 => {
                let nn = word(at, skip);
                Decoded { length: skip + 2, text: format!("jp {},{{addr}}", CC[y as usize]), address: Some(nn), falls_through: true, target: Some(nn) }
            }
            3 => match y {
                0 => {
                    let nn = word(at, skip);
                    Decoded { length: skip + 2, text: "jp {addr}".into(), address: Some(nn), falls_through: false, target: Some(nn) }
                }
                2 => plain(skip + 1, format!("out ({}),a", hex8(at(skip)))),
                3 => plain(skip + 1, format!("in a,({})", hex8(at(skip)))),
                4 => plain(skip, format!("ex (sp),{}", rp(2, &RP))),
                5 => plain(skip, "ex de,hl".into()),
                6 => plain(skip, "di".into()),
                _ => plain(skip, "ei".into()),
            },
            4 => {
                let nn = word(at, skip);
                Decoded { length: skip + 2, text: format!("call {},{{addr}}", CC[y as usize]), address: Some(nn), falls_through: true, target: Some(nn) }
            }
            5 if q == 0 => plain(skip, format!("push {}", rp(p, &RP2))),
            5 => {
                let nn = word(at, skip);
                Decoded { length: skip + 2, text: "call {addr}".into(), address: Some(nn), falls_through: true, target: Some(nn) }
            }
            6 => plain(skip + 1, format!("{}{}", ALU[y as usize], hex8(at(skip)))),
            _ => {
                let t = u16::from(y) * 8;
                Decoded { length: skip, text: format!("rst {}", hex8(t as u8)), address: None, falls_through: true, target: Some(t) }
            }
        },
    }
}

fn z80_index(at: &dyn Fn(usize) -> u8, pc: u16, reg: &str) -> Decoded {
    let op = at(1);
    if op == 0xCB {
        let d = at(2) as i8;
        let o = at(3);
        let (x, y, z) = (o >> 6, (o >> 3) & 7, o & 7);
        let place = if d < 0 { format!("({reg}-{})", -i16::from(d)) } else { format!("({reg}+{d})") };
        // The forms that also copy to a register are written as sjasmplus writes them, with the register after.
        let copy = if z == 6 { String::new() } else { format!(",{}", R[z as usize]) };
        let text = match x {
            0 => format!("{} {place}{copy}", ROT[y as usize]),
            1 => format!("bit {y},{place}"),
            2 => format!("res {y},{place}{copy}"),
            _ => format!("set {y},{place}{copy}"),
        };
        return plain(4, text);
    }
    if matches!(op, 0xDD | 0xFD | 0xED) {
        return Decoded { length: 1, text: format!("db {}", hex8(at(0))), address: None, falls_through: true, target: None };
    }
    let mut decoded = z80_plain(&|i| at(i), pc, op, Some((reg, 2)));
    // A prefix on an instruction that doesn't use HL changes nothing: it's kept as DB.
    if !decoded.text.contains(reg) {
        decoded = Decoded { length: 1, text: format!("db {}", hex8(at(0))), address: None, falls_through: true, target: None };
    }
    decoded
}

fn z80_ed(at: &dyn Fn(usize) -> u8, pc: u16) -> Decoded {
    let o = at(1);
    let _ = pc;
    let (x, y, z, p, q) = (o >> 6, (o >> 3) & 7, o & 7, (o >> 4) & 3, (o >> 3) & 1);
    let text = match o {
        // The Next's.
        0x23 => "swapnib".into(),
        0x24 => "mirror a".into(),
        0x27 => return plain(3, format!("test {}", hex8(at(2)))),
        0x28 => "bsla de,b".into(),
        0x29 => "bsra de,b".into(),
        0x2A => "bsrl de,b".into(),
        0x2B => "bsrf de,b".into(),
        0x2C => "brlc de,b".into(),
        0x30 => "mul d,e".into(),
        0x31 => "add hl,a".into(),
        0x32 => "add de,a".into(),
        0x33 => "add bc,a".into(),
        0x34..=0x36 => return plain(4, format!("add {},${:04X}", ["hl", "de", "bc"][(o - 0x34) as usize], word(at, 2))),
        // PUSH nn, big end first.
        0x8A => return plain(4, format!("push ${:04X}", u16::from_be_bytes([at(2), at(3)]))),
        0x90 => "outinb".into(),
        0x91 => return plain(4, format!("nextreg {},{}", hex8(at(2)), hex8(at(3)))),
        0x92 => return plain(3, format!("nextreg {},a", hex8(at(2)))),
        0x93 => "pixeldn".into(),
        0x94 => "pixelad".into(),
        0x95 => "setae".into(),
        0x98 => return Decoded { length: 2, text: "jp (c)".into(), address: None, falls_through: false, target: None },
        0xA4 => "ldix".into(),
        0xA5 => "ldws".into(),
        0xAC => "lddx".into(),
        0xB4 => "ldirx".into(),
        0xB7 => "ldpirx".into(),
        0xBC => "lddrx".into(),
        _ => match x {
            1 => match z {
                0 if y == 6 => "in f,(c)".into(),
                0 => format!("in {},(c)", R[y as usize]),
                1 if y == 6 => "out (c),0".into(),
                1 => format!("out (c),{}", R[y as usize]),
                2 => format!("{} hl,{}", if q == 0 { "sbc" } else { "adc" }, RP[p as usize]),
                3 => {
                    let nn = word(at, 2);
                    let text = if q == 0 { format!("ld ({{addr}}),{}", RP[p as usize]) } else { format!("ld {},({{addr}})", RP[p as usize]) };
                    return Decoded { length: 4, text, address: Some(nn), falls_through: true, target: None };
                }
                4 => "neg".into(),
                5 if y == 1 => return Decoded { length: 2, text: "reti".into(), address: None, falls_through: false, target: None },
                5 => return Decoded { length: 2, text: "retn".into(), address: None, falls_through: false, target: None },
                6 => format!("im {}", [0, 0, 1, 2, 0, 0, 1, 2][y as usize]),
                _ => match y {
                    0 => "ld i,a".into(),
                    1 => "ld r,a".into(),
                    2 => "ld a,i".into(),
                    3 => "ld a,r".into(),
                    4 => "rrd".into(),
                    5 => "rld".into(),
                    _ => return plain(2, format!("db $ED,{}", hex8(o))),
                },
            },
            2 if z <= 3 && y >= 4 => [["ldi", "cpi", "ini", "outi"], ["ldd", "cpd", "ind", "outd"], ["ldir", "cpir", "inir", "otir"], ["lddr", "cpdr", "indr", "otdr"]][(y - 4) as usize][z as usize].into(),
            _ => return plain(2, format!("db $ED,{}", hex8(o))),
        },
    };
    plain(2, text)
}

// ---------- 6502 ----------

/// A 6502 instruction, from the assembler's own table of opcodes.
pub fn m6502(at: &dyn Fn(usize) -> u8, pc: u16) -> Decoded {
    use crate::asm::m6502::{Mode, decode};
    let op = at(0);
    let Some((name, mode)) = decode(op) else {
        return plain(1, format!("!byte {}", hex8(op)));
    };
    let byte = at(1);
    let nn = word(at, 1);
    let (length, operand, address) = match mode {
        Mode::Implied => (1, String::new(), None),
        Mode::Accumulator => (1, " a".into(), None),
        Mode::Immediate => (2, format!(" #{}", hex8(byte)), None),
        Mode::ZeroPage => (2, format!(" {}", hex8(byte)), None),
        Mode::ZeroPageX => (2, format!(" {},x", hex8(byte)), None),
        Mode::ZeroPageY => (2, format!(" {},y", hex8(byte)), None),
        Mode::Absolute => (3, " {addr}".into(), Some(nn)),
        Mode::AbsoluteX => (3, " {addr},x".into(), Some(nn)),
        Mode::AbsoluteY => (3, " {addr},y".into(), Some(nn)),
        Mode::Indirect => (3, " ({addr})".into(), Some(nn)),
        Mode::IndirectX => (2, format!(" ({},x)", hex8(byte)), None),
        Mode::IndirectY => (2, format!(" ({}),y", hex8(byte)), None),
        Mode::Relative => (2, " {addr}".into(), Some(pc.wrapping_add(2).wrapping_add(byte as i8 as u16))),
    };
    let (falls_through, target) = match name {
        "jmp" => (false, (mode == Mode::Absolute).then_some(nn)),
        "rts" | "rti" | "brk" => (false, None),
        "jsr" => (true, Some(nn)),
        _ if mode == Mode::Relative => (true, address),
        _ => (true, None),
    };
    Decoded { length, text: format!("{name}{operand}"), address, falls_through, target }
}

// ---------- 6809 ----------

const REG_6809: [&str; 16] = ["d", "x", "y", "u", "s", "pc", "?", "?", "a", "b", "cc", "dp", "?", "?", "?", "?"];

/// An indexed operand from its postbyte on: how it's written (with {addr} for a PC-relative target or an address
/// it names), how many bytes, and the address.
fn indexed_6809(at: &dyn Fn(usize) -> u8, from: usize, end_of: &dyn Fn(usize) -> u16) -> (String, usize, Option<u16>) {
    let post = at(from);
    let r = ["x", "y", "u", "s"][usize::from((post >> 5) & 3)];
    if post & 0x80 == 0 {
        let n = ((post & 0x1F) as i8) << 3 >> 3;
        return (format!("<<{n},{r}"), 1, None);
    }
    let indirect = post & 0x10 != 0;
    let (inner, length, address) = match post & 0x0F {
        0x0 => (format!(",{r}+"), 1, None),
        0x1 => (format!(",{r}++"), 1, None),
        0x2 => (format!(",-{r}"), 1, None),
        0x3 => (format!(",--{r}"), 1, None),
        0x4 => (format!(",{r}"), 1, None),
        0x5 => (format!("b,{r}"), 1, None),
        0x6 => (format!("a,{r}"), 1, None),
        0x8 => (format!("<{},{r}", at(from + 1) as i8), 2, None),
        0x9 => (format!(">{},{r}", u16::from_be_bytes([at(from + 1), at(from + 2)]) as i16), 3, None),
        0xB => (format!("d,{r}"), 1, None),
        0xC => {
            let target = end_of(2).wrapping_add(at(from + 1) as i8 as u16);
            ("{addr},pcr".into(), 2, Some(target))
        }
        0xD => {
            let target = end_of(3).wrapping_add(u16::from_be_bytes([at(from + 1), at(from + 2)]));
            ("{addr},pcr".into(), 3, Some(target))
        }
        0xF if indirect => ("{addr}".into(), 3, Some(u16::from_be_bytes([at(from + 1), at(from + 2)]))),
        _ => return ("?".into(), 0, None),
    };
    (if indirect { format!("[{inner}]") } else { inner }, length, address)
}

/// A 6809 instruction, from the assembler's own tables.
pub fn m6809(at: &dyn Fn(usize) -> u8, pc: u16) -> Decoded {
    use crate::asm::m6809::{BRANCHES, INHERENT, MEMORY, SHIFTS};
    let page = match at(0) {
        0x10 | 0x11 => Some(at(0)),
        _ => None,
    };
    let skip = usize::from(page.is_some());
    let op = at(skip);
    let unknown = || Decoded { length: 1, text: format!("fcb {}", hex8(at(0))), address: None, falls_through: true, target: None };
    let be = |from: usize| u16::from_be_bytes([at(from), at(from + 1)]);
    // Where the instruction ends, for a PC-relative operand, given how long its operand turned out.
    let ends = |operand_length: usize| move |extra: usize| pc.wrapping_add((skip + 1 + operand_length.max(extra)) as u16);
    let _ = ends;
    for (name, bytes) in INHERENT {
        if bytes.len() == skip + 1 && (0..=skip).all(|i| at(i) == bytes[i]) {
            let stops = matches!(name, "rts" | "rti");
            return Decoded { length: skip + 1, text: name.to_string(), address: None, falls_through: !stops, target: None };
        }
    }
    // Branches, short and long.
    if page.is_none() && (0x20..=0x2F).contains(&op) || page.is_none() && op == 0x8D {
        let target = pc.wrapping_add(2).wrapping_add(at(1) as i8 as u16);
        let name = if op == 0x8D { "bsr" } else { BRANCHES.iter().find(|(_, o)| *o == op).map_or("?", |(n, _)| n) };
        return Decoded { length: 2, text: format!("{name} {{addr}}"), address: Some(target), falls_through: op != 0x20, target: (op != 0x21).then_some(target) };
    }
    if (page.is_none() && matches!(op, 0x16 | 0x17)) || (page == Some(0x10) && (0x21..=0x2F).contains(&op)) {
        let length = skip + 3;
        let target = pc.wrapping_add(length as u16).wrapping_add(be(skip + 1));
        let name = match (page, op) {
            (None, 0x16) => "lbra".to_string(),
            (None, _) => "lbsr".to_string(),
            _ => format!("l{}", BRANCHES.iter().find(|(_, o)| *o == op).map_or("?", |(n, _)| n)),
        };
        return Decoded { length, text: format!("{name} {{addr}}"), address: Some(target), falls_through: op != 0x16 || page.is_some(), target: (name != "lbrn").then_some(target) };
    }
    // Instructions on memory or with an immediate value, by their column: immediate, direct, indexed, extended.
    let base = op & 0xCF;
    if let Some(memory) = MEMORY.iter().find(|m| m.prefix == page && m.opcode == base && op >= 0x80) {
        let column = (op >> 4) & 3;
        let jsr = memory.name == "jsr";
        let decoded = match column {
            0 if memory.immediate => {
                let (text, length) = if memory.wide { (format!("#${:04X}", be(skip + 1)), 3) } else { (format!("#{}", hex8(at(skip + 1))), 2) };
                Decoded { length: skip + length, text: format!("{} {text}", memory.name), address: None, falls_through: true, target: None }
            }
            0 => return unknown(),
            1 => Decoded { length: skip + 2, text: format!("{} <{}", memory.name, hex8(at(skip + 1))), address: None, falls_through: true, target: None },
            2 => {
                let (text, length, address) = indexed_6809(&|i| at(i), skip + 1, &|n| pc.wrapping_add((skip + 1 + n) as u16));
                if length == 0 {
                    return unknown();
                }
                Decoded { length: skip + 1 + length, text: format!("{} {text}", memory.name), address, falls_through: true, target: None }
            }
            _ => {
                let a = be(skip + 1);
                Decoded { length: skip + 3, text: format!("{} >{{addr}}", memory.name), address: Some(a), falls_through: true, target: jsr.then_some(a) }
            }
        };
        return decoded;
    }
    if page.is_some() {
        return unknown();
    }
    // Shifts and the like: on A, on B, or on memory; and JMP, which sits among them.
    let low = op & 0x0F;
    let shift = SHIFTS.iter().find(|(_, o)| *o == low).map(|(n, _)| *n);
    match (op >> 4, shift) {
        (0x4, Some(name)) => return plain(1, format!("{name}a")),
        (0x5, Some(name)) => return plain(1, format!("{name}b")),
        _ => {}
    }
    let jmp = low == 0xE;
    let name = if jmp { Some("jmp") } else { shift };
    if let Some(name) = name.filter(|_| matches!(op >> 4, 0x0 | 0x6 | 0x7)) {
        let (text, length, address) = match op >> 4 {
            0x0 => (format!("<{}", hex8(at(1))), 1, None),
            0x6 => indexed_6809(at, 1, &|n| pc.wrapping_add(1 + n as u16)),
            _ => (">{addr}".to_string(), 2, Some(be(1))),
        };
        if length == 0 {
            return unknown();
        }
        let target = if jmp && op >> 4 == 0x7 { address } else { None };
        return Decoded { length: 1 + length, text: format!("{name} {text}"), address, falls_through: !jmp, target };
    }
    let regs = |post: u8| format!("{},{}", REG_6809[usize::from(post >> 4)], REG_6809[usize::from(post & 15)]);
    let stack = |post: u8, user: bool| {
        let names = [(0x80, "pc"), (0x40, if user { "s" } else { "u" }), (0x20, "y"), (0x10, "x"), (0x08, "dp"), (0x04, "b"), (0x02, "a"), (0x01, "cc")];
        let list: Vec<&str> = names.iter().rev().filter(|(bit, _)| post & bit != 0).map(|(_, n)| *n).collect();
        list.join(",")
    };
    match op {
        0x30..=0x33 => {
            let (text, length, address) = indexed_6809(at, 1, &|n| pc.wrapping_add(1 + n as u16));
            if length == 0 {
                return unknown();
            }
            Decoded { length: 1 + length, text: format!("{} {text}", ["leax", "leay", "leas", "leau"][usize::from(op - 0x30)]), address, falls_through: true, target: None }
        }
        0x34..=0x37 if at(1) != 0 => {
            let returns = matches!(op, 0x35 | 0x37) && at(1) & 0x80 != 0;
            Decoded { length: 2, text: format!("{} {}", ["pshs", "puls", "pshu", "pulu"][usize::from(op - 0x34)], stack(at(1), op >= 0x36)), address: None, falls_through: !returns, target: None }
        }
        0x1E | 0x1F if !regs(at(1)).contains('?') => plain(2, format!("{} {}", if op == 0x1E { "exg" } else { "tfr" }, regs(at(1)))),
        0x1A => plain(2, format!("orcc #{}", hex8(at(1)))),
        0x1C => plain(2, format!("andcc #{}", hex8(at(1)))),
        0x3C => plain(2, format!("cwai #{}", hex8(at(1)))),
        _ => unknown(),
    }
}

// ---------- The whole ----------

/// How a byte was taken: as the start of an instruction, as part of one, or as data.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Seen {
    Data,
    Start,
    Inside,
}

/// Disassembles `bytes`, which load at `origin`, tracing the code from each of `starts`. The source names every
/// address jumped to, called or used inside the program with a label; what isn't code is DB.
pub fn disassemble(cpu: Cpu, bytes: &[u8], origin: u16, starts: &[u16]) -> String {
    let end = usize::from(origin) + bytes.len();
    let inside = |a: u16| usize::from(a) >= usize::from(origin) && usize::from(a) < end;
    let decode = |a: u16| -> Decoded {
        let offset = usize::from(a - origin);
        let at = |i: usize| bytes.get(offset + i).copied().unwrap_or(0);
        match cpu {
            Cpu::M6502 => m6502(&at, a),
            Cpu::M6809 => m6809(&at, a),
            _ => z80(&at, a),
        }
    };
    let mut seen = vec![Seen::Data; bytes.len()];
    let mut decoded: BTreeMap<u16, Decoded> = BTreeMap::new();
    let mut labels: BTreeSet<u16> = starts.iter().copied().filter(|a| inside(*a)).collect();
    let mut queue: Vec<u16> = labels.iter().copied().collect();
    while let Some(mut pc) = queue.pop() {
        while inside(pc) && seen[usize::from(pc - origin)] == Seen::Data {
            let d = decode(pc);
            // An instruction running off the end, or into one already found, is data.
            let fits = usize::from(pc) + d.length <= end && (1..d.length).all(|i| seen[usize::from(pc - origin) + i] == Seen::Data);
            if !fits || d.text.starts_with("db ") || d.text.starts_with("!byte") || d.text.starts_with("fcb ") {
                break;
            }
            seen[usize::from(pc - origin)] = Seen::Start;
            for i in 1..d.length {
                seen[usize::from(pc - origin) + i] = Seen::Inside;
            }
            if let Some(t) = d.target.filter(|t| inside(*t)) {
                labels.insert(t);
                queue.push(t);
            }
            if let Some(a) = d.address.filter(|a| inside(*a)) {
                labels.insert(a);
            }
            let next = d.falls_through;
            let length = d.length;
            decoded.insert(pc, d);
            if !next {
                break;
            }
            pc = pc.wrapping_add(length as u16);
        }
    }
    // A label inside an instruction can't be written: those addresses are written as their instruction's label+n,
    // so that instruction gets the label.
    let start_of = |a: u16| {
        let mut back = a;
        while back > origin && seen[usize::from(back - origin)] == Seen::Inside {
            back -= 1;
        }
        back
    };
    let used: Vec<u16> = decoded.values().filter_map(|d| d.address).filter(|a| inside(*a)).collect();
    for address in used {
        labels.insert(start_of(address));
    }
    let name = |a: u16| format!("L{a:04X}");
    let label_for = |a: u16| -> String {
        if !inside(a) {
            return format!("${a:04X}");
        }
        let mut back = a;
        while back > origin && seen[usize::from(back - origin)] == Seen::Inside {
            back -= 1;
        }
        if back == a { name(a) } else { format!("{}+{}", name(back), a - back) }
    };
    let (byte_word, org) = match cpu {
        Cpu::M6502 => ("!byte", "* = "),
        Cpu::M6809 => ("fcb", "org "),
        _ => ("db", "org "),
    };
    let mut lines = vec![format!("        {org}${origin:04X}")];
    let mut at = origin;
    while usize::from(at) < end {
        let label = if labels.contains(&at) { format!("{}:", name(at)) } else { String::new() };
        let pad = |label: &str| format!("{label:<8}");
        if let Some(d) = decoded.get(&at) {
            let text = d.text.replace("{addr}", &d.address.map(label_for).unwrap_or_default());
            lines.push(format!("{}{text}", pad(&label)));
            at = at.wrapping_add(d.length as u16);
            continue;
        }
        // Data, up to the next label or code, eight bytes a line, with text as text.
        let mut run = Vec::new();
        while usize::from(at) + run.len() < end && run.len() < 8 {
            let a = at.wrapping_add(run.len() as u16);
            if !run.is_empty() && (labels.contains(&a) || decoded.contains_key(&a)) {
                break;
            }
            run.push(bytes[usize::from(a - origin)]);
        }
        let items: Vec<String> = run.iter().map(|b| hex8(*b)).collect();
        lines.push(format!("{}{byte_word} {}", pad(&label), items.join(",")));
        at = at.wrapping_add(run.len() as u16);
    }
    lines.join("\n") + "\n"
}

/// The same, made safe: assembled again with Limoniter's own assembler, any instruction that wouldn't give back
/// its bytes is written as DB, until it all assembles to exactly what it came from.
pub fn disassemble_checked(cpu: Cpu, bytes: &[u8], origin: u16, starts: &[u16]) -> String {
    let mut source = disassemble(cpu, bytes, origin, starts);
    for _ in 0..8 {
        let assembly = crate::asm::assemble(cpu.isa(), std::path::Path::new("disassembly.asm"), &source, &|_| Err("no".into()));
        let mut lines: Vec<String> = source.lines().map(str::to_string).collect();
        let mut changed = false;
        let image = |placed: &crate::asm::Placed| placed.bytes.clone();
        for placed in &assembly.lines {
            let offset = placed.address - i64::from(origin);
            if offset < 0 || placed.bytes.is_empty() {
                continue;
            }
            let wanted = bytes.get(offset as usize..offset as usize + placed.bytes.len());
            if wanted != Some(&image(placed)[..]) {
                let original: Vec<u8> = bytes.get(offset as usize..).unwrap_or(&[]).iter().take(placed.bytes.len().max(1)).copied().collect();
                let line = &mut lines[placed.line];
                let label: String = line.split(':').next().filter(|_| line.contains(':') && line.starts_with('L')).map(|l| format!("{l}:")).unwrap_or_default();
                let word = match cpu {
                    Cpu::M6502 => "!byte",
                    Cpu::M6809 => "fcb",
                    _ => "db",
                };
                *line = format!("{label:<8}{word} {}", original.iter().map(|b| hex8(*b)).collect::<Vec<_>>().join(","));
                changed = true;
            }
        }
        for problem in &assembly.problems {
            if let Some(line) = lines.get_mut(problem.line)
                && !line.trim_start().starts_with("db")
                && !line.contains("!byte")
                && !line.contains("fcb ")
            {
                *line = format!("        ; {line}");
                changed = true;
            }
        }
        source = lines.join("\n") + "\n";
        if !changed {
            break;
        }
    }
    source
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(cpu: Cpu, bytes: &[u8], origin: u16) -> String {
        // Every byte taken as code: each instruction followed by the next.
        let source = disassemble_checked(cpu, bytes, origin, &[origin]);
        let assembly = crate::asm::assemble(cpu.isa(), std::path::Path::new("t.asm"), &source, &|_| Err("no".into()));
        assert!(assembly.ok(), "{:?}\n{source}", assembly.problems.first());
        assert_eq!(assembly.image(i64::from(origin), i64::from(origin) + bytes.len() as i64), bytes, "{source}");
        source
    }

    #[test]
    fn every_z80_instruction_comes_back_as_itself() {
        for (name, binary) in [("z80_all", &include_bytes!("asm/testdata/z80_all.bin")[..]), ("z80n_all", &include_bytes!("asm/testdata/z80n_all.bin")[..])] {
            // Decoded straight through, instruction after instruction, as the test file was written.
            let mut at = 0;
            let mut lines = Vec::new();
            while at < binary.len() {
                let d = z80(&|i| binary.get(at + i).copied().unwrap_or(0), 0x8000 + at as u16);
                lines.push(d.text.replace("{addr}", &format!("${:04X}", d.address.unwrap_or(0))));
                at += d.length;
            }
            let source = format!("  org $8000\n{}\n", lines.iter().map(|l| format!("  {l}")).collect::<Vec<_>>().join("\n"));
            let assembly = crate::asm::assemble(&crate::asm::z80::Z80, std::path::Path::new("t.asm"), &source, &|_| Err("no".into()));
            assert!(assembly.ok(), "{name}: {:?}", assembly.problems.first().map(|p| (p, source.lines().nth(p.line))));
            let back = assembly.image(0x8000, 0x8000 + binary.len() as i64);
            let first = back.iter().zip(binary.iter()).position(|(a, b)| a != b);
            assert_eq!(first, None, "{name}: differs at {first:?}");
            let db = lines.iter().filter(|l| l.starts_with("db")).count();
            assert!(db < 10, "{name}: {db} kept as DB");
        }
    }

    #[test]
    fn every_6502_instruction_comes_back_as_itself() {
        let binary = include_bytes!("asm/testdata/m6502_all.bin");
        let mut at = 0;
        let mut lines = Vec::new();
        while at < binary.len() {
            let d = m6502(&|i| binary.get(at + i).copied().unwrap_or(0), 0x1000 + at as u16);
            lines.push(d.text.replace("{addr}", &format!("${:04X}", d.address.unwrap_or(0))));
            at += d.length;
        }
        let source = format!("  * = $1000\n{}\n", lines.iter().map(|l| format!("  {l}")).collect::<Vec<_>>().join("\n"));
        let assembly = crate::asm::assemble(&crate::asm::m6502::M6502, std::path::Path::new("t.asm"), &source, &|_| Err("no".into()));
        assert!(assembly.ok(), "{:?}", assembly.problems.first().map(|p| (p, source.lines().nth(p.line))));
        let back = assembly.image(0x1000, 0x1000 + binary.len() as i64);
        let first = back.iter().zip(binary.iter()).position(|(a, b)| a != b);
        assert_eq!(first, None, "differs at {first:?}: {:?}", first.map(|f| (&back[f..f + 3], &binary[f..f + 3])));
        assert!(lines.iter().filter(|l| l.starts_with("!byte")).count() < 4);
    }

    #[test]
    fn every_6809_instruction_comes_back_as_itself() {
        let binary = include_bytes!("asm/testdata/m6809_all.bin");
        let mut at = 0;
        let mut lines = Vec::new();
        while at < binary.len() {
            let d = m6809(&|i| binary.get(at + i).copied().unwrap_or(0), 0x3000 + at as u16);
            lines.push(d.text.replace("{addr}", &format!("${:04X}", d.address.unwrap_or(0))));
            at += d.length;
        }
        let source = format!(" org $3000\n{}\n", lines.iter().map(|l| format!(" {l}")).collect::<Vec<_>>().join("\n"));
        let assembly = crate::asm::assemble(&crate::asm::m6809::M6809, std::path::Path::new("t.asm"), &source, &|_| Err("no".into()));
        assert!(assembly.ok(), "{:?}", assembly.problems.first().map(|p| (p, source.lines().nth(p.line))));
        let back = assembly.image(0x3000, 0x3000 + binary.len() as i64);
        let first = back.iter().zip(binary.iter()).position(|(a, b)| a != b);
        let line = first.and_then(|f| assembly.lines.iter().rfind(|p| p.address <= 0x3000 + f as i64)).and_then(|p| source.lines().nth(p.line));
        assert_eq!(first, None, "{line:?}");
        let fcb = lines.iter().filter(|l| l.starts_with("fcb")).count();
        assert!(fcb <= 8, "{fcb} kept as FCB (the test file has eight bytes of data): {:?}", lines.iter().enumerate().filter(|(_, l)| l.starts_with("fcb")).map(|(i, l)| (l.clone(), lines.get(i + 1).cloned())).collect::<Vec<_>>());
    }

    /// The 48K ROM, traced from its start, its RSTs and its NMI: it comes back byte for byte, most of it as code.
    #[test]
    fn the_spectrum_rom_comes_back_as_itself() {
        let rom = include_bytes!("../vendor/rustzx-core/src/zx/roms/48.rom");
        let starts: Vec<u16> = (0..8).map(|n| n * 8).chain([0x66]).collect();
        let source = disassemble_checked(Cpu::Z80, rom, 0, &starts);
        let assembly = crate::asm::assemble(&crate::asm::z80::Z80, std::path::Path::new("rom.asm"), &source, &|_| Err("no".into()));
        assert!(assembly.ok(), "{:?}", assembly.problems.first());
        let back = assembly.image(0, 0x4000);
        if let Some(first) = back.iter().zip(rom.iter()).position(|(a, b)| a != b) {
            let line = assembly.lines.iter().rfind(|p| p.address <= first as i64).map(|p| source.lines().nth(p.line).unwrap_or(""));
            panic!("differs at ${first:04X}: {:02X?} for {:02X?}, {line:?}", &back[first..first + 4], &rom[first..first + 4]);
        }
        let code = source.lines().filter(|l| !l.contains(" db ") && !l.trim_start().starts_with("db")).count();
        assert!(code > 2000, "{code} lines of code found");
        assert!(source.contains("L0000:  di"), "{}", &source[..300]);
    }

    #[test]
    fn code_is_traced_and_labelled_and_data_left_as_data() {
        // start: ld hl,msg; call print; jr start; print: ret; msg: db "HI",0
        let bytes = [0x21, 0x09, 0x80, 0xCD, 0x08, 0x80, 0x18, 0xF8, 0xC9, b'H', b'I', 0];
        let source = round_trip(Cpu::Z80, &bytes, 0x8000);
        assert!(source.contains("L8000:  ld hl,L8009"), "{source}");
        assert!(source.contains("call L8008"), "{source}");
        assert!(source.contains("jr L8000"), "{source}");
        assert!(source.contains("L8008:  ret"), "{source}");
        assert!(source.contains("L8009:  db $48,$49,$00"), "the message isn't taken for code: {source}");
        // An address outside the program is just an address.
        let source = round_trip(Cpu::Z80, &[0xCD, 0x09, 0x16, 0xC9], 0x8000);
        assert!(source.contains("call $1609"), "{source}");
    }
}
