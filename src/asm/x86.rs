//! The x86, from the 8086 and 8088 through the 186, the 286 and the 386, in NASM's syntax: registers, memory
//! as [bx+si+4] or [es:ebx+ecx*4], sizes as BYTE, WORD and DWORD, SHORT, NEAR and FAR. BITS 16 or 32 says the
//! code's own size (16 to begin with), and CPU 8086, 186, 286 or 386 which instructions it may use: one made
//! for a later processor says so. Each instruction takes NASM's choice of encoding, the shortest it can: a
//! short jump where the label is near enough, a sign-extended byte where the value fits.

use super::{Isa, Line};

pub struct X86;

/// The processors, each with the instructions of those before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    I8086 = 0,
    I186 = 1,
    I286 = 2,
    I386 = 3,
    I486 = 4,
    /// The Pentium (586): CPUID's own, RDTSC, CMPXCHG8B, and the Pentium MMX's MMX.
    P5 = 5,
    /// The Pentium Pro and II (686): CMOV, FCOMI, SYSENTER, FXSAVE.
    P6 = 6,
    /// Anything later: SSE, AVX and the rest.
    Any = 7,
}

impl Level {
    fn name(self) -> &'static str {
        match self {
            Level::I8086 => "8086",
            Level::I186 => "186",
            Level::I286 => "286",
            Level::I386 => "386",
            Level::I486 => "486",
            Level::P5 => "Pentium",
            Level::P6 => "Pentium Pro",
            Level::Any => "later processor",
        }
    }

    fn from(value: i64) -> Level {
        match value {
            0 => Level::I8086,
            1 => Level::I186,
            2 => Level::I286,
            3 => Level::I386,
            4 => Level::I486,
            5 => Level::P5,
            6 => Level::P6,
            _ => Level::Any,
        }
    }
}

/// A general register: its number (0 to 7, as encoded) and size in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Reg {
    num: u8,
    size: u8,
}

#[derive(Clone, Debug, PartialEq)]
struct Mem {
    seg: Option<u8>,
    base: Option<Reg>,
    index: Option<Reg>,
    scale: u8,
    /// The displacement's expression, if there is one.
    disp: Option<String>,
    /// A size given (BYTE, WORD, DWORD...).
    size: Option<u8>,
}

#[derive(Clone, Debug, PartialEq)]
enum Op {
    Reg(Reg),
    Seg(u8),
    Control(u8),
    Debug(u8),
    Test(u8),
    Mem(Mem),
    Imm { text: String, size: Option<u8>, strict: bool },
    /// Any other register iced knows: the x87's ST0 to ST7, MMX's MM0 to MM7, XMM0 and the rest.
    Named(iced_x86::Register),
    /// seg:offset, for a far jump or call.
    Far { seg: String, offset: String },
}

/// SHORT, NEAR or FAR before a jump's target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Distance {
    Any,
    Short,
    Near,
    Far,
}

const REGS8: [&str; 8] = ["al", "cl", "dl", "bl", "ah", "ch", "dh", "bh"];
const REGS16: [&str; 8] = ["ax", "cx", "dx", "bx", "sp", "bp", "si", "di"];
const REGS32: [&str; 8] = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];
const SEGS: [&str; 6] = ["es", "cs", "ss", "ds", "fs", "gs"];
const SEG_PREFIX: [u8; 6] = [0x26, 0x2E, 0x36, 0x3E, 0x64, 0x65];

/// The conditions, as Jcc and SETcc name them, with their numbers.
const CONDITIONS: [(&str, u8); 30] = [
    ("o", 0),
    ("no", 1),
    ("b", 2),
    ("c", 2),
    ("nae", 2),
    ("ae", 3),
    ("nb", 3),
    ("nc", 3),
    ("e", 4),
    ("z", 4),
    ("ne", 5),
    ("nz", 5),
    ("be", 6),
    ("na", 6),
    ("a", 7),
    ("nbe", 7),
    ("s", 8),
    ("ns", 9),
    ("p", 10),
    ("pe", 10),
    ("np", 11),
    ("po", 11),
    ("l", 12),
    ("nge", 12),
    ("ge", 13),
    ("nl", 13),
    ("le", 14),
    ("ng", 14),
    ("g", 15),
    ("nle", 15),
];

