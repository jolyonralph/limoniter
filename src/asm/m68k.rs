//! The Motorola 68000, as in the Amiga, the Atari ST, the early Macintosh and the Sega Mega Drive: its
//! instructions and addressing modes as vasm writes them in Motorola syntax, nothing made shorter or quicker
//! behind the programmer's back (as vasm does with -no-opt).

use super::{Isa, Line, split_operands};

pub struct M68k;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Size {
    Byte,
    Word,
    Long,
}

impl Size {
    /// The usual size field: byte 0, word 1, long 2.
    fn bits(self) -> u16 {
        match self {
            Size::Byte => 0,
            Size::Word => 1,
            Size::Long => 2,
        }
    }
}

/// An index register in (d8,An,Xn): its number, whether it's an address register, and whether all 32 bits count.
#[derive(Clone, Debug, PartialEq)]
struct Index {
    reg: u16,
    address: bool,
    long: bool,
}

/// An effective address.
#[derive(Clone, Debug, PartialEq)]
enum Ea {
    D(u16),
    A(u16),
    Indirect(u16),
    PostIncrement(u16),
    PreDecrement(u16),
    Displacement(u16, String),
    Indexed(u16, String, Index),
    AbsoluteWord(String),
    AbsoluteLong(String),
    PcDisplacement(String),
    PcIndexed(String, Index),
    Immediate(String),
    Sr,
    Ccr,
    Usp,
    /// A list of registers, for MOVEM: bit 0 is D0, bit 15 is A7.
    List(u16),
}

impl Ea {
    /// The six bits of mode and register.
    fn bits(&self) -> u16 {
        match self {
            Ea::D(r) => *r,
            Ea::A(r) => 0o10 | r,
            Ea::Indirect(r) => 0o20 | r,
            Ea::PostIncrement(r) => 0o30 | r,
            Ea::PreDecrement(r) => 0o40 | r,
            Ea::Displacement(r, _) => 0o50 | r,
            Ea::Indexed(r, ..) => 0o60 | r,
            Ea::AbsoluteWord(_) => 0o70,
            Ea::AbsoluteLong(_) => 0o71,
            Ea::PcDisplacement(_) => 0o72,
            Ea::PcIndexed(..) => 0o73,
            Ea::Immediate(_) => 0o74,
            _ => 0,
        }
    }

    fn is_memory(&self) -> bool {
        !matches!(self, Ea::D(_) | Ea::A(_) | Ea::Immediate(_) | Ea::Sr | Ea::Ccr | Ea::Usp | Ea::List(_))
    }
}

fn register(text: &str) -> Option<Ea> {
    let lower = text.trim().to_ascii_lowercase();
    if lower == "sp" {
        return Some(Ea::A(7));
    }
    let (kind, number) = lower.split_at(1.min(lower.len()));
    let number: u16 = number.parse().ok().filter(|n| *n < 8)?;
    match kind {
        "d" => Some(Ea::D(number)),
        "a" => Some(Ea::A(number)),
        _ => None,
    }
}

fn index_register(text: &str) -> Option<Index> {
    let lower = text.trim().to_ascii_lowercase();
    let (name, long) = match lower.rsplit_once('.') {
        Some((name, "l")) => (name.to_string(), true),
        Some((name, "w")) => (name.to_string(), false),
        _ => (lower.clone(), false),
    };
    match register(&name)? {
        Ea::D(reg) => Some(Index { reg, address: false, long }),
        Ea::A(reg) => Some(Index { reg, address: true, long }),
        _ => None,
    }
}

/// A list of registers: d0-d3/a0/a5-a6.
fn register_list(text: &str) -> Option<u16> {
    let mut mask = 0u16;
    for part in text.split('/') {
        let bit = |name: &str| -> Option<u16> {
            match register(name)? {
                Ea::D(r) => Some(r),
                Ea::A(r) => Some(8 + r),
                _ => None,
            }
        };
        match part.split_once('-') {
            Some((from, to)) => {
                let (from, to) = (bit(from)?, bit(to)?);
                for r in from.min(to)..=from.max(to) {
                    mask |= 1 << r;
                }
            }
            None => mask |= 1 << bit(part)?,
        }
    }
    Some(mask)
}

