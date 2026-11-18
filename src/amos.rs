//! AMOS programs (`.AMOS`), the Amiga's BASIC from Europress: listed as AMOS lists them, and tokenised back from
//! a listing. The format is as Stuart Caie's amostools and the AMOS file formats page on exotica.org.uk describe it:
//! a 16-byte header saying which AMOS saved it and whether the program was tested, the length of the tokenised
//! code, the code (lines of 2-byte tokens, each line its length in words and its indent first), then the memory
//! banks saved with it ("AmBs").
//!
//! Tokens $0000 to $004E (and $2B6A) are names and constants; every other is an offset into AMOS's token table
//! (`tokens.rs`), or $004E with an extension's slot and offset. Encrypted procedures are decrypted to be listed.
//! A program tokenised here is saved as not yet tested: AMOS fills in the jumps for loops, IFs and procedures,
//! and its variables' places, when it next tests it, as it does after any line is edited.

mod tokens;

use crate::basic::Listing;
use std::collections::HashMap;
use std::sync::OnceLock;

const TK_VAR: u16 = 0x0006;
const TK_LAB: u16 = 0x000C;
const TK_PRO: u16 = 0x0012;
const TK_LGO: u16 = 0x0018;
const TK_BIN: u16 = 0x001E;
const TK_CH1: u16 = 0x0026;
const TK_CH2: u16 = 0x002E;
const TK_HEX: u16 = 0x0036;
const TK_ENT: u16 = 0x003E;
const TK_FL: u16 = 0x0046;
const TK_EXT: u16 = 0x004E;
const TK_DFL: u16 = 0x2B6A;
const TK_REM1: u16 = 0x064A;
const TK_REM2: u16 = 0x0652;
const TK_PROC: u16 = 0x0376;
const TK_END_PROC: u16 = 0x0390;
const TK_PAREN: u16 = 0x0074;
const TK_COLON: u16 = 0x0054;

/// Tokens with extra bytes after them, and how many: AMOS fills them in when it tests the program.
fn extra(token: u16) -> usize {
    match token {
        0x023C | 0x0250 | 0x0268 | 0x027E | 0x02BE | 0x02D0 | 0x0404 | 0x25A4 => 2,
        0x0290 | 0x029E | 0x0316 => 4,
        TK_PROC => 8,
        0x2A40 | 0x2A4A | 0x2A54 | 0x2A64 => 6,
        _ => 0,
    }
}

fn word(b: &[u8], at: usize) -> u16 {
    b.get(at..at + 2).map_or(0, |b| u16::from_be_bytes([b[0], b[1]]))
}

fn long(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4).map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

struct Table {
    by_key: HashMap<u32, (u8, &'static str, &'static str)>,
    /// Names to tokenise, longest first: (lower-case name without a leading space, each form's key in order).
    by_name: Vec<(String, Vec<u32>)>,
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut by_key = HashMap::new();
        let mut by_name: Vec<(String, Vec<u32>)> = Vec::new();
        for &(key, kind, name, params) in tokens::TOKENS.iter() {
            by_key.entry(key).or_insert((kind, name, params));
            let lower = name.trim_start().to_lowercase();
            if lower.is_empty() {
                continue;
            }
            match by_name.iter_mut().find(|(n, _)| *n == lower) {
                Some((_, keys)) => keys.push(key),
                None => by_name.push((lower, vec![key])),
            }
        }
        by_name.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
        Table { by_key, by_name }
    })
}

fn is_function(kind: u8) -> bool {
    matches!(kind, b'O' | b'0' | b'1' | b'2' | b'V')
}