fn condition(name: &str) -> Option<u8> {
    CONDITIONS.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

/// The instructions with no operands: their bytes, the processor they need, and the operand size they set.
const PLAIN: &[(&str, &[u8], Level, u8)] = &[
    ("nop", &[0x90], Level::I8086, 0),
    ("hlt", &[0xF4], Level::I8086, 0),
    ("cmc", &[0xF5], Level::I8086, 0),
    ("clc", &[0xF8], Level::I8086, 0),
    ("stc", &[0xF9], Level::I8086, 0),
    ("cli", &[0xFA], Level::I8086, 0),
    ("sti", &[0xFB], Level::I8086, 0),
    ("cld", &[0xFC], Level::I8086, 0),
    ("std", &[0xFD], Level::I8086, 0),
    ("sahf", &[0x9E], Level::I8086, 0),
    ("lahf", &[0x9F], Level::I8086, 0),
    ("daa", &[0x27], Level::I8086, 0),
    ("das", &[0x2F], Level::I8086, 0),
    ("aaa", &[0x37], Level::I8086, 0),
    ("aas", &[0x3F], Level::I8086, 0),
    ("xlat", &[0xD7], Level::I8086, 0),
    ("xlatb", &[0xD7], Level::I8086, 0),
    ("wait", &[0x9B], Level::I8086, 0),
    ("fwait", &[0x9B], Level::I8086, 0),
    ("into", &[0xCE], Level::I8086, 0),
    ("int3", &[0xCC], Level::I8086, 0),
    ("int1", &[0xF1], Level::I386, 0),
    ("icebp", &[0xF1], Level::I386, 0),
    ("lock", &[0xF0], Level::I8086, 0),
    ("cbw", &[0x98], Level::I8086, 2),
    ("cwde", &[0x98], Level::I386, 4),
    ("cwd", &[0x99], Level::I8086, 2),
    ("cdq", &[0x99], Level::I386, 4),
    ("pushf", &[0x9C], Level::I8086, 0),
    ("pushfw", &[0x9C], Level::I8086, 2),
    ("pushfd", &[0x9C], Level::I386, 4),
    ("popf", &[0x9D], Level::I8086, 0),
    ("popfw", &[0x9D], Level::I8086, 2),
    ("popfd", &[0x9D], Level::I386, 4),
    ("pusha", &[0x60], Level::I186, 0),
    ("pushaw", &[0x60], Level::I186, 2),
    ("pushad", &[0x60], Level::I386, 4),
    ("popa", &[0x61], Level::I186, 0),
    ("popaw", &[0x61], Level::I186, 2),
    ("popad", &[0x61], Level::I386, 4),
    ("leave", &[0xC9], Level::I186, 0),
    ("iret", &[0xCF], Level::I8086, 0),
    ("iretw", &[0xCF], Level::I8086, 2),
    ("iretd", &[0xCF], Level::I386, 4),
    ("movsb", &[0xA4], Level::I8086, 0),
    ("movsw", &[0xA5], Level::I8086, 2),
    ("movsd", &[0xA5], Level::I386, 4),
    ("cmpsb", &[0xA6], Level::I8086, 0),
    ("cmpsw", &[0xA7], Level::I8086, 2),
    ("cmpsd", &[0xA7], Level::I386, 4),
    ("stosb", &[0xAA], Level::I8086, 0),
    ("stosw", &[0xAB], Level::I8086, 2),
    ("stosd", &[0xAB], Level::I386, 4),
    ("lodsb", &[0xAC], Level::I8086, 0),
    ("lodsw", &[0xAD], Level::I8086, 2),
    ("lodsd", &[0xAD], Level::I386, 4),
    ("scasb", &[0xAE], Level::I8086, 0),
    ("scasw", &[0xAF], Level::I8086, 2),
    ("scasd", &[0xAF], Level::I386, 4),
    ("insb", &[0x6C], Level::I186, 0),
    ("insw", &[0x6D], Level::I186, 2),
    ("insd", &[0x6D], Level::I386, 4),
    ("outsb", &[0x6E], Level::I186, 0),
    ("outsw", &[0x6F], Level::I186, 2),
    ("outsd", &[0x6F], Level::I386, 4),
    ("clts", &[0x0F, 0x06], Level::I286, 0),
    ("invd", &[0x0F, 0x08], Level::I486, 0),
    ("wbinvd", &[0x0F, 0x09], Level::I486, 0),
    ("cpuid", &[0x0F, 0xA2], Level::I486, 0),
    ("salc", &[0xD6], Level::I8086, 0),
];

/// The arithmetic group, by its /digit and the low opcodes: ADD 00, OR 08, ADC 10, SBB 18, AND 20, SUB 28, XOR 30, CMP 38.
const ARITHMETIC: [&str; 8] = ["add", "or", "adc", "sbb", "and", "sub", "xor", "cmp"];
/// The shifts and rotates, by their /digit.
const SHIFTS: [(&str, u8); 8] = [("rol", 0), ("ror", 1), ("rcl", 2), ("rcr", 3), ("shl", 4), ("sal", 4), ("shr", 5), ("sar", 7)];
/// The F6/F7 group: TEST is /0, NOT /2, NEG /3, MUL /4, IMUL /5, DIV /6, IDIV /7.
const UNARY: [(&str, u8); 5] = [("not", 2), ("neg", 3), ("mul", 4), ("div", 6), ("idiv", 7)];
/// The 286's protected-mode group 0F 00 and 0F 01, by their /digit.
const SYSTEM: [(&str, u8, u8); 11] = [("sldt", 0x00, 0), ("str", 0x00, 1), ("lldt", 0x00, 2), ("ltr", 0x00, 3), ("verr", 0x00, 4), ("verw", 0x00, 5), ("sgdt", 0x01, 0), ("sidt", 0x01, 1), ("lgdt", 0x01, 2), ("lidt", 0x01, 3), ("smsw", 0x01, 4)];
const BIT_TESTS: [(&str, u8, u8); 4] = [("bt", 0xA3, 4), ("bts", 0xAB, 5), ("btr", 0xB3, 6), ("btc", 0xBB, 7)];

const OTHERS: &[&str] = &[
    "mov", "movzx", "movsx", "lea", "lds", "les", "lss", "lfs", "lgs", "xchg", "test", "inc", "dec", "push", "pop", "imul", "jmp", "call", "ret", "retn", "retf", "int", "in", "out", "loop", "loope", "loopz", "loopne", "loopnz", "jcxz", "jecxz", "enter", "bound", "arpl", "lar", "lsl", "lmsw", "invlpg", "bsf", "bsr", "shld", "shrd", "aam", "aad", "rep", "repe", "repz", "repne", "repnz", "bswap", "xadd", "cmpxchg", "a16", "a32", "o16", "o32",
];

impl Isa for X86 {
    fn name(&self) -> &'static str {
        "x86"
    }

    fn is_instruction(&self, word: &str) -> bool {
        let word = word.to_ascii_lowercase();
        let word = word.as_str();
        PLAIN.iter().any(|(n, ..)| *n == word)
            || ARITHMETIC.contains(&word)
            || SHIFTS.iter().any(|(n, _)| *n == word)
            || UNARY.iter().any(|(n, _)| *n == word)
            || SYSTEM.iter().any(|(n, ..)| *n == word)
            || BIT_TESTS.iter().any(|(n, ..)| *n == word)
            || OTHERS.contains(&word)
            || word.strip_prefix('j').and_then(condition).is_some()
            || word.strip_prefix("set").and_then(condition).is_some()
            || table::knows(word)
    }

    fn nasm(&self) -> bool {
        true
    }

    fn is_directive(&self, word: &str) -> bool {
        matches!(word.to_ascii_lowercase().as_str(), "bits" | "use16" | "use32" | "cpu" | "resb" | "resw" | "resd" | "resq" | "rest" | "dq" | "align" | "section" | "segment" | "global" | "extern" | "absolute" | "times")
    }

    fn directive(&self, word: &str, operands: &str, line: &mut Line) -> Option<Result<Vec<u8>, String>> {
        match word {
            "bits" | "use16" | "use32" => {
                let bits = match word {
                    "use16" => 16,
                    "use32" => 32,
                    _ => match operands.trim() {
                        "16" => 16,
                        "32" => 32,
                        other => return Some(Err(crate::i18n::tf!("BITS is 16 or 32 for the 8086 to the 386, not {other}"))),
                    },
                };
                line.set_setting("bits", bits);
                Some(Ok(Vec::new()))
            }
            "cpu" => {
                let level = match operands.trim().to_ascii_lowercase().as_str() {
                    "8086" | "8088" => Level::I8086,
                    "186" | "80186" | "188" => Level::I186,
                    "286" | "80286" => Level::I286,
                    "386" | "80386" => Level::I386,
                    "486" | "80486" => Level::I486,
                    "586" | "pentium" | "p5" => Level::P5,
                    "686" | "ppro" | "p6" | "p2" | "pentiumpro" => Level::P6,
                    "any" | "all" | "default" | "p3" | "p4" | "katmai" | "willamette" | "prescott" | "x64" => Level::Any,
                    other => return Some(Err(crate::i18n::tf!("CPU {other} isn't one Limoniter's x86 assembler knows: 8086, 186, 286, 386, 486, 586 (Pentium), 686 (Pentium Pro) or ANY"))),
                };
                line.set_setting("cpu", level as i64);
                Some(Ok(Vec::new()))
            }
            "resb" | "resw" | "resd" | "resq" | "rest" => {
                let unit = match word {
                    "resb" => 1,
                    "resw" => 2,
                    "resd" => 4,
                    "resq" => 8,
                    _ => 10,
                };
                Some(line.eval(operands).and_then(|n| if (0..=0x100_0000).contains(&n) { Ok(vec![0; (n * unit) as usize]) } else { Err(crate::i18n::tf!("{n} is too many to reserve")) }))
            }
            "dq" => Some(split(operands).iter().map(|item| line.eval(item).map(|v| v.to_le_bytes().to_vec())).collect::<Result<Vec<_>, _>>().map(|parts| parts.concat())),
            // In code, NASM fills to an alignment with NOPs.
            "align" => {
                let mut parts = split(operands).into_iter();
                let to = line.eval(&parts.next().unwrap_or_default()).ok()?.max(1);
                let fill = match parts.next() {
                    Some(f) if f.trim().eq_ignore_ascii_case("db 0") => 0,
                    _ => 0x90u8,
                };
                let here = line.here();
                Some(Ok(vec![fill; ((to - here.rem_euclid(to)) % to) as usize]))
            }
            "section" | "segment" => {
                let name = operands.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
                if matches!(name.as_str(), ".text" | "text" | "code" | ".code" | "_text") {
                    Some(Ok(Vec::new()))
                } else {
                    Some(Err(crate::i18n::tf!("Limoniter's x86 assembler keeps a program in one piece, as it's written: SECTION {name} would move what follows elsewhere, as NASM does, so leave it out and put the data where it's to go")))
                }
            }
            "global" | "extern" | "absolute" => Some(Err(crate::i18n::tf!("{} is for linking, and Limoniter's x86 assembler makes plain binaries: .COM programs and boot sectors", word.to_ascii_uppercase()))),
            _ => None,
        }
    }

    fn encode(&self, mnemonic: &str, operands: &str, line: &mut Line) -> Result<Vec<u8>, String> {
        let bits = line.setting("bits").unwrap_or(16) as u8;
        let level = Level::from(line.setting("cpu").unwrap_or(Level::Any as i64));
        let (bytes, needs) = encode(mnemonic, operands, line, bits)?;
        if needs > level {
            return Err(crate::i18n::tf!("{} needs a {} or later, and the source says CPU {}", mnemonic.to_ascii_uppercase(), needs.name(), level.name()));
        }
        Ok(bytes)
    }

    fn local_prefixes(&self) -> &'static [char] {
        &['.']
    }
}

