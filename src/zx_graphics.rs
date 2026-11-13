//! The Spectrum's graphic characters in a BASIC listing: its block graphics (codes 128 to 143) and its
//! user-defined graphics (UDGs, 144 to 164).
//!
//! Listings write them three ways, and all three are read when a listing is tokenised:
//! - Limoniter's own: the block graphics as the Unicode characters that look like them (▝ ▘ ▀ ...), and a UDG as
//!   `{UDG-A}`, with `{$xx}` for any code.
//! - zmakebas's: a backslash and a letter for a UDG (`\a`), or two of space, `'`, `.` and `:` for a block graphic,
//!   which draw its top and bottom halves (`\':` is code 135), and `\{n}` for any code.
//! - bas2tap's, which SGD and Taper write too: `{A}` for a UDG, `{-1}` to `{-8}` for the block graphics as GRAPH and
//!   a number types them, `{+1}` to `{+8}` for the inverted ones as GRAPH, Caps Shift and a number type them,
//!   `{(C)}` for the copyright sign and `{XX}` in hex for any code.
//!
//! This also has GRAPH mode's keys, and finding the pictures UDGs were given, so the editor can draw them.

use crate::charset::{self, Charset};

/// How graphic characters are written in a listing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Notation {
    #[default]
    Limoniter,
    Zmakebas,
    Bas2tap,
}

impl Notation {
    pub const ALL: [Self; 3] = [Self::Limoniter, Self::Zmakebas, Self::Bas2tap];

    /// As the settings file has it.
    pub fn id(self) -> &'static str {
        match self {
            Self::Limoniter => "limoniter",
            Self::Zmakebas => "zmakebas",
            Self::Bas2tap => "bas2tap",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|n| n.id() == id)
    }

    /// As Settings offers it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Limoniter => "▛ and {UDG-A} (Limoniter)",
            Self::Zmakebas => "\\:' and \\a (zmakebas)",
            Self::Bas2tap => "{-1} and {A} (bas2tap, SGD, Taper)",
        }
    }
}

/// The 21 UDGs' pictures, A to U, eight rows of eight pixels each, where they're known.
pub type Udgs = [Option<[u8; 8]>; 21];

/// What a Spectrum BASIC document keeps beyond its text: whether GRAPH mode is on, and the UDGs' pictures.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ZxText {
    pub graph: bool,
    pub udgs: Udgs,
    /// Where the pictures came from, for the status bar's tooltip.
    pub udgs_from: Option<String>,
    /// How GRAPH mode writes what it types, and so how UDGs are found to draw.
    pub notation: Notation,
}

impl ZxText {
    pub fn has_pictures(&self) -> bool {
        self.udgs.iter().any(Option::is_some)
    }
}

fn zx() -> &'static Charset {
    charset::known("zx-spectrum")
}

/// The character a block graphic is shown as.
pub fn block_char(code: u8) -> Option<char> {
    zx().entry(0, code).and_then(|e| e.unicode)
}

fn code_of_char(c: char) -> Option<u8> {
    zx().table(0).and_then(|t| t.byte_of(c))
}

/// What GRAPH and a number key type: 1 to 7 are the pictures on their keys and 8 is the empty one, and with Caps
/// Shift each comes out inverted.
pub fn graph_digit(digit: u8, inverted: bool) -> u8 {
    let code = 0x80 + digit % 8;
    if inverted { code ^ 0x0F } else { code }
}

/// zmakebas's two characters for a block graphic, from its four quarters: bit 1 is the top left, bit 0 the top
/// right, bit 3 the bottom left and bit 2 the bottom right.
const ZMAKEBAS_BLOCKS: [&str; 16] = ["  ", " '", "' ", "''", " .", " :", "'.", "':", ". ", ".'", ": ", ":'", "..", ".:", ":.", "::"];

