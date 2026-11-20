//! The Sony SPC700, the processor in the SNES's sound unit (the S-SMP), which runs the music from its own 64K: a
//! 6502-like chip with its own instructions and Sony's way of writing them, destination first. MOV A,#$12;
//! MOV A,[$12+X] and MOV A,[$12]+Y for the indirect forms; ! before an address to make it absolute (MOV A,!$1234);
//! $12.3 for bit 3 of a byte in the direct page, and $1234.5 for a bit anywhere in the first 8K (MOV1, AND1 and the
//! rest). Each opcode's form is listed once, and both assembling and disassembling use the list.

use super::{Isa, Line, split_operands};
use crate::disasm::Decoded;

pub struct Spc700;

/// An operand's form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    /// None.
    Imp,
    A,
    X,
    Y,
    Ya,
    Sp,
    Psw,
    C,
    /// (X), (X)+ and (Y): the direct page at X or Y.
    XInd,
    XInc,
    YInd,
    Imm,
    Dp,
    DpX,
    DpY,
    Abs,
    AbsX,
    AbsY,
    /// [dp+X], [dp]+Y, and JMP [!abs+X].
    DpXInd,
    DpIndY,
    AbsXInd,
    /// A bit of a byte in the direct page: dp.bit, the bit part of the opcode.
    DpBit(u8),
    /// A bit anywhere in the first 8K, and the same inverted: mem.bit and /mem.bit.
    MemBit,
    NotMemBit,
    Rel,
    /// PCALL's byte, a call to $FF00 and that.
    Upage,
    /// TCALL's number, part of the opcode.
    Tcall(u8),
}

use Op::*;

