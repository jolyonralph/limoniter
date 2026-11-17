//! STOS BASIC programs (`.BAS`), the Atari ST's games BASIC from Mandarin: listed as STOS lists them, and tokenised
//! back from a listing, both following STOS 2.06's own routines (`detok` and `tokenise` in BASIC.S, as Clickteam
//! released its source).
//!
//! The file is "Lionpoulos", the length of the program and its banks, sixteen longs for the banks (the first the
//! program's own length), then the program: lines of a length word, a line number and the tokens, ending with a
//! zero byte, each even in length, and a zero word after the last. Keywords are one byte ($80 to $F9) or two
//! ($A0 or $B8 and a second); $FA is a variable, $FB to $FF numbers and strings, $A8 and $C0 an extension's
//! instruction or function. A variable, a number and a branching keyword ($98 to $9F) are followed, at the next
//! even place, by a long: the variable's flags and length (STOS fills in the rest as it runs), the number, or
//! room for where the branch goes. Floats are Motorola Fast Floating Point.

mod tokens;

use crate::basic::Listing;
use std::collections::HashMap;
use std::sync::OnceLock;

const HEADER: &[u8; 10] = b"Lionpoulos";
/// The bank table and the lengths before the program.
const PROGRAM_AT: usize = 10 + 4 + 64;
const REM: u8 = 0x8A;
const FN: u8 = 0xC9;
const VARIABLE: u8 = 0xFA;
const BINARY: u8 = 0xFB;
const STRING: u8 = 0xFC;
const HEX: u8 = 0xFD;
const INTEGER: u8 = 0xFE;
const FLOAT: u8 = 0xFF;
/// What STOS's float package leaves in the second long of a float.
const FLOAT_FILLER: u32 = 0x1234_5678;

fn word(b: &[u8], at: usize) -> usize {
    b.get(at..at + 2).map_or(0, |b| usize::from(u16::from_be_bytes([b[0], b[1]])))
}

fn long(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4).map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// Whether bytes are a STOS program.
pub fn looks_like(data: &[u8]) -> bool {
    data.starts_with(HEADER) && data.len() >= PROGRAM_AT
}

/// Each code's name, as LIST shows it: the first keyword in the table with that code.
fn names() -> &'static HashMap<Vec<u8>, &'static str> {
    static NAMES: OnceLock<HashMap<Vec<u8>, &'static str>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names = HashMap::new();
        for (name, code) in tokens::TOKENS {
            names.entry(code.to_vec()).or_insert(name);
        }
        names
    })
}

/// Lists a STOS program as STOS's LIST shows it.
pub fn list(data: &[u8]) -> Listing {
    let mut listing = Listing { dialect: "STOS BASIC", ..Listing::default() };
    if !looks_like(data) {
        listing.notes.push("This isn't a STOS program: it starts \u{201C}Lionpoulos\u{201D}.".into());
        return listing;
    }
    let length = (long(data, 14) as usize).min(data.len() - PROGRAM_AT);
    let program = &data[PROGRAM_AT..PROGRAM_AT + length];
    let banks: Vec<(usize, u32)> = (1..16).map(|i| (i, long(data, 14 + i * 4))).filter(|(_, v)| *v != 0).collect();
    if !banks.is_empty() {
        let list: Vec<String> = banks.iter().map(|(n, v)| format!("{n} ({} bytes)", group(u64::from(v & 0x00FF_FFFF)))).collect();
        listing.info.push(("Memory banks".into(), list.join(", ")));
        listing.notes.push("Its memory banks aren't kept when the listing is tokenised again: load them in STOS, or keep this file.".into());
    }
    let mut at = 0;
    let mut extensions = 0;
    while at + 4 <= program.len() {
        let line_length = word(program, at);
        if line_length == 0 {
            break;
        }
        let end = (at + line_length).min(program.len());
        let (text, from_extensions) = list_line(program, at, end);
        extensions += from_extensions;
        listing.lines.push(text);
        at += line_length;
    }
    if extensions > 0 {
        listing.notes.push(format!("{extensions} instructions come from STOS extensions, listed as extension_A_3C (the extension's letter and the instruction's code), as STOS can't name them without the extension either."));
    }
    listing
}

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

/// Fast Floating Point to a number: a 24-bit mantissa, the sign in bit 7 and the exponent below it.
fn ffp(value: u32) -> f64 {
    let e = i32::from((value & 0x7F) as u8);
    if value >> 8 == 0 {
        return 0.0;
    }
    let magnitude = f64::from(value >> 8) * 2f64.powi(e - 88);
    if value & 0x80 != 0 { -magnitude } else { magnitude }
}