/// A code written in a notation, for codes that aren't keywords or plain characters.
pub fn written(code: u8, notation: Notation) -> String {
    match notation {
        Notation::Limoniter => match code {
            0x90..=0xA4 => format!("{{UDG-{}}}", char::from(code - 0x90 + b'A')),
            _ => match block_char(code).filter(|c| !c.is_control() && *c != '{' && *c != '}') {
                Some(c) if (0x20..0x90).contains(&code) => c.to_string(),
                _ => format!("{{${code:02x}}}"),
            },
        },
        Notation::Zmakebas => match code {
            b'\\' => "\\\\".into(),
            0x7F => "\\*".into(),
            0x80..=0x8F => format!("\\{}", ZMAKEBAS_BLOCKS[usize::from(code - 0x80)]),
            0x90..=0xA4 => format!("\\{}", char::from(code - 0x90 + b'a')),
            // zmakebas reads bytes, so £ is ` and ↑ is ^, as the Spectrum's codes are.
            0x20..=0x7E if code != b'{' && code != b'}' => char::from(code).to_string(),
            _ => format!("\\{{{code}}}"),
        },
        Notation::Bas2tap => match code {
            0x7F => "{(C)}".into(),
            0x80..=0x87 => format!("{{-{}}}", if code == 0x80 { 8 } else { code - 0x80 }),
            0x88..=0x8F => format!("{{+{}}}", match (code - 0x88) ^ 7 { 0 => 8, n => n }),
            0x90..=0xA4 => format!("{{{}}}", char::from(code - 0x90 + b'A')),
            0x20..=0x7E if code != b'{' && code != b'}' => char::from(code).to_string(),
            _ => format!("{{{code:02X}}}"),
        },
    }
}

/// bas2tap's own escapes, as codes: `{A}` to `{U}`, `{-1}` to `{+8}`, `{(C)}`, `{CODE}`, `{CAT}` and `{XX}`.
pub fn bas2tap_escape(inside: &str) -> Option<u8> {
    let bytes = inside.as_bytes();
    match bytes {
        [letter] if letter.to_ascii_uppercase().is_ascii_uppercase() && letter.to_ascii_uppercase() <= b'U' => Some(0x90 + letter.to_ascii_uppercase() - b'A'),
        [b'-', digit @ b'1'..=b'8'] => Some(0x80 + (digit - b'0') % 8),
        [b'+', digit @ b'1'..=b'8'] => Some(0x88 + (((digit - b'0') % 8) ^ 7)),
        [a, b] if a.is_ascii_hexdigit() && b.is_ascii_hexdigit() => u8::from_str_radix(inside, 16).ok(),
        _ if inside.eq_ignore_ascii_case("(C)") => Some(0x7F),
        _ if inside.eq_ignore_ascii_case("CODE") => Some(0xAF),
        _ if inside.eq_ignore_ascii_case("CAT") => Some(0xCF),
        _ => None,
    }
}

