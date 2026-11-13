//! The Z80, with its undocumented instructions (the halves of IX and IY, SLL, OUT (C),0, and the rest), and the
//! ZX Spectrum Next's Z80N instructions (NEXTREG, MUL, LDIRX and the others).

use super::{Isa, Line, split_operands};

pub struct Z80;

const PLAIN: [(&str, &[u8]); 47] = [
    ("nop", &[0x00]),
    ("rlca", &[0x07]),
    ("rrca", &[0x0F]),
    ("rla", &[0x17]),
    ("rra", &[0x1F]),
    ("daa", &[0x27]),
    ("cpl", &[0x2F]),
    ("scf", &[0x37]),
    ("ccf", &[0x3F]),
    ("halt", &[0x76]),
    ("exx", &[0xD9]),
    ("di", &[0xF3]),
    ("ei", &[0xFB]),
    ("neg", &[0xED, 0x44]),
    ("retn", &[0xED, 0x45]),
    ("reti", &[0xED, 0x4D]),
    ("rrd", &[0xED, 0x67]),
    ("rld", &[0xED, 0x6F]),
    ("ldi", &[0xED, 0xA0]),
    ("cpi", &[0xED, 0xA1]),
    ("ini", &[0xED, 0xA2]),
    ("outi", &[0xED, 0xA3]),
    ("ldd", &[0xED, 0xA8]),
    ("cpd", &[0xED, 0xA9]),
    ("ind", &[0xED, 0xAA]),
    ("outd", &[0xED, 0xAB]),
    ("ldir", &[0xED, 0xB0]),
    ("cpir", &[0xED, 0xB1]),
    ("inir", &[0xED, 0xB2]),
    ("otir", &[0xED, 0xB3]),
    ("lddr", &[0xED, 0xB8]),
    ("cpdr", &[0xED, 0xB9]),
    ("indr", &[0xED, 0xBA]),
    ("otdr", &[0xED, 0xBB]),
    // The Next's.
    ("swapnib", &[0xED, 0x23]),
    ("outinb", &[0xED, 0x90]),
    ("pixeldn", &[0xED, 0x93]),
    ("pixelad", &[0xED, 0x94]),
    ("setae", &[0xED, 0x95]),
    ("ldix", &[0xED, 0xA4]),
    ("ldws", &[0xED, 0xA5]),
    ("lddx", &[0xED, 0xAC]),
    ("ldirx", &[0xED, 0xB4]),
    ("ldpirx", &[0xED, 0xB7]),
    ("lddrx", &[0xED, 0xBC]),
    ("ldirscale", &[0xED, 0xB6]),
    ("otib", &[0xED, 0x90]),
];

const OTHERS: [&str; 39] = [
    "ld", "push", "pop", "ex", "add", "adc", "sub", "sbc", "and", "xor", "or", "cp", "inc", "dec", "im", "jp", "jr", "djnz", "call", "ret", "rst", "in", "out", "rlc", "rrc", "rl", "rr", "sla", "sra", "sll", "sl1", "srl", "bit", "res", "set", "mirror", "test", "mul", "nextreg",
];

const NEXT_SHIFTS: [(&str, u8); 5] = [("bsla", 0x28), ("bsra", 0x29), ("bsrl", 0x2A), ("bsrf", 0x2B), ("brlc", 0x2C)];