/// Each opcode, $00 to $FF: its instruction and its two operands, as MAME's disassembler lists them (with NOT1, DAA,
/// CMPW, ADDW and SUBW put right, as Sony's manual has them).
const OPCODES: [(&str, [Op; 2]); 256] = [
    ("nop", [Imp, Imp]),
    ("tcall", [Tcall(0), Imp]),
    ("set1", [DpBit(0), Imp]),
    ("bbs", [DpBit(0), Rel]),
    ("or", [A, Dp]),
    ("or", [A, Abs]),
    ("or", [A, XInd]),
    ("or", [A, DpXInd]),
    ("or", [A, Imm]),
    ("or", [Dp, Dp]),
    ("or1", [C, MemBit]),
    ("asl", [Dp, Imp]),
    ("asl", [Abs, Imp]),
    ("push", [Psw, Imp]),
    ("tset1", [Abs, Imp]),
    ("brk", [Imp, Imp]),
    ("bpl", [Rel, Imp]),
    ("tcall", [Tcall(1), Imp]),
    ("clr1", [DpBit(0), Imp]),
    ("bbc", [DpBit(0), Rel]),
    ("or", [A, DpX]),
    ("or", [A, AbsX]),
    ("or", [A, AbsY]),
    ("or", [A, DpIndY]),
    ("or", [Dp, Imm]),
    ("or", [XInd, YInd]),
    ("decw", [Dp, Imp]),
    ("asl", [DpX, Imp]),
    ("asl", [A, Imp]),
    ("dec", [X, Imp]),
    ("cmp", [X, Abs]),
    ("jmp", [AbsXInd, Imp]),
    ("clrp", [Imp, Imp]),
    ("tcall", [Tcall(2), Imp]),
    ("set1", [DpBit(1), Imp]),
    ("bbs", [DpBit(1), Rel]),
    ("and", [A, Dp]),
    ("and", [A, Abs]),
    ("and", [A, XInd]),
    ("and", [A, DpXInd]),
    ("and", [A, Imm]),
    ("and", [Dp, Dp]),
    ("or1", [C, NotMemBit]),
    ("rol", [Dp, Imp]),
    ("rol", [Abs, Imp]),
    ("push", [A, Imp]),
    ("cbne", [Dp, Rel]),
    ("bra", [Rel, Imp]),
    ("bmi", [Rel, Imp]),
    ("tcall", [Tcall(3), Imp]),
    ("clr1", [DpBit(1), Imp]),
    ("bbc", [DpBit(1), Rel]),
    ("and", [A, DpX]),
    ("and", [A, AbsX]),
    ("and", [A, AbsY]),
    ("and", [A, DpIndY]),
    ("and", [Dp, Imm]),
    ("and", [XInd, YInd]),
    ("incw", [Dp, Imp]),
    ("rol", [DpX, Imp]),
    ("rol", [A, Imp]),
    ("inc", [X, Imp]),
    ("cmp", [X, Dp]),
    ("call", [Abs, Imp]),
    ("setp", [Imp, Imp]),
    ("tcall", [Tcall(4), Imp]),
    ("set1", [DpBit(2), Imp]),
    ("bbs", [DpBit(2), Rel]),
    ("eor", [A, Dp]),
    ("eor", [A, Abs]),
    ("eor", [A, XInd]),
    ("eor", [A, DpXInd]),
    ("eor", [A, Imm]),
    ("eor", [Dp, Dp]),
    ("and1", [C, MemBit]),
    ("lsr", [Dp, Imp]),
    ("lsr", [Abs, Imp]),
    ("push", [X, Imp]),
    ("tclr1", [Abs, Imp]),
    ("pcall", [Upage, Imp]),
    ("bvc", [Rel, Imp]),
    ("tcall", [Tcall(5), Imp]),
    ("clr1", [DpBit(2), Imp]),
    ("bbc", [DpBit(2), Rel]),
    ("eor", [A, DpX]),
    ("eor", [A, AbsX]),
    ("eor", [A, AbsY]),
    ("eor", [A, DpIndY]),
    ("eor", [Dp, Imm]),
    ("eor", [XInd, YInd]),
    ("cmpw", [Ya, Dp]),
    ("lsr", [DpX, Imp]),
    ("lsr", [A, Imp]),
    ("mov", [X, A]),
    ("cmp", [Y, Abs]),
    ("jmp", [Abs, Imp]),
    ("clrc", [Imp, Imp]),
    ("tcall", [Tcall(6), Imp]),
    ("set1", [DpBit(3), Imp]),
    ("bbs", [DpBit(3), Rel]),
    ("cmp", [A, Dp]),
    ("cmp", [A, Abs]),
    ("cmp", [A, XInd]),
    ("cmp", [A, DpXInd]),
    ("cmp", [A, Imm]),
    ("cmp", [Dp, Dp]),
    ("and1", [C, NotMemBit]),
    ("ror", [Dp, Imp]),
    ("ror", [Abs, Imp]),
    ("push", [Y, Imp]),
    ("dbnz", [Dp, Rel]),
    ("ret", [Imp, Imp]),
    ("bvs", [Rel, Imp]),
    ("tcall", [Tcall(7), Imp]),
    ("clr1", [DpBit(3), Imp]),
    ("bbc", [DpBit(3), Rel]),
    ("cmp", [A, DpX]),
    ("cmp", [A, AbsX]),
    ("cmp", [A, AbsY]),
    ("cmp", [A, DpIndY]),
    ("cmp", [Dp, Imm]),
    ("cmp", [XInd, YInd]),
    ("addw", [Ya, Dp]),
    ("ror", [DpX, Imp]),
    ("ror", [A, Imp]),
    ("mov", [A, X]),
    ("cmp", [Y, Dp]),
    ("reti", [Imp, Imp]),
    ("setc", [Imp, Imp]),
    ("tcall", [Tcall(8), Imp]),
    ("set1", [DpBit(4), Imp]),
    ("bbs", [DpBit(4), Rel]),
    ("adc", [A, Dp]),
    ("adc", [A, Abs]),
    ("adc", [A, XInd]),
    ("adc", [A, DpXInd]),
    ("adc", [A, Imm]),
    ("adc", [Dp, Dp]),
    ("eor1", [C, MemBit]),
    ("dec", [Dp, Imp]),
    ("dec", [Abs, Imp]),
    ("mov", [Y, Imm]),
    ("pop", [Psw, Imp]),
    ("mov", [Dp, Imm]),
    ("bcc", [Rel, Imp]),
    ("tcall", [Tcall(9), Imp]),
    ("clr1", [DpBit(4), Imp]),
    ("bbc", [DpBit(4), Rel]),
    ("adc", [A, DpX]),
    ("adc", [A, AbsX]),
    ("adc", [A, AbsY]),
    ("adc", [A, DpIndY]),
    ("adc", [Dp, Imm]),
    ("adc", [XInd, YInd]),
    ("subw", [Ya, Dp]),
    ("dec", [DpX, Imp]),
    ("dec", [A, Imp]),
    ("mov", [X, Sp]),
    ("div", [Ya, X]),
    ("xcn", [A, Imp]),
    ("ei", [Imp, Imp]),
    ("tcall", [Tcall(10), Imp]),
    ("set1", [DpBit(5), Imp]),
    ("bbs", [DpBit(5), Rel]),
    ("sbc", [A, Dp]),
    ("sbc", [A, Abs]),
    ("sbc", [A, XInd]),
    ("sbc", [A, DpXInd]),
    ("sbc", [A, Imm]),
    ("sbc", [Dp, Dp]),
    ("mov1", [C, MemBit]),
    ("inc", [Dp, Imp]),
    ("inc", [Abs, Imp]),
    ("cmp", [Y, Imm]),
    ("pop", [A, Imp]),
    ("mov", [XInc, A]),
    ("bcs", [Rel, Imp]),
    ("tcall", [Tcall(11), Imp]),
    ("clr1", [DpBit(5), Imp]),
    ("bbc", [DpBit(5), Rel]),
    ("sbc", [A, DpX]),
    ("sbc", [A, AbsX]),
    ("sbc", [A, AbsY]),
    ("sbc", [A, DpIndY]),
    ("sbc", [Dp, Imm]),
    ("sbc", [XInd, YInd]),
    ("movw", [Ya, Dp]),
    ("inc", [DpX, Imp]),
    ("inc", [A, Imp]),
    ("mov", [Sp, X]),
    ("das", [A, Imp]),
    ("mov", [A, XInc]),
    ("di", [Imp, Imp]),
    ("tcall", [Tcall(12), Imp]),
    ("set1", [DpBit(6), Imp]),
    ("bbs", [DpBit(6), Rel]),
    ("mov", [Dp, A]),
    ("mov", [Abs, A]),
    ("mov", [XInd, A]),
    ("mov", [DpXInd, A]),
    ("cmp", [X, Imm]),
    ("mov", [Abs, X]),
    ("mov1", [MemBit, C]),
    ("mov", [Dp, Y]),
    ("mov", [Abs, Y]),
    ("mov", [X, Imm]),
    ("pop", [X, Imp]),
    ("mul", [Ya, Imp]),
    ("bne", [Rel, Imp]),
    ("tcall", [Tcall(13), Imp]),
    ("clr1", [DpBit(6), Imp]),
    ("bbc", [DpBit(6), Rel]),
    ("mov", [DpX, A]),
    ("mov", [AbsX, A]),
    ("mov", [AbsY, A]),
    ("mov", [DpIndY, A]),
    ("mov", [Dp, X]),
    ("mov", [DpY, X]),
    ("movw", [Dp, Ya]),
    ("mov", [DpX, Y]),
    ("dec", [Y, Imp]),
    ("mov", [A, Y]),
    ("cbne", [DpX, Rel]),
    ("daa", [A, Imp]),
    ("clrv", [Imp, Imp]),
    ("tcall", [Tcall(14), Imp]),
    ("set1", [DpBit(7), Imp]),
    ("bbs", [DpBit(7), Rel]),
    ("mov", [A, Dp]),
    ("mov", [A, Abs]),
    ("mov", [A, XInd]),
    ("mov", [A, DpXInd]),
    ("mov", [A, Imm]),
    ("mov", [X, Abs]),
    ("not1", [MemBit, Imp]),
    ("mov", [Y, Dp]),
    ("mov", [Y, Abs]),
    ("notc", [Imp, Imp]),
    ("pop", [Y, Imp]),
    ("sleep", [Imp, Imp]),
    ("beq", [Rel, Imp]),
    ("tcall", [Tcall(15), Imp]),
    ("clr1", [DpBit(7), Imp]),
    ("bbc", [DpBit(7), Rel]),
    ("mov", [A, DpX]),
    ("mov", [A, AbsX]),
    ("mov", [A, AbsY]),
    ("mov", [A, DpIndY]),
    ("mov", [X, Dp]),
    ("mov", [X, DpY]),
    ("mov", [Dp, Dp]),
    ("mov", [Y, DpX]),
    ("inc", [Y, Imp]),
    ("mov", [Y, A]),
    ("dbnz", [Y, Rel]),
    ("stop", [Imp, Imp]),
];