/// A listing in Limoniter's notation, as it's written in another. Controls such as `{INK 2}` are the same in
/// bas2tap's; zmakebas has none, so there they're their codes.
pub fn rewrite(line: &str, notation: Notation) -> String {
    if notation == Notation::Limoniter {
        return line.to_string();
    }
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '{'
            && let Some(close) = chars[i..].iter().position(|ch| *ch == '}')
        {
            let inside: String = chars[i + 1..i + close].iter().collect();
            let control = inside.starts_with(|c: char| c.is_ascii_alphabetic()) && !inside.starts_with("UDG-");
            match crate::basic::zx_escape(&inside) {
                Ok(_) if control && notation == Notation::Bas2tap => out.push_str(&chars[i..=i + close].iter().collect::<String>()),
                Ok(codes) => codes.into_iter().for_each(|code| out.push_str(&written(code, notation))),
                Err(_) => out.push_str(&chars[i..=i + close].iter().collect::<String>()),
            }
            i += close + 1;
            continue;
        }
        match code_of_char(c) {
            Some(code) if code >= 0x7F || matches!(code, 0x5E | 0x60) || (notation == Notation::Zmakebas && c == '\\') => out.push_str(&written(code, notation)),
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

/// How long a zmakebas escape starting at a backslash is, if it is one.
fn zmakebas_escape_at(chars: &[char], at: usize) -> Option<(usize, Vec<u8>)> {
    let next = *chars.get(at + 1)?;
    match next {
        'a'..='u' | 'A'..='U' => Some((2, vec![0x90 + (next.to_ascii_lowercase() as u8 - b'a')])),
        '\\' => Some((2, vec![b'\\'])),
        '@' => Some((2, vec![b'@'])),
        '*' => Some((2, vec![0x7F])),
        ' ' | '\'' | '.' | ':' => {
            let pair: String = chars.get(at + 1..at + 3)?.iter().collect();
            ZMAKEBAS_BLOCKS.iter().position(|b| *b == pair).map(|n| (3, vec![0x80 + n as u8]))
        }
        '{' => {
            let close = chars[at..].iter().position(|c| *c == '}')?;
            let number: String = chars[at + 2..at + close].iter().collect::<String>().trim().to_string();
            let value = if let Some(hex) = number.strip_prefix("0x").or_else(|| number.strip_prefix("0X")) {
                u32::from_str_radix(hex, 16).ok()
            } else if number.len() > 1 && number.starts_with('0') {
                u32::from_str_radix(&number[1..], 8).ok()
            } else {
                number.parse().ok()
            }?;
            u8::try_from(value).ok().map(|code| (close + 1, vec![code]))
        }
        _ => None,
    }
}

/// Whether a listing is written for zmakebas: it has backslashes, and every one starts one of zmakebas's escapes.
/// Anywhere else a backslash is just that, so a listing with one that isn't an escape is read as typed.
pub fn uses_zmakebas<S: AsRef<str>>(lines: &[S]) -> bool {
    let mut found = false;
    for line in lines {
        let chars: Vec<char> = line.as_ref().chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\\' {
                // One at the end of a line carries the line on to the next.
                if i + 1 == chars.len() {
                    found = true;
                    break;
                }
                let Some((used, _)) = zmakebas_escape_at(&chars, i) else {
                    return false;
                };
                found = true;
                i += used;
            } else {
                i += 1;
            }
        }
    }
    found
}

/// A listing written for zmakebas, its lines that end in a backslash joined to the next, as zmakebas joins them,
/// and its comments, the lines starting with `#`, left out. A comment is blank so the lines keep their places.
pub fn join_zmakebas<S: AsRef<str>>(lines: &[S]) -> Vec<String> {
    let mut joined: Vec<String> = Vec::new();
    let mut carrying = false;
    for line in lines {
        let line = line.as_ref();
        if line.starts_with('#') && !carrying {
            joined.push(String::new());
            continue;
        }
        let (text, carries) = match line.strip_suffix('\\') {
            // An escaped backslash at the end is a backslash.
            Some(text) if !text.ends_with('\\') || text.chars().rev().take_while(|c| *c == '\\').count() % 2 == 0 => (text, true),
            _ => (line, false),
        };
        match joined.last_mut().filter(|_| carrying) {
            // zmakebas drops spaces outside strings, so the indent a continued line has isn't part of it.
            Some(last) => last.push_str(text.trim_start()),
            None => joined.push(text.to_string()),
        }
        carrying = carries;
    }
    joined
}

