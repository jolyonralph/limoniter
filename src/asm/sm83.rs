//! The Sharp SM83, the Game Boy's processor: a Z80's cousin, without IX and IY, the shadow registers, IN and OUT,
//! with LDH for the hardware's page at $FF00, SWAP, and loads that move HL on (LD A,[HL+]). Written as RGBDS writes
//! it: memory in square brackets, SECTION to say where code goes, DEF X EQU 1 for constants and `0123 for a row
//! of a tile. Each instruction's forms are listed once, and both assembling and disassembling use the list.

use super::{Isa, Line, split_operands};
use crate::disasm::Decoded;
use std::sync::OnceLock;

pub struct Sm83;

/// What an instruction's operand is, in one of its forms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// A word written as it is: a register, a condition, [HL+] and the like.
    Word(&'static str),
    /// A byte.
    N8,
    /// A word.
    N16,
    /// [address], a word.
    Memory,
    /// [address] in the page at $FF00, for LDH: a byte.
    High,
    /// A signed byte added to SP.
    Offset,
    /// SP+offset, for LD HL,SP+e8.
    SpOffset,
    /// Where JR goes, a byte from -128 to 127 after it.
    Relative,
    /// A bit number, 0 to 7, part of the opcode.
    Bit(u8),
    /// RST's address, part of the opcode.
    Vector(u8),
}

/// One form of an instruction: its mnemonic, its operands, and its opcode (after $CB for those with one).
#[derive(Clone, Debug)]
struct Form {
    mnemonic: &'static str,
    operands: Vec<Shape>,
    prefixed: bool,
    opcode: u8,
}

const R: [&str; 8] = ["b", "c", "d", "e", "h", "l", "[hl]", "a"];
const RP: [&str; 4] = ["bc", "de", "hl", "sp"];
const RP2: [&str; 4] = ["bc", "de", "hl", "af"];
const CC: [&str; 4] = ["nz", "z", "nc", "c"];
const ALU: [&str; 8] = ["add", "adc", "sub", "sbc", "and", "xor", "or", "cp"];
const ROT: [&str; 8] = ["rlc", "rrc", "rl", "rr", "sla", "sra", "swap", "srl"];

/// Every form, worked out from the opcode's bits as the Z80's are, with the SM83's own in place of the Z80's.
fn forms() -> &'static [Form] {
    static FORMS: OnceLock<Vec<Form>> = OnceLock::new();
    FORMS.get_or_init(|| {
        use Shape::*;
        let mut list = Vec::new();
        let mut add = |mnemonic: &'static str, operands: Vec<Shape>, opcode: u8| list.push(Form { mnemonic, operands, prefixed: false, opcode });
        add("nop", vec![], 0x00);
        add("ld", vec![Memory, Word("sp")], 0x08);
        add("stop", vec![], 0x10);
        add("jr", vec![Relative], 0x18);
        for (n, cc) in CC.iter().enumerate() {
            add("jr", vec![Word(cc), Relative], 0x20 + 8 * n as u8);
            add("ret", vec![Word(cc)], 0xC0 + 8 * n as u8);
            add("jp", vec![Word(cc), N16], 0xC2 + 8 * n as u8);
            add("call", vec![Word(cc), N16], 0xC4 + 8 * n as u8);
        }
        for (p, pair) in RP.iter().enumerate() {
            let p = p as u8;
            add("ld", vec![Word(pair), N16], 0x01 + 16 * p);
            add("add", vec![Word("hl"), Word(pair)], 0x09 + 16 * p);
            add("inc", vec![Word(pair)], 0x03 + 16 * p);
            add("dec", vec![Word(pair)], 0x0B + 16 * p);
        }
        for (p, at) in ["[bc]", "[de]", "[hl+]", "[hl-]"].iter().enumerate() {
            add("ld", vec![Word(at), Word("a")], 0x02 + 16 * p as u8);
            add("ld", vec![Word("a"), Word(at)], 0x0A + 16 * p as u8);
        }
        for (y, r) in R.iter().enumerate() {
            let y = y as u8;
            add("inc", vec![Word(r)], 0x04 + 8 * y);
            add("dec", vec![Word(r)], 0x05 + 8 * y);
            add("ld", vec![Word(r), N8], 0x06 + 8 * y);
        }
        for (y, name) in ["rlca", "rrca", "rla", "rra", "daa", "cpl", "scf", "ccf"].iter().enumerate() {
            add(name, vec![], 0x07 + 8 * y as u8);
        }
        for (y, to) in R.iter().enumerate() {
            for (z, from) in R.iter().enumerate() {
                let opcode = 0x40 + 8 * y as u8 + z as u8;
                if opcode == 0x76 {
                    add("halt", vec![], 0x76);
                } else {
                    add("ld", vec![Word(to), Word(from)], opcode);
                }
            }
        }
        for (y, name) in ALU.iter().enumerate() {
            for (z, r) in R.iter().enumerate() {
                add(name, vec![Word("a"), Word(r)], 0x80 + 8 * y as u8 + z as u8);
            }
            add(name, vec![Word("a"), N8], 0xC6 + 8 * y as u8);
        }
        for (p, pair) in RP2.iter().enumerate() {
            add("pop", vec![Word(pair)], 0xC1 + 16 * p as u8);
            add("push", vec![Word(pair)], 0xC5 + 16 * p as u8);
        }
        for n in 0..8u8 {
            add("rst", vec![Vector(n * 8)], 0xC7 + 8 * n);
        }
        add("jp", vec![N16], 0xC3);
        add("ret", vec![], 0xC9);
        add("call", vec![N16], 0xCD);
        add("reti", vec![], 0xD9);
        add("ldh", vec![High, Word("a")], 0xE0);
        add("ldh", vec![Word("[c]"), Word("a")], 0xE2);
        add("add", vec![Word("sp"), Offset], 0xE8);
        add("jp", vec![Word("hl")], 0xE9);
        add("ld", vec![Memory, Word("a")], 0xEA);
        add("ldh", vec![Word("a"), High], 0xF0);
        add("ldh", vec![Word("a"), Word("[c]")], 0xF2);
        add("di", vec![], 0xF3);
        add("ld", vec![Word("hl"), SpOffset], 0xF8);
        add("ld", vec![Word("sp"), Word("hl")], 0xF9);
        add("ld", vec![Word("a"), Memory], 0xFA);
        add("ei", vec![], 0xFB);
        // After $CB: the shifts and rotations, then BIT, RES and SET.
        for (y, name) in ROT.iter().enumerate() {
            for (z, r) in R.iter().enumerate() {
                list.push(Form { mnemonic: name, operands: vec![Word(r)], prefixed: true, opcode: 8 * y as u8 + z as u8 });
            }
        }
        for (x, name) in ["bit", "res", "set"].iter().enumerate() {
            for bit in 0..8u8 {
                for (z, r) in R.iter().enumerate() {
                    list.push(Form { mnemonic: name, operands: vec![Bit(bit), Word(r)], prefixed: true, opcode: 0x40 * (x as u8 + 1) + 8 * bit + z as u8 });
                }
            }
        }
        list
    })
}

