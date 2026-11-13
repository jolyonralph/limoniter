//! The Motorola 6809, as in the Dragon 32 and 64, the TRS-80 Color Computer and the Vectrex: every addressing mode,
//! indexed ones and all, as lwasm writes them. Direct page is used where an address is known to be in the page
//! SETDP names; < and > force direct or extended.

use super::{Isa, Line, split_operands};

pub struct M6809;

/// An instruction that reads or writes memory: its page prefix, its immediate opcode (direct is 0x10 more,
/// indexed 0x20, extended 0x30), whether its values are 16 bits, and whether it has an immediate form.
pub(crate) struct Memory {
    pub name: &'static str,
    pub prefix: Option<u8>,
    pub opcode: u8,
    pub wide: bool,
    pub immediate: bool,
}

const fn m(name: &'static str, prefix: Option<u8>, opcode: u8, wide: bool, immediate: bool) -> Memory {
    Memory { name, prefix, opcode, wide, immediate }
}

pub(crate) const MEMORY: &[Memory] = &[
    m("suba", None, 0x80, false, true),
    m("cmpa", None, 0x81, false, true),
    m("sbca", None, 0x82, false, true),
    m("subd", None, 0x83, true, true),
    m("anda", None, 0x84, false, true),
    m("bita", None, 0x85, false, true),
    m("lda", None, 0x86, false, true),
    m("sta", None, 0x87, false, false),
    m("eora", None, 0x88, false, true),
    m("adca", None, 0x89, false, true),
    m("ora", None, 0x8A, false, true),
    m("adda", None, 0x8B, false, true),
    m("cmpx", None, 0x8C, true, true),
    m("jsr", None, 0x8D, true, false),
    m("ldx", None, 0x8E, true, true),
    m("stx", None, 0x8F, true, false),
    m("subb", None, 0xC0, false, true),
    m("cmpb", None, 0xC1, false, true),
    m("sbcb", None, 0xC2, false, true),
    m("addd", None, 0xC3, true, true),
    m("andb", None, 0xC4, false, true),
    m("bitb", None, 0xC5, false, true),
    m("ldb", None, 0xC6, false, true),
    m("stb", None, 0xC7, false, false),
    m("eorb", None, 0xC8, false, true),
    m("adcb", None, 0xC9, false, true),
    m("orb", None, 0xCA, false, true),
    m("addb", None, 0xCB, false, true),
    m("ldd", None, 0xCC, true, true),
    m("std", None, 0xCD, true, false),
    m("ldu", None, 0xCE, true, true),
    m("stu", None, 0xCF, true, false),
    m("cmpd", Some(0x10), 0x83, true, true),
    m("cmpy", Some(0x10), 0x8C, true, true),
    m("ldy", Some(0x10), 0x8E, true, true),
    m("sty", Some(0x10), 0x8F, true, false),
    m("lds", Some(0x10), 0xCE, true, true),
    m("sts", Some(0x10), 0xCF, true, false),
    m("cmpu", Some(0x11), 0x83, true, true),
    m("cmps", Some(0x11), 0x8C, true, true),
];

/// Instructions on memory alone (direct 0x00, indexed 0x60, extended 0x70), or on A (0x40) or B (0x50).
pub(crate) const SHIFTS: [(&str, u8); 12] = [("neg", 0x0), ("com", 0x3), ("lsr", 0x4), ("ror", 0x6), ("asr", 0x7), ("asl", 0x8), ("lsl", 0x8), ("rol", 0x9), ("dec", 0xA), ("inc", 0xC), ("tst", 0xD), ("clr", 0xF)];

pub(crate) const INHERENT: [(&str, &[u8]); 11] = [("nop", &[0x12]), ("sync", &[0x13]), ("daa", &[0x19]), ("sex", &[0x1D]), ("rts", &[0x39]), ("abx", &[0x3A]), ("rti", &[0x3B]), ("mul", &[0x3D]), ("swi", &[0x3F]), ("swi2", &[0x10, 0x3F]), ("swi3", &[0x11, 0x3F])];