/// How many bytes an operand takes after the opcode.
fn size(op: Op) -> usize {
    match op {
        Imm | Dp | DpX | DpY | DpXInd | DpIndY | DpBit(_) | Rel | Upage => 1,
        Abs | AbsX | AbsY | AbsXInd | MemBit | NotMemBit => 2,
        _ => 0,
    }
}

/// What's written for an operand that's a word of its own.
fn word_of(op: Op) -> Option<&'static str> {
    Some(match op {
        A => "a",
        X => "x",
        Y => "y",
        Ya => "ya",
        Sp => "sp",
        Psw => "psw",
        C => "c",
        XInd => "(x)",
        XInc => "(x)+",
        YInd => "(y)",
        _ => return None,
    })
}

/// An operand as written.
#[derive(Clone, Debug, PartialEq)]
enum Written {
    Word(&'static str),
    Imm(String),
    /// An address, whether ! forced it to be absolute, and its index.
    Address(String, bool, Option<char>),
    DpXInd(String),
    DpIndY(String),
    AbsXInd(String),
    /// address.bit, and whether / came before it.
    Bit(String, String, bool),
}

fn parse(text: &str) -> Written {
    let text = text.trim();
    let squeezed: String = text.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    for word in ["a", "x", "y", "ya", "sp", "psw", "c", "(x)", "(x)+", "(y)"] {
        if squeezed == word {
            return Written::Word(word);
        }
    }
    if let Some(value) = text.strip_prefix('#') {
        return Written::Imm(value.trim().to_string());
    }
    // [dp+X], [dp]+Y and [!abs+X]; MAME writes them in parentheses.
    let bracketed = squeezed.starts_with(['[', '(']);
    if bracketed {
        let inner = &text[1..];
        let close = inner.rfind([']', ')']).unwrap_or(inner.len());
        let (inside, after) = (inner[..close].trim(), inner[close + 1..].trim().to_ascii_lowercase().replace(' ', ""));
        if after == "+y" {
            return Written::DpIndY(inside.to_string());
        }
        if after.is_empty()
            && let Some(address) = inside.to_ascii_lowercase().replace(' ', "").strip_suffix("+x").map(|_| inside[..inside.rfind('+').unwrap_or(0)].trim())
        {
            return match address.strip_prefix('!') {
                Some(abs) => Written::AbsXInd(abs.trim().to_string()),
                None => Written::DpXInd(address.to_string()),
            };
        }
    }
    let (inverted, rest) = match text.strip_prefix('/') {
        Some(rest) => (true, rest.trim()),
        None => (false, text),
    };
    // address.bit: the last dot followed by a digit or a name, not inside a number.
    if let Some(dot) = rest.rfind('.')
        && dot > 0
        && !rest[dot + 1..].is_empty()
        && !rest[dot + 1..].contains(['+', ')', ']'])
    {
        let address = rest[..dot].trim().trim_start_matches('!');
        return Written::Bit(address.to_string(), rest[dot + 1..].trim().to_string(), inverted);
    }
    let lower = rest.to_ascii_lowercase().replace(' ', "");
    let (body, index) = match lower.rsplit_once('+') {
        Some((_, "x")) => (&rest[..rest.rfind('+').unwrap_or(0)], Some('x')),
        Some((_, "y")) => (&rest[..rest.rfind('+').unwrap_or(0)], Some('y')),
        _ => (rest, None),
    };
    let body = body.trim();
    match body.strip_prefix('!') {
        Some(abs) => Written::Address(abs.trim().to_string(), true, index),
        None => Written::Address(body.to_string(), false, index),
    }
}

/// Whether a written operand can be an operand of this form, and if a direct page form only when it fits a byte.
fn fits(op: Op, written: &Written, small: bool) -> bool {
    match (op, written) {
        (_, Written::Word(w)) => word_of(op) == Some(*w),
        (Imm | Upage, Written::Imm(_)) => op == Imm,
        (Upage | Tcall(_), Written::Address(_, false, None)) => true,
        (Dp, Written::Address(_, forced, None)) => small && !forced,
        (DpX, Written::Address(_, forced, Some('x'))) => small && !forced,
        (DpY, Written::Address(_, forced, Some('y'))) => small && !forced,
        // An address that fits the direct page goes there, unless ! says otherwise.
        (Abs, Written::Address(_, forced, None)) => *forced || !small,
        (AbsX, Written::Address(_, forced, Some('x'))) => *forced || !small,
        (AbsY, Written::Address(_, forced, Some('y'))) => *forced || !small,
        (Rel, Written::Address(_, false, None)) => true,
        (DpXInd, Written::DpXInd(_)) | (DpIndY, Written::DpIndY(_)) | (AbsXInd, Written::AbsXInd(_)) => true,
        (DpBit(_), Written::Bit(_, _, false)) => true,
        (MemBit, Written::Bit(_, _, false)) | (NotMemBit, Written::Bit(_, _, true)) => true,
        _ => false,
    }
}

impl Isa for Spc700 {
    fn name(&self) -> &'static str {
        "SPC700"
    }