fn to_ffp(value: f64) -> u32 {
    if value == 0.0 || !value.is_finite() {
        return 0;
    }
    let sign = if value < 0.0 { 0x80 } else { 0 };
    let value = value.abs();
    let mut e = value.log2().floor() as i32 + 1;
    let mut m = (value / 2f64.powi(e - 24)).round() as u64;
    if m >= 1 << 24 {
        m >>= 1;
        e += 1;
    }
    ((m as u32) << 8) | sign | ((e + 64).clamp(1, 127) as u32)
}

fn float_text(value: f64) -> String {
    let f = value as f32;
    let mut text = if f != 0.0 && (f.abs() >= 1e9 || f.abs() < 1e-5) { format!("{f:E}") } else { format!("{f}") };
    if !text.contains('.') && !text.contains('E') {
        text += ".0";
    }
    text
}

/// One line, as `detok` writes it: the spaces it puts between things are its own, from what came before.
fn list_line(program: &[u8], start: usize, end: usize) -> (String, usize) {
    let mut out = format!("{} ", word(program, start + 2));
    // What came before: $FF for nothing (no space), 0 for a letter or other character, else the token.
    let mut before: u8 = 0xFF;
    let mut rem = false;
    let mut p = start + 4;
    let mut extensions = 0;
    let even = |p: usize| if (p - start) % 2 == 1 { p + 1 } else { p };
    while p < end {
        let c = program[p];
        p += 1;
        if c == 0 {
            break;
        }
        if rem {
            out.push(char::from(c));
            continue;
        }
        if c < 0x80 {
            if c == b':' {
                out += " : ";
                before = 0xFF;
                continue;
            }
            if before != 0 && before < 0xB0 {
                out.push(' ');
            }
            out.push(char::from(c));
            before = 0;
            continue;
        }
        // Variables, numbers and strings: at the next even place.
        if c >= VARIABLE {
            p = even(p);
            if before != 0 && before < 0xEA {
                out.push(' ');
            }
            match c {
                VARIABLE => {}
                HEX => out += &format!("${:X}", long(program, p)),
                INTEGER => out += &(long(program, p) as i32).to_string(),
                BINARY => out += &format!("%{:b}", long(program, p)),
                FLOAT => {
                    out += &float_text(ffp(long(program, p)));
                    p += 4;
                }
                _ => {
                    // A string: its length, then its characters.
                    let length = long(program, p) as usize;
                    p += 4;
                    out.push('"');
                    out.extend(program.get(p..(p + length).min(end)).unwrap_or(&[]).iter().map(|&b| char::from(b)));
                    out.push('"');
                    p += length;
                    before = 0;
                    continue;
                }
            }
            before = 0;
            p += 4;
            continue;
        }
        // A branching keyword: room for where it goes, at the next even place, then its name.
        if (0x98..0xA0).contains(&c) {
            p = even(p) + 4;
        }
        if before < 0xEA && c < 0xEA && (c < 0xB8 || before != 0) {
            out.push(' ');
        }
        if c == REM {
            rem = true;
        }
        before = c;
        let code: Vec<u8> = match c {
            0xA0 | 0xB8 => {
                let second = program.get(p).copied().unwrap_or(0);
                p += 1;
                vec![c, second]
            }
            0xA8 | 0xC0 => {
                // An extension's: its number, then its own code.
                let (number, code) = (program.get(p).copied().unwrap_or(0), program.get(p + 1).copied().unwrap_or(0));
                p += 2;
                extensions += 1;
                out += &format!("extension_{}_{code:02X}", char::from(b'A' + number.min(25)));
                continue;
            }
            _ => vec![c],
        };
        match names().get(&code) {
            Some(name) => out += name,
            None => out += &format!("token_{}", code.iter().map(|b| format!("{b:02X}")).collect::<String>()),
        }
    }
    (out, extensions)
}

// ---------- Tokenising ----------