/// The words an operand can be, and what each stands for: [HLI] is [HL+], and [$FF00+C] is [C].
fn word_named(text: &str) -> Option<&'static str> {
    let squeezed: String = text.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    // Parentheses as well as brackets, as older Game Boy assemblers wrote memory, for the registers alone.
    let inner = squeezed.strip_prefix('[').and_then(|t| t.strip_suffix(']')).or_else(|| squeezed.strip_prefix('(').and_then(|t| t.strip_suffix(')')));
    if let Some(inner) = inner {
        return Some(match inner {
            "hl" => "[hl]",
            "hl+" | "hli" => "[hl+]",
            "hl-" | "hld" => "[hl-]",
            "bc" => "[bc]",
            "de" => "[de]",
            "c" | "$ff00+c" | "0xff00+c" | "ff00h+c" => "[c]",
            _ => return None,
        });
    }
    ["a", "b", "c", "d", "e", "h", "l", "af", "bc", "de", "hl", "sp", "nz", "z", "nc"].into_iter().find(|w| *w == squeezed)
}

/// An operand as written.
#[derive(Clone, Debug, PartialEq)]
enum Written {
    Word(&'static str),
    /// [expression].
    Memory(String),
    /// SP+expression, or SP-expression: the offset, with its sign.
    SpOffset(String),
    Value(String),
}

fn parse(text: &str) -> Written {
    let text = text.trim();
    if let Some(word) = word_named(text) {
        return Written::Word(word);
    }
    if let Some(inner) = text.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        // [SP+n], as some write LD HL,SP+n.
        if let Written::SpOffset(offset) = parse(inner) {
            return Written::SpOffset(offset);
        }
        return Written::Memory(inner.trim().to_string());
    }
    let lower = text.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("sp").filter(|r| r.trim_start().starts_with(['+', '-'])) {
        let rest = rest.trim_start();
        let offset = &text[text.len() - rest.len()..];
        let offset = offset.strip_prefix('+').unwrap_or(offset);
        return Written::SpOffset(offset.trim().to_string());
    }
    Written::Value(text.to_string())
}