/// Branches: the short opcode (the long one is 0x10 before it, except LBRA and LBSR).
pub(crate) const BRANCHES: [(&str, u8); 18] = [("bra", 0x20), ("brn", 0x21), ("bhi", 0x22), ("bls", 0x23), ("bcc", 0x24), ("bhs", 0x24), ("bcs", 0x25), ("blo", 0x25), ("bne", 0x26), ("beq", 0x27), ("bvc", 0x28), ("bvs", 0x29), ("bpl", 0x2A), ("bmi", 0x2B), ("bge", 0x2C), ("blt", 0x2D), ("bgt", 0x2E), ("ble", 0x2F)];

fn memory(name: &str) -> Option<&'static Memory> {
    MEMORY.iter().find(|m| m.name == name)
}

fn shift(name: &str) -> Option<(u8, Option<char>)> {
    // NEGA, CLRB and so on work on a register; NEG and CLR on memory.
    if let Some((_, op)) = SHIFTS.iter().find(|(n, _)| *n == name) {
        return Some((*op, None));
    }
    let (stem, register) = name.split_at(name.len().checked_sub(1)?);
    let register = register.chars().next()?;
    if register != 'a' && register != 'b' {
        return None;
    }
    SHIFTS.iter().find(|(n, _)| *n == stem).map(|(_, op)| (*op, Some(register)))
}

fn branch(name: &str) -> Option<(bool, u8)> {
    match name {
        "bsr" => return Some((false, 0x8D)),
        "lbsr" => return Some((true, 0x17)),
        "lbra" => return Some((true, 0x16)),
        _ => {}
    }
    if let Some(short) = name.strip_prefix('l')
        && let Some((_, op)) = BRANCHES.iter().find(|(n, _)| *n == short)
    {
        return Some((true, *op));
    }
    BRANCHES.iter().find(|(n, _)| *n == name).map(|(_, op)| (false, *op))
}

fn register_code(name: &str) -> Option<u8> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "d" => 0,
        "x" => 1,
        "y" => 2,
        "u" => 3,
        "s" => 4,
        "pc" | "pcr" => 5,
        "a" => 8,
        "b" => 9,
        "cc" | "ccr" => 10,
        "dp" => 11,
        _ => return None,
    })
}

fn stack_bit(name: &str, user: bool) -> Option<u8> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "pc" => 0x80,
        "u" if !user => 0x40,
        "s" if user => 0x40,
        "y" => 0x20,
        "x" => 0x10,
        "dp" => 0x08,
        "b" => 0x04,
        "a" => 0x02,
        "d" => 0x06,
        "cc" | "ccr" => 0x01,
        "all" => 0xFF,
        _ => return None,
    })
}

fn index_register(name: &str) -> Option<u8> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "x" => 0,
        "y" => 1,
        "u" => 2,
        "s" => 3,
        _ => return None,
    })
}