fn operand(text: &str) -> Result<Ea, String> {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    if let Some(value) = text.strip_prefix('#') {
        return Ok(Ea::Immediate(value.to_string()));
    }
    match lower.as_str() {
        "sr" => return Ok(Ea::Sr),
        "ccr" => return Ok(Ea::Ccr),
        "usp" => return Ok(Ea::Usp),
        _ => {}
    }
    if let Some(r) = register(text) {
        return Ok(r);
    }
    if (lower.contains('/') || lower.contains('-')) && !lower.contains('(')
        && let Some(mask) = register_list(&lower)
    {
        return Ok(Ea::List(mask));
    }
    if let Some(inner) = lower.strip_prefix("-(").and_then(|r| r.strip_suffix(')')) {
        return match register(inner) {
            Some(Ea::A(r)) => Ok(Ea::PreDecrement(r)),
            _ => Err(format!("-({inner}) needs an address register")),
        };
    }
    if let Some(inner) = lower.strip_prefix('(').and_then(|r| r.strip_suffix(")+")) {
        return match register(inner) {
            Some(Ea::A(r)) => Ok(Ea::PostIncrement(r)),
            _ => Err(format!("({inner})+ needs an address register")),
        };
    }
    // d(An), d(An,Xn), (d,An), (d,An,Xn), (An), and the same from PC.
    if lower.ends_with(')')
        && let Some(open) = matching_open(text)
    {
        let outside = text[..open].trim();
        let inside: Vec<String> = split_operands(&text[open + 1..text.len() - 1]);
        let (displacement, base, index) = match (outside.is_empty(), inside.as_slice()) {
            (true, [base]) => ("0".to_string(), base.clone(), None),
            (true, [d, base]) if register(d).is_none() && !d.eq_ignore_ascii_case("pc") => (d.clone(), base.clone(), None),
            (true, [base, x]) => ("0".to_string(), base.clone(), Some(x.clone())),
            (true, [d, base, x]) => (d.clone(), base.clone(), Some(x.clone())),
            (false, [base]) => (outside.to_string(), base.clone(), None),
            (false, [base, x]) => (outside.to_string(), base.clone(), Some(x.clone())),
            _ => return Err(format!("{text} isn't an address the 68000 has")),
        };
        let index = index.map(|x| index_register(&x).ok_or_else(|| format!("{x} isn't an index register"))).transpose()?;
        if base.eq_ignore_ascii_case("pc") {
            return Ok(match index {
                Some(x) => Ea::PcIndexed(displacement, x),
                None => Ea::PcDisplacement(displacement),
            });
        }
        let Some(Ea::A(r)) = register(&base) else {
            // (expr) on its own is just an address in brackets.
            if index.is_none() && outside.is_empty() && inside.len() == 1 {
                return Ok(Ea::AbsoluteLong(text.to_string()));
            }
            return Err(format!("{base} isn't an address register"));
        };
        return Ok(match index {
            Some(x) => Ea::Indexed(r, displacement, x),
            None if outside.is_empty() && inside.len() == 1 => Ea::Indirect(r),
            None => Ea::Displacement(r, displacement),
        });
    }
    // An address, long unless .w says otherwise.
    if let Some(address) = lower.strip_suffix(".w") {
        return Ok(Ea::AbsoluteWord(text[..address.len()].to_string()));
    }
    if let Some(address) = lower.strip_suffix(".l") {
        return Ok(Ea::AbsoluteLong(text[..address.len()].to_string()));
    }
    Ok(Ea::AbsoluteLong(text.to_string()))
}