/// Other names some assemblers use, made into RGBDS's: LDI and LDD are LD with [HL+] and [HL-], LDIO is LDH, and
/// LDHL SP,n is LD HL,SP+n.
fn canonical(mnemonic: &str, operands: Vec<Written>) -> (&str, Vec<Written>) {
    let moved = |operands: Vec<Written>, to: &'static str| operands.into_iter().map(|o| if o == Written::Word("[hl]") { Written::Word(to) } else { o }).collect();
    match mnemonic {
        "ldi" => ("ld", moved(operands, "[hl+]")),
        "ldd" => ("ld", moved(operands, "[hl-]")),
        "ldio" => ("ldh", operands),
        "ldhl" => match &operands[..] {
            [Written::Word("sp"), Written::Value(offset)] => ("ld", vec![Written::Word("hl"), Written::SpOffset(offset.clone())]),
            _ => ("ld", operands),
        },
        // JP [HL] is how the Z80's JP (HL) is often written: it jumps to HL, not to what HL points at.
        "jp" if operands == [Written::Word("[hl]")] => ("jp", vec![Written::Word("hl")]),
        // SUB B for SUB A,B, and so on: A is the only register they work on.
        _ if ALU.contains(&mnemonic) && operands.len() == 1 && !(mnemonic == "add" && matches!(operands[0], Written::Word("sp" | "hl"))) => {
            let mut with_a = vec![Written::Word("a")];
            with_a.extend(operands);
            (mnemonic, with_a)
        }
        // LD [C],A and LD A,[$FF00+C] are LDH's.
        "ld" if operands.contains(&Written::Word("[c]")) => ("ldh", operands),
        _ => (mnemonic, operands),
    }
}

const OTHER_NAMES: [&str; 4] = ["ldi", "ldd", "ldio", "ldhl"];

impl Isa for Sm83 {
    fn name(&self) -> &'static str {
        "SM83"
    }

    fn is_instruction(&self, word: &str) -> bool {
        forms().iter().any(|f| f.mnemonic == word) || OTHER_NAMES.contains(&word)
    }

    fn local_prefixes(&self) -> &'static [char] {
        &['.']
    }

    fn cycles(&self, bytes: &[u8]) -> Option<String> {
        cycles(bytes)
    }

    fn is_directive(&self, word: &str) -> bool {
        matches!(word, "section" | "endsection" | "export" | "global" | "purge" | "elif" | "def" | "redef")
    }

    fn directive(&self, word: &str, operands: &str, line: &mut Line) -> Option<Result<Vec<u8>, String>> {
        match word {
            "section" => Some(section(operands, line).map(|()| Vec::new())),
            // Labels are all known to one source here, so exporting them changes nothing.
            "endsection" | "export" | "global" | "purge" => Some(Ok(Vec::new())),
            // DW and DB with `0123 rows of a tile.
            "dw" | "db" if operands.contains('`') => Some(tile_rows(word == "dw", operands, line)),
            _ => None,
        }
    }

    fn translate(&self, line: &str) -> Option<String> {
        translate(line)
    }

    fn encode(&self, mnemonic: &str, operands: &str, line: &mut Line) -> Result<Vec<u8>, String> {
        let written: Vec<Written> = split_operands(operands).iter().map(|o| parse(o)).collect();
        let (mnemonic, written) = canonical(mnemonic, written);
        let upper = mnemonic.to_ascii_uppercase();
        // STOP is followed by a byte the processor skips; RGBDS writes 0 there.
        if mnemonic == "stop" {
            let after = match written.first() {
                Some(Written::Value(v)) => line.eval(v)? as u8,
                _ => 0,
            };
            return Ok(vec![0x10, after]);
        }
        // BIT, RES, SET and RST: the number is part of the opcode.
        let number = match (mnemonic, written.first()) {
            ("bit" | "res" | "set" | "rst", Some(Written::Value(v))) => Some(line.eval(v)?),
            _ => None,
        };
        let candidates: Vec<&Form> = forms().iter().filter(|f| f.mnemonic == mnemonic && f.operands.len() == written.len()).collect();
        if candidates.is_empty() {
            return Err(if forms().iter().any(|f| f.mnemonic == mnemonic) { crate::i18n::tf!("{upper} can't take {} operands", written.len()) } else { crate::i18n::tf!("{mnemonic} isn't an SM83 instruction") });
        }
        let fits = |shape: &Shape, op: &Written| match (shape, op) {
            (Shape::Word(w), Written::Word(o)) => w == o,
            (Shape::N8 | Shape::N16 | Shape::Offset | Shape::Relative, Written::Value(_)) => true,
            (Shape::Memory | Shape::High, Written::Memory(_)) => true,
            (Shape::SpOffset, Written::SpOffset(_)) => true,
            (Shape::Bit(b), Written::Value(_)) => number == Some(i64::from(*b)),
            (Shape::Vector(v), Written::Value(_)) => number == Some(i64::from(*v)),
            _ => false,
        };
        let Some(form) = candidates.iter().find(|f| f.operands.iter().zip(&written).all(|(s, o)| fits(s, o))) else {
            if let Some(n) = number {
                return Err(match mnemonic {
                    "rst" => crate::i18n::tf!("RST goes to $00, $08, $10 and so on up to $38, and {n} isn't one"),
                    _ if !(0..8).contains(&n) => crate::i18n::tf!("{upper}'s bit is 0 to 7, and this is {n}"),
                    _ => crate::i18n::tf!("{upper} can't take {operands}"),
                });
            }
            return Err(crate::i18n::tf!("{upper} can't take {}", operands.trim()));
        };
        let mut bytes = if form.prefixed { vec![0xCB, form.opcode] } else { vec![form.opcode] };
        for (shape, op) in form.operands.iter().zip(&written) {
            let text = match op {
                Written::Value(t) | Written::Memory(t) | Written::SpOffset(t) => t.as_str(),
                Written::Word(_) => continue,
            };
            match shape {
                Shape::N8 => {
                    let v = line.eval(text)?;
                    if line.checking() && !(-128..=255).contains(&v) {
                        return Err(crate::i18n::tf!("{text} is {v}, which doesn't fit in a byte"));
                    }
                    bytes.push(v as u8);
                }
                Shape::N16 | Shape::Memory => {
                    let v = line.eval(text)?;
                    if line.checking() && !(-32768..=65535).contains(&v) {
                        return Err(crate::i18n::tf!("{text} is {v}, which doesn't fit in 16 bits"));
                    }
                    bytes.extend((v as u16).to_le_bytes());
                }
                Shape::High => {
                    // RGBDS writes the whole address ($FF44); a byte on its own is taken as in that page too.
                    let v = line.eval(text)?;
                    if line.checking() && !(0..=0xFF).contains(&v) && !(0xFF00..=0xFFFF).contains(&v) {
                        return Err(crate::i18n::tf!("LDH reaches $FF00 to $FFFF, and {text} is ${v:X}"));
                    }
                    bytes.push(v as u8);
                }
                Shape::Offset | Shape::SpOffset => {
                    let v = line.eval(text)?;
                    if line.checking() && !(-128..=127).contains(&v) {
                        return Err(crate::i18n::tf!("{text} is {v}, and SP can only be moved by -128 to 127"));
                    }
                    bytes.push(v as u8);
                }
                Shape::Relative => {
                    let target = line.eval(text)?;
                    let offset = target - (line.here() + 2);
                    if line.checking() && !(-128..=127).contains(&offset) {
                        return Err(crate::i18n::tf!("{text} is {offset} bytes away, out of JR's reach (-128 to 127); use JP"));
                    }
                    bytes.push(offset as u8);
                }
                Shape::Word(_) | Shape::Bit(_) | Shape::Vector(_) => {}
            }
        }
        Ok(bytes)
    }
}