impl Isa for M6809 {
    fn name(&self) -> &'static str {
        "6809"
    }

    fn big_endian(&self) -> bool {
        true
    }

    fn operand_ends_at_space(&self) -> bool {
        true
    }

    fn is_instruction(&self, word: &str) -> bool {
        memory(word).is_some() || shift(word).is_some() || branch(word).is_some() || INHERENT.iter().any(|(n, _)| *n == word) || matches!(word, "jmp" | "leax" | "leay" | "leas" | "leau" | "pshs" | "puls" | "pshu" | "pulu" | "tfr" | "exg" | "orcc" | "andcc" | "cwai")
    }

    fn encode(&self, mnemonic: &str, operands: &str, line: &mut Line) -> Result<Vec<u8>, String> {
        let upper = mnemonic.to_ascii_uppercase();
        if let Some((_, bytes)) = INHERENT.iter().find(|(n, _)| *n == mnemonic) {
            return Ok(bytes.to_vec());
        }
        if let Some((long, op)) = branch(mnemonic) {
            let target = line.eval(operands)?;
            let length: i64 = match (long, op) {
                (false, _) => 2,
                (true, 0x16 | 0x17) => 3,
                (true, _) => 4,
            };
            let offset = target - (line.here() + length);
            if !long {
                if line.checking() && !(-128..=127).contains(&offset) {
                    return Err(format!("{operands} is {offset} bytes away, out of a short branch's reach; use L{upper}"));
                }
                return Ok(vec![op, offset as u8]);
            }
            let [high, low] = (offset as u16).to_be_bytes();
            return Ok(if length == 3 { vec![op, high, low] } else { vec![0x10, op, high, low] });
        }
        match mnemonic {
            "orcc" | "andcc" | "cwai" => {
                let value = operands.strip_prefix('#').ok_or_else(|| format!("{upper} takes # and a value"))?;
                let op = match mnemonic {
                    "orcc" => 0x1A,
                    "andcc" => 0x1C,
                    _ => 0x3C,
                };
                return Ok(vec![op, byte(line, value)?]);
            }
            "tfr" | "exg" => {
                let items = split_operands(operands);
                let [from, to] = items.as_slice() else {
                    return Err(format!("{upper} takes two registers"));
                };
                let (Some(a), Some(b)) = (register_code(from), register_code(to)) else {
                    return Err(format!("{upper} takes two registers, as {upper} A,B"));
                };
                return Ok(vec![if mnemonic == "tfr" { 0x1F } else { 0x1E }, a << 4 | b]);
            }
            "pshs" | "puls" | "pshu" | "pulu" => {
                let user = mnemonic.ends_with('u');
                let mut mask = 0u8;
                for name in split_operands(operands) {
                    mask |= stack_bit(&name, user).ok_or_else(|| format!("{name} can't go on {} stack", if user { "the U" } else { "the S" }))?;
                }
                let op = match mnemonic {
                    "pshs" => 0x34,
                    "puls" => 0x35,
                    "pshu" => 0x36,
                    _ => 0x37,
                };
                return Ok(vec![op, mask]);
            }
            "leax" | "leay" | "leas" | "leau" => {
                let op = 0x30 + ["leax", "leay", "leas", "leau"].iter().position(|n| *n == mnemonic).unwrap_or(0) as u8;
                let mut bytes = vec![op];
                bytes.extend(indexed(line, operands, 1)?.ok_or_else(|| format!("{upper} takes an indexed address, as {upper} 2,X"))?);
                return Ok(bytes);
            }
            _ => {}
        }
        if let Some((op, register)) = shift(mnemonic) {
            if let Some(register) = register {
                return Ok(vec![op | if register == 'a' { 0x40 } else { 0x50 }]);
            }
            return address(line, operands, None, [op, op | 0x60, op | 0x70], 1);
        }
        if mnemonic == "jmp" {
            return address(line, operands, None, [0x0E, 0x6E, 0x7E], 1);
        }
        let found = memory(mnemonic).ok_or_else(|| format!("{mnemonic} isn't a 6809 instruction"))?;
        let mut head = Vec::new();
        head.extend(found.prefix);
        if let Some(value) = operands.strip_prefix('#') {
            if !found.immediate {
                return Err(format!("{upper} can't take an immediate value"));
            }
            head.push(found.opcode);
            if found.wide {
                head.extend((line.eval(value)? as u16).to_be_bytes());
            } else {
                head.push(byte(line, value)?);
            }
            return Ok(head);
        }
        address(line, operands, found.prefix, [found.opcode + 0x10, found.opcode + 0x20, found.opcode + 0x30], head.len() + 1)
    }
}

fn byte(line: &mut Line, text: &str) -> Result<u8, String> {
    let value = line.eval(text)?;
    if line.checking() && !(-128..=255).contains(&value) {
        return Err(format!("{} is {value}, which doesn't fit in a byte", text.trim()));
    }
    Ok(value as u8)
}

/// An address: indexed, direct or extended, with the opcodes for each. `before` is how many bytes come before
/// the postbyte, for offsets from PC.
fn address(line: &mut Line, operands: &str, prefix: Option<u8>, [direct, indexed_op, extended]: [u8; 3], before: usize) -> Result<Vec<u8>, String> {
    let mut bytes: Vec<u8> = prefix.into_iter().collect();
    if let Some(post) = indexed(line, operands, before)? {
        bytes.push(indexed_op);
        bytes.extend(post);
        return Ok(bytes);
    }
    let (text, forced) = match operands.trim() {
        t if t.starts_with('<') => (&t[1..], Some(true)),
        t if t.starts_with('>') => (&t[1..], Some(false)),
        t => (t, None),
    };
    let page = line.direct_page();
    let in_page = line.known(text).zip(page).is_some_and(|(v, dp)| (v >> 8) & 0xFF == dp && (0..=0xFFFF).contains(&v));
    let value = line.eval(text)?;
    if forced.unwrap_or(in_page) {
        bytes.push(direct);
        bytes.push(value as u8);
    } else {
        bytes.push(extended);
        bytes.extend((value as u16).to_be_bytes());
    }
    Ok(bytes)
}