/// How many arguments follow a keyword: inside the brackets after a function, or up to the end of the
/// statement after an instruction, each kept apart by a comma or To.
fn count_arguments(text: &str, function: bool) -> usize {
    let text = text.trim_start();
    let (body, until_bracket) = if function {
        match text.strip_prefix('(') {
            Some(inside) => (inside, true),
            None => return 0,
        }
    } else {
        (text, false)
    };
    let mut depth = 0i32;
    let mut separators = 0;
    let mut any = false;
    let mut chars = body.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' | '\'' => {
                any = true;
                for (_, d) in chars.by_ref() {
                    if d == c {
                        break;
                    }
                }
            }
            '(' | '[' => {
                depth += 1;
                any = true;
            }
            ')' | ']' if depth == 0 && until_bracket => break,
            ')' | ']' => depth -= 1,
            ':' if depth == 0 && !until_bracket => break,
            ',' if depth == 0 => separators += 1,
            ' ' => {}
            _ => {
                let rest = body[i..].to_ascii_lowercase();
                let word_before = i > 0 && body[..i].chars().last().is_some_and(is_name_char);
                let word = |w: &str| rest.starts_with(w) && !word_before && !rest[w.len()..].starts_with(is_name_char);
                if depth == 0 && word("to") {
                    separators += 1;
                    chars.next();
                } else if depth == 0 && !until_bracket && (word("else") || word("then")) {
                    break;
                } else {
                    any = true;
                }
            }
        }
    }
    if any || separators > 0 { separators + 1 } else { 0 }
}

/// Which of a keyword's forms AMOS stores: an instruction's at the start of a statement and a function's after
/// it, with as many parameters as there are arguments.
fn choose(keys: &[u32], after: &str, start_of_statement: bool) -> u32 {
    let entry = |key: &u32| table().by_key[key];
    let wanted: Vec<u32> = keys.iter().copied().filter(|k| is_function(entry(k).0) != start_of_statement).collect();
    let candidates = if wanted.is_empty() { keys.to_vec() } else { wanted };
    if candidates.len() == 1 {
        return candidates[0];
    }
    let arguments = count_arguments(after, is_function(entry(&candidates[0]).0));
    // Add's two forms have no parameters in the table: AMOS reads Add v,n and Add v,n,a To b itself.
    if entry(&candidates[0]).1 == "Add" {
        return candidates[usize::from(arguments >= 4).min(candidates.len() - 1)];
    }
    candidates.iter().copied().find(|k| entry(k).2.chars().filter(char::is_ascii_digit).count() == arguments).unwrap_or(candidates[0])
}

/// Whether bytes are an AMOS program.
pub fn looks_like(data: &[u8]) -> bool {
    data.len() >= 20 && (data.starts_with(b"AMOS Basic") || data.starts_with(b"AMOS Pro"))
}

/// AMOS's single-precision floats: a 24-bit mantissa, then the exponent (bit 7 unused), as the Motorola Fast
/// Floating Point library has them.
pub fn ffp_to_f64(value: u32) -> f64 {
    let e = (value & 0x7F) as i32;
    if e == 0 { 0.0 } else { f64::from(value >> 8) * 2f64.powi(e - 88) }
}

pub fn f64_to_ffp(value: f64) -> u32 {
    if value <= 0.0 || !value.is_finite() {
        return 0;
    }
    let mut e = value.log2().floor() as i32 + 1;
    let mut m = (value / 2f64.powi(e - 24)).round() as u64;
    if m >= 1 << 24 {
        m >>= 1;
        e += 1;
    }
    let exponent = (e + 64).clamp(1, 127) as u32;
    ((m as u32) << 8) | exponent
}

fn float_text(value: f64) -> String {
    // As AMOS shows it: %G of a single-precision value, with .0 if there's no point or exponent.
    let f = value as f32;
    let mut text = if f != 0.0 && (f.abs() >= 1e7 || f.abs() < 1e-4) { format!("{f:E}") } else { format!("{f}") };
    if text.contains('e') || text.contains('E') {
        text = text.replace('e', "E");
    }
    if !text.contains('.') && !text.contains('E') {
        text += ".0";
    }
    text
}