/// Where a SECTION's kind of memory is, its size, and whether it holds what's assembled (ROM) or only leaves room.
const KINDS: [(&str, i64, i64, bool); 8] = [
    ("rom0", 0x0000, 0x4000, true),
    ("romx", 0x4000, 0x4000, true),
    ("vram", 0x8000, 0x2000, false),
    ("sram", 0xA000, 0x2000, false),
    ("wram0", 0xC000, 0x1000, false),
    ("wramx", 0xD000, 0x1000, false),
    ("oam", 0xFE00, 0xA0, false),
    ("hram", 0xFF80, 0x7F, false),
];

/// The settings that keep, for each kind of memory, where the next SECTION without an address of its own goes.
const NEXT: [&str; 8] = ["sm83 next rom0", "sm83 next romx", "sm83 next vram", "sm83 next sram", "sm83 next wram0", "sm83 next wramx", "sm83 next oam", "sm83 next hram"];
const CURRENT: &str = "sm83 section kind";
const CURRENT_BANK: &str = "sm83 section bank";
const BANK_NEXT: &str = "sm83 next romx bank";

/// The first byte after the cartridge's header, where code goes when nothing says otherwise.
pub const AFTER_HEADER: i64 = 0x150;

/// SECTION "name", KIND[$address], BANK[n]: what follows goes there. A section without an address follows the last of
/// its kind (ROM0's start after the cartridge's header); one in a ROM bank other than the first is put further on in
/// the ROM, where that bank is, while its labels have the addresses it's seen at.
fn section(operands: &str, line: &mut Line) -> Result<(), String> {
    // Where the section being left got to, for the next of its kind.
    if let Some(kind) = line.setting(CURRENT) {
        let here = line.here();
        line.set_setting(NEXT[kind as usize], here);
        if kind == 1 {
            let bank = line.setting(CURRENT_BANK).unwrap_or(1);
            line.set_setting(BANK_NEXT, bank);
        }
    }
    let items = split_operands(operands);
    let Some(kind_text) = items.get(1) else {
        return Err(crate::i18n::tr("SECTION needs a name and a kind of memory: SECTION \"Main\", ROM0").into());
    };
    let lower = kind_text.to_ascii_lowercase().replace(' ', "");
    let (kind_name, address) = match lower.split_once('[') {
        Some((kind, _)) => (kind.to_string(), Some(kind_text[kind_text.find('[').unwrap_or(0) + 1..kind_text.rfind(']').unwrap_or(kind_text.len())].to_string())),
        None => (lower.clone(), None),
    };
    let Some(index) = KINDS.iter().position(|(name, ..)| *name == kind_name) else {
        return Err(crate::i18n::tf!("{kind_text} isn't a kind of Game Boy memory (ROM0, ROMX, VRAM, SRAM, WRAM0, WRAMX, OAM or HRAM)"));
    };
    let (_, start, size, rom) = KINDS[index];
    // BANK[n], and ALIGN[n], which is left to the program.
    let bank = items.iter().skip(2).find_map(|item| {
        let lower = item.to_ascii_lowercase();
        let rest = lower.trim().strip_prefix("bank")?.trim_start().strip_prefix('[')?;
        Some(item[item.len() - rest.len()..].trim_end().trim_end_matches(']').to_string())
    });
    let bank = match bank {
        Some(text) => line.eval(&text)?,
        None if index == 1 && address.is_none() => line.setting(BANK_NEXT).unwrap_or(1),
        None => 1,
    };
    let logical = match &address {
        Some(text) => line.eval(text)?,
        None => {
            let next = line.setting(NEXT[index]).unwrap_or(if index == 0 { AFTER_HEADER } else { start });
            // A section can't follow into the cartridge's header: it goes after it.
            if index == 0 && (0x104..AFTER_HEADER).contains(&next) { AFTER_HEADER } else { next }
        }
    };
    if line.checking() && !(start..start + size).contains(&logical) {
        return Err(crate::i18n::tf!("${logical:04X} isn't in {} (${start:04X} to ${:04X})", kind_text.trim(), start + size - 1));
    }
    if index == 1 && line.checking() && !(1..512).contains(&bank) {
        return Err(crate::i18n::tf!("ROMX's bank is 1 to 511, and this is {bank}"));
    }
    let physical = if index == 1 { bank * 0x4000 + (logical - 0x4000) } else { logical };
    line.set_setting(CURRENT, index as i64);
    line.set_setting(CURRENT_BANK, bank);
    line.place(physical, logical, !rom);
    Ok(())
}