/// An operand.
#[derive(Clone, Debug, PartialEq)]
enum Op {
    /// An 8-bit register by its number in the instruction (B 0 to A 7), with DD or FD for a half of IX or IY.
    R(u8, Option<u8>),
    I,
    Refresh,
    Pair(Pair),
    /// (HL), (BC), (DE), (SP), (C).
    At(Pair),
    PortC,
    /// (IX+d) or (IY+d): the prefix, and the displacement as written.
    Indexed(u8, String),
    /// (nn).
    Memory(String),
    Value(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pair {
    Bc,
    De,
    Hl,
    Sp,
    Af,
    AfAlt,
    Ix,
    Iy,
}

impl Pair {
    /// Its number where BC, DE, HL and SP are 0 to 3 (IX and IY standing for HL).
    fn number(self) -> Option<u8> {
        match self {
            Pair::Bc => Some(0),
            Pair::De => Some(1),
            Pair::Hl | Pair::Ix | Pair::Iy => Some(2),
            Pair::Sp => Some(3),
            _ => None,
        }
    }

    /// Its number for PUSH and POP, where AF is 3.
    fn stack_number(self) -> Option<u8> {
        match self {
            Pair::Af => Some(3),
            Pair::Sp => None,
            other => other.number(),
        }
    }

    fn prefix(self) -> Option<u8> {
        match self {
            Pair::Ix => Some(0xDD),
            Pair::Iy => Some(0xFD),
            _ => None,
        }
    }
}

fn pair_named(name: &str) -> Option<Pair> {
    Some(match name {
        "bc" => Pair::Bc,
        "de" => Pair::De,
        "hl" => Pair::Hl,
        "sp" => Pair::Sp,
        "af" => Pair::Af,
        "af'" => Pair::AfAlt,
        "ix" => Pair::Ix,
        "iy" => Pair::Iy,
        _ => return None,
    })
}

fn register_named(name: &str) -> Option<Op> {
    Some(match name {
        "b" => Op::R(0, None),
        "c" => Op::R(1, None),
        "d" => Op::R(2, None),
        "e" => Op::R(3, None),
        "h" => Op::R(4, None),
        "l" => Op::R(5, None),
        "a" => Op::R(7, None),
        "ixh" | "xh" | "hx" | "ixu" => Op::R(4, Some(0xDD)),
        "ixl" | "xl" | "lx" => Op::R(5, Some(0xDD)),
        "iyh" | "yh" | "hy" | "iyu" => Op::R(4, Some(0xFD)),
        "iyl" | "yl" | "ly" => Op::R(5, Some(0xFD)),
        "i" => Op::I,
        "r" => Op::Refresh,
        _ => return None,
    })
}

/// Whether the text is wholly inside one pair of brackets.
fn bracketed(text: &str) -> Option<&str> {
    let (open, close) = match text.chars().next()? {
        '(' => ('(', ')'),
        '[' => ('[', ']'),
        _ => return None,
    };
    if !text.ends_with(close) {
        return None;
    }
    let mut depth = 0;
    for (i, c) in text.char_indices() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 && i != text.len() - 1 {
                return None;
            }
        }
    }
    Some(&text[1..text.len() - 1])
}

fn operand(text: &str) -> Op {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    if let Some(op) = register_named(&lower) {
        return op;
    }
    if let Some(pair) = pair_named(&lower) {
        return Op::Pair(pair);
    }
    if let Some(inner) = bracketed(text) {
        let inner = inner.trim();
        let lower = inner.to_ascii_lowercase();
        match lower.as_str() {
            "hl" => return Op::At(Pair::Hl),
            "bc" => return Op::At(Pair::Bc),
            "de" => return Op::At(Pair::De),
            "sp" => return Op::At(Pair::Sp),
            "c" => return Op::PortC,
            _ => {}
        }
        for (name, prefix) in [("ix", 0xDD), ("iy", 0xFD)] {
            if let Some(rest) = lower.strip_prefix(name) {
                let rest = rest.trim_start();
                if rest.is_empty() {
                    return Op::Indexed(prefix, "0".into());
                }
                if rest.starts_with('+') || rest.starts_with('-') {
                    return Op::Indexed(prefix, inner[inner.len() - rest.len()..].to_string());
                }
            }
        }
        return Op::Memory(inner.to_string());
    }
    Op::Value(text.to_string())
}

fn condition(text: &str) -> Option<u8> {
    Some(match text.trim().to_ascii_lowercase().as_str() {
        "nz" => 0,
        "z" => 1,
        "nc" => 2,
        "c" => 3,
        "po" => 4,
        "pe" => 5,
        "p" => 6,
        "m" => 7,
        _ => return None,
    })
}

const ALU: [&str; 8] = ["add", "adc", "sub", "sbc", "and", "xor", "or", "cp"];
const ROTATES: [&str; 8] = ["rlc", "rrc", "rl", "rr", "sla", "sra", "sll", "srl"];

struct Values<'l, 'p, 'a> {
    line: &'l mut Line<'p, 'a>,
}