/// Where the bracket that ends the text opens.
fn matching_open(text: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in text.char_indices().rev() {
        match c {
            ')' => depth += 1,
            '(' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// The extension words an address needs. `at` is where the first of them goes, for PC relative ones.
fn extension(ea: &Ea, size: Size, line: &mut Line, at: i64) -> Result<Vec<u16>, String> {
    let checking = line.checking();
    let brief = |x: &Index, d: i64| -> Result<u16, String> {
        if checking && !(-128..=127).contains(&d) {
            return Err(format!("the offset {d} doesn't fit in 8 bits (-128 to 127)"));
        }
        Ok((x.address as u16) << 15 | x.reg << 12 | (x.long as u16) << 11 | (d as u8 as u16))
    };
    let word = |v: i64, what: &str| -> Result<u16, String> {
        if checking && !(-32768..=65535).contains(&v) {
            return Err(format!("{what} {v} doesn't fit in 16 bits"));
        }
        Ok(v as u16)
    };
    Ok(match ea {
        Ea::Displacement(_, d) => vec![word(line.eval(d)?, "the offset")?],
        Ea::Indexed(_, d, x) => vec![brief(x, line.eval(d)?)?],
        Ea::AbsoluteWord(a) => vec![word(line.eval(a)?, "the address")?],
        Ea::AbsoluteLong(a) => {
            let v = line.eval(a)?;
            vec![(v >> 16) as u16, v as u16]
        }
        Ea::PcDisplacement(d) => vec![word(line.eval(d)? - at, "the offset from PC")?],
        Ea::PcIndexed(d, x) => vec![brief(x, line.eval(d)? - at)?],
        Ea::Immediate(v) => {
            let v = line.eval(v)?;
            match size {
                Size::Byte => vec![v as u8 as u16],
                Size::Word => vec![v as u16],
                Size::Long => vec![(v >> 16) as u16, v as u16],
            }
        }
        _ => Vec::new(),
    })
}

fn condition(name: &str) -> Option<u16> {
    Some(match name {
        "t" => 0,
        "f" => 1,
        "hi" => 2,
        "ls" => 3,
        "cc" | "hs" => 4,
        "cs" | "lo" => 5,
        "ne" => 6,
        "eq" => 7,
        "vc" => 8,
        "vs" => 9,
        "pl" => 10,
        "mi" => 11,
        "ge" => 12,
        "lt" => 13,
        "gt" => 14,
        "le" => 15,
        _ => return None,
    })
}

const PLAIN: [(&str, u16); 7] = [("rts", 0x4E75), ("rte", 0x4E73), ("rtr", 0x4E77), ("nop", 0x4E71), ("reset", 0x4E70), ("trapv", 0x4E76), ("illegal", 0x4AFC)];

const OTHERS: [&str; 64] = [
    "move", "movea", "moveq", "movem", "lea", "pea", "clr", "neg", "negx", "not", "tst", "add", "adda", "addi", "addq", "addx", "sub", "suba", "subi", "subq", "subx", "and", "andi", "or", "ori", "eor", "eori", "cmp", "cmpa", "cmpi", "cmpm", "mulu", "muls", "divu", "divs", "asl", "asr", "lsl", "lsr", "rol", "ror", "roxl", "roxr", "btst", "bchg", "bclr", "bset", "jmp", "jsr", "stop", "trap", "link", "unlk", "swap", "ext", "exg", "chk", "tas", "nbcd", "abcd", "sbcd", "dbra", "bsr", "bra",
];

/// The mnemonic without its size, and the size.
fn split_size(mnemonic: &str) -> (&str, Option<Size>) {
    match mnemonic.rsplit_once('.') {
        Some((name, "b")) => (name, Some(Size::Byte)),
        Some((name, "w")) => (name, Some(Size::Word)),
        Some((name, "l")) => (name, Some(Size::Long)),
        Some((name, "s")) => (name, Some(Size::Byte)),
        _ => (mnemonic, None),
    }
}

fn is_branch(name: &str) -> bool {
    name == "bra" || name == "bsr" || name.strip_prefix('b').and_then(condition).is_some_and(|c| c > 1)
}

fn is_dbcc(name: &str) -> bool {
    name == "dbra" || name.strip_prefix("db").and_then(condition).is_some()
}

fn is_scc(name: &str) -> bool {
    name.strip_prefix('s').and_then(condition).is_some()
}

impl Isa for M68k {
    fn name(&self) -> &'static str {
        "68000"
    }

    fn big_endian(&self) -> bool {
        true
    }

    fn operand_ends_at_space(&self) -> bool {
        true
    }

    fn is_instruction(&self, word: &str) -> bool {
        let (name, _) = split_size(word);
        PLAIN.iter().any(|(n, _)| *n == name) || OTHERS.contains(&name) || is_branch(name) || is_dbcc(name) || is_scc(name)
    }

    fn encode(&self, mnemonic: &str, operands: &str, line: &mut Line) -> Result<Vec<u8>, String> {
        let words = encode(mnemonic, operands, line)?;
        Ok(words.iter().flat_map(|w| w.to_be_bytes()).collect())
    }
}

fn encode(mnemonic: &str, operands: &str, line: &mut Line) -> Result<Vec<u16>, String> {
    let (name, size) = split_size(mnemonic);
    let upper = mnemonic.to_ascii_uppercase();
    let here = line.here();
    let texts = split_operands(operands);
    let ops: Vec<Ea> = texts.iter().map(|t| operand(t)).collect::<Result<_, _>>()?;
    let wrong = || Err(format!("{upper} {operands} isn't a 68000 instruction"));
    let sz = size.unwrap_or(Size::Word);
    // The opcode word, then each address's extension words, the source's first.
    let with = |op: u16, eas: &[(&Ea, Size)], line: &mut Line| -> Result<Vec<u16>, String> {
        let mut words = vec![op];
        for (ea, size) in eas {
            let at = here + 2 * words.len() as i64;
            words.extend(extension(ea, *size, line, at)?);
        }
        Ok(words)
    };
    if let Some((_, op)) = PLAIN.iter().find(|(n, _)| *n == name) {
        return Ok(vec![*op]);
    }
    if is_branch(name) {
        let target = line.eval(operands)?;
        let base: u16 = match name {
            "bra" => 0x6000,
            "bsr" => 0x6100,
            _ => 0x6000 | condition(&name[1..]).unwrap_or(0) << 8,
        };
        let offset = target - (here + 2);
        if mnemonic.ends_with(".s") || mnemonic.ends_with(".b") {
            if line.checking() && (!(-128..=127).contains(&offset) || offset == 0) {
                return Err(format!("{operands} is {offset} bytes away, out of a short branch's reach; use {}.W", name.to_ascii_uppercase()));
            }
            return Ok(vec![base | (offset as u8 as u16)]);
        }
        if line.checking() && !(-32768..=32767).contains(&offset) {
            return Err(format!("{operands} is too far away for a branch"));
        }
        return Ok(vec![base, offset as u16]);
    }
    if is_dbcc(name) {
        let cc = if name == "dbra" { 1 } else { condition(&name[2..]).unwrap_or(1) };
        let [Ea::D(r), _] = ops.as_slice() else { return wrong() };
        let offset = line.eval(&texts[1])? - (here + 2);
        return Ok(vec![0x50C8 | cc << 8 | r, offset as u16]);
    }
    if is_scc(name) {
        let cc = condition(&name[1..]).unwrap_or(0);
        let [ea] = ops.as_slice() else { return wrong() };
        return with(0x50C0 | cc << 8 | ea.bits(), &[(ea, Size::Byte)], line);
    }
    match (name, ops.as_slice()) {
        ("moveq", [Ea::Immediate(v), Ea::D(r)]) => {
            let v = line.eval(v)?;
            if line.checking() && !(-128..=255).contains(&v) {
                return Err(format!("MOVEQ takes -128 to 127, and {v} isn't"));
            }
            Ok(vec![0x7000 | r << 9 | (v as u8 as u16)])
        }
        ("move", [Ea::Sr, dst]) => with(0x40C0 | dst.bits(), &[(dst, Size::Word)], line),
        ("move", [src, Ea::Ccr]) => with(0x44C0 | src.bits(), &[(src, Size::Word)], line),
        ("move", [src, Ea::Sr]) => with(0x46C0 | src.bits(), &[(src, Size::Word)], line),
        ("move", [Ea::Usp, Ea::A(r)]) => Ok(vec![0x4E68 | r]),
        ("move", [Ea::A(r), Ea::Usp]) => Ok(vec![0x4E60 | r]),
        ("move" | "movea", [src, dst]) => {
            let code = match sz {
                Size::Byte => 1,
                Size::Long => 2,
                Size::Word => 3,
            };
            if matches!(dst, Ea::Immediate(_) | Ea::PcDisplacement(_) | Ea::PcIndexed(..)) {
                return Err(format!("{upper} can't write to {}", texts[1]));
            }
            let d = dst.bits();
            let op = code << 12 | (d & 7) << 9 | (d >> 3) << 6 | src.bits();
            with(op, &[(src, sz), (dst, sz)], line)
        }
        ("movem", [from, to]) => {
            // Registers to memory, or memory to registers; one register on its own is a list of one.
            let single = |ea: &Ea| match ea {
                Ea::List(mask) => Some(*mask),
                Ea::D(r) => Some(1 << r),
                Ea::A(r) => Some(1 << (8 + r)),
                _ => None,
            };
            let long = if sz == Size::Long { 0x40 } else { 0 };
            let (op, mask, ea) = match (single(from), single(to)) {
                (Some(mask), None) if to.is_memory() => {
                    // To -(An), the mask runs the other way.
                    let mask = if matches!(to, Ea::PreDecrement(_)) { mask.reverse_bits() } else { mask };
                    (0x4880 | long | to.bits(), mask, to)
                }
                (None, Some(mask)) if from.is_memory() => (0x4C80 | long | from.bits(), mask, from),
                _ => return wrong(),
            };
            let mut words = vec![op, mask];
            words.extend(extension(ea, sz, line, here + 4)?);
            Ok(words)
        }
        ("lea", [src, Ea::A(r)]) => with(0x41C0 | r << 9 | src.bits(), &[(src, Size::Long)], line),
        ("pea", [src]) => with(0x4840 | src.bits(), &[(src, Size::Long)], line),
        ("clr" | "neg" | "negx" | "not" | "tst", [ea]) => {
            let base = match name {
                "clr" => 0x4200,
                "neg" => 0x4400,
                "negx" => 0x4000,
                "not" => 0x4600,
                _ => 0x4A00,
            };
            with(base | sz.bits() << 6 | ea.bits(), &[(ea, sz)], line)
        }
        ("addq" | "subq", [Ea::Immediate(v), dst]) => {
            let v = line.eval(v)?;
            if line.checking() && !(1..=8).contains(&v) {
                return Err(format!("{upper} takes 1 to 8, and {v} isn't"));
            }
            let base = if name == "addq" { 0x5000 } else { 0x5100 };
            with(base | ((v as u16) & 7) << 9 | sz.bits() << 6 | dst.bits(), &[(dst, sz)], line)
        }
        ("addx" | "subx" | "abcd" | "sbcd", [src, dst]) => {
            let base: u16 = match name {
                "addx" => 0xD100 | sz.bits() << 6,
                "subx" => 0x9100 | sz.bits() << 6,
                "abcd" => 0xC100,
                _ => 0x8100,
            };
            match (src, dst) {
                (Ea::D(y), Ea::D(x)) => Ok(vec![base | x << 9 | y]),
                (Ea::PreDecrement(y), Ea::PreDecrement(x)) => Ok(vec![base | x << 9 | 8 | y]),
                _ => wrong(),
            }
        }
        ("cmpm", [Ea::PostIncrement(y), Ea::PostIncrement(x)]) => Ok(vec![0xB108 | x << 9 | sz.bits() << 6 | y]),
        ("mulu" | "muls" | "divu" | "divs" | "chk", [src, Ea::D(r)]) => {
            let base = match name {
                "mulu" => 0xC0C0,
                "muls" => 0xC1C0,
                "divu" => 0x80C0,
                "divs" => 0x81C0,
                _ => 0x4180,
            };
            with(base | r << 9 | src.bits(), &[(src, Size::Word)], line)
        }
        ("addi" | "subi" | "andi" | "ori" | "eori" | "cmpi", [Ea::Immediate(_), Ea::Ccr | Ea::Sr]) | ("and" | "or" | "eor", [Ea::Immediate(_), Ea::Ccr | Ea::Sr]) => {
            let to_sr = matches!(ops[1], Ea::Sr);
            let base = match name.trim_end_matches('i') {
                "and" => 0x023C,
                "or" => 0x003C,
                "eor" => 0x0A3C,
                _ => return wrong(),
            } | if to_sr { 0x40 } else { 0 };
            with(base, &[(&ops[0], if to_sr { Size::Word } else { Size::Byte })], line)
        }
        ("addi" | "subi" | "andi" | "ori" | "eori" | "cmpi", [src @ Ea::Immediate(_), dst]) => immediate(name, sz, src, dst, &with, line),
        ("adda" | "suba" | "cmpa", [src, Ea::A(r)]) | ("add" | "sub" | "cmp", [src, Ea::A(r)]) => {
            let base = match name.trim_end_matches('a') {
                "add" => 0xD0C0,
                "sub" => 0x90C0,
                _ => 0xB0C0,
            } | if sz == Size::Long { 0x100 } else { 0 };
            with(base | r << 9 | src.bits(), &[(src, sz)], line)
        }
        ("add" | "sub" | "and" | "or" | "cmp", [src, Ea::D(r)]) => {
            let base = arithmetic_base(name);
            with(base | r << 9 | sz.bits() << 6 | src.bits(), &[(src, sz)], line)
        }
        ("add" | "sub" | "and" | "or" | "eor", [Ea::D(r), dst]) if dst.is_memory() || name == "eor" => {
            let base = if name == "eor" { 0xB000 } else { arithmetic_base(name) };
            with(base | r << 9 | 0x100 | sz.bits() << 6 | dst.bits(), &[(dst, sz)], line)
        }
        ("add" | "sub" | "and" | "or" | "eor" | "cmp", [src @ Ea::Immediate(_), dst]) => immediate(&format!("{name}i"), sz, src, dst, &with, line),
        (shift @ ("asl" | "asr" | "lsl" | "lsr" | "rol" | "ror" | "roxl" | "roxr"), rest) => {
            let kind: u16 = match &shift[..shift.len() - 1] {
                "as" => 0,
                "ls" => 1,
                "rox" => 2,
                _ => 3,
            };
            let left = if shift.ends_with('l') { 0x100 } else { 0 };
            match rest {
                [Ea::Immediate(count), Ea::D(r)] => {
                    let n = line.eval(count)?;
                    if line.checking() && !(1..=8).contains(&n) {
                        return Err(format!("{upper} shifts 1 to 8 places at once, and {n} isn't"));
                    }
                    Ok(vec![0xE000 | ((n as u16) & 7) << 9 | left | sz.bits() << 6 | kind << 3 | r])
                }
                [Ea::D(count), Ea::D(r)] => Ok(vec![0xE000 | count << 9 | left | sz.bits() << 6 | 0x20 | kind << 3 | r]),
                [Ea::D(r)] => Ok(vec![0xE000 | 1 << 9 | left | sz.bits() << 6 | kind << 3 | r]),
                [ea] if ea.is_memory() => with(0xE0C0 | kind << 9 | left | ea.bits(), &[(ea, Size::Word)], line),
                _ => wrong(),
            }
        }
        (bit @ ("btst" | "bchg" | "bclr" | "bset"), [number, dst]) => {
            let kind: u16 = match bit {
                "btst" => 0,
                "bchg" => 1,
                "bclr" => 2,
                _ => 3,
            };
            match number {
                Ea::D(r) => with(0x0100 | r << 9 | kind << 6 | dst.bits(), &[(dst, Size::Byte)], line),
                Ea::Immediate(n) => {
                    let n = line.eval(n)?;
                    let mut words = vec![0x0800 | kind << 6 | dst.bits(), n as u16 & 0xFF];
                    words.extend(extension(dst, Size::Byte, line, here + 4)?);
                    Ok(words)
                }
                _ => wrong(),
            }
        }
        ("jmp", [ea]) => with(0x4EC0 | ea.bits(), &[(ea, Size::Long)], line),
        ("jsr", [ea]) => with(0x4E80 | ea.bits(), &[(ea, Size::Long)], line),
        ("stop", [Ea::Immediate(v)]) => Ok(vec![0x4E72, line.eval(v)? as u16]),
        ("trap", [Ea::Immediate(v)]) => {
            let n = line.eval(v)?;
            if line.checking() && !(0..16).contains(&n) {
                return Err(format!("TRAP takes 0 to 15, and {n} isn't"));
            }
            Ok(vec![0x4E40 | n as u16 & 15])
        }
        ("link", [Ea::A(r), Ea::Immediate(v)]) => Ok(vec![0x4E50 | r, line.eval(v)? as u16]),
        ("unlk", [Ea::A(r)]) => Ok(vec![0x4E58 | r]),
        ("swap", [Ea::D(r)]) => Ok(vec![0x4840 | r]),
        ("ext", [Ea::D(r)]) => Ok(vec![if sz == Size::Long { 0x48C0 } else { 0x4880 } | r]),
        ("exg", [Ea::D(x), Ea::D(y)]) => Ok(vec![0xC140 | x << 9 | y]),
        ("exg", [Ea::A(x), Ea::A(y)]) => Ok(vec![0xC148 | x << 9 | y]),
        ("exg", [Ea::D(x), Ea::A(y)]) | ("exg", [Ea::A(y), Ea::D(x)]) => Ok(vec![0xC188 | x << 9 | y]),
        ("tas", [ea]) => with(0x4AC0 | ea.bits(), &[(ea, Size::Byte)], line),
        ("nbcd", [ea]) => with(0x4800 | ea.bits(), &[(ea, Size::Byte)], line),
        _ => wrong(),
    }
}

fn arithmetic_base(name: &str) -> u16 {
    match name {
        "add" => 0xD000,
        "sub" => 0x9000,
        "and" => 0xC000,
        "or" => 0x8000,
        _ => 0xB000,
    }
}

type With<'a> = dyn Fn(u16, &[(&Ea, Size)], &mut Line) -> Result<Vec<u16>, String> + 'a;

/// ADDI, SUBI, ANDI, ORI, EORI and CMPI: the value, then the destination's extension words.
fn immediate(name: &str, sz: Size, src: &Ea, dst: &Ea, with: &With, line: &mut Line) -> Result<Vec<u16>, String> {
    let base = match name {
        "addi" => 0x0600,
        "subi" => 0x0400,
        "andi" => 0x0200,
        "ori" => 0x0000,
        "eori" => 0x0A00,
        _ => 0x0C00,
    };
    with(base | sz.bits() << 6 | dst.bits(), &[(src, sz), (dst, sz)], line)
}