/// DW `01233210: each row of eight pixels, its colours 0 to 3, as the two bytes the Game Boy keeps it in (the low
/// bits of each pixel first, then the high). With DB, the low byte alone.
fn tile_rows(words: bool, operands: &str, line: &mut Line) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    for item in split_operands(operands) {
        let value = match item.strip_prefix('`') {
            Some(pixels) => {
                if pixels.is_empty() || pixels.len() > 8 || !pixels.chars().all(|c| matches!(c, '0'..='3')) {
                    return Err(crate::i18n::tf!("`{pixels} isn't a row of pixels: up to eight of 0, 1, 2 and 3"));
                }
                let (mut low, mut high) = (0u8, 0u8);
                for c in pixels.chars() {
                    let colour = c as u8 - b'0';
                    low = (low << 1) | (colour & 1);
                    high = (high << 1) | (colour >> 1);
                }
                i64::from(u16::from_le_bytes([low, high]))
            }
            None => line.eval(&item)?,
        };
        if words {
            bytes.extend((value as u16).to_le_bytes());
        } else {
            bytes.push(value as u8);
        }
    }
    Ok(bytes)
}

/// RGBDS's ways that Limoniter's assembler writes differently: DEF X EQU 1 (and DEF X = 1) for X EQU 1, ELIF for
/// ELSEIF, and @ for where it's at. Strings and comments are left as they are.
pub fn translate(line: &str) -> Option<String> {
    let code_end = super::strip_comment(line).len();
    let (code, comment) = line.split_at(code_end);
    let trimmed = code.trim_start();
    let indent = &code[..code.len() - trimmed.len()];
    let mut out = trimmed.to_string();
    let first = trimmed.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    if matches!(first.as_str(), "def" | "redef") {
        out = trimmed[first.len()..].trim_start().to_string();
        // A constant has to start the line to be taken as a label.
        return Some(format!("{}{comment}", at_sign(&out)));
    }
    if first == "elif" {
        out = format!("elseif{}", &trimmed[4..]);
    }
    let changed = at_sign(&out);
    if changed == trimmed && first != "elif" {
        return None;
    }
    Some(format!("{indent}{changed}{comment}"))
}

/// @ on its own, outside quotes, is where it's at, as $ is to Limoniter's assembler.
fn at_sign(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut quote: Option<char> = None;
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' => quote = Some(c),
            None if c == '@' => {
                let before = i.checked_sub(1).map(|j| chars[j]);
                let after = chars.get(i + 1).copied();
                let name = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
                if !name(before) && !name(after) {
                    out.push('$');
                    continue;
                }
            }
            None => {}
        }
        out.push(c);
    }
    out
}