impl Values<'_, '_, '_> {
    fn byte(&mut self, text: &str) -> Result<u8, String> {
        let value = self.line.eval(text)?;
        if self.line.checking() && !(-128..=255).contains(&value) {
            return Err(too_big(text, value, "a byte"));
        }
        Ok(value as u8)
    }

    fn word(&mut self, text: &str) -> Result<[u8; 2], String> {
        let value = self.line.eval(text)?;
        if self.line.checking() && !(-32768..=65535).contains(&value) {
            return Err(too_big(text, value, "16 bits"));
        }
        Ok((value as u16).to_le_bytes())
    }

    fn displacement(&mut self, text: &str) -> Result<u8, String> {
        let value = self.line.eval(text)?;
        if self.line.checking() && !(-128..=127).contains(&value) {
            return Err(format!("the offset {value} is out of reach (-128 to 127)"));
        }
        Ok(value as u8)
    }

    /// A relative jump from an instruction of two bytes here.
    fn relative(&mut self, text: &str) -> Result<u8, String> {
        let target = self.line.eval(text)?;
        let offset = target - (self.line.here() + 2);
        if self.line.checking() && !(-128..=127).contains(&offset) {
            return Err(format!("{text} is {offset} bytes away, out of a relative jump's reach (-128 to 127); use JP"));
        }
        Ok(offset as u8)
    }
}

/// An 8-bit operand as the register field of an instruction: its number, prefix and displacement.
fn as_r(op: &Op) -> Option<(u8, Option<u8>, Option<&str>)> {
    match op {
        Op::R(code, prefix) => Some((*code, *prefix, None)),
        Op::At(Pair::Hl) => Some((6, None, None)),
        Op::Indexed(prefix, disp) => Some((6, Some(*prefix), Some(disp.as_str()))),
        _ => None,
    }
}

/// Two register fields that can go in one instruction: IXH can't go with H, nor (IX+d) with IXH.
fn compatible(a: (u8, Option<u8>, Option<&str>), b: (u8, Option<u8>, Option<&str>)) -> Result<Option<u8>, String> {
    let wrong = Err("those registers can't be used together".to_string());
    match (a.2.is_some(), b.2.is_some()) {
        (true, true) => return wrong,
        (true, false) => return if b.1.is_some() { wrong } else { Ok(a.1) },
        (false, true) => return if a.1.is_some() { wrong } else { Ok(b.1) },
        _ => {}
    }
    match (a.1, b.1) {
        (Some(x), Some(y)) if x != y => wrong,
        (Some(p), None) if matches!(b.0, 4..=6) => {
            let _ = p;
            wrong
        }
        (None, Some(_)) if matches!(a.0, 4..=6) => wrong,
        (p, q) => Ok(p.or(q)),
    }
}

/// Builds an instruction: its prefix, the opcode, its displacement, and what follows.
fn build(prefix: Option<u8>, opcode: &[u8], displacement: Option<u8>, after: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(4);
    bytes.extend(prefix);
    bytes.extend_from_slice(opcode);
    bytes.extend(displacement);
    bytes.extend_from_slice(after);
    bytes
}