/// A line written for zmakebas, in Limoniter's notation. A brace is a character in zmakebas, unless it starts one
/// of Limoniter's or bas2tap's escapes, which are read too. zmakebas leaves tabs out, wherever they are.
pub fn from_zmakebas(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => match zmakebas_escape_at(&chars, i) {
                Some((used, codes)) => {
                    codes.into_iter().for_each(|code| out.push_str(&written(code, Notation::Limoniter)));
                    i += used;
                }
                None => {
                    out.push('\\');
                    i += 1;
                }
            },
            '{' => {
                let close = chars[i..].iter().position(|c| *c == '}');
                match close.filter(|close| crate::basic::zx_escape(&chars[i + 1..i + close].iter().collect::<String>()).is_ok()) {
                    Some(close) => {
                        out.extend(&chars[i..=i + close]);
                        i += close + 1;
                    }
                    None => {
                        out.push_str("{$7b}");
                        i += 1;
                    }
                }
            }
            '}' => {
                out.push_str("{$7d}");
                i += 1;
            }
            '\t' => i += 1,
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// What GRAPH mode types for a key, in a notation: a block graphic for 1 to 8 and a UDG for A to U.
pub fn graph_text(key: char, inverted: bool, notation: Notation) -> Option<String> {
    let code = match key.to_ascii_uppercase() {
        d @ '1'..='8' => graph_digit(d as u8 - b'0', inverted),
        l @ 'A'..='U' => 0x90 + (l as u8 - b'A'),
        _ => return None,
    };
    Some(written(code, notation))
}

/// Where UDGs are written on a line, in characters, with which UDG: `{UDG-A}` and bas2tap's `{A}`, and with
/// `zmakebas`, its `\a`.
pub fn udg_spans(line: &str, zmakebas: bool) -> Vec<(usize, usize, usize)> {
    let chars: Vec<char> = line.chars().collect();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '{' => {
                let Some(close) = chars[i..].iter().take(8).position(|c| *c == '}') else {
                    i += 1;
                    continue;
                };
                let inside: String = chars[i + 1..i + close].iter().collect();
                let letter = inside.strip_prefix("UDG-").unwrap_or(&inside);
                if let [l] = letter.as_bytes()
                    && (b'A'..=b'U').contains(&l.to_ascii_uppercase())
                    && (letter.len() < inside.len() || bas2tap_escape(&inside).is_some())
                {
                    spans.push((i, i + close + 1, usize::from(l.to_ascii_uppercase() - b'A')));
                }
                i += close + 1;
            }
            '\\' if zmakebas => match zmakebas_escape_at(&chars, i) {
                Some((used, codes)) => {
                    if let [code @ 0x90..=0xA4] = codes[..] {
                        spans.push((i, i + used, usize::from(code - 0x90)));
                    }
                    i += used;
                }
                None => i += 1,
            },
            _ => i += 1,
        }
    }
    spans
}

/// Pictures for UDGs from bytes, eight to a UDG, starting with `first` (0 for A).
pub fn udgs_from_bytes(bytes: &[u8], first: usize) -> Udgs {
    let mut udgs = Udgs::default();
    for (n, rows) in bytes.as_chunks::<8>().0.iter().enumerate() {
        if let Some(slot) = udgs.get_mut(first + n) {
            *slot = Some(*rows);
        }
    }
    udgs
}

/// USR "a" on a 48K Spectrum and a 16K one: where the UDGs start when nothing has moved them.
const UDG_AT: [usize; 2] = [0xFF58, 0x7F58];

/// The UDGs a tape loads: a CODE block that covers where they're kept, as LOAD "" CODE USR "a" loads one.
pub fn udgs_on_tape(data: &[u8]) -> Option<Udgs> {
    let (blocks, _) = crate::basic::tape_blocks(data);
    let mut header: Option<(usize, usize)> = None;
    let mut found: Option<Udgs> = None;
    for block in blocks {
        let (flag, body) = (block.first().copied(), block.get(1..block.len().saturating_sub(1)).unwrap_or(&[]));
        if flag == Some(0x00) && body.len() >= 17 {
            header = (body[0] == 3).then(|| (usize::from(u16::from_le_bytes([body[13], body[14]])), usize::from(u16::from_le_bytes([body[11], body[12]]))));
            continue;
        }
        if let Some((start, length)) = header.take() {
            let body = &body[..length.min(body.len())];
            for at in UDG_AT {
                if start <= at && start + body.len() >= at + 8 {
                    let bytes = &body[at - start..body.len().min(at - start + 168)];
                    let udgs = udgs_from_bytes(bytes, 0);
                    // A later block that covers them is what they'd end up as.
                    found = Some(udgs);
                    break;
                }
            }
        }
    }
    found
}