    fn is_instruction(&self, word: &str) -> bool {
        OPCODES.iter().any(|(n, _)| *n == word)
    }

    fn local_prefixes(&self) -> &'static [char] {
        &['.', '@', '_']
    }

    fn encode(&self, mnemonic: &str, operands: &str, line: &mut Line) -> Result<Vec<u8>, String> {
        let upper = mnemonic.to_ascii_uppercase();
        let written: Vec<Written> = split_operands(operands).iter().map(|o| parse(o)).collect();
        if !OPCODES.iter().any(|(n, _)| *n == mnemonic) {
            return Err(crate::i18n::tf!("{mnemonic} isn't an SPC700 instruction"));
        }
        // Whether each address is known to fit the direct page, a byte.
        let mut small = Vec::new();
        let mut numbers = Vec::new();
        for w in &written {
            let (text, bit) = match w {
                Written::Address(t, ..) | Written::DpXInd(t) | Written::DpIndY(t) => (Some(t.clone()), None),
                Written::Bit(t, b, _) => (Some(t.clone()), Some(b.clone())),
                _ => (None, None),
            };
            small.push(text.as_deref().is_some_and(|t| line.known(t).is_some_and(|v| (0..256).contains(&v))));
            numbers.push((text.as_deref().and_then(|t| line.known(t)), bit.as_deref().and_then(|b| line.known(b))));
        }
        let candidates = OPCODES.iter().enumerate().filter(|(_, (n, _))| *n == mnemonic);
        let mut chosen = None;
        for (opcode, (_, ops)) in candidates {
            let count = ops.iter().filter(|o| **o != Imp).count();
            if count != written.len() {
                continue;
            }
            let ok = ops.iter().take(count).zip(&written).enumerate().all(|(i, (op, w))| {
                match (op, w) {
                    // The bit, or TCALL's number, is part of the opcode.
                    (DpBit(b), Written::Bit(..)) => small[i] && numbers[i].1 == Some(i64::from(*b)),
                    (Tcall(n), Written::Address(..)) => numbers[i].0 == Some(i64::from(*n)),
                    _ => fits(*op, w, small[i]),
                }
            });
            if ok {
                chosen = Some((opcode as u8, ops));
                break;
            }
        }
        // An address that isn't known to fit a byte yet, or a direct page form only: try again allowing the direct page.
        if chosen.is_none() {
            for (opcode, (_, ops)) in OPCODES.iter().enumerate().filter(|(_, (n, _))| *n == mnemonic) {
                let count = ops.iter().filter(|o| **o != Imp).count();
                if count == written.len() && ops.iter().take(count).zip(&written).all(|(op, w)| fits(*op, w, true) && !matches!(op, DpBit(_) | Tcall(_))) {
                    chosen = Some((opcode as u8, ops));
                    break;
                }
            }
        }
        let Some((opcode, ops)) = chosen else {
            return Err(crate::i18n::tf!("{upper} can't take {}", operands.trim()));
        };
        let length = 1 + ops.iter().map(|o| size(*o)).sum::<usize>();
        let mut parts: Vec<Vec<u8>> = Vec::new();
        for (op, w) in ops.iter().zip(&written) {
            let text = match w {
                Written::Imm(t) | Written::Address(t, ..) | Written::DpXInd(t) | Written::DpIndY(t) | Written::AbsXInd(t) => t.clone(),
                Written::Bit(t, ..) => t.clone(),
                Written::Word(_) => continue,
            };
            let part = match op {
                Imm | Dp | DpX | DpY | DpXInd | DpIndY | DpBit(_) | Upage => {
                    let v = line.eval(&text)?;
                    let v = if *op == Upage && (0xFF00..=0xFFFF).contains(&v) { v - 0xFF00 } else { v };
                    if line.checking() && !(-128..=255).contains(&v) {
                        return Err(crate::i18n::tf!("{text} is {v}, which doesn't fit in a byte"));
                    }
                    vec![v as u8]
                }
                Abs | AbsX | AbsY | AbsXInd => {
                    let v = line.eval(&text)?;
                    if line.checking() && !(-32768..=65535).contains(&v) {
                        return Err(crate::i18n::tf!("{text} is {v}, which doesn't fit in 16 bits"));
                    }
                    (v as u16).to_le_bytes().to_vec()
                }
                MemBit | NotMemBit => {
                    let Written::Bit(_, bit, _) = w else { continue };
                    let (address, bit) = (line.eval(&text)?, line.eval(bit)?);
                    if line.checking() && (!(0..0x2000).contains(&address) || !(0..8).contains(&bit)) {
                        return Err(crate::i18n::tf!("{upper}'s bit is in the first 8K ($0000 to $1FFF), bits 0 to 7"));
                    }
                    (((bit as u16) << 13) | address as u16).to_le_bytes().to_vec()
                }
                Rel => {
                    let target = line.eval(&text)?;
                    let offset = target - (line.here() + length as i64);
                    if line.checking() && !(-128..=127).contains(&offset) {
                        return Err(crate::i18n::tf!("{text} is {offset} bytes away, out of a branch's reach (-128 to 127)"));
                    }
                    vec![offset as u8]
                }
                _ => continue,
            };
            parts.push(part);
        }
        // MOV dp,dp and the like, and dp,#value: the source (or value) comes first in the bytes.
        if matches!(ops, [Dp, Dp] | [Dp, Imm]) {
            parts.reverse();
        }
        let mut bytes = vec![opcode];
        bytes.extend(parts.into_iter().flatten());
        Ok(bytes)
    }
}