impl Isa for Z80 {
    fn name(&self) -> &'static str {
        "Z80"
    }

    fn colon_separates(&self) -> bool {
        true
    }

    fn is_instruction(&self, word: &str) -> bool {
        PLAIN.iter().any(|(name, _)| *name == word) || OTHERS.contains(&word) || NEXT_SHIFTS.iter().any(|(name, _)| *name == word) || word == "exa"
    }

    fn cycles(&self, bytes: &[u8]) -> Option<String> {
        cycles(bytes)
    }

    fn encode(&self, mnemonic: &str, operands: &str, line: &mut Line) -> Result<Vec<u8>, String> {
        if let Some((_, bytes)) = PLAIN.iter().find(|(name, _)| *name == mnemonic) {
            if !operands.is_empty() {
                return Err(format!("{} takes nothing after it", mnemonic.to_ascii_uppercase()));
            }
            return Ok(bytes.to_vec());
        }
        let texts = split_operands(operands);
        let ops: Vec<Op> = texts.iter().map(|t| operand(t)).collect();
        let mut v = Values { line };
        let wrong = || Err(format!("{} {} isn't a Z80 instruction", mnemonic.to_ascii_uppercase(), operands));
        match (mnemonic, ops.as_slice()) {
            ("ld", [a, b]) => load(a, b, &mut v).unwrap_or_else(wrong),
            ("push", [Op::Pair(p)]) => match p.stack_number() {
                Some(n) => Ok(build(p.prefix(), &[0xC5 | n << 4], None, &[])),
                None => wrong(),
            },
            ("push", [Op::Value(value)]) => {
                // The Next's PUSH nn, high byte first.
                let [low, high] = v.word(value)?;
                Ok(vec![0xED, 0x8A, high, low])
            }
            ("pop", [Op::Pair(p)]) => match p.stack_number() {
                Some(n) => Ok(build(p.prefix(), &[0xC1 | n << 4], None, &[])),
                None => wrong(),
            },
            ("ex", [Op::Pair(Pair::De), Op::Pair(Pair::Hl)]) | ("ex", [Op::Pair(Pair::Hl), Op::Pair(Pair::De)]) => Ok(vec![0xEB]),
            ("ex", [Op::Pair(Pair::Af), Op::Pair(Pair::AfAlt | Pair::Af)]) => Ok(vec![0x08]),
            ("exa", []) => Ok(vec![0x08]),
            ("ex", [Op::At(Pair::Sp), Op::Pair(p @ (Pair::Hl | Pair::Ix | Pair::Iy))]) | ("ex", [Op::Pair(p @ (Pair::Hl | Pair::Ix | Pair::Iy)), Op::At(Pair::Sp)]) => Ok(build(p.prefix(), &[0xE3], None, &[])),
            ("add", [Op::Pair(target @ (Pair::Hl | Pair::Ix | Pair::Iy)), Op::Pair(source)]) => {
                // ADD IX,IX but not ADD IX,HL.
                let n = match (target, source) {
                    (t, s) if s == t => 2,
                    (_, Pair::Hl | Pair::Ix | Pair::Iy) => return wrong(),
                    (_, s) => s.number().ok_or("that pair can't be added")?,
                };
                Ok(build(target.prefix(), &[0x09 | n << 4], None, &[]))
            }
            // The Next's ADD HL,A and ADD HL,nn.
            ("add", [Op::Pair(p @ (Pair::Hl | Pair::De | Pair::Bc)), Op::R(7, None)]) => Ok(vec![0xED, 0x31 + [Pair::Hl, Pair::De, Pair::Bc].iter().position(|x| x == p).unwrap_or(0) as u8]),
            ("add", [Op::Pair(p @ (Pair::Hl | Pair::De | Pair::Bc)), Op::Value(value)]) => {
                let [low, high] = v.word(value)?;
                Ok(vec![0xED, 0x34 + [Pair::Hl, Pair::De, Pair::Bc].iter().position(|x| x == p).unwrap_or(0) as u8, low, high])
            }
            ("adc", [Op::Pair(Pair::Hl), Op::Pair(p)]) => match p.number().filter(|_| p.prefix().is_none()) {
                Some(n) => Ok(vec![0xED, 0x4A | n << 4]),
                None => wrong(),
            },
            ("sbc", [Op::Pair(Pair::Hl), Op::Pair(p)]) => match p.number().filter(|_| p.prefix().is_none()) {
                Some(n) => Ok(vec![0xED, 0x42 | n << 4]),
                None => wrong(),
            },
            (alu, [a, b]) if ALU.contains(&alu) && *a == Op::R(7, None) => arithmetic(alu, b, &mut v).unwrap_or_else(wrong),
            (alu, [b]) if ALU.contains(&alu) => arithmetic(alu, b, &mut v).unwrap_or_else(wrong),
            ("inc" | "dec", [Op::Pair(p)]) => match p.number() {
                Some(n) => Ok(build(p.prefix(), &[if mnemonic == "inc" { 0x03 } else { 0x0B } | n << 4], None, &[])),
                None => wrong(),
            },
            ("inc" | "dec", [op]) => match as_r(op) {
                Some((code, prefix, disp)) => {
                    let displacement = disp.map(|d| v.displacement(d)).transpose()?;
                    Ok(build(prefix, &[if mnemonic == "inc" { 0x04 } else { 0x05 } | code << 3], displacement, &[]))
                }
                None => wrong(),
            },
            ("im", [Op::Value(mode)]) => match v.line.eval(mode)? {
                0 => Ok(vec![0xED, 0x46]),
                1 => Ok(vec![0xED, 0x56]),
                2 => Ok(vec![0xED, 0x5E]),
                _ => Err("IM takes 0, 1 or 2".into()),
            },
            ("jp", [Op::At(Pair::Hl)]) | ("jp", [Op::Pair(Pair::Hl)]) => Ok(vec![0xE9]),
            ("jp", [Op::Indexed(prefix, disp)]) if disp == "0" => Ok(vec![*prefix, 0xE9]),
            ("jp", [Op::Pair(p @ (Pair::Ix | Pair::Iy))]) => Ok(build(p.prefix(), &[0xE9], None, &[])),
            ("jp", [Op::PortC]) => Ok(vec![0xED, 0x98]),
            ("jp", [target]) => {
                let target = value_text(target).ok_or_else(not_address)?;
                Ok(build(None, &[0xC3], None, &v.word(target)?))
            }
            ("jp", [_, _]) => {
                let cc = condition(&texts[0]).ok_or_else(|| format!("{} isn't a condition", texts[0]))?;
                Ok(build(None, &[0xC2 | cc << 3], None, &v.word(&texts[1])?))
            }
            ("call", [target]) => {
                let target = value_text(target).ok_or_else(not_address)?;
                Ok(build(None, &[0xCD], None, &v.word(target)?))
            }
            ("call", [_, _]) => {
                let cc = condition(&texts[0]).ok_or_else(|| format!("{} isn't a condition", texts[0]))?;
                Ok(build(None, &[0xC4 | cc << 3], None, &v.word(&texts[1])?))
            }
            ("jr", [target]) => {
                let target = value_text(target).ok_or_else(not_address)?;
                Ok(vec![0x18, v.relative(target)?])
            }
            ("jr", [_, _]) => {
                let cc = condition(&texts[0]).filter(|cc| *cc < 4).ok_or_else(|| format!("JR can't use the condition {}", texts[0]))?;
                Ok(vec![0x20 | cc << 3, v.relative(&texts[1])?])
            }
            ("djnz", [target]) => {
                let target = value_text(target).ok_or_else(not_address)?;
                Ok(vec![0x10, v.relative(target)?])
            }
            ("ret", []) => Ok(vec![0xC9]),
            ("ret", [_]) => {
                let cc = condition(&texts[0]).ok_or_else(|| format!("{} isn't a condition", texts[0]))?;
                Ok(vec![0xC0 | cc << 3])
            }
            ("rst", [Op::Value(value)]) => {
                let n = v.line.eval(value)?;
                match n {
                    0 | 8 | 0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38 => Ok(vec![0xC7 | n as u8]),
                    1..=7 => Ok(vec![0xC7 | (n as u8) << 3]),
                    _ => Err(format!("RST can't go to {n}")),
                }
            }
            ("in", [Op::R(7, None), Op::Memory(port)]) => Ok(vec![0xDB, v.byte(port)?]),
            ("in", [Op::R(code, None), Op::PortC]) => Ok(vec![0xED, 0x40 | code << 3]),
            ("in", [Op::PortC]) => Ok(vec![0xED, 0x70]),
            ("in", [Op::Value(f), Op::PortC]) if f.eq_ignore_ascii_case("f") => Ok(vec![0xED, 0x70]),
            ("out", [Op::Memory(port), Op::R(7, None)]) => Ok(vec![0xD3, v.byte(port)?]),
            ("out", [Op::PortC, Op::R(code, None)]) => Ok(vec![0xED, 0x41 | code << 3]),
            ("out", [Op::PortC, Op::Value(zero)]) => {
                if v.line.eval(zero)? == 0 {
                    Ok(vec![0xED, 0x71])
                } else {
                    wrong()
                }
            }
            (rotate, [op]) if ROTATES.contains(&rotate) || rotate == "sl1" => {
                let n = ROTATES.iter().position(|r| *r == rotate).unwrap_or(6) as u8;
                bits(n << 3, op, None, &mut v).unwrap_or_else(wrong)
            }
            (rotate, [op, Op::R(copy, None)]) if ROTATES.contains(&rotate) || rotate == "sl1" => {
                let n = ROTATES.iter().position(|r| *r == rotate).unwrap_or(6) as u8;
                bits(n << 3, op, Some(*copy), &mut v).unwrap_or_else(wrong)
            }
            ("bit" | "res" | "set", [Op::Value(bit), op, rest @ ..]) => {
                let b = v.line.eval(bit)?;
                if !(0..=7).contains(&b) {
                    return Err(format!("there's no bit {b} in a byte"));
                }
                let base = match mnemonic {
                    "bit" => 0x40,
                    "res" => 0x80,
                    _ => 0xC0,
                };
                let copy = match rest {
                    [] => None,
                    [Op::R(copy, None)] if mnemonic != "bit" => Some(*copy),
                    _ => return wrong(),
                };
                bits(base | (b as u8) << 3, op, copy, &mut v).unwrap_or_else(wrong)
            }
            // The Next's.
            ("mirror", [] | [Op::R(7, None)]) => Ok(vec![0xED, 0x24]),
            ("mul", [] | [Op::R(2, None), Op::R(3, None)]) => Ok(vec![0xED, 0x30]),
            ("test", [Op::Value(value)]) => Ok(vec![0xED, 0x27, v.byte(value)?]),
            ("nextreg", [Op::Value(register), Op::R(7, None)]) => Ok(vec![0xED, 0x92, v.byte(register)?]),
            ("nextreg", [Op::Value(register), Op::Value(value)]) => Ok(vec![0xED, 0x91, v.byte(register)?, v.byte(value)?]),
            (shift, [Op::Pair(Pair::De), Op::R(0, None)]) if NEXT_SHIFTS.iter().any(|(n, _)| *n == shift) => Ok(vec![0xED, NEXT_SHIFTS.iter().find(|(n, _)| *n == shift).map_or(0, |(_, op)| *op)]),
            _ => wrong(),
        }
    }
}