/// Decrypts (or encrypts again) a procedure whose line starts at `at`, as AMOS's ProCode does.
fn decrypt_procedure(code: &mut [u8], at: usize) {
    if at + 12 > code.len() || code[at + 10] & 0x10 != 0 {
        return;
    }
    let size = long(code, at + 4) as usize;
    let end = at + size + 8 + 6;
    if end > code.len() {
        return;
    }
    let mut key: u32 = ((size as u32) << 8) | u32::from(code[at + 11]);
    let mut key2: u32 = 1;
    let key3 = u32::from(word(code, at + 8));
    let mut line = at + usize::from(code[at]) * 2;
    while line < end {
        let length = usize::from(code[line]) * 2;
        if length == 0 {
            return;
        }
        let next = line + length;
        let mut p = line + 4;
        while p + 1 < next.min(code.len()) {
            code[p] ^= (key >> 8) as u8;
            code[p + 1] ^= key as u8;
            key = (key & 0xFFFF_0000) | (key.wrapping_add(key2) & 0xFFFF);
            key2 = (key2 + key3) & 0xFFFF;
            key = key.rotate_right(1);
            p += 2;
        }
        line = next;
    }
    code[at + 10] ^= 0x20;
}

/// Lists an AMOS program as AMOS shows it.
pub fn list(data: &[u8]) -> Listing {
    let mut listing = Listing { dialect: "AMOS", ..Listing::default() };
    if !looks_like(data) {
        listing.notes.push(crate::i18n::tr("This isn't an AMOS program: it starts \u{201C}AMOS Basic\u{201D} or \u{201C}AMOS Pro\u{201D}.").into());
        return listing;
    }
    let header: String = data[..16].iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
    let pro = header.starts_with("AMOS Pro");
    listing.dialect = if pro { "AMOS Professional" } else { "AMOS" };
    let version = match &header[..] {
        h if h.starts_with("AMOS Pro111") => "AMOS Professional 1.11",
        h if h.starts_with("AMOS Pro101") => "AMOS Professional 1.0",
        h if h.starts_with("AMOS Pro") => "AMOS Professional",
        h if h.contains("134") => "AMOS Professional, kept compatible with AMOS 1.3",
        h if h.contains("1.3") => "AMOS 1.3",
        h if h.contains("1.23") => "AMOS 1.2",
        _ => "AMOS 1.0 or 1.1",
    };
    listing.info.push((crate::i18n::tr("Saved by").into(), version.into()));
    let tested = header.as_bytes().get(11) == Some(&b'V');
    listing.info.push(("Tested".into(), if tested { "Yes".into() } else { crate::i18n::tr("No: AMOS tests it before it runs").into() }));
    let length = (long(data, 16) as usize).min(data.len() - 20);
    let mut code = data[20..20 + length].to_vec();
    let mut lines = Vec::new();
    let mut unknown = 0;
    let mut at = 0;
    let mut encrypted = 0;
    while at + 2 <= code.len() {
        let line_length = usize::from(code[at]) * 2;
        if line_length == 0 {
            listing.notes.push(crate::i18n::tr("A line says it's empty, so the rest can't be read.").into());
            break;
        }
        let end = (at + line_length).min(code.len());
        let (text, skip, unknowns) = list_line(&mut code, at, end, &mut encrypted);
        unknown += unknowns;
        lines.push(text);
        at = end + skip;
    }
    listing.lines = lines;
    if encrypted > 0 {
        listing.notes.push(if encrypted == 1 { crate::i18n::tr("1 locked procedure was decrypted to be listed.").to_string() } else { crate::i18n::tf!("{encrypted} locked procedures were decrypted to be listed.") });
    }
    if unknown > 0 {
        listing.notes.push(crate::i18n::tf!("{unknown} instructions come from extensions other than AMOS's own four, and are listed as Extension_slot_offset."));
    }
    // The memory banks saved with it.
    let banks_at = 20 + length;
    if data.get(banks_at..banks_at + 4) == Some(b"AmBs") {
        let count = word(data, banks_at + 4);
        if count > 0 {
            let names = bank_names(&data[banks_at + 6..], count);
            listing.info.push((crate::i18n::tr("Memory banks").into(), if names.is_empty() { count.to_string() } else { format!("{count}: {}", names.join(", ")) }));
            listing.notes.push(crate::i18n::tr("Its memory banks aren't kept when the listing is tokenised again: load them in AMOS, or keep this file.").into());
        }
    }
    listing
}