/// The keyword starting at `at`, as `sstok` finds it: the first in the table whose letters match, spaces skipped in
/// both and case ignored. Where it ends, and its code.
fn keyword(line: &[u8], at: usize, table: &[(&'static str, &'static [u8])]) -> Option<(usize, &'static [u8])> {
    'next: for (name, code) in table {
        let mut p = at;
        for &k in name.as_bytes() {
            if k == b' ' {
                continue;
            }
            loop {
                let Some(&c) = line.get(p) else { continue 'next };
                p += 1;
                if c == b' ' {
                    continue;
                }
                let c = if c >= 0x80 { b'.' } else { c.to_ascii_lowercase() };
                if c != k {
                    continue 'next;
                }
                break;
            }
        }
        return Some((p, code));
    }
    None
}

/// Writes a long at the next even place, as `pokepair` does.
fn poke_even(out: &mut Vec<u8>, value: u32) {
    if out.len() % 2 == 1 {
        out.push(0);
    }
    out.extend_from_slice(&value.to_be_bytes());
}

/// A number at `at`, as `valprg` reads it: decimal (a float with a point or an exponent), $hex or %binary.
fn number(line: &[u8], at: usize) -> Option<(usize, u8, u32, Option<u32>)> {
    let text = std::str::from_utf8(&line[at..]).ok()?;
    if let Some(hex) = text.strip_prefix('$') {
        let digits = hex.bytes().take_while(u8::is_ascii_hexdigit).count();
        let value = u32::from_str_radix(&hex[..digits], 16).ok()?;
        return Some((at + 1 + digits, HEX, value, None));
    }
    if let Some(binary) = text.strip_prefix('%') {
        let digits = binary.bytes().take_while(|b| *b == b'0' || *b == b'1').count();
        let value = u32::from_str_radix(&binary[..digits], 2).ok()?;
        return Some((at + 1 + digits, BINARY, value, None));
    }
    if !text.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let mut length = text.bytes().take_while(|b| b.is_ascii_digit() || *b == b'.').count();
    let mut float = text[..length].contains('.');
    let rest = &text[length..];
    if rest.starts_with(['e', 'E']) {
        let sign = usize::from(rest[1..].starts_with(['+', '-']));
        let digits = rest[1 + sign..].bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 || sign > 0 {
            length += 1 + sign + digits;
            float = true;
        }
    }
    let written = &text[..length];
    if float {
        let value: f64 = written.replace(['e', 'E'], "e").parse().ok()?;
        Some((at + length, FLOAT, to_ffp(value), Some(FLOAT_FILLER)))
    } else {
        let value: u64 = written.parse().ok()?;
        Some((at + length, INTEGER, value as u32, None))
    }
}