fn too_big(text: &str, value: i64, size: &str) -> String {
    if text.trim() == value.to_string() { format!("{value} doesn't fit in {size}") } else { format!("{text} is {value}, which doesn't fit in {size}") }
}

fn not_address() -> String {
    "that isn't an address".into()
}

fn value_text(op: &Op) -> Option<&str> {
    match op {
        Op::Value(text) => Some(text),
        _ => None,
    }
}


/// ADD A, ADC A, SUB, SBC A, AND, XOR, OR and CP.
fn arithmetic(alu: &str, op: &Op, v: &mut Values) -> Option<Result<Vec<u8>, String>> {
    let n = ALU.iter().position(|a| *a == alu)? as u8;
    if let Some((code, prefix, disp)) = as_r(op) {
        let displacement = match disp.map(|d| v.displacement(d)).transpose() {
            Ok(d) => d,
            Err(problem) => return Some(Err(problem)),
        };
        return Some(Ok(build(prefix, &[0x80 | n << 3 | code], displacement, &[])));
    }
    match op {
        Op::Value(value) => Some(v.byte(value).map(|b| vec![0xC6 | n << 3, b])),
        _ => None,
    }
}

/// The CB instructions: rotates and shifts, BIT, RES and SET; with (IX+d), the result can also be copied to a
/// register, as the undocumented forms do.
fn bits(opcode: u8, op: &Op, copy: Option<u8>, v: &mut Values) -> Option<Result<Vec<u8>, String>> {
    let (code, prefix, disp) = as_r(op)?;
    match (prefix, disp) {
        (Some(prefix), Some(disp)) => {
            let displacement = match v.displacement(disp) {
                Ok(d) => d,
                Err(problem) => return Some(Err(problem)),
            };
            Some(Ok(vec![prefix, 0xCB, displacement, opcode | copy.unwrap_or(6)]))
        }
        (Some(_), None) => None,
        _ if copy.is_some() => None,
        _ => Some(Ok(vec![0xCB, opcode | code])),
    }
}