/// The banks after the program: sprites, icons, or a named bank (Music, Samples, Pac.Pic...).
fn bank_names(data: &[u8], count: u16) -> Vec<String> {
    let mut names = Vec::new();
    let mut at = 0;
    for _ in 0..count {
        match data.get(at..at + 4) {
            Some(b"AmSp") | Some(b"AmIc") => {
                let icons = &data[at..at + 4] == b"AmIc";
                let n = usize::from(word(data, at + 4));
                let mut p = at + 6;
                for _ in 0..n {
                    let (w, h, depth) = (usize::from(word(data, p)), usize::from(word(data, p + 2)), usize::from(word(data, p + 4)));
                    p += 10 + w * 2 * h * depth;
                }
                p += 64;
                names.push(format!("{} ({n})", if icons { "Icons" } else { "Sprites" }));
                at = p;
            }
            Some(b"AmBk") => {
                let number = word(data, at + 4);
                let size = (long(data, at + 8) & 0x0FFF_FFFF) as usize;
                let name: String = data.get(at + 12..at + 20).unwrap_or(&[]).iter().map(|&b| b as char).collect::<String>().trim().to_string();
                names.push(format!("{number} {name}"));
                at += 12 + size;
            }
            _ => break,
        }
    }
    names
}

/// One line: its text, bytes of a compiled procedure to skip after it, and how many tokens weren't known.
fn list_line(code: &mut [u8], start: usize, end: usize, encrypted: &mut usize) -> (String, usize, usize) {
    let mut out = String::new();
    let indent = code[start + 1];
    for _ in 1..indent.max(1) {
        out.push(' ');
    }
    let mut p = start + 2;
    let (mut add_space, mut start_of_line, mut label_at_end) = (false, true, false);
    let mut skip = 0;
    let mut unknown = 0;
    while p + 2 <= end {
        let token = word(code, p);
        if token == 0 {
            break;
        }
        p += 2;
        label_at_end = false;
        if token <= TK_LGO {
            if add_space {
                out.push(' ');
            }
            let length = usize::from(code.get(p + 2).copied().unwrap_or(0));
            let flags = code.get(p + 3).copied().unwrap_or(0);
            for i in 0..length {
                let c = code.get(p + 4 + i).copied().unwrap_or(0);
                if c == 0 {
                    break;
                }
                out.push((c as char).to_ascii_uppercase());
            }
            if token == TK_LAB {
                if !code.get(p + 4).is_some_and(u8::is_ascii_digit) {
                    out.push(':');
                }
                add_space = true;
                label_at_end = true;
            } else {
                if flags & 1 != 0 {
                    out.push('#');
                } else if flags & 2 != 0 {
                    out.push('$');
                }
                add_space = false;
            }
            p += if length & 1 == 1 { 5 } else { 4 } + length;
        } else if token < TK_EXT || token == TK_DFL {
            if add_space {
                out.push(' ');
                add_space = false;
            }
            match token {
                TK_BIN => {
                    out += &format!("%{:b}", long(code, p));
                    p += 4;
                }
                TK_CH1 | TK_CH2 => {
                    let quote = if token == TK_CH1 { '"' } else { '\'' };
                    let length = usize::from(word(code, p));
                    let text: String = code.get(p + 2..p + 2 + length).unwrap_or(&[]).iter().take_while(|&&b| b != 0).map(|&b| char::from(b)).collect();
                    out.push(quote);
                    out += &text;
                    out.push(quote);
                    p += 2 + length + (length & 1);
                }
                TK_HEX => {
                    out += &format!("${:X}", long(code, p));
                    p += 4;
                }
                TK_ENT => {
                    out += &(long(code, p) as i32).to_string();
                    p += 4;
                }
                TK_FL => {
                    out += &float_text(ffp_to_f64(long(code, p)));
                    p += 4;
                }
                TK_DFL => {
                    let bits = u64::from(long(code, p)) << 32 | u64::from(long(code, p + 4));
                    let mut text = format!("{}", f64::from_bits(bits & !(1 << 63)));
                    if !text.contains('.') {
                        text += ".0";
                    }
                    out += &text;
                    p += 8;
                }
                _ => {
                    out += &format!("Illegal_Constant_{token:04X}");
                }
            }
        } else {
            let key = if token == TK_EXT {
                let key = u32::from(code.get(p).copied().unwrap_or(0)) << 16 | u32::from(word(code, p + 2));
                p += 4;
                key
            } else {
                u32::from(token)
            };
            match table().by_key.get(&key) {
                Some(&(kind, name, _)) => {
                    let paren = token == TK_PAREN;
                    let function = matches!(kind, b'O' | b'0' | b'1' | b'2' | b'V');
                    if !function && !start_of_line {
                        add_space = true;
                    }
                    if !paren && add_space && !name.starts_with(' ') {
                        out.push(' ');
                    }
                    out += name;
                    add_space = kind == b'I';
                }
                None => {
                    out += &format!(" Extension_{}_{:04X}", key >> 16, key & 0xFFFF);
                    add_space = true;
                    unknown += 1;
                }
            }
            match token {
                TK_REM1 | TK_REM2 => {
                    let length = usize::from(code.get(p + 1).copied().unwrap_or(0));
                    let text: String = code.get(p + 2..p + 2 + length).unwrap_or(&[]).iter().take_while(|&&b| b != 0).map(|&b| char::from(b)).collect();
                    out += &text;
                    p += 2 + length + (length & 1);
                }
                TK_PROC => {
                    let flags = code.get(p + 6).copied().unwrap_or(0);
                    if flags & 0x20 != 0 {
                        decrypt_procedure(code, start);
                        *encrypted += 1;
                    }
                    if flags & 0x10 != 0 {
                        // A compiled procedure: its code isn't tokens, and runs to its End Proc.
                        skip = (long(code, p) as usize + 8).saturating_sub(end - start);
                        out += "   ' (compiled procedure: its code can't be listed)";
                    }
                    p += 8;
                }
                t => p += extra(t),
            }
        }
        start_of_line = false;
    }
    if add_space && !label_at_end {
        out.push(' ');
    }
    (out, skip, unknown)
}