/// Tokenises one line, which starts with its number.
fn tokenise_line(text: &str) -> Result<Vec<u8>, String> {
    let line: Vec<u8> = text.chars().map(|c| u32::from(c).min(255) as u8).collect();
    let mut p = line.iter().take_while(|b| **b == b' ').count();
    let digits = line[p..].iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return Err(crate::i18n::tr("a STOS line starts with its number").into());
    }
    let line_number: usize = std::str::from_utf8(&line[p..p + digits]).ok().and_then(|n| n.parse().ok()).filter(|n| (1..=65535).contains(n)).ok_or(crate::i18n::tr("the line number isn't 1 to 65535"))?;
    p += digits;
    let mut out = vec![0, 0];
    out.extend_from_slice(&(line_number as u16).to_be_bytes());
    let (mut rem, mut fn_follows) = (false, false);
    let mut string: Option<usize> = None;
    // The variable being read: where its flags go, and how long its name is so far.
    let mut variable: Option<(usize, u8)> = None;
    let close = |out: &mut Vec<u8>, variable: &mut Option<(usize, u8)>, suffix: Option<u8>, array: bool| {
        if let Some((at, mut length)) = variable.take() {
            let mut flags = 0;
            match suffix {
                Some(b'$') => {
                    flags |= 0x80;
                    length += 1;
                    out.push(b'$');
                }
                Some(b'#') => {
                    flags |= 0x40;
                    length += 1;
                    out.push(b'#');
                }
                _ => {}
            }
            if array {
                flags |= 0x20;
            }
            out[at] = length | flags;
        }
    };
    while p < line.len() {
        let c = line[p];
        if rem {
            out.push(c);
            p += 1;
            continue;
        }
        if c == b'"' {
            match string.take() {
                Some(at) => {
                    let length = (out.len() - at - 4) as u32;
                    out[at..at + 4].copy_from_slice(&length.to_be_bytes());
                }
                None => {
                    close(&mut out, &mut variable, None, false);
                    fn_follows = false;
                    out.push(STRING);
                    poke_even(&mut out, 0);
                    string = Some(out.len() - 4);
                }
            }
            p += 1;
            continue;
        }
        if string.is_some() {
            out.push(c);
            p += 1;
            continue;
        }
        if c == b' ' {
            p += 1;
            continue;
        }
        // A keyword: while a variable's name is being read only the few that can end one.
        let table: &[(&str, &[u8])] = if variable.is_some() { &tokens::MINI } else { &tokens::TOKENS };
        if let Some((after, code)) = keyword(&line, p, table) {
            close(&mut out, &mut variable, None, false);
            out.extend_from_slice(code);
            if code[0] == REM {
                rem = true;
            }
            if (0x98..0xA0).contains(&code[0]) {
                poke_even(&mut out, 0);
            }
            fn_follows = code[0] == FN;
            p = after;
            continue;
        }
        // An extension's instruction or function, as the listing writes it.
        if variable.is_none()
            && let Some(rest) = text.get(p..).and_then(|t| t.strip_prefix("extension_"))
        {
            let letter = rest.bytes().next().unwrap_or(b'A');
            if let Some(code) = rest.get(2..4).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.extend_from_slice(&[if code % 2 == 0 { 0xA8 } else { 0xC0 }, letter.saturating_sub(b'A'), code]);
                p += "extension_".len() + 4;
                continue;
            }
        }
        let upper = c.to_ascii_uppercase();
        if let Some((_, length)) = variable.as_mut() {
            if upper == b'_' || upper.is_ascii_uppercase() || upper.is_ascii_digit() {
                *length += 1;
                if *length < 0x1E {
                    out.push(upper);
                    p += 1;
                } else {
                    // A name of 30: STOS counts this character but doesn't keep it, and starts a new name with it.
                    close(&mut out, &mut variable, None, false);
                }
                continue;
            }
            // The end of the name: a suffix, and a bracket after it for an array.
            let suffix = matches!(upper, b'$' | b'#').then_some(upper);
            if suffix.is_some() {
                p += 1;
            }
            let array = line.get(p) == Some(&b'(') && !fn_follows;
            close(&mut out, &mut variable, suffix, array);
            fn_follows = false;
            continue;
        }
        if upper.is_ascii_uppercase() {
            out.push(VARIABLE);
            poke_even(&mut out, 0);
            variable = Some((out.len() - 4, 1));
            out.push(upper);
            p += 1;
            continue;
        }
        fn_follows = false;
        if let Some((after, kind, value, second)) = number(&line, p) {
            out.push(kind);
            poke_even(&mut out, value);
            if let Some(second) = second {
                out.extend_from_slice(&second.to_be_bytes());
            }
            p = after;
            continue;
        }
        out.push(upper);
        p += 1;
    }
    close(&mut out, &mut variable, None, false);
    if let Some(at) = string {
        let length = (out.len() - at - 4) as u32;
        out[at..at + 4].copy_from_slice(&length.to_be_bytes());
    }
    out.push(0);
    if out.len() % 2 == 1 {
        out.push(0);
    }
    if out.len() > 0xFFFF {
        return Err(crate::i18n::tr("the line is too long").into());
    }
    let length = out.len() as u16;
    out[..2].copy_from_slice(&length.to_be_bytes());
    Ok(out)
}

/// Tokenises a listing into a STOS program, its lines in order of their numbers.
pub fn tokenise<S: AsRef<str>>(lines: &[S]) -> Result<Vec<u8>, String> {
    let mut tokenised: Vec<(usize, Vec<u8>)> = Vec::new();
    for (n, line) in lines.iter().enumerate() {
        let line = line.as_ref();
        if line.trim().is_empty() {
            continue;
        }
        let bytes = tokenise_line(line).map_err(|e| crate::i18n::tf!("Line {}: {e}", n + 1))?;
        let number = word(&bytes, 2);
        // A line given twice: the later one stands, as typing it again in STOS would.
        tokenised.retain(|(n, _)| *n != number);
        tokenised.push((number, bytes));
    }
    tokenised.sort_by_key(|(n, _)| *n);
    let mut program: Vec<u8> = tokenised.into_iter().flat_map(|(_, b)| b).collect();
    program.extend_from_slice(&[0, 0]);
    let mut out = HEADER.to_vec();
    out.extend_from_slice(&(program.len() as u32).to_be_bytes());
    out.extend_from_slice(&(program.len() as u32).to_be_bytes());
    out.extend_from_slice(&[0; 60]);
    out.extend(program);
    Ok(out)
}

#[cfg(test)]
mod tests;