fn load(a: &Op, b: &Op, v: &mut Values) -> Option<Result<Vec<u8>, String>> {
    (|| -> Option<Result<Vec<u8>, String>> {
        // Between 8-bit registers and memory through HL, IX or IY.
        if let (Some(x), Some(y)) = (as_r(a), as_r(b)) {
            if x.0 == 6 && y.0 == 6 {
                return None;
            }
            let prefix = match compatible(x, y) {
                Ok(p) => p,
                Err(problem) => return Some(Err(problem)),
            };
            let displacement = match x.2.or(y.2).map(|d| v.displacement(d)).transpose() {
                Ok(d) => d,
                Err(problem) => return Some(Err(problem)),
            };
            return Some(Ok(build(prefix, &[0x40 | x.0 << 3 | y.0], displacement, &[])));
        }
        Some(match (a, b) {
            (op, Op::Value(value)) if as_r(op).is_some() => {
                let (code, prefix, disp) = as_r(op)?;
                let displacement = disp.map(|d| v.displacement(d)).transpose();
                let byte = v.byte(value);
                match (displacement, byte) {
                    (Ok(d), Ok(n)) => Ok(build(prefix, &[0x06 | code << 3], d, &[n])),
                    (Err(problem), _) | (_, Err(problem)) => Err(problem),
                }
            }
            (Op::R(7, None), Op::At(Pair::Bc)) => Ok(vec![0x0A]),
            (Op::R(7, None), Op::At(Pair::De)) => Ok(vec![0x1A]),
            (Op::At(Pair::Bc), Op::R(7, None)) => Ok(vec![0x02]),
            (Op::At(Pair::De), Op::R(7, None)) => Ok(vec![0x12]),
            (Op::R(7, None), Op::Memory(address)) => v.word(address).map(|w| build(None, &[0x3A], None, &w)),
            (Op::Memory(address), Op::R(7, None)) => v.word(address).map(|w| build(None, &[0x32], None, &w)),
            (Op::I, Op::R(7, None)) => Ok(vec![0xED, 0x47]),
            (Op::Refresh, Op::R(7, None)) => Ok(vec![0xED, 0x4F]),
            (Op::R(7, None), Op::I) => Ok(vec![0xED, 0x57]),
            (Op::R(7, None), Op::Refresh) => Ok(vec![0xED, 0x5F]),
            (Op::Pair(p), Op::Value(value)) => {
                let n = p.number()?;
                v.word(value).map(|w| build(p.prefix(), &[0x01 | n << 4], None, &w))
            }
            (Op::Pair(p @ (Pair::Hl | Pair::Ix | Pair::Iy)), Op::Memory(address)) => v.word(address).map(|w| build(p.prefix(), &[0x2A], None, &w)),
            (Op::Memory(address), Op::Pair(p @ (Pair::Hl | Pair::Ix | Pair::Iy))) => v.word(address).map(|w| build(p.prefix(), &[0x22], None, &w)),
            (Op::Pair(p), Op::Memory(address)) => {
                let n = p.number()?;
                v.word(address).map(|w| build(None, &[0xED, 0x4B | n << 4], None, &w))
            }
            (Op::Memory(address), Op::Pair(p)) => {
                let n = p.number()?;
                v.word(address).map(|w| build(None, &[0xED, 0x43 | n << 4], None, &w))
            }
            (Op::Pair(Pair::Sp), Op::Pair(p @ (Pair::Hl | Pair::Ix | Pair::Iy))) => Ok(build(p.prefix(), &[0xF9], None, &[])),
            _ => return None,
        })
    })()
}