/// Splits operands at commas that aren't inside brackets or quotes.
fn split(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut depth, mut quote, mut start) = (0i32, None, 0);
    for (i, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"' | '`') => quote = Some(c),
            (None, '[' | '(') => depth += 1,
            (None, ']' | ')') => depth -= 1,
            (None, ',') if depth == 0 => {
                out.push(text[start..i].trim().to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    let last = text[start..].trim();
    if !last.is_empty() || !out.is_empty() {
        out.push(last.to_string());
    }
    out
}

fn reg(name: &str) -> Option<Reg> {
    let name = name.to_ascii_lowercase();
    for (size, list) in [(1u8, &REGS8), (2, &REGS16), (4, &REGS32)] {
        if let Some(n) = list.iter().position(|r| *r == name) {
            return Some(Reg { num: n as u8, size });
        }
    }
    None
}

fn seg(name: &str) -> Option<u8> {
    SEGS.iter().position(|s| s.eq_ignore_ascii_case(name)).map(|n| n as u8)
}

/// Takes a leading keyword (BYTE, SHORT...) off an operand.
fn strip_word<'a>(text: &'a str, words: &[&str]) -> (Option<String>, &'a str) {
    let trimmed = text.trim_start();
    let first: String = trimmed.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    let lower = first.to_ascii_lowercase();
    if words.contains(&lower.as_str()) && trimmed[first.len()..].starts_with(|c: char| c.is_whitespace() || c == '[') {
        return (Some(lower), trimmed[first.len()..].trim_start());
    }
    (None, trimmed)
}

fn size_of_word(word: &str) -> Option<u8> {
    match word {
        "byte" => Some(1),
        "word" => Some(2),
        "dword" => Some(4),
        "fword" => Some(6),
        "qword" => Some(8),
        "tword" => Some(10),
        "oword" | "xmmword" => Some(16),
        "yword" | "ymmword" => Some(32),
        "zword" | "zmmword" => Some(64),
        _ => None,
    }
}

/// An operand, and SHORT, NEAR or FAR before it.
fn operand(text: &str) -> Result<(Op, Distance), String> {
    let mut rest = text.trim();
    let mut size = None;
    let mut distance = Distance::Any;
    let mut strict = false;
    loop {
        let (word, after) = strip_word(rest, &["byte", "word", "dword", "fword", "qword", "tword", "oword", "xmmword", "yword", "ymmword", "zword", "zmmword", "short", "near", "far", "strict", "ptr"]);
        let Some(word) = word else { break };
        match word.as_str() {
            "short" => distance = Distance::Short,
            "near" => distance = Distance::Near,
            "far" => distance = Distance::Far,
            "strict" => strict = true,
            "ptr" => {}
            w => size = size_of_word(w),
        }
        rest = after;
    }
    // MASM's "word ptr": PTR after a size.
    if let Some(after) = rest.strip_prefix("ptr ").or_else(|| rest.strip_prefix("PTR ")) {
        rest = after.trim_start();
    }
    if rest.is_empty() {
        return Err(crate::i18n::tr("an operand is missing").into());
    }
    // A segment written before the brackets: es:[bx].
    let outside_seg = rest.split_once(':').filter(|(s, after)| seg(s.trim()).is_some() && after.trim_start().starts_with('['));
    if rest.starts_with('[') || outside_seg.is_some() {
        let (seg_before, inner) = match outside_seg {
            Some((s, after)) => (seg(s.trim()), after.trim()),
            None => (None, rest),
        };
        let inner = inner.strip_prefix('[').and_then(|i| i.strip_suffix(']')).ok_or_else(|| crate::i18n::tf!("{text} isn't closed with ]"))?;
        let mut mem = memory(inner)?;
        if mem.seg.is_none() {
            mem.seg = seg_before;
        }
        mem.size = size;
        return Ok((Op::Mem(mem), distance));
    }
    if let Some(r) = reg(rest) {
        return Ok((Op::Reg(r), distance));
    }
    let lower = rest.to_ascii_lowercase();
    if let Some(s) = seg(&lower) {
        return Ok((Op::Seg(s), distance));
    }
    // ST(1) is ST1, and ST alone ST0.
    let named = if lower == "st" { "st0".to_string() } else { lower.replace(['(', ')'], "") };
    if !lower.starts_with(['c', 'd', 't'])
        && let Some(r) = table::register(&named)
    {
        return Ok((Op::Named(r), distance));
    }
    for (prefix, make) in [("cr", Op::Control as fn(u8) -> Op), ("dr", Op::Debug), ("tr", Op::Test)] {
        if let Some(n) = lower.strip_prefix(prefix).and_then(|n| n.parse::<u8>().ok()).filter(|n| *n < 8) {
            return Ok((make(n), distance));
        }
    }
    // seg:offset, both values.
    if let Some((s, o)) = rest.split_once(':')
        && !s.contains(['\'', '"'])
    {
        return Ok((Op::Far { seg: s.trim().to_string(), offset: o.trim().to_string() }, distance));
    }
    Ok((Op::Imm { text: rest.to_string(), size, strict }, distance))
}

/// What's inside the brackets: a segment, registers (an index perhaps scaled) and a displacement.
fn memory(inner: &str) -> Result<Mem, String> {
    let mut text = inner.trim();
    let mut mem = Mem { seg: None, base: None, index: None, scale: 1, disp: None, size: None };
    if let Some((s, after)) = text.split_once(':')
        && let Some(s) = seg(s.trim())
    {
        mem.seg = Some(s);
        text = after.trim();
    }
    // Terms between + and - at the top level, each with its sign.
    let mut terms: Vec<(char, String)> = Vec::new();
    let (mut depth, mut start, mut sign) = (0i32, 0, '+');
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            '+' | '-' if depth == 0 && i > start => {
                // A sign right after an operator (as 4*-2) belongs to the term.
                let before: String = bytes[start..i].iter().collect();
                if !before.trim_end().ends_with(['*', '/', '<', '>', '&', '|', '^']) {
                    terms.push((sign, before.trim().to_string()));
                    sign = c;
                    start = i + 1;
                }
            }
            '+' | '-' if depth == 0 && i == start => {
                sign = if c == '-' { if sign == '-' { '+' } else { '-' } } else { sign };
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    terms.push((sign, bytes[start..].iter().collect::<String>().trim().to_string()));
    let mut disp = String::new();
    for (sign, term) in terms {
        if term.is_empty() {
            continue;
        }
        // A register, perhaps scaled: ecx*4 or 4*ecx.
        let (r, scale) = match term.split_once('*') {
            Some((a, b)) if reg(a.trim()).is_some() => (reg(a.trim()), b.trim().parse::<u8>().ok()),
            Some((a, b)) if reg(b.trim()).is_some() => (reg(b.trim()), a.trim().parse::<u8>().ok()),
            _ => (reg(&term), Some(1)),
        };
        match (r, scale) {
            (Some(r), Some(scale)) => {
                if sign == '-' {
                    return Err(crate::i18n::tf!("a register can't be taken away in an address: -{term}"));
                }
                if r.size == 1 {
                    return Err(crate::i18n::tf!("{term} can't be used in an address"));
                }
                if scale != 1 || mem.base.is_some() && mem.index.is_none() && r.size == 4 && mem.base.map(|b| b.size) == Some(4) {
                    if mem.index.is_some() {
                        return Err(crate::i18n::tf!("an address can have only one scaled register: {inner}"));
                    }
                    if scale != 1 {
                        mem.index = Some(r);
                        mem.scale = scale;
                    } else {
                        mem.index = Some(r);
                    }
                } else if mem.base.is_none() {
                    mem.base = Some(r);
                } else if mem.index.is_none() {
                    mem.index = Some(r);
                } else {
                    return Err(crate::i18n::tf!("too many registers in the address {inner}"));
                }
            }
            (Some(_), None) => return Err(crate::i18n::tf!("{term}: a register is scaled by 1, 2, 4 or 8")),
            _ => {
                if !disp.is_empty() || sign == '-' {
                    disp.push(sign);
                }
                disp.push('(');
                disp.push_str(&term);
                disp.push(')');
            }
        }
    }
    if !disp.is_empty() {
        mem.disp = Some(disp);
    }
    Ok(mem)
}

/// What an instruction becomes: its prefixes, opcode, ModR/M and the rest, and the processor it needs.
struct Out {
    bytes: Vec<u8>,
    needs: Level,
}

impl Out {
    fn new() -> Out {
        Out { bytes: Vec::new(), needs: Level::I8086 }
    }

    fn need(&mut self, level: Level) {
        self.needs = self.needs.max(level);
    }
}

fn fits_i8(v: i64) -> bool {
    (-128..=127).contains(&v)
}

/// The value of an expression, as a number of `size` bytes, checked on the last pass.
fn imm_bytes(line: &mut Line, text: &str, size: u8) -> Result<Vec<u8>, String> {
    let v = line.eval(text)?;
    if line.checking() {
        let (low, high) = match size {
            1 => (-128, 255),
            2 => (-32768, 65535),
            _ => (-(1i64 << 31), (1i64 << 32) - 1),
        };
        if !(low..=high).contains(&v) {
            return Err(crate::i18n::tf!("{text} is {v}, which doesn't fit in {size} bytes"));
        }
    }
    Ok(v.to_le_bytes()[..usize::from(size)].to_vec())
}

/// The ModR/M byte (with SIB and displacement) for a register field and an operand: a register or memory.
/// Prefixes it needs (a segment, the address size) are put in `prefixes`.
fn modrm(field: u8, op: &Op, bits: u8, line: &mut Line, out: &mut Out, prefixes: &mut Vec<u8>) -> Result<Vec<u8>, String> {
    let mem = match op {
        Op::Reg(r) => return Ok(vec![0xC0 | (field << 3) | r.num]),
        Op::Mem(m) => m,
        _ => return Err(crate::i18n::tr("that operand has to be a register or memory").into()),
    };
    if let Some(s) = mem.seg {
        if s >= 4 {
            out.need(Level::I386);
        }
        prefixes.push(SEG_PREFIX[usize::from(s)]);
    }
    let addr32 = match (mem.base, mem.index) {
        (Some(b), _) if b.size == 4 => true,
        (_, Some(i)) if i.size == 4 => true,
        (None, None) => bits == 32,
        _ => false,
    };
    if mem.base.is_some_and(|b| b.size == 4) && mem.index.is_some_and(|i| i.size == 2) || mem.base.is_some_and(|b| b.size == 2) && mem.index.is_some_and(|i| i.size == 4) {
        return Err(crate::i18n::tr("an address can't mix 16-bit and 32-bit registers").into());
    }
    if addr32 {
        out.need(Level::I386);
    }
    if addr32 != (bits == 32) {
        prefixes.push(0x67);
    }
    let disp_value = match &mem.disp {
        Some(d) => Some((line.eval(d)?, line.known(d))),
        None => None,
    };
    if !addr32 {
        // 16-bit addresses: the eight combinations of BX, BP, SI and DI.
        let names = |r: Option<Reg>| r.map(|r| REGS16[usize::from(r.num)]);
        let (b, i) = (names(mem.base), names(mem.index));
        if mem.scale != 1 {
            return Err(crate::i18n::tr("a 16-bit address can't scale a register: that needs 32-bit registers (and a 386)").into());
        }
        let rm = match (b, i) {
            (Some("bx"), Some("si")) | (Some("si"), Some("bx")) => Some(0),
            (Some("bx"), Some("di")) | (Some("di"), Some("bx")) => Some(1),
            (Some("bp"), Some("si")) | (Some("si"), Some("bp")) => Some(2),
            (Some("bp"), Some("di")) | (Some("di"), Some("bp")) => Some(3),
            (Some("si"), None) => Some(4),
            (Some("di"), None) => Some(5),
            (Some("bp"), None) => Some(6),
            (Some("bx"), None) => Some(7),
            (None, None) => None,
            _ => return Err(crate::i18n::tr("a 16-bit address is BX or BP, with SI or DI, and a number: [bx+si+4]").into()),
        };
        let Some(rm) = rm else {
            // [disp16] alone.
            let v = disp_value.map_or(0, |(v, _)| v);
            let mut bytes = vec![(field << 3) | 6];
            bytes.extend((v as u16).to_le_bytes());
            return Ok(bytes);
        };
        return Ok(match disp_value {
            // [bp] has no form without a displacement: a zero byte.
            None if rm == 6 => vec![0x40 | (field << 3) | 6, 0],
            None => vec![(field << 3) | rm],
            Some((v, known)) if v == 0 && known.is_some() && rm != 6 => vec![(field << 3) | rm],
            Some((v, known)) if known.is_some() && fits_i8(v) => vec![0x40 | (field << 3) | rm, v as u8],
            Some((v, _)) => {
                let mut bytes = vec![0x80 | (field << 3) | rm];
                bytes.extend((v as u16).to_le_bytes());
                bytes
            }
        });
    }
    // 32-bit addresses: a base, an index scaled by 1, 2, 4 or 8, a displacement.
    let (mut base, mut index, mut scale) = (mem.base, mem.index, mem.scale);
    // NASM's way with [eax*2] alone: [eax+eax], which needs no 32-bit displacement.
    if base.is_none() && index.is_some() && scale == 2 {
        base = index;
        scale = 1;
    }
    // [esp+eax]: ESP can't be an index, so the two swap.
    if index.is_some_and(|i| i.num == 4) && scale == 1 {
        std::mem::swap(&mut base, &mut index);
    }
    if index.is_some_and(|i| i.num == 4) {
        return Err(crate::i18n::tr("ESP can't be an address's index").into());
    }
    let ss = match scale {
        1 => 0,
        2 => 1,
        4 => 2,
        8 => 3,
        _ => return Err(crate::i18n::tf!("a register is scaled by 1, 2, 4 or 8, not {scale}")),
    };
    let disp_bytes = |mode: u8, v: i64| -> Vec<u8> {
        match mode {
            1 => vec![v as u8],
            2 => (v as u32).to_le_bytes().to_vec(),
            _ => Vec::new(),
        }
    };
    // How the displacement is kept: none, a byte or four.
    let mode = |base_is_ebp: bool| -> (u8, i64) {
        match disp_value {
            None if base_is_ebp => (1, 0),
            None => (0, 0),
            Some((v, Some(_))) if v == 0 && !base_is_ebp => (0, 0),
            Some((v, Some(_))) if fits_i8(v) => (1, v),
            Some((v, _)) => (2, v),
        }
    };
    match (base, index) {
        (None, None) => {
            let v = disp_value.map_or(0, |(v, _)| v);
            let mut bytes = vec![(field << 3) | 5];
            bytes.extend((v as u32).to_le_bytes());
            Ok(bytes)
        }
        (Some(b), None) if b.num != 4 => {
            let (m, v) = mode(b.num == 5);
            let mut bytes = vec![(m << 6) | (field << 3) | b.num];
            bytes.extend(disp_bytes(m, v));
            Ok(bytes)
        }
        (None, Some(i)) => {
            // No base: SIB with base 101 and a 32-bit displacement.
            let v = disp_value.map_or(0, |(v, _)| v);
            let mut bytes = vec![(field << 3) | 4, (ss << 6) | (i.num << 3) | 5];
            bytes.extend((v as u32).to_le_bytes());
            Ok(bytes)
        }
        (Some(b), i) => {
            let (m, v) = mode(b.num == 5);
            let index_num = i.map_or(4, |i| i.num);
            let mut bytes = vec![(m << 6) | (field << 3) | 4, (ss << 6) | (index_num << 3) | b.num];
            bytes.extend(disp_bytes(m, v));
            Ok(bytes)
        }
    }
}

/// The operand-size prefix for a size, in this mode; and the 386 for 32-bit operands.
fn size_prefix(size: u8, bits: u8, out: &mut Out, prefixes: &mut Vec<u8>) {
    if size == 4 {
        out.need(Level::I386);
    }
    if (size == 4 && bits == 16) || (size == 2 && bits == 32) {
        prefixes.push(0x66);
    }
}

fn op_size(op: &Op) -> Option<u8> {
    match op {
        Op::Reg(r) => Some(r.size),
        Op::Mem(m) => m.size,
        Op::Seg(_) => Some(2),
        _ => None,
    }
}

/// The size two operands agree on, or the one that's said.
fn agreed(a: &Op, b: &Op, name: &str) -> Result<u8, String> {
    match (op_size(a), op_size(b)) {
        (Some(x), Some(y)) if x != y => Err(crate::i18n::tf!("{name}'s operands are different sizes")),
        (Some(x), _) | (None, Some(x)) => Ok(x),
        (None, None) => Err(crate::i18n::tf!("{name} needs a size: say BYTE, WORD or DWORD")),
    }
}

/// Puts the pieces together: prefixes, then the opcode and what follows.
fn finish(prefixes: Vec<u8>, rest: Vec<u8>, out: &mut Out) -> (Vec<u8>, Level) {
    let mut bytes = Vec::new();
    // NASM's order: a lock or repeat, the segment, the operand size, the address size.
    let order = |b: &u8| match b {
        0xF0 | 0xF2 | 0xF3 => 0,
        0x26 | 0x2E | 0x36 | 0x3E | 0x64 | 0x65 => 1,
        0x66 => 2,
        _ => 3,
    };
    let mut prefixes = prefixes;
    prefixes.sort_by_key(order);
    bytes.extend(prefixes);
    bytes.extend(rest);
    out.bytes = bytes.clone();
    (bytes, out.needs)
}

/// An instruction with a ModR/M: prefixes, opcode bytes, the ModR/M for a field and operand, and what follows.
fn with_modrm(opcode: &[u8], field: u8, rm: &Op, size: u8, bits: u8, tail: Vec<u8>, line: &mut Line, out: &mut Out) -> Result<(Vec<u8>, Level), String> {
    let mut prefixes = Vec::new();
    if size > 1 {
        size_prefix(size, bits, out, &mut prefixes);
    }
    let m = modrm(field, rm, bits, line, out, &mut prefixes)?;
    let mut rest = opcode.to_vec();
    rest.extend(m);
    rest.extend(tail);
    Ok(finish(prefixes, rest, out))
}

/// The bytes a relative jump needs to reach `target` from an instruction `length` bytes long starting here.
fn relative(line: &Line, target: i64, length: i64, size: u8) -> Vec<u8> {
    let offset = target - (line.here() + length);
    offset.to_le_bytes()[..usize::from(size)].to_vec()
}

fn encode(mnemonic: &str, operand_text: &str, line: &mut Line, bits: u8) -> Result<(Vec<u8>, Level), String> {
    let name = mnemonic.to_ascii_uppercase();
    let texts = split(operand_text);
    let parsed: Vec<(Op, Distance)> = texts.iter().filter(|t| !t.is_empty()).map(|t| operand(t)).collect::<Result<_, _>>()?;
    let ops: Vec<Op> = parsed.iter().map(|(o, _)| o.clone()).collect();
    let distance = parsed.first().map_or(Distance::Any, |(_, d)| *d);
    let mut out = Out::new();
    let count = |n: usize| -> Result<(), String> {
        if ops.len() == n {
            Ok(())
        } else if n == 0 {
            Err(crate::i18n::tf!("{name} takes no operands"))
        } else if n == 1 {
            Err(crate::i18n::tf!("{name} takes one operand"))
        } else {
            Err(crate::i18n::tf!("{name} takes {n} operands"))
        }
    };

    // Prefixes written as instructions: REP MOVSB, LOCK, and NASM's A16, O32 and the rest.
    if let Some(prefix) = match mnemonic {
        "rep" | "repe" | "repz" => Some(0xF3u8),
        "repne" | "repnz" => Some(0xF2),
        "lock" if !operand_text.trim().is_empty() => Some(0xF0),
        "a16" | "a32" | "o16" | "o32" => Some(if mnemonic.starts_with('a') { 0x67 } else { 0x66 }),
        _ => None,
    } {
        let wanted = if mnemonic.ends_with("32") { 32 } else { 16 };
        let mut bytes = if (prefix == 0x66 || prefix == 0x67) && wanted == bits { Vec::new() } else { vec![prefix] };
        let rest = operand_text.trim();
        if rest.is_empty() {
            return Ok((bytes, Level::I8086));
        }
        let (op, operands) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let (more, needs) = encode(&op.to_ascii_lowercase(), operands, line, bits)?;
        bytes.extend(more);
        return Ok((bytes, needs.max(if wanted == 32 { Level::I386 } else { Level::I8086 })));
    }

    // No operands.
    if let Some((_, bytes, level, size)) = PLAIN.iter().find(|(n, ..)| *n == mnemonic) {
        count(0)?;
        out.need(*level);
        let mut prefixes = Vec::new();
        if *size > 1 {
            size_prefix(*size, bits, &mut out, &mut prefixes);
        }
        return Ok(finish(prefixes, bytes.to_vec(), &mut out));
    }

    // ADD, OR, ADC, SBB, AND, SUB, XOR, CMP.
    if let Some(n) = ARITHMETIC.iter().position(|a| *a == mnemonic) {
        count(2)?;
        let n = n as u8;
        let (a, b) = (&ops[0], &ops[1]);
        return match (a, b) {
            (Op::Reg(_) | Op::Mem(_), Op::Reg(r)) => {
                let size = agreed(a, b, &name)?;
                with_modrm(&[n * 8 + u8::from(size > 1)], r.num, a, size, bits, Vec::new(), line, &mut out)
            }
            (Op::Reg(r), Op::Mem(_)) => {
                let size = agreed(a, b, &name)?;
                with_modrm(&[n * 8 + 2 + u8::from(size > 1)], r.num, b, size, bits, Vec::new(), line, &mut out)
            }
            (Op::Reg(_) | Op::Mem(_), Op::Imm { text, size: isize, strict }) => {
                let size = op_size(a).ok_or_else(|| crate::i18n::tf!("{name} needs a size: say BYTE, WORD or DWORD"))?;
                if size == 1 {
                    let imm = imm_bytes(line, text, 1)?;
                    if matches!(a, Op::Reg(Reg { num: 0, .. })) {
                        let mut prefixes = Vec::new();
                        let mut rest = vec![n * 8 + 4];
                        rest.extend(imm);
                        let _ = &mut prefixes;
                        return Ok(finish(prefixes, rest, &mut out));
                    }
                    return with_modrm(&[0x80], n, a, 1, bits, imm, line, &mut out);
                }
                // A value that fits a signed byte goes as one (83), unless the size was insisted on.
                let small = match isize {
                    Some(1) => true,
                    Some(_) if *strict => false,
                    _ => line.known(text).is_some_and(fits_i8) && isize.is_none_or(|s| s != size || !*strict),
                };
                if small {
                    let v = line.eval(text)?;
                    return with_modrm(&[0x83], n, a, size, bits, vec![v as u8], line, &mut out);
                }
                let imm = imm_bytes(line, text, size)?;
                if matches!(a, Op::Reg(Reg { num: 0, .. })) {
                    let mut prefixes = Vec::new();
                    size_prefix(size, bits, &mut out, &mut prefixes);
                    let mut rest = vec![n * 8 + 5];
                    rest.extend(imm);
                    return Ok(finish(prefixes, rest, &mut out));
                }
                with_modrm(&[0x81], n, a, size, bits, imm, line, &mut out)
            }
            _ => Err(crate::i18n::tf!("{name} takes a register or memory, and a register, memory or number")),
        };
    }

    // ROL, ROR, RCL, RCR, SHL, SAL, SHR, SAR.
    if let Some((_, field)) = SHIFTS.iter().find(|(n, _)| *n == mnemonic) {
        count(2)?;
        let target = &ops[0];
        let size = op_size(target).ok_or_else(|| crate::i18n::tf!("{name} needs a size: say BYTE, WORD or DWORD"))?;
        let wide = u8::from(size > 1);
        return match &ops[1] {
            Op::Reg(Reg { num: 1, size: 1 }) => with_modrm(&[0xD2 + wide], *field, target, size, bits, Vec::new(), line, &mut out),
            Op::Imm { text, .. } => {
                let v = line.eval(text)?;
                if line.known(text) == Some(1) {
                    with_modrm(&[0xD0 + wide], *field, target, size, bits, Vec::new(), line, &mut out)
                } else {
                    out.need(Level::I186);
                    with_modrm(&[0xC0 + wide], *field, target, size, bits, vec![v as u8], line, &mut out)
                }
            }
            _ => Err(crate::i18n::tf!("{name} shifts by 1, by CL or (on the 186 and later) by a number")),
        };
    }

    // NOT, NEG, MUL, DIV, IDIV.
    if let Some((_, field)) = UNARY.iter().find(|(n, _)| *n == mnemonic) {
        count(1)?;
        let size = op_size(&ops[0]).ok_or_else(|| crate::i18n::tf!("{name} needs a size: say BYTE, WORD or DWORD"))?;
        return with_modrm(&[0xF6 + u8::from(size > 1)], *field, &ops[0], size, bits, Vec::new(), line, &mut out);
    }

    // SLDT, STR, LLDT, LTR, VERR, VERW, SGDT, SIDT, LGDT, LIDT, SMSW.
    if let Some((_, second, field)) = SYSTEM.iter().find(|(n, ..)| *n == mnemonic) {
        count(1)?;
        out.need(Level::I286);
        if matches!(mnemonic, "sgdt" | "sidt" | "lgdt" | "lidt") && !matches!(ops[0], Op::Mem(_)) {
            return Err(crate::i18n::tf!("{name} takes memory: the table's limit and base"));
        }
        let size = match &ops[0] {
            Op::Reg(r) if r.size == 1 => return Err(crate::i18n::tf!("{name} takes a 16-bit register")),
            Op::Reg(r) if matches!(mnemonic, "sldt" | "str" | "smsw") => r.size,
            _ => 2,
        };
        let mut prefixes = Vec::new();
        if size == 4 {
            size_prefix(4, bits, &mut out, &mut prefixes);
        }
        let m = modrm(*field, &ops[0], bits, line, &mut out, &mut prefixes)?;
        let mut rest = vec![0x0F, *second];
        rest.extend(m);
        return Ok(finish(prefixes, rest, &mut out));
    }

    // BT, BTS, BTR, BTC.
    if let Some((_, opcode, field)) = BIT_TESTS.iter().find(|(n, ..)| *n == mnemonic) {
        count(2)?;
        out.need(Level::I386);
        return match &ops[1] {
            Op::Reg(r) => {
                let size = agreed(&ops[0], &ops[1], &name)?;
                with_modrm(&[0x0F, *opcode], r.num, &ops[0], size, bits, Vec::new(), line, &mut out)
            }
            Op::Imm { text, .. } => {
                let size = op_size(&ops[0]).ok_or_else(|| crate::i18n::tf!("{name} needs a size: say WORD or DWORD"))?;
                let v = line.eval(text)?;
                with_modrm(&[0x0F, 0xBA], *field, &ops[0], size, bits, vec![v as u8], line, &mut out)
            }
            _ => Err(crate::i18n::tf!("{name} tests a bit given by a register or a number")),
        };
    }

    // Jcc.
    if let Some(cc) = mnemonic.strip_prefix('j').and_then(condition) {
        count(1)?;
        let Op::Imm { text, .. } = &ops[0] else {
            return Err(crate::i18n::tf!("{name} jumps to a label"));
        };
        let target = line.eval(text)?;
        let fits_short = line.known(text).is_some_and(|t| fits_i8(t - (line.here() + 2)));
        if distance != Distance::Near && (fits_short || distance == Distance::Short) {
            if line.checking() && !fits_i8(target - (line.here() + 2)) {
                return Err(crate::i18n::tf!("{text} is {} bytes away, out of a short jump's reach", target - (line.here() + 2)));
            }
            let mut rest = vec![0x70 + cc];
            rest.extend(relative(line, target, 2, 1));
            return Ok(finish(Vec::new(), rest, &mut out));
        }
        // A near conditional jump is the 386's: before it, a short jump over a near one is the way.
        let size = if bits == 32 { 4 } else { 2 };
        let level = Level::from(line.setting("cpu").unwrap_or(Level::Any as i64));
        if level < Level::I386 && line.checking() {
            return Err(crate::i18n::tf!("{text} is {} bytes away, out of a short jump's reach, and near conditional jumps need a 386: jump over a JMP with the opposite condition", target - (line.here() + 2)));
        }
        out.need(Level::I386);
        let mut rest = vec![0x0F, 0x80 + cc];
        rest.extend(relative(line, target, 2 + i64::from(size), size));
        return Ok(finish(Vec::new(), rest, &mut out));
    }

    // SETcc.
    if let Some(cc) = mnemonic.strip_prefix("set").and_then(condition) {
        count(1)?;
        out.need(Level::I386);
        if op_size(&ops[0]).is_some_and(|s| s != 1) {
            return Err(crate::i18n::tf!("{name} sets a byte"));
        }
        return with_modrm(&[0x0F, 0x90 + cc], 0, &ops[0], 1, bits, Vec::new(), line, &mut out);
    }

    match mnemonic {
        "mov" => {
            count(2)?;
            let (a, b) = (&ops[0], &ops[1]);
            match (a, b) {
                // The accumulator and a plain address: A0 to A3.
                (Op::Reg(r), Op::Mem(m)) | (Op::Mem(m), Op::Reg(r)) if r.num == 0 && m.base.is_none() && m.index.is_none() && m.size.is_none_or(|s| s == r.size) => {
                    let to_memory = matches!(a, Op::Mem(_));
                    let mut prefixes = Vec::new();
                    if r.size > 1 {
                        size_prefix(r.size, bits, &mut out, &mut prefixes);
                    }
                    if let Some(s) = m.seg {
                        prefixes.push(SEG_PREFIX[usize::from(s)]);
                    }
                    let addr32 = bits == 32;
                    let v = m.disp.as_ref().map_or(Ok(0), |d| line.eval(d))?;
                    let mut rest = vec![0xA0 + u8::from(r.size > 1) + if to_memory { 2 } else { 0 }];
                    if addr32 {
                        rest.extend((v as u32).to_le_bytes());
                    } else {
                        rest.extend((v as u16).to_le_bytes());
                    }
                    Ok(finish(prefixes, rest, &mut out))
                }
                (Op::Reg(_) | Op::Mem(_), Op::Reg(r)) => {
                    let size = agreed(a, b, &name)?;
                    with_modrm(&[0x88 + u8::from(size > 1)], r.num, a, size, bits, Vec::new(), line, &mut out)
                }
                (Op::Reg(r), Op::Mem(_)) => {
                    let size = agreed(a, b, &name)?;
                    with_modrm(&[0x8A + u8::from(size > 1)], r.num, b, size, bits, Vec::new(), line, &mut out)
                }
                (Op::Reg(_) | Op::Mem(_), Op::Seg(s)) => {
                    if *s >= 4 {
                        out.need(Level::I386);
                    }
                    let size = match a {
                        Op::Reg(r) => r.size,
                        _ => 2,
                    };
                    with_modrm(&[0x8C], *s, a, size, bits, Vec::new(), line, &mut out)
                }
                (Op::Seg(s), Op::Reg(_) | Op::Mem(_)) => {
                    if *s == 1 {
                        return Err(crate::i18n::tr("CS can't be moved into: a far jump sets it").into());
                    }
                    if *s >= 4 {
                        out.need(Level::I386);
                    }
                    // NASM writes MOV DS,AX with no operand-size prefix, whatever the mode.
                    with_modrm(&[0x8E], *s, b, 0, bits, Vec::new(), line, &mut out)
                }
                (Op::Reg(r), Op::Imm { text, .. }) => {
                    let mut prefixes = Vec::new();
                    if r.size > 1 {
                        size_prefix(r.size, bits, &mut out, &mut prefixes);
                    }
                    let mut rest = vec![if r.size == 1 { 0xB0 } else { 0xB8 } + r.num];
                    rest.extend(imm_bytes(line, text, r.size)?);
                    Ok(finish(prefixes, rest, &mut out))
                }
                (Op::Mem(_), Op::Imm { text, size: isize, .. }) => {
                    let size = op_size(a).or(*isize).ok_or_else(|| crate::i18n::tf!("{name} needs a size: say BYTE, WORD or DWORD"))?;
                    let imm = imm_bytes(line, text, size)?;
                    with_modrm(&[0xC6 + u8::from(size > 1)], 0, a, size, bits, imm, line, &mut out)
                }
                (Op::Reg(r), Op::Control(n) | Op::Debug(n) | Op::Test(n)) | (Op::Control(n) | Op::Debug(n) | Op::Test(n), Op::Reg(r)) => {
                    out.need(Level::I386);
                    if r.size != 4 {
                        return Err(crate::i18n::tr("the 386's control, debug and test registers move to and from 32-bit registers").into());
                    }
                    let to_special = !matches!(a, Op::Reg(_));
                    let special = if matches!(a, Op::Reg(_)) { b } else { a };
                    let base = match special {
                        Op::Control(_) => 0x20,
                        Op::Debug(_) => 0x21,
                        _ => 0x24,
                    } + if to_special { 2 } else { 0 };
                    Ok(finish(Vec::new(), vec![0x0F, base, 0xC0 | (n << 3) | r.num], &mut out))
                }
                _ => Err(crate::i18n::tf!("MOV can't move {} to {}", texts[1], texts[0])),
            }
        }
        "movzx" | "movsx" => {
            count(2)?;
            out.need(Level::I386);
            let Op::Reg(r) = &ops[0] else {
                return Err(crate::i18n::tf!("{name} puts its value in a register"));
            };
            let from = op_size(&ops[1]).ok_or_else(|| crate::i18n::tf!("{name} needs to know the size it widens: say BYTE or WORD"))?;
            if from >= r.size {
                return Err(crate::i18n::tf!("{name} widens a smaller value into a bigger register"));
            }
            let opcode = if mnemonic == "movzx" { 0xB6 } else { 0xBE } + u8::from(from == 2);
            with_modrm(&[0x0F, opcode], r.num, &ops[1], r.size, bits, Vec::new(), line, &mut out)
        }
        "lea" | "lds" | "les" | "lss" | "lfs" | "lgs" | "bound" | "lar" | "lsl" | "bsf" | "bsr" => {
            count(2)?;
            let Op::Reg(r) = &ops[0] else {
                return Err(crate::i18n::tf!("{name} puts its value in a register"));
            };
            let memory_only = matches!(mnemonic, "lea" | "lds" | "les" | "lss" | "lfs" | "lgs" | "bound");
            if memory_only && !matches!(ops[1], Op::Mem(_)) {
                return Err(crate::i18n::tf!("{name} takes memory"));
            }
            let opcode: &[u8] = match mnemonic {
                "lea" => &[0x8D],
                "lds" => &[0xC5],
                "les" => &[0xC4],
                "lss" => &[0x0F, 0xB2],
                "lfs" => &[0x0F, 0xB4],
                "lgs" => &[0x0F, 0xB5],
                "bound" => &[0x62],
                "lar" => &[0x0F, 0x02],
                "lsl" => &[0x0F, 0x03],
                "bsf" => &[0x0F, 0xBC],
                _ => &[0x0F, 0xBD],
            };
            out.need(match mnemonic {
                "lss" | "lfs" | "lgs" | "bsf" | "bsr" => Level::I386,
                "lar" | "lsl" => Level::I286,
                "bound" => Level::I186,
                _ => Level::I8086,
            });
            // The operand's own size doesn't matter to LEA and its kin: the register's does.
            let mut source = ops[1].clone();
            if let Op::Mem(m) = &mut source {
                m.size = None;
            }
            with_modrm(opcode, r.num, &source, r.size, bits, Vec::new(), line, &mut out)
        }
        "xchg" => {
            count(2)?;
            let (a, b) = (&ops[0], &ops[1]);
            let size = agreed(a, b, &name)?;
            match (a, b) {
                // The accumulator and another register: 90+r.
                (Op::Reg(x), Op::Reg(y)) if size > 1 && (x.num == 0 || y.num == 0) => {
                    let other = if x.num == 0 { y.num } else { x.num };
                    let mut prefixes = Vec::new();
                    size_prefix(size, bits, &mut out, &mut prefixes);
                    Ok(finish(prefixes, vec![0x90 + other], &mut out))
                }
                (Op::Reg(_) | Op::Mem(_), Op::Reg(r)) => with_modrm(&[0x86 + u8::from(size > 1)], r.num, a, size, bits, Vec::new(), line, &mut out),
                (Op::Reg(r), Op::Mem(_)) => with_modrm(&[0x86 + u8::from(size > 1)], r.num, b, size, bits, Vec::new(), line, &mut out),
                _ => Err(crate::i18n::tr("XCHG swaps a register with a register or memory").into()),
            }
        }
        "test" => {
            count(2)?;
            let (a, b) = (&ops[0], &ops[1]);
            match (a, b) {
                (Op::Reg(_) | Op::Mem(_), Op::Reg(r)) => {
                    let size = agreed(a, b, &name)?;
                    with_modrm(&[0x84 + u8::from(size > 1)], r.num, a, size, bits, Vec::new(), line, &mut out)
                }
                (Op::Reg(r), Op::Mem(_)) => {
                    let size = agreed(a, b, &name)?;
                    with_modrm(&[0x84 + u8::from(size > 1)], r.num, b, size, bits, Vec::new(), line, &mut out)
                }
                (Op::Reg(_) | Op::Mem(_), Op::Imm { text, .. }) => {
                    let size = op_size(a).ok_or_else(|| crate::i18n::tf!("{name} needs a size: say BYTE, WORD or DWORD"))?;
                    let imm = imm_bytes(line, text, size)?;
                    if matches!(a, Op::Reg(Reg { num: 0, .. })) {
                        let mut prefixes = Vec::new();
                        if size > 1 {
                            size_prefix(size, bits, &mut out, &mut prefixes);
                        }
                        let mut rest = vec![0xA8 + u8::from(size > 1)];
                        rest.extend(imm);
                        return Ok(finish(prefixes, rest, &mut out));
                    }
                    with_modrm(&[0xF6 + u8::from(size > 1)], 0, a, size, bits, imm, line, &mut out)
                }
                _ => Err(crate::i18n::tr("TEST compares a register or memory with a register or a number").into()),
            }
        }
        "inc" | "dec" => {
            count(1)?;
            let field = u8::from(mnemonic == "dec");
            match &ops[0] {
                Op::Reg(r) if r.size > 1 => {
                    let mut prefixes = Vec::new();
                    size_prefix(r.size, bits, &mut out, &mut prefixes);
                    Ok(finish(prefixes, vec![0x40 + field * 8 + r.num], &mut out))
                }
                op => {
                    let size = op_size(op).ok_or_else(|| crate::i18n::tf!("{name} needs a size: say BYTE, WORD or DWORD"))?;
                    with_modrm(&[0xFE + u8::from(size > 1)], field, op, size, bits, Vec::new(), line, &mut out)
                }
            }
        }
        "push" | "pop" => {
            count(1)?;
            let push = mnemonic == "push";
            match &ops[0] {
                Op::Reg(r) if r.size > 1 => {
                    let mut prefixes = Vec::new();
                    size_prefix(r.size, bits, &mut out, &mut prefixes);
                    Ok(finish(prefixes, vec![if push { 0x50 } else { 0x58 } + r.num], &mut out))
                }
                Op::Seg(s) => {
                    let bytes: Vec<u8> = match (s, push) {
                        (0, true) => vec![0x06],
                        (0, false) => vec![0x07],
                        (1, true) => vec![0x0E],
                        (1, false) => return Err(crate::i18n::tr("POP CS isn't an instruction: a far jump or return sets CS").into()),
                        (2, true) => vec![0x16],
                        (2, false) => vec![0x17],
                        (3, true) => vec![0x1E],
                        (3, false) => vec![0x1F],
                        (4, true) => vec![0x0F, 0xA0],
                        (4, false) => vec![0x0F, 0xA1],
                        (_, true) => vec![0x0F, 0xA8],
                        (_, false) => vec![0x0F, 0xA9],
                    };
                    if *s >= 4 {
                        out.need(Level::I386);
                    }
                    Ok(finish(Vec::new(), bytes, &mut out))
                }
                Op::Imm { text, size: isize, .. } if push => {
                    out.need(Level::I186);
                    let size = isize.filter(|s| *s > 1).unwrap_or(if bits == 32 { 4 } else { 2 });
                    let mut prefixes = Vec::new();
                    size_prefix(size, bits, &mut out, &mut prefixes);
                    let byte = *isize == Some(1) || line.known(text).is_some_and(fits_i8);
                    let rest = if byte {
                        vec![0x6A, line.eval(text)? as u8]
                    } else {
                        let mut r = vec![0x68];
                        r.extend(imm_bytes(line, text, size)?);
                        r
                    };
                    Ok(finish(prefixes, rest, &mut out))
                }
                op @ Op::Mem(_) => {
                    let size = op_size(op).unwrap_or(if bits == 32 { 4 } else { 2 });
                    if size == 1 {
                        return Err(crate::i18n::tf!("{name} takes a word or a dword, not a byte"));
                    }
                    with_modrm(if push { &[0xFF] } else { &[0x8F] }, if push { 6 } else { 0 }, op, size, bits, Vec::new(), line, &mut out)
                }
                _ => Err(crate::i18n::tf!("{name} takes a 16- or 32-bit register, a segment register, memory, or (to push) a number")),
            }
        }
        "imul" => match ops.len() {
            1 => {
                let size = op_size(&ops[0]).ok_or_else(|| crate::i18n::tf!("{name} needs a size: say BYTE, WORD or DWORD"))?;
                with_modrm(&[0xF6 + u8::from(size > 1)], 5, &ops[0], size, bits, Vec::new(), line, &mut out)
            }
            2 | 3 => {
                let Op::Reg(r) = &ops[0] else {
                    return Err(crate::i18n::tr("IMUL with more than one operand puts its product in a register").into());
                };
                if r.size == 1 {
                    return Err(crate::i18n::tr("IMUL with more than one operand works on words and dwords").into());
                }
                // IMUL reg,imm is IMUL reg,reg,imm.
                let (source, imm) = match (ops.len(), &ops[1]) {
                    (2, Op::Imm { text, .. }) => (Op::Reg(*r), Some(text.clone())),
                    (2, other) => (other.clone(), None),
                    (_, other) => match &ops[2] {
                        Op::Imm { text, .. } => (other.clone(), Some(text.clone())),
                        _ => return Err(crate::i18n::tr("IMUL's third operand is a number").into()),
                    },
                };
                match imm {
                    None => {
                        out.need(Level::I386);
                        with_modrm(&[0x0F, 0xAF], r.num, &source, r.size, bits, Vec::new(), line, &mut out)
                    }
                    Some(text) => {
                        out.need(Level::I186);
                        if line.known(&text).is_some_and(fits_i8) {
                            let v = line.eval(&text)?;
                            with_modrm(&[0x6B], r.num, &source, r.size, bits, vec![v as u8], line, &mut out)
                        } else {
                            let imm = imm_bytes(line, &text, r.size)?;
                            with_modrm(&[0x69], r.num, &source, r.size, bits, imm, line, &mut out)
                        }
                    }
                }
            }
            _ => Err(crate::i18n::tr("IMUL takes one, two or three operands").into()),
        },
        "jmp" | "call" => {
            count(1)?;
            let call = mnemonic == "call";
            match &ops[0] {
                Op::Far { seg, offset } => {
                    let size = if bits == 32 { 4 } else { 2 };
                    let mut rest = vec![if call { 0x9A } else { 0xEA }];
                    rest.extend(imm_bytes(line, offset, size)?);
                    rest.extend(imm_bytes(line, seg, 2)?);
                    Ok(finish(Vec::new(), rest, &mut out))
                }
                Op::Imm { text, .. } => {
                    let target = line.eval(text)?;
                    if !call && distance != Distance::Near {
                        let fits_short = line.known(text).is_some_and(|t| fits_i8(t - (line.here() + 2)));
                        if fits_short || distance == Distance::Short {
                            if line.checking() && !fits_i8(target - (line.here() + 2)) {
                                return Err(crate::i18n::tf!("{text} is {} bytes away, out of a short jump's reach", target - (line.here() + 2)));
                            }
                            let mut rest = vec![0xEB];
                            rest.extend(relative(line, target, 2, 1));
                            return Ok(finish(Vec::new(), rest, &mut out));
                        }
                    }
                    let size = if bits == 32 { 4 } else { 2 };
                    let mut rest = vec![if call { 0xE8 } else { 0xE9 }];
                    rest.extend(relative(line, target, 1 + i64::from(size), size));
                    Ok(finish(Vec::new(), rest, &mut out))
                }
                op @ (Op::Reg(_) | Op::Mem(_)) => {
                    let far = distance == Distance::Far;
                    let field = match (call, far) {
                        (true, false) => 2,
                        (true, true) => 3,
                        (false, false) => 4,
                        (false, true) => 5,
                    };
                    let size = if far { 0 } else { op_size(op).unwrap_or(if bits == 32 { 4 } else { 2 }) };
                    // A far pointer's size is the offset's: a dword (16:16) or an fword (16:32).
                    let size = match (far, op_size(op)) {
                        (true, Some(6)) => 4,
                        (true, _) => if bits == 32 { 4 } else { 2 },
                        _ => size,
                    };
                    let mut target = op.clone();
                    if let Op::Mem(m) = &mut target {
                        m.size = None;
                    }
                    with_modrm(&[0xFF], field, &target, if size == 1 { 2 } else { size }, bits, Vec::new(), line, &mut out)
                }
                _ => Err(crate::i18n::tf!("{name} goes to a label, a register, memory, or seg:offset")),
            }
        }
        "ret" | "retn" | "retf" => {
            let far = mnemonic == "retf";
            match ops.first() {
                None => Ok(finish(Vec::new(), vec![if far { 0xCB } else { 0xC3 }], &mut out)),
                Some(Op::Imm { text, .. }) => {
                    let mut rest = vec![if far { 0xCA } else { 0xC2 }];
                    rest.extend(imm_bytes(line, text, 2)?);
                    Ok(finish(Vec::new(), rest, &mut out))
                }
                _ => Err(crate::i18n::tf!("{name} takes nothing, or the bytes to take off the stack")),
            }
        }
        "int" => {
            count(1)?;
            let Op::Imm { text, .. } = &ops[0] else {
                return Err(crate::i18n::tr("INT takes the interrupt's number").into());
            };
            let mut rest = vec![0xCD];
            rest.extend(imm_bytes(line, text, 1)?);
            Ok(finish(Vec::new(), rest, &mut out))
        }
        "in" | "out" => {
            count(2)?;
            let (acc, port) = if mnemonic == "in" { (&ops[0], &ops[1]) } else { (&ops[1], &ops[0]) };
            let Op::Reg(r) = acc else {
                return Err(crate::i18n::tf!("{name} uses AL, AX or EAX"));
            };
            if r.num != 0 {
                return Err(crate::i18n::tf!("{name} uses AL, AX or EAX"));
            }
            let wide = u8::from(r.size > 1);
            let out_bit = if mnemonic == "out" { 2 } else { 0 };
            let mut prefixes = Vec::new();
            if r.size > 1 {
                size_prefix(r.size, bits, &mut out, &mut prefixes);
            }
            let rest = match port {
                Op::Reg(Reg { num: 2, size: 2 }) => vec![0xEC + wide + out_bit],
                Op::Imm { text, .. } => {
                    let mut r = vec![0xE4 + wide + out_bit];
                    r.extend(imm_bytes(line, text, 1)?);
                    r
                }
                _ => return Err(crate::i18n::tf!("{name}'s port is DX or a number up to 255")),
            };
            Ok(finish(prefixes, rest, &mut out))
        }
        "loop" | "loope" | "loopz" | "loopne" | "loopnz" | "jcxz" | "jecxz" => {
            count(1)?;
            let Op::Imm { text, .. } = &ops[0] else {
                return Err(crate::i18n::tf!("{name} jumps to a label"));
            };
            let opcode = match mnemonic {
                "loop" => 0xE2,
                "loope" | "loopz" => 0xE1,
                "loopne" | "loopnz" => 0xE0,
                _ => 0xE3,
            };
            let mut prefixes = Vec::new();
            match mnemonic {
                "jcxz" if bits == 32 => prefixes.push(0x67),
                "jecxz" => {
                    out.need(Level::I386);
                    if bits == 16 {
                        prefixes.push(0x67);
                    }
                }
                _ => {}
            }
            let target = line.eval(text)?;
            let length = 2 + prefixes.len() as i64;
            if line.checking() && !fits_i8(target - (line.here() + length)) {
                return Err(crate::i18n::tf!("{text} is {} bytes away, out of {name}'s reach of 128", target - (line.here() + length)));
            }
            let mut rest = vec![opcode];
            rest.extend(relative(line, target, length, 1));
            Ok(finish(prefixes, rest, &mut out))
        }
        "enter" => {
            count(2)?;
            out.need(Level::I186);
            let (Op::Imm { text: a, .. }, Op::Imm { text: b, .. }) = (&ops[0], &ops[1]) else {
                return Err(crate::i18n::tr("ENTER takes the stack's room and the nesting level").into());
            };
            let mut rest = vec![0xC8];
            rest.extend(imm_bytes(line, a, 2)?);
            rest.extend(imm_bytes(line, b, 1)?);
            Ok(finish(Vec::new(), rest, &mut out))
        }
        "aam" | "aad" => {
            let base = match ops.first() {
                None => 10,
                Some(Op::Imm { text, .. }) => line.eval(text)?,
                _ => return Err(crate::i18n::tf!("{name} takes nothing, or the number base")),
            };
            Ok(finish(Vec::new(), vec![if mnemonic == "aam" { 0xD4 } else { 0xD5 }, base as u8], &mut out))
        }
        "arpl" => {
            count(2)?;
            out.need(Level::I286);
            let Op::Reg(r) = &ops[1] else {
                return Err(crate::i18n::tr("ARPL takes a 16-bit register or memory, then a 16-bit register").into());
            };
            with_modrm(&[0x63], r.num, &ops[0], 0, bits, Vec::new(), line, &mut out)
        }
        "lmsw" => {
            count(1)?;
            out.need(Level::I286);
            with_modrm(&[0x0F, 0x01], 6, &ops[0], 0, bits, Vec::new(), line, &mut out)
        }
        "invlpg" => {
            count(1)?;
            out.need(Level::I486);
            with_modrm(&[0x0F, 0x01], 7, &ops[0], 0, bits, Vec::new(), line, &mut out)
        }
        "shld" | "shrd" => {
            count(3)?;
            out.need(Level::I386);
            let Op::Reg(r) = &ops[1] else {
                return Err(crate::i18n::tf!("{name}'s second operand is a register"));
            };
            let size = agreed(&ops[0], &ops[1], &name)?;
            let base = if mnemonic == "shld" { 0xA4 } else { 0xAC };
            match &ops[2] {
                Op::Reg(Reg { num: 1, size: 1 }) => with_modrm(&[0x0F, base + 1], r.num, &ops[0], size, bits, Vec::new(), line, &mut out),
                Op::Imm { text, .. } => {
                    let v = line.eval(text)?;
                    with_modrm(&[0x0F, base], r.num, &ops[0], size, bits, vec![v as u8], line, &mut out)
                }
                _ => Err(crate::i18n::tf!("{name} shifts by CL or a number")),
            }
        }
        "bswap" => {
            count(1)?;
            out.need(Level::I486);
            match &ops[0] {
                Op::Reg(r) if r.size == 4 => {
                    let mut prefixes = Vec::new();
                    size_prefix(4, bits, &mut out, &mut prefixes);
                    Ok(finish(prefixes, vec![0x0F, 0xC8 + r.num], &mut out))
                }
                _ => Err(crate::i18n::tr("BSWAP turns a 32-bit register's bytes round").into()),
            }
        }
        "xadd" | "cmpxchg" => {
            count(2)?;
            out.need(Level::I486);
            let Op::Reg(r) = &ops[1] else {
                return Err(crate::i18n::tf!("{name}'s second operand is a register"));
            };
            let size = agreed(&ops[0], &ops[1], &name)?;
            let base = if mnemonic == "xadd" { 0xC0 } else { 0xB0 };
            with_modrm(&[0x0F, base + u8::from(size > 1)], r.num, &ops[0], size, bits, Vec::new(), line, &mut out)
        }
        _ => table::encode(mnemonic, &ops, line, bits),
    }
}

mod table;

#[cfg(test)]
mod tests;