/// An instruction at `pc`, for disassembling, written in Sony's way.
pub fn decode(at: &dyn Fn(usize) -> u8, pc: u16) -> Decoded {
    let opcode = at(0);
    let (name, ops) = OPCODES[usize::from(opcode)];
    let length = 1 + ops.iter().map(|o| size(*o)).sum::<usize>();
    let mut at_byte = 1;
    let mut texts = Vec::new();
    let mut address = None;
    let mut target = None;
    // The two direct page bytes of MOV dp,dp (and dp,#value) are the other way round.
    let swapped = matches!(ops, [Dp, Dp] | [Dp, Imm]);
    for (index, op) in ops.iter().enumerate() {
        let from = if swapped { if index == 0 { 2 } else { 1 } } else { at_byte };
        let b = at(from);
        let w = u16::from_le_bytes([at(from), at(from + 1)]);
        let text = match op {
            Imp => continue,
            Imm => format!("#${b:02X}"),
            Dp => format!("${b:02X}"),
            DpX => format!("${b:02X}+x"),
            DpY => format!("${b:02X}+y"),
            DpXInd => format!("[${b:02X}+x]"),
            DpIndY => format!("[${b:02X}]+y"),
            DpBit(n) => format!("${b:02X}.{n}"),
            Upage => format!("${b:02X}"),
            Tcall(n) => n.to_string(),
            Abs | AbsX | AbsY | AbsXInd => {
                address = Some(w);
                if matches!(name, "call" | "jmp") && *op == Abs {
                    target = Some(w);
                }
                match op {
                    Abs => "!{addr}".to_string(),
                    AbsX => "!{addr}+x".to_string(),
                    AbsY => "!{addr}+y".to_string(),
                    _ => "[!{addr}+x]".to_string(),
                }
            }
            MemBit | NotMemBit => format!("{}${:04X}.{}", if *op == NotMemBit { "/" } else { "" }, w & 0x1FFF, w >> 13),
            Rel => {
                let t = pc.wrapping_add(length as u16).wrapping_add(b as i8 as u16);
                address = Some(t);
                target = Some(t);
                "{addr}".to_string()
            }
            other => word_of(*other).unwrap_or("?").to_string(),
        };
        at_byte += size(*op);
        texts.push(text);
    }
    let stops = matches!(name, "bra" | "jmp" | "ret" | "reti" | "brk" | "stop" | "sleep");
    let text = if texts.is_empty() { name.to_string() } else { format!("{name} {}", texts.join(",")) };
    Decoded { length, text, address, falls_through: !stops, target }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const MAME: &str = include_str!("testdata/spc700_mame.txt");
    const ARES: &str = include_str!("testdata/spc700_ares.txt");

    fn mame_name(op: Op) -> String {
        match op {
            Imp => "IMP".into(),
            A => "A".into(),
            X => "X".into(),
            Y => "Y".into(),
            Ya => "YA".into(),
            Sp => "SP".into(),
            Psw => "PSW".into(),
            C => "C".into(),
            XInd => "XI".into(),
            XInc => "XII".into(),
            YInd => "YI".into(),
            Imm => "IMM".into(),
            Dp => "DP".into(),
            DpX => "DPX".into(),
            DpY => "DPY".into(),
            Abs => "ABS".into(),
            AbsX => "ABX".into(),
            AbsY => "ABY".into(),
            DpXInd => "DXI".into(),
            DpIndY => "DIY".into(),
            AbsXInd => "AXI".into(),
            DpBit(n) => format!("DP{n}"),
            MemBit => "MEMN".into(),
            NotMemBit => "MEMI".into(),
            Rel => "REL".into(),
            Upage => "UPAG".into(),
            Tcall(n) => format!("N{n}"),
        }
    }

    #[test]
    fn every_opcode_is_as_mame_and_ares_have_it_and_goes_both_ways() {
        let mame: Vec<&str> = MAME.lines().filter(|l| !l.starts_with('#')).collect();
        let ares: Vec<&str> = ARES.lines().filter(|l| !l.starts_with('#')).collect();
        assert_eq!((mame.len(), ares.len()), (256, 256));
        for (opcode, (name, ops)) in OPCODES.iter().enumerate() {
            let words: Vec<&str> = mame[opcode].split(' ').collect();
            let theirs = format!("{} {} {}", words[0], words[1], words[2]);
            let ours = format!("{name} {} {}", mame_name(ops[0]), mame_name(ops[1]));
            match opcode {
                0xEA => assert_eq!((theirs.as_str(), ours.as_str()), ("not1 IMP IMP", "not1 MEMN IMP")),
                0xDF => assert_eq!((theirs.as_str(), ours.as_str()), ("daa IMP IMP", "daa A IMP")),
                // MAME leaves out YA, which Sony writes: CMPW YA,dp.
                0x5A | 0x7A | 0x9A => assert_eq!(theirs, format!("{name} DP IMP")),
                _ => assert_eq!(theirs, ours, "{opcode:02X}"),
            }
            let length = 1 + ops.iter().map(|o| size(*o)).sum::<usize>();
            let ares_length: usize = ares[opcode].split(' ').next().unwrap().parse().unwrap();
            // ares writes CMPW, ADDW and SUBW with a word, but they take a byte of the direct page, as MAME, Sony
            // and their own MOVW have it.
            let ares_length = if matches!(opcode, 0x5A | 0x7A | 0x9A) { ares_length - 1 } else { ares_length };
            assert_eq!(length, ares_length, "{opcode:02X}: {}", ares[opcode]);
            // Some bytes after it, disassembled, then assembled again at the same place.
            let bytes: Vec<u8> = [opcode as u8, 0x12, 0x34].into_iter().take(length).collect();
            let decoded = decode(&|i| bytes.get(i).copied().unwrap_or(0), 0x0400);
            assert_eq!(decoded.length, length);
            let text = decoded.text.replace("{addr}", &decoded.address.map(|a| format!("${a:04X}")).unwrap_or_default());
            let source = format!("  org $0400\n  {text}\n");
            let assembly = crate::asm::assemble(&Spc700, Path::new("s.asm"), &source, &|_| Err("no".into()));
            assert!(assembly.ok(), "{opcode:02X} {text}: {:?}", assembly.problems);
            assert_eq!(assembly.image(0x400, 0x400 + length as i64), bytes, "{opcode:02X} {text}");
        }
    }

    #[test]
    fn programs_assemble_as_sony_writes_them() {
        let source = "  org $0200\nstart:\n  mov a,#$12\n  mov $f2,#$6c\n  mov $10,$11\n  mov a,!$1234+x\n  mov a,[$20]+y\n  and1 c,/$0123.4\n  bbs $30.2,start\n  cbne $40,start\n  dbnz y,start\n  tcall 3\n  pcall $ffc0\n  jmp [!table+x]\n  movw ya,$14\n  set1 $50.7\ntable:\n";
        let assembly = crate::asm::assemble(&Spc700, Path::new("s.asm"), source, &|_| Err("no".into()));
        assert!(assembly.ok(), "{:?}", assembly.problems);
        let image = assembly.image(0x200, assembly.highest().unwrap());
        assert_eq!(
            image,
            [
                0xE8, 0x12, // MOV A,#$12
                0x8F, 0x6C, 0xF2, // MOV dp,#value: the value first
                0xFA, 0x11, 0x10, // MOV dp,dp: the source first
                0xF5, 0x34, 0x12, // MOV A,!abs+X
                0xF7, 0x20, // MOV A,[dp]+Y
                0x6A, 0x23, 0x81, // AND1 C,/mem.bit: the bit in the top three bits
                0x43, 0x30, 0xED, // BBS dp.bit,rel
                0x2E, 0x40, 0xEA, // CBNE dp,rel
                0xFE, 0xE8, // DBNZ Y,rel
                0x31, // TCALL 3
                0x4F, 0xC0, // PCALL to $FFC0
                0x1F, 0x22, 0x02, // JMP [!table+X]
                0xBA, 0x14, // MOVW YA,dp
                0xE2, 0x50, // SET1 dp.7
            ]
        );
        // MOV1 C,/mem.bit isn't one: only AND1 and OR1 take an inverted bit.
        let wrong = crate::asm::assemble(&Spc700, Path::new("s.asm"), "  mov1 c,/$0123.4\n", &|_| Err("no".into()));
        assert!(wrong.problems.iter().any(|p| p.message.contains("MOV1 can't take")), "{:?}", wrong.problems);
        assert_eq!(assembly.symbols["table"], 0x222);
    }
}