/// An instruction at `pc`, from `at(0)` on, for disassembling: as RGBDS writes it, with {addr} where an address it
/// jumps to, calls or uses goes.
pub fn decode(at: &dyn Fn(usize) -> u8, pc: u16) -> Decoded {
    let op = at(0);
    let (prefixed, opcode) = if op == 0xCB { (true, at(1)) } else { (false, op) };
    let Some(form) = forms().iter().find(|f| f.prefixed == prefixed && f.opcode == opcode) else {
        return Decoded { length: 1, text: format!("db ${op:02X}"), address: None, falls_through: false, target: None };
    };
    let mut length = if prefixed { 2 } else { 1 };
    if form.mnemonic == "stop" {
        return Decoded { length: 2, text: "stop".into(), address: None, falls_through: true, target: None };
    }
    let mut address = None;
    let mut target = None;
    let mut parts = Vec::new();
    for shape in &form.operands {
        let text = match shape {
            Shape::Word(w) => w.to_string(),
            Shape::N8 => {
                length += 1;
                format!("${:02X}", at(length - 1))
            }
            Shape::N16 => {
                let word = u16::from_le_bytes([at(length), at(length + 1)]);
                length += 2;
                address = Some(word);
                if matches!(form.mnemonic, "jp" | "call") {
                    target = Some(word);
                }
                "{addr}".into()
            }
            Shape::Memory => {
                let word = u16::from_le_bytes([at(length), at(length + 1)]);
                length += 2;
                address = Some(word);
                "[{addr}]".into()
            }
            Shape::High => {
                length += 1;
                match hardware_name(0xFF00 | u16::from(at(length - 1))) {
                    Some(name) => format!("[{name}]"),
                    None => format!("[$FF{:02X}]", at(length - 1)),
                }
            }
            Shape::Offset | Shape::SpOffset => {
                length += 1;
                let v = at(length - 1) as i8;
                let sign = if v < 0 { "-" } else { "+" };
                let text = format!("${:02X}", v.unsigned_abs());
                if *shape == Shape::SpOffset { format!("sp{sign}{text}") } else if v < 0 { format!("-{text}") } else { text }
            }
            Shape::Relative => {
                length += 1;
                let t = pc.wrapping_add(2).wrapping_add(at(1) as i8 as u16);
                address = Some(t);
                target = Some(t);
                "{addr}".into()
            }
            Shape::Bit(b) => b.to_string(),
            Shape::Vector(v) => {
                target = Some(u16::from(*v));
                format!("${v:02X}")
            }
        };
        parts.push(text);
    }
    let conditional = form.operands.first().is_some_and(|s| matches!(s, Shape::Word("nz" | "z" | "nc" | "c")) && matches!(form.mnemonic, "jr" | "jp" | "call" | "ret"));
    let stops = matches!(form.mnemonic, "jr" | "jp" | "ret" | "reti") && !conditional;
    let text = if parts.is_empty() { form.mnemonic.to_string() } else { format!("{} {}", form.mnemonic, parts.join(",")) };
    // JP HL goes somewhere only known as it runs, so it has no target; RST's is its vector.
    Decoded { length, text, address, falls_through: !stops, target }
}

/// The Game Boy's hardware registers in the page LDH reaches, as hardware.inc names them, for a disassembly's LDHs.
pub const HARDWARE: [(u16, &str); 30] = [
    (0xFF00, "rP1"),
    (0xFF01, "rSB"),
    (0xFF02, "rSC"),
    (0xFF04, "rDIV"),
    (0xFF05, "rTIMA"),
    (0xFF06, "rTMA"),
    (0xFF07, "rTAC"),
    (0xFF0F, "rIF"),
    (0xFF10, "rNR10"),
    (0xFF11, "rNR11"),
    (0xFF12, "rNR12"),
    (0xFF13, "rNR13"),
    (0xFF14, "rNR14"),
    (0xFF24, "rNR50"),
    (0xFF25, "rNR51"),
    (0xFF26, "rNR52"),
    (0xFF40, "rLCDC"),
    (0xFF41, "rSTAT"),
    (0xFF42, "rSCY"),
    (0xFF43, "rSCX"),
    (0xFF44, "rLY"),
    (0xFF45, "rLYC"),
    (0xFF46, "rDMA"),
    (0xFF47, "rBGP"),
    (0xFF48, "rOBP0"),
    (0xFF49, "rOBP1"),
    (0xFF4A, "rWY"),
    (0xFF4B, "rWX"),
    (0xFF4D, "rKEY1"),
    (0xFFFF, "rIE"),
];

pub fn hardware_name(address: u16) -> Option<&'static str> {
    HARDWARE.iter().find(|(a, _)| *a == address).map(|(_, n)| *n)
}