/// The UDGs a program sets up from its DATA lines, when it POKEs them in at USR "x": the numbers in its DATA
/// lines, in order, eight to a UDG from the first one it names. A DATA line that names a UDG first, as
/// `DATA "b",24,60,...` does, puts the eight numbers after it in that one. Lines with anything else in their DATA,
/// or numbers that aren't bytes, stop it.
pub fn udgs_from_data<S: AsRef<str>>(lines: &[S]) -> Option<Udgs> {
    let upper: Vec<String> = lines.iter().map(|l| l.as_ref().to_ascii_uppercase()).collect();
    let usr_at = upper.iter().find_map(|line| {
        let at = line.find("USR \"")?;
        (line.contains("POKE") && line.contains("READ")).then_some(())?;
        let letter = line.as_bytes().get(at + 5)?.to_ascii_uppercase();
        (b'A'..=b'U').contains(&letter).then(|| usize::from(letter - b'A'))
    })?;
    let mut udgs = Udgs::default();
    let mut next = usr_at;
    let mut row = 0;
    let mut any = false;
    for line in &upper {
        let Some(at) = data_start(line) else {
            continue;
        };
        for item in line[at..].split(',') {
            let item = item.trim();
            if let Some(name) = item.strip_prefix('"').and_then(|i| i.strip_suffix('"')) {
                let letter = name.bytes().next()?.to_ascii_uppercase();
                if name.len() != 1 || !(b'A'..=b'U').contains(&letter) || row != 0 {
                    return any.then_some(udgs);
                }
                next = usize::from(letter - b'A');
                continue;
            }
            let value = if let Some(bits) = item.strip_prefix("BIN") {
                u8::from_str_radix(bits.trim(), 2).ok()
            } else {
                item.parse::<u8>().ok()
            };
            let (Some(value), true) = (value, next < 21) else {
                return any.then_some(udgs);
            };
            udgs[next].get_or_insert([0; 8])[row] = value;
            any = true;
            row += 1;
            if row == 8 {
                row = 0;
                next += 1;
            }
        }
    }
    any.then_some(udgs)
}