// ---------- Tokenising ----------

/// Where an identifier can be: a variable, a procedure called by name, or a label jumped to.
fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn push_name(out: &mut Vec<u8>, token: u16, name: &str, flags: u8) {
    let lower = name.to_ascii_lowercase();
    let length = lower.len() + (lower.len() & 1);
    out.extend_from_slice(&token.to_be_bytes());
    out.extend_from_slice(&[0, 0, length as u8, flags]);
    out.extend_from_slice(lower.as_bytes());
    if lower.len() & 1 == 1 {
        out.push(0);
    }
}

fn push_string(out: &mut Vec<u8>, token: u16, text: &str) {
    let bytes: Vec<u8> = text.chars().map(|c| u32::from(c).min(255) as u8).collect();
    out.extend_from_slice(&token.to_be_bytes());
    out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    out.extend_from_slice(&bytes);
    if bytes.len() & 1 == 1 {
        out.push(0);
    }
}

/// The keyword at the start of `text`, if one is: the longest that fits, ending where a word does, and its forms.
fn keyword_at(text: &str) -> Option<(usize, &'static [u32])> {
    let lower = text.to_ascii_lowercase();
    for (name, keys) in &table().by_name {
        if lower.starts_with(name.as_str()) {
            let last = name.chars().last().unwrap_or(' ');
            let next = text[name.len()..].chars().next();
            // A word ends where the next character can't go on with it: "Print" in "Printer" isn't Print.
            if is_name_char(last) && next.is_some_and(is_name_char) {
                continue;
            }
            return Some((name.len(), keys.as_slice()));
        }
    }
    None
}