/// The postbyte and offset of an indexed address (n,X  ,X+  A,Y  [n,PCR] ...), or None if it isn't one.
fn indexed(line: &mut Line, operands: &str, before: usize) -> Result<Option<Vec<u8>>, String> {
    let text = operands.trim();
    let (text, indirect) = match text.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        Some(inner) => (inner.trim(), true),
        None => (text, false),
    };
    let ind = if indirect { 0x10 } else { 0 };
    let Some((offset, register)) = text.rsplit_once(',') else {
        // [address]: extended indirect.
        if indirect {
            let value = line.eval(text)?;
            let mut bytes = vec![0x9F];
            bytes.extend((value as u16).to_be_bytes());
            return Ok(Some(bytes));
        }
        return Ok(None);
    };
    let register = register.trim();
    let offset = offset.trim();
    let lower = register.to_ascii_lowercase();
    // Auto increment and decrement.
    let (name, post) = if let Some(r) = lower.strip_suffix("++") {
        (r.to_string(), Some(0x81))
    } else if let Some(r) = lower.strip_suffix('+') {
        (r.to_string(), Some(0x80))
    } else if let Some(r) = lower.strip_prefix("--") {
        (r.to_string(), Some(0x83))
    } else if let Some(r) = lower.strip_prefix('-') {
        (r.to_string(), Some(0x82))
    } else {
        (lower.clone(), None)
    };
    if name == "pc" || name == "pcr" {
        // From the end of the instruction: the offset is to the label (PCR), or as written (PC).
        let value = line.eval(if offset.is_empty() { "0" } else { offset })?;
        let small = line.known(offset).is_some();
        let short_offset = if name == "pcr" { value - (line.here() + before as i64 + 2) } else { value };
        if small && (-128..=127).contains(&short_offset) {
            return Ok(Some(vec![0x8C | ind, short_offset as u8]));
        }
        let long_offset = if name == "pcr" { value - (line.here() + before as i64 + 3) } else { value };
        let mut bytes = vec![0x8D | ind];
        bytes.extend((long_offset as u16).to_be_bytes());
        return Ok(Some(bytes));
    }
    let Some(r) = index_register(&name) else {
        return Err(format!("{register} can't be indexed from; X, Y, U, S or PC can"));
    };
    let rr = r << 5;
    if let Some(post) = post {
        if !offset.is_empty() {
            return Err("an offset can't go with auto increment or decrement".into());
        }
        if indirect && (post == 0x80 || post == 0x82) {
            return Err("only ,R++ and ,--R can be indirect".into());
        }
        return Ok(Some(vec![0x80 | rr | post | ind]));
    }
    match offset.to_ascii_lowercase().as_str() {
        "" => return Ok(Some(vec![0x84 | rr | ind])),
        "a" => return Ok(Some(vec![0x86 | rr | ind])),
        "b" => return Ok(Some(vec![0x85 | rr | ind])),
        "d" => return Ok(Some(vec![0x8B | rr | ind])),
        _ => {}
    }
    // A size forced with < or >, or the smallest that fits once the value's known.
    let (offset, forced) = match offset {
        o if o.starts_with("<<") => (&o[2..], Some(5)),
        o if o.starts_with('<') => (&o[1..], Some(8)),
        o if o.starts_with('>') => (&o[1..], Some(16)),
        o => (o, None),
    };
    let known = line.known(offset);
    let value = line.eval(offset)?;
    let size = forced.unwrap_or(match known {
        Some(0) => 0,
        Some(v) if (-16..=15).contains(&v) && !indirect => 5,
        Some(v) if (-128..=127).contains(&v) => 8,
        _ => 16,
    });
    Ok(Some(match size {
        0 => vec![0x84 | rr | ind],
        5 => vec![rr | (value as u8 & 0x1F)],
        8 => vec![0x88 | rr | ind, value as u8],
        _ => {
            let mut bytes = vec![0x89 | rr | ind];
            bytes.extend((value as u16).to_be_bytes());
            bytes
        }
    }))
}