/// How many clock cycles (at 4.19 MHz, four to each of the processor's own) an instruction takes: "8", or "12/8"
/// for a conditional one, taken or not.
pub fn cycles(bytes: &[u8]) -> Option<String> {
    let op = *bytes.first()?;
    if op == 0xCB {
        let cb = *bytes.get(1)?;
        let memory = cb & 7 == 6;
        let n = match (cb >> 6, memory) {
            (_, false) => 8,
            (1, true) => 12,
            (_, true) => 16,
        };
        return Some(n.to_string());
    }
    let form = forms().iter().find(|f| !f.prefixed && f.opcode == op)?;
    let memory_operand = form.operands.iter().any(|s| matches!(s, Shape::Word("[hl]" | "[hl+]" | "[hl-]" | "[bc]" | "[de]" | "[c]")));
    let n: (u32, Option<u32>) = match op {
        0x00 | 0x07 | 0x0F | 0x17 | 0x1F | 0x27 | 0x2F | 0x37 | 0x3F | 0x76 | 0xF3 | 0xFB | 0xE9 | 0x10 => (4, None),
        0x01 | 0x11 | 0x21 | 0x31 => (12, None),
        0x08 => (20, None),
        0x18 => (12, None),
        0x20 | 0x28 | 0x30 | 0x38 => (12, Some(8)),
        0xC0 | 0xC8 | 0xD0 | 0xD8 => (20, Some(8)),
        0xC2 | 0xCA | 0xD2 | 0xDA => (16, Some(12)),
        0xC4 | 0xCC | 0xD4 | 0xDC => (24, Some(12)),
        0xC3 => (16, None),
        0xCD => (24, None),
        0xC9 | 0xD9 => (16, None),
        0xC1 | 0xD1 | 0xE1 | 0xF1 => (12, None),
        0xC5 | 0xD5 | 0xE5 | 0xF5 | 0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => (16, None),
        0xE0 | 0xF0 => (12, None),
        0xEA | 0xFA => (16, None),
        0xE8 => (16, None),
        0xF8 => (12, None),
        0xF9 => (8, None),
        0x03 | 0x13 | 0x23 | 0x33 | 0x0B | 0x1B | 0x2B | 0x3B | 0x09 | 0x19 | 0x29 | 0x39 => (8, None),
        0x34 | 0x35 => (12, None),
        0x36 => (12, None),
        _ if form.operands.contains(&Shape::N8) => (8, None),
        _ if memory_operand => (8, None),
        _ => (4, None),
    };
    Some(match n {
        (taken, Some(not)) => format!("{taken}/{not}"),
        (n, None) => n.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const TABLE: &str = include_str!("testdata/sm83_opcodes.tsv");

    fn assemble(source: &str) -> crate::asm::Assembly {
        crate::asm::assemble(&Sm83, Path::new("t.asm"), source, &|_| Err("no".into()))
    }

    fn bytes_of(source: &str) -> Vec<u8> {
        let assembly = assemble(source);
        assert!(assembly.ok(), "{source}: {:?}", assembly.problems);
        let (low, high) = (assembly.lowest().unwrap(), assembly.highest().unwrap());
        assembly.image(low, high)
    }

    fn table() -> Vec<(Vec<u8>, &'static str)> {
        TABLE
            .lines()
            .filter(|l| !l.starts_with('#') && !l.is_empty())
            .map(|l| {
                let (hex, text) = l.split_once('\t').unwrap();
                (hex.split(' ').map(|h| u8::from_str_radix(h, 16).unwrap()).collect(), text)
            })
            .collect()
    }

    #[test]
    fn every_instruction_assembles_as_the_opcode_tables_say() {
        let table = table();
        // 500: the 256 after $CB, and 244 of the 256 without (eleven aren't instructions, and $CB is the prefix).
        assert_eq!(table.len(), 500);
        for (bytes, text) in &table {
            assert_eq!(&bytes_of(&format!("  org $100\n  {text}\n")), bytes, "{text}");
        }
        // Each opcode once, and the table's forms are the only ones.
        let mut seen: Vec<Vec<u8>> = table.iter().map(|(b, _)| b[..if b[0] == 0xCB { 2 } else { 1 }].to_vec()).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 500);
        assert_eq!(forms().len(), 500);
    }

    #[test]
    fn every_instruction_disassembles_back_to_itself() {
        for (bytes, text) in table() {
            let at = |i: usize| bytes.get(i).copied().unwrap_or(0);
            let decoded = decode(&at, 0x100);
            assert_eq!(decoded.length, bytes.len(), "{text}");
            let shown = decoded.text.replace("{addr}", &decoded.address.map(|a| format!("${a:04X}")).unwrap_or_default());
            // The disassembly names the hardware's registers; put the address back to compare.
            let shown = HARDWARE.iter().fold(shown, |s, (a, n)| s.replace(&format!("[{n}]"), &format!("[${a:04X}]")));
            // And it assembles to the same bytes.
            assert_eq!(bytes_of(&format!("  org $100\n  {shown}\n")), bytes, "{text} came back as {shown}");
            assert_eq!(shown.to_ascii_lowercase(), text.to_ascii_lowercase(), "{:02X?}", bytes);
        }
        // Not instructions: $D3 and the other ten, which lock the processor up.
        for op in [0xD3, 0xDB, 0xDD, 0xE3, 0xE4, 0xEB, 0xEC, 0xED, 0xF4, 0xFC, 0xFD] {
            assert_eq!(decode(&|i| if i == 0 { op } else { 0 }, 0).text, format!("db ${op:02X}"));
        }
    }

    #[test]
    fn other_ways_of_writing_them_are_taken() {
        let same = [
            ("ld a,[hli]", "ld a,[hl+]"),
            ("ldi a,[hl]", "ld a,[hl+]"),
            ("ldd [hl],a", "ld [hl-],a"),
            ("ld (hl),a", "ld [hl],a"),
            ("ld a,(hld)", "ld a,[hl-]"),
            ("ldh [$44],a", "ldh [$FF44],a"),
            ("ldio a,[$ff44]", "ldh a,[$FF44]"),
            ("ld [$ff00+c],a", "ldh [c],a"),
            ("ld a,[c]", "ldh a,[c]"),
            ("jp [hl]", "jp hl"),
            ("sub b", "sub a,b"),
            ("cp $10", "cp a,$10"),
            ("xor a", "xor a,a"),
            ("ldhl sp,-2", "ld hl,sp-2"),
            ("ld hl,[sp+4]", "ld hl,sp+4"),
            ("add sp,-1", "add sp,-$01"),
            ("rst 56", "rst $38"),
            ("bit 7, [ hl ]", "bit 7,[hl]"),
            ("LD A, [HL+]", "ld a,[hl+]"),
        ];
        for (one, other) in same {
            assert_eq!(bytes_of(&format!("  org $100\n  {one}\n")), bytes_of(&format!("  org $100\n  {other}\n")), "{one}");
        }
    }

    #[test]
    fn mistakes_are_said() {
        for (source, said) in [
            ("ld [hl],[hl]", "LD can't take"),
            ("bit 8,a", "0 to 7"),
            ("rst 3", "RST goes to"),
            ("jr $200", "out of JR's reach"),
            ("ldh [$1234],a", "LDH reaches"),
            ("add sp,200", "-128 to 127"),
            ("ex de,hl", "ex isn't a"),
            ("ld ix,0", "LD can't take"),
        ] {
            let assembly = assemble(&format!("  org $100\n  {source}\n"));
            assert!(assembly.problems.iter().any(|p| p.message.contains(said)), "{source}: {:?}", assembly.problems);
        }
    }

    #[test]
    fn rgbds_sections_constants_and_tiles() {
        let source = r#"
DEF rLCDC EQU $FF40
DEF COUNT = 3
SECTION "Header", ROM0[$100]
    nop
    jp Start
    ds $150 - @, 0
SECTION "Main", ROM0
Start:
    ldh [rLCDC], a
.loop
    jr .loop
SECTION "Tiles", ROM0
Tile:
    dw `01233210, `33333333
SECTION "Bank2", ROMX[$4000], BANK[2]
Far:
    db COUNT
SECTION "Vars", WRAM0
counter: ds 1
other: ds 2
SECTION "More", WRAM0
third: ds 1
SECTION "Fast", HRAM
fast: ds 1
"#;
        let assembly = assemble(source);
        assert!(assembly.ok(), "{:?}", assembly.problems);
        let symbols = &assembly.symbols;
        assert_eq!(symbols["Start"], 0x150);
        assert_eq!(symbols["Tile"], 0x154);
        assert_eq!(symbols["Far"], 0x4000);
        assert_eq!((symbols["counter"], symbols["other"], symbols["third"], symbols["fast"]), (0xC000, 0xC001, 0xC003, 0xFF80));
        let image = assembly.image(0, assembly.highest().unwrap());
        assert_eq!(&image[0x100..0x104], &[0x00, 0xC3, 0x50, 0x01]);
        assert_eq!(&image[0x150..0x154], &[0xE0, 0x40, 0x18, 0xFE]);
        // `01233210: low bits 0,1,0,1,1,0,1,0; high bits 0,0,1,1,1,1,0,0.
        assert_eq!(&image[0x154..0x158], &[0b0101_1010, 0b0011_1100, 0xFF, 0xFF]);
        // Bank 2 is further on in the ROM, at $8000, though its labels are at $4000; RAM makes no bytes.
        assert_eq!(image[0x8000], 3);
        assert_eq!(image.len(), 0x8001);
    }

    #[test]
    fn cycles_are_counted() {
        assert_eq!(cycles(&[0x00]).as_deref(), Some("4"));
        assert_eq!(cycles(&[0x20, 0x00]).as_deref(), Some("12/8"));
        assert_eq!(cycles(&[0xCD, 0, 0]).as_deref(), Some("24"));
        assert_eq!(cycles(&[0xCB, 0x46]).as_deref(), Some("12"));
        assert_eq!(cycles(&[0xCB, 0x86]).as_deref(), Some("16"));
        assert_eq!(cycles(&[0x7E]).as_deref(), Some("8"));
        assert_eq!(cycles(&[0x3E, 1]).as_deref(), Some("8"));
        assert_eq!(cycles(&[0xE0, 0x40]).as_deref(), Some("12"));
    }
}