/// Tokenises one line; with `double`, floats are kept in double precision, as after Set Double Precision.
fn tokenise_line(line: &str, double: bool) -> Result<Vec<u8>, String> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let text = line.trim_start_matches(' ');
    // An empty line has no indent at all, as AMOS keeps it.
    let indent = if text.is_empty() { 0 } else { (indent + 1).min(255) as u8 };
    let mut out = vec![0u8, indent];
    let mut rest = text;
    let mut start_of_statement = true;
    let mut first = true;
    // After Goto, Gosub, Restore and the like a name or number is a label; after Then or Else only when nothing
    // else follows it (If A Then 100, but If A Then B=1); after Procedure or Proc a name is a procedure's.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Next {
        Anything,
        Label,
        LabelIfAlone,
        Procedure,
    }
    let mut next = Next::Anything;
    let mut previous_was_procedure = false;
    while !rest.is_empty() {
        let trimmed = rest.trim_start_matches(' ');
        if trimmed.is_empty() {
            break;
        }
        rest = trimmed;
        let c = rest.chars().next().unwrap_or(' ');
        // A label at the start of the line: a name and a colon, or a line number.
        if first {
            let digits = rest.chars().take_while(char::is_ascii_digit).count();
            if digits > 0 {
                push_name(&mut out, TK_LAB, &rest[..digits], 0);
                rest = &rest[digits..];
                first = false;
                continue;
            }
            let name_length = rest.chars().take_while(|c| is_name_char(*c)).count();
            if name_length > 0 && is_name_start(c) && rest[name_length..].starts_with(':') && keyword_at(rest).is_none_or(|(l, _)| l < name_length) {
                push_name(&mut out, TK_LAB, &rest[..name_length], 0);
                rest = &rest[name_length + 1..];
                first = false;
                continue;
            }
        }
        first = false;
        // Strings.
        if c == '"' || (c == '\'' && !start_of_statement) {
            let close = rest[1..].find(c).map_or(rest.len(), |i| i + 1);
            push_string(&mut out, if c == '"' { TK_CH1 } else { TK_CH2 }, &rest[1..close.min(rest.len())]);
            rest = rest.get(close + 1..).unwrap_or("");
            start_of_statement = false;
            next = Next::Anything;
            continue;
        }
        // Numbers.
        if c == '$' && rest[1..].starts_with(|c: char| c.is_ascii_hexdigit()) {
            let digits = rest[1..].chars().take_while(char::is_ascii_hexdigit).count();
            let value = u32::from_str_radix(&rest[1..1 + digits], 16).map_err(|_| crate::i18n::tf!("${} is too large a number", &rest[1..1 + digits]))?;
            out.extend_from_slice(&TK_HEX.to_be_bytes());
            out.extend_from_slice(&value.to_be_bytes());
            rest = &rest[1 + digits..];
            start_of_statement = false;
            continue;
        }
        if c == '%' && rest[1..].starts_with(['0', '1']) {
            let digits = rest[1..].chars().take_while(|c| *c == '0' || *c == '1').count();
            let value = u32::from_str_radix(&rest[1..1 + digits], 2).map_err(|_| crate::i18n::tr("a binary number is too long").to_string())?;
            out.extend_from_slice(&TK_BIN.to_be_bytes());
            out.extend_from_slice(&value.to_be_bytes());
            rest = &rest[1 + digits..];
            start_of_statement = false;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && rest[1..].starts_with(|c: char| c.is_ascii_digit())) {
            let mut length = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').count();
            let mut float = rest[..length].contains('.');
            // An exponent: E, then a sign and digits.
            let after = &rest[length..];
            if after.starts_with(['e', 'E']) {
                let sign = usize::from(after[1..].starts_with(['+', '-']));
                let digits = after[1 + sign..].chars().take_while(char::is_ascii_digit).count();
                if digits > 0 {
                    length += 1 + sign + digits;
                    float = true;
                }
            }
            let number = &rest[..length];
            let label = matches!(next, Next::Label | Next::LabelIfAlone) && !float;
            if label {
                // Goto 100: a line number is a label.
                push_name(&mut out, TK_LGO, number, 0);
            } else if float {
                let value: f64 = number.parse().map_err(|_| crate::i18n::tf!("{number} isn't a number AMOS can read"))?;
                if double {
                    out.extend_from_slice(&TK_DFL.to_be_bytes());
                    out.extend_from_slice(&value.to_bits().to_be_bytes());
                } else {
                    out.extend_from_slice(&TK_FL.to_be_bytes());
                    out.extend_from_slice(&f64_to_ffp(value).to_be_bytes());
                }
            } else {
                let value: i64 = number.parse().map_err(|_| crate::i18n::tf!("{number} is too large a number"))?;
                out.extend_from_slice(&TK_ENT.to_be_bytes());
                out.extend_from_slice(&(value as u32).to_be_bytes());
            }
            rest = &rest[length..];
            start_of_statement = false;
            // A list of labels (On X Goto 10,20) goes on after a comma.
            next = if label && next == Next::Label { Next::Label } else { Next::Anything };
            continue;
        }
        // A keyword, an operator or punctuation.
        let name_length = rest.chars().take_while(|c| is_name_char(*c)).count();
        let keyword = keyword_at(rest).filter(|(length, _)| *length >= name_length || !is_name_start(c));
        if let Some((length, keys)) = keyword {
            let key = choose(keys, &rest[length..], start_of_statement);
            let token = key as u16;
            if key >> 16 != 0 {
                out.extend_from_slice(&TK_EXT.to_be_bytes());
                out.extend_from_slice(&[(key >> 16) as u8, 0]);
                out.extend_from_slice(&token.to_be_bytes());
            } else {
                out.extend_from_slice(&token.to_be_bytes());
            }
            rest = &rest[length..];
            match token {
                TK_REM1 | TK_REM2 if key >> 16 == 0 => {
                    let text: Vec<u8> = rest.chars().map(|c| u32::from(c).min(255) as u8).collect();
                    let length = text.len().min(254);
                    out.extend_from_slice(&[0, length as u8]);
                    out.extend_from_slice(&text[..length]);
                    if length & 1 == 1 {
                        out.push(0);
                    }
                    rest = "";
                }
                t if key >> 16 == 0 => out.extend(std::iter::repeat_n(0, extra(t))),
                _ => {}
            }
            let name = table().by_key.get(&key).map_or("", |(_, n, _)| *n).trim().to_ascii_lowercase();
            next = match name.as_str() {
                "goto" | "gosub" | "restore" | "resume" | "resume label" | "on error goto" | "every" => Next::Label,
                "then" | "else" => Next::LabelIfAlone,
                "procedure" | "proc" => {
                    previous_was_procedure = name == "procedure";
                    Next::Procedure
                }
                // On X Goto A,B,C: each after a comma is a label too.
                "," if next == Next::Label => Next::Label,
                _ => Next::Anything,
            };
            start_of_statement = token == TK_COLON || name == "then" || name == "else";
            continue;
        }
        // A name: a variable, a procedure called by name (with [), or a label after Goto or Gosub.
        if is_name_start(c) {
            let mut name = &rest[..name_length];
            let mut flags = 0;
            let mut used = name_length;
            match rest[name_length..].chars().next() {
                Some('#') => {
                    flags = 1;
                    used += 1;
                }
                Some('$') => {
                    flags = 2;
                    used += 1;
                }
                _ => {}
            }
            let after = rest[used..].trim_start();
            // An array: AMOS marks the name, as it does the type.
            if after.starts_with('(') {
                flags |= 0x40;
            }
            let alone = after.is_empty() || after.starts_with(':') || after.to_ascii_lowercase().starts_with("else");
            // A procedure's own name, after Procedure, is a variable token marked as a procedure's; a call by
            // name is a procedure token, marked the same.
            let token = match next {
                Next::Label => TK_LGO,
                // If A Then NAME calls the procedure NAME.
                Next::LabelIfAlone if alone => {
                    flags = 0x80;
                    TK_PRO
                }
                Next::Procedure if previous_was_procedure => {
                    flags = 0x80;
                    TK_VAR
                }
                Next::Procedure => {
                    flags = 0x80;
                    TK_PRO
                }
                _ if after.starts_with('[') || (start_of_statement && alone) => {
                    flags = 0x80;
                    TK_PRO
                }
                _ => TK_VAR,
            };
            next = if token == TK_LGO && next == Next::Label { Next::Label } else { Next::Anything };
            if name.len() > 255 {
                name = &name[..255];
            }
            push_name(&mut out, token, name, flags);
            rest = &rest[used..];
            start_of_statement = false;
            continue;
        }
        return Err(crate::i18n::tf!("\u{201C}{c}\u{201D} isn't something AMOS can read here"));
    }
    out.extend_from_slice(&[0, 0]);
    if out.len() & 1 == 1 {
        out.push(0);
    }
    let words = out.len() / 2;
    if words > 255 {
        return Err(crate::i18n::tr("the line is too long for AMOS, which keeps 510 bytes a line").into());
    }
    out[0] = words as u8;
    Ok(out)
}