/// Where a line's DATA items start, if it's a DATA line.
fn data_start(line: &str) -> Option<usize> {
    let body = line.trim_start().trim_start_matches(|c: char| c.is_ascii_digit()).trim_start();
    let at = line.len() - body.len();
    body.starts_with("DATA").then_some(at + 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_keys_type_what_the_spectrum_does() {
        assert_eq!(graph_digit(1, false), 0x81, "GRAPH 1 is the top right quarter");
        assert_eq!(graph_digit(8, false), 0x80, "GRAPH 8 is the empty one");
        assert_eq!(graph_digit(8, true), 0x8F, "and with Caps Shift the full block");
        assert_eq!(graph_digit(3, true), 0x8C, "GRAPH Caps Shift 3 is the bottom half");
        assert_eq!(graph_text('a', false, Notation::Limoniter).as_deref(), Some("{UDG-A}"));
        assert_eq!(graph_text('U', false, Notation::Zmakebas).as_deref(), Some("\\u"));
        assert_eq!(graph_text('3', false, Notation::Bas2tap).as_deref(), Some("{-3}"));
        assert_eq!(graph_text('3', true, Notation::Bas2tap).as_deref(), Some("{+3}"));
        assert_eq!(graph_text('v', false, Notation::Limoniter), None);
    }

    #[test]
    fn every_graphic_goes_through_each_notation_and_back() {
        for code in 0x80..=0xA4u8 {
            let ours = written(code, Notation::Limoniter);
            for notation in Notation::ALL {
                let line = format!("10 PRINT \"{ours}\"");
                let other = rewrite(&line, notation);
                let back = crate::basic::tokenise_spectrum(&[other.as_str()], crate::basic::ZxDialect::Spectrum).unwrap();
                assert_eq!(back[4..back.len() - 1], [0xF5, b'"', code, b'"'], "{code:02x} written {other} for {notation:?}");
            }
        }
    }

    #[test]
    fn zmakebas_and_bas2tap_write_them_as_their_tools_do() {
        // From zmakebas's manual: \': is 135 and \:. is 142.
        assert_eq!(written(135, Notation::Zmakebas), "\\':");
        assert_eq!(written(142, Notation::Zmakebas), "\\:.");
        // bas2tap: {-X} is 0x80 + X mod 8, {+X} is 0x88 + (X mod 8 xor 7).
        assert_eq!(bas2tap_escape("-8"), Some(0x80));
        assert_eq!(bas2tap_escape("+1"), Some(0x8E));
        assert_eq!(bas2tap_escape("+8"), Some(0x8F));
        assert_eq!(bas2tap_escape("(c)"), Some(0x7F));
        assert_eq!(bas2tap_escape("1f"), Some(0x1F));
        let line = "  10 PRINT {INK 2}\"\u{259B}{UDG-B}\\\u{A9}\"";
        assert_eq!(rewrite(line, Notation::Bas2tap), "  10 PRINT {INK 2}\"{+4}{B}\\{(C)}\"");
        assert_eq!(rewrite(line, Notation::Zmakebas), "  10 PRINT \\{16}\\{2}\"\\:'\\b\\\\\\*\"");
    }

    #[test]
    fn a_zmakebas_listing_is_known_by_its_backslashes() {
        assert!(uses_zmakebas(&["10 PRINT \"\\a\\::\\{0x41}\""]));
        assert!(!uses_zmakebas(&["10 PRINT \"C:\\WORK\""]), "\\W isn't an escape, so that backslash is a backslash");
        assert!(!uses_zmakebas(&["10 PRINT \"{UDG-A}\""]));
        assert_eq!(from_zmakebas("10 PRINT \"\\a{x}\\{65}\\''\""), "10 PRINT \"{UDG-A}{$7b}x{$7d}A\u{2580}\"");
        let program = crate::basic::tokenise_spectrum(&["10 PRINT \"\\a\\:'\\\\\"", "20 REM {"], crate::basic::ZxDialect::Spectrum).unwrap();
        assert_eq!(program[4..10], [0xF5, b'"', 0x90, 0x8B, b'\\', b'"']);
        assert_eq!(&program[program.len() - 3..], [0xEA, b'{', 0x0D], "a brace that starts no escape is a brace in zmakebas");
        // From zmakebas's demo: comments, a line carried on with a backslash, and tabs.
        let demo = ["# a comment", r#"10 print "\a";\"#, "\t   \"\\b\"", r"20 rem \\"];
        assert_eq!(join_zmakebas(&demo), ["", r#"10 print "\a";"\b""#, r"20 rem \\"]);
        let program = crate::basic::tokenise_spectrum(&demo, crate::basic::ZxDialect::Spectrum).unwrap();
        assert_eq!(program[4..13], [0xF5, b'"', 0x90, b'"', b';', b'"', 0x91, b'"', 0x0D]);
        assert_eq!(program[17..20], [0xEA, b'\\', 0x0D], "an escaped backslash at the end doesn't carry the line on");
    }

    /// Writes a listing with every graphic in each notation, and the program Limoniter makes of it, for
    /// zmakebas and bas2tap to be checked against: `ZX_NOTATION_OUT=folder cargo test -- --ignored notation_files`.
    #[test]
    #[ignore]
    fn notation_files() {
        let Some(folder) = std::env::var_os("ZX_NOTATION_OUT").map(std::path::PathBuf::from) else {
            return;
        };
        // A listing from elsewhere, such as zmakebas's demo, tokenised to compare with what its tool makes.
        if let Some(listing) = std::env::var_os("ZX_LISTING") {
            let text = std::fs::read_to_string(listing).unwrap();
            let lines: Vec<&str> = text.lines().collect();
            std::fs::write(folder.join("listing.bin"), crate::basic::tokenise_spectrum(&lines, crate::basic::ZxDialect::Spectrum).unwrap()).unwrap();
        }
        let blocks: String = (0x80..=0x8Fu8).map(|code| written(code, Notation::Limoniter)).collect();
        let udgs: String = (0x90..=0xA4u8).map(|code| written(code, Notation::Limoniter)).collect();
        let lines = [format!("10 PRINT \"{blocks}\""), format!("20 PRINT \"{udgs}\""), "30 PRINT {INK 2}\"\\\u{A9}\u{A3}\"".to_string(), format!("40 REM {blocks}{udgs}")];
        let program = crate::basic::tokenise_spectrum(&lines, crate::basic::ZxDialect::Spectrum).unwrap();
        std::fs::write(folder.join("ours.bin"), &program).unwrap();
        for notation in Notation::ALL {
            let text: Vec<String> = lines.iter().map(|line| rewrite(line, notation)).collect();
            assert_eq!(crate::basic::tokenise_spectrum(&text, crate::basic::ZxDialect::Spectrum).unwrap(), program, "{notation:?}");
            std::fs::write(folder.join(format!("{}.bas", notation.id())), text.join("\n") + "\n").unwrap();
        }
    }

    #[test]
    fn udgs_are_found_in_each_notation() {
        assert_eq!(udg_spans("10 PRINT \"{UDG-A}x{b}{AT 1,2}{-1}\"", false), [(10, 17, 0), (18, 21, 1)]);
        assert_eq!(udg_spans("10 PRINT \"\\a\\\\\\u\"", true), [(10, 12, 0), (14, 16, 20)]);
        assert!(udg_spans("10 PRINT \"\\a\"", false).is_empty());
    }

    #[test]
    fn udg_pictures_come_from_data_lines_and_tapes() {
        let lines = ["10 FOR n=0 TO 15: READ a: POKE USR \"c\"+n,a: NEXT n", "20 DATA 1,2,3,4,5,6,7,8", "30 DATA BIN 11110000,255,0,0,0,0,0,9"];
        let udgs = udgs_from_data(&lines).unwrap();
        assert_eq!(udgs[2], Some([1, 2, 3, 4, 5, 6, 7, 8]));
        assert_eq!(udgs[3], Some([0xF0, 255, 0, 0, 0, 0, 0, 9]));
        assert_eq!(udgs[0], None);
        let named = ["10 FOR n=1 TO 2: READ u$: FOR r=0 TO 7: READ a: POKE USR u$+r,a: NEXT r: NEXT n", "20 DATA \"b\",1,1,1,1,1,1,1,1,\"t\",2,2,2,2,2,2,2,2"];
        assert_eq!(udgs_from_data(&named), None, "USR u$ names no UDG, so its DATA could be anything");
        let named = ["5 LET x=USR \"a\": POKE x,0: READ y", "20 DATA \"b\",1,1,1,1,1,1,1,1,\"t\",2,2,2,2,2,2,2,2"];
        let udgs = udgs_from_data(&named).unwrap();
        assert_eq!((udgs[1], udgs[19]), (Some([1; 8]), Some([2; 8])));
        assert_eq!(udgs_from_data(&["10 PRINT 1", "20 DATA 1,2"]), None, "with no POKE USR, DATA is just data");

        let mut code = vec![3u8];
        code.extend(b"udg       ");
        code.extend(16u16.to_le_bytes());
        code.extend(0xFF58u16.to_le_bytes());
        code.extend(0x8000u16.to_le_bytes());
        let pictures: Vec<u8> = (1..=16).collect();
        let mut tape = crate::basic::tap_block(0x00, &code);
        tape.extend(crate::basic::tap_block(0xFF, &pictures));
        let udgs = udgs_on_tape(&tape).unwrap();
        assert_eq!(udgs[0], Some([1, 2, 3, 4, 5, 6, 7, 8]));
        assert_eq!(udgs[1], Some([9, 10, 11, 12, 13, 14, 15, 16]));
        assert_eq!(udgs[2], None);
    }
}