/// How long an instruction takes, in T-states: one number, or two for a condition not met and met.
pub fn cycles(bytes: &[u8]) -> Option<String> {
    let first = *bytes.first()?;
    let one = |t: u32| Some(t.to_string());
    let two = |a: u32, b: u32| Some(format!("{a}/{b}"));
    match first {
        0xCB => {
            let op = *bytes.get(1)?;
            let memory = op & 7 == 6;
            match (op >> 6, memory) {
                (1, true) => one(12),
                (_, true) => one(15),
                _ => one(8),
            }
        }
        0xDD | 0xFD => {
            let op = *bytes.get(1)?;
            if op == 0xCB {
                let last = *bytes.get(3)?;
                return if last >> 6 == 1 { one(20) } else { one(23) };
            }
            // Like the HL instruction, four more; with (IX+d), more again.
            let base: u32 = cycles(&bytes[1..]).and_then(|t| t.split('/').next().and_then(|n| n.parse().ok()))?;
            let indexed = matches!(op, 0x34..=0x36) || (op & 0xC7 == 0x46 && op != 0x76) || (op & 0xF8 == 0x70 && op != 0x76) || (op & 0xC7 == 0x86);
            if matches!(op, 0x34 | 0x35) {
                return one(23);
            }
            if op == 0x36 {
                return one(19);
            }
            if indexed {
                return one(19);
            }
            one(base + 4)
        }
        0xED => {
            let op = *bytes.get(1)?;
            match op {
                0x43 | 0x53 | 0x63 | 0x73 | 0x4B | 0x5B | 0x6B | 0x7B => one(20),
                0x67 | 0x6F => one(18),
                0x47 | 0x4F | 0x57 | 0x5F => one(9),
                0x42 | 0x52 | 0x62 | 0x72 | 0x4A | 0x5A | 0x6A | 0x7A => one(15),
                0x45 | 0x4D => one(14),
                0xA0..=0xA3 | 0xA8..=0xAB => one(16),
                0xB0..=0xB3 | 0xB8..=0xBB => two(16, 21),
                0x40..=0x7F if op & 7 <= 1 => one(12),
                0x30 => one(8),
                0x91 => one(20),
                0x92 => one(17),
                0x8A => one(23),
                0x34..=0x36 => one(16),
                0xA4 | 0xAC | 0xA5 => one(16),
                0xB4 | 0xBC | 0xB7 => two(16, 21),
                0x27 => one(11),
                _ => one(8),
            }
        }
        _ => {
            let op = first;
            let t = match op {
                0x00 | 0x07 | 0x0F | 0x17 | 0x1F | 0x27 | 0x2F | 0x37 | 0x3F | 0x08 | 0xD9 | 0xEB | 0xF3 | 0xFB | 0xE9 => 4,
                0x76 => 4,
                0x40..=0x7F => {
                    if op & 7 == 6 || (op >> 3) & 7 == 6 {
                        7
                    } else {
                        4
                    }
                }
                0x80..=0xBF => {
                    if op & 7 == 6 {
                        7
                    } else {
                        4
                    }
                }
                0x01 | 0x11 | 0x21 | 0x31 => 10,
                0x02 | 0x12 | 0x0A | 0x1A => 7,
                0x03 | 0x13 | 0x23 | 0x33 | 0x0B | 0x1B | 0x2B | 0x3B => 6,
                0x34 | 0x35 => 11,
                0x36 => 10,
                0x09 | 0x19 | 0x29 | 0x39 => 11,
                0x22 | 0x2A => 16,
                0x32 | 0x3A => 13,
                0x10 => return two(8, 13),
                0x18 => 12,
                0x20 | 0x28 | 0x30 | 0x38 => return two(7, 12),
                0xC3 => 10,
                0xCD => 17,
                0xC9 => 10,
                0xC0 | 0xC8 | 0xD0 | 0xD8 | 0xE0 | 0xE8 | 0xF0 | 0xF8 => return two(5, 11),
                0xC2 | 0xCA | 0xD2 | 0xDA | 0xE2 | 0xEA | 0xF2 | 0xFA => 10,
                0xC4 | 0xCC | 0xD4 | 0xDC | 0xE4 | 0xEC | 0xF4 | 0xFC => return two(10, 17),
                0xC5 | 0xD5 | 0xE5 | 0xF5 => 11,
                0xC1 | 0xD1 | 0xE1 | 0xF1 => 10,
                0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => 11,
                0xE3 => 19,
                0xF9 => 6,
                0xDB | 0xD3 => 11,
                0xC6 | 0xCE | 0xD6 | 0xDE | 0xE6 | 0xEE | 0xF6 | 0xFE => 7,
                op if op & 0xC7 == 0x04 || op & 0xC7 == 0x05 => 4,
                op if op & 0xC7 == 0x06 => 7,
                _ => 4,
            };
            one(t)
        }
    }
}