/// Tokenises a listing into an AMOS program, saved as not yet tested. AMOS Professional's header is written
/// when the program uses what only it has (Else If, Equ and the like), otherwise one AMOS 1.3 loads too.
pub fn tokenise<S: AsRef<str>>(lines: &[S]) -> Result<Vec<u8>, String> {
    let mut code = Vec::new();
    let mut procedures: Vec<usize> = Vec::new();
    let mut pro_only = false;
    // Set Double Precision, anywhere, makes AMOS Professional keep every float in double precision.
    let double = lines.iter().any(|l| l.as_ref().to_ascii_lowercase().contains("set double precision"));
    for (n, line) in lines.iter().enumerate() {
        let line = line.as_ref();
        let bytes = tokenise_line(line, double).map_err(|e| crate::i18n::tf!("Line {}: {e}", n + 1))?;
        let first = word(&bytes, 2);
        if first == TK_PROC {
            procedures.push(code.len());
        }
        let end_proc = first == TK_END_PROC;
        let mut p = 2;
        while p + 2 <= bytes.len() {
            let t = word(&bytes, p);
            if matches!(t, 0x25A4 | 0x2A40 | 0x2A4A | 0x2A54 | 0x2A64 | TK_DFL) {
                pro_only = true;
            }
            p += 2;
        }
        code.extend(bytes);
        if end_proc && let Some(start) = procedures.pop() {
            // The Procedure line keeps how far it is to the line after its End Proc, less 14.
            let distance = (code.len() - start - 14) as u32;
            code[start + 4..start + 8].copy_from_slice(&distance.to_be_bytes());
        }
    }
    let mut out = Vec::with_capacity(code.len() + 30);
    out.extend_from_slice(if pro_only { b"AMOS Pro101v\0\0\0\0" } else { b"AMOS Basic v134 " });
    out.extend_from_slice(&(code.len() as u32).to_be_bytes());
    out.extend(code);
    out.extend_from_slice(b"AmBs\0\0");
    Ok(out)
}

#[cfg(test)]
mod tests;
