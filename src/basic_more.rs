//! More tokenised BASICs: BBC BASIC (listed and tokenised), MSX-BASIC and Atari BASIC (listed).
//!
//! - BBC BASIC II and IV, on the BBC Micro and Master: the keyword table, tokens and flags are the BASIC IV ROM's
//!   own (8bs.com's disassembly, "8456 BASIC Keyword List"), in the ROM's order, which is the order it matches
//!   keywords and abbreviations in. Line numbers after GOTO and the like are the three-byte form xania.org's
//!   "BBC BASIC binary line number format" describes. Tokenising follows the ROM's flags: conditional keywords,
//!   pseudo-variables taking +&40 at the start of a statement, line numbers, and nothing tokenised after REM,
//!   DATA, `*` commands, FN and PROC names, strings and hex numbers.
//! - MSX-BASIC: the intermediate codes and number formats of the MSX2 Technical Handbook (ASCII Corporation, 1987),
//!   chapter 2, table 2.20 and figure 2.12. Listed only: which numbers MSX keeps as single or double precision,
//!   and how LIST then writes them, couldn't be checked against a machine, so tokenising isn't offered.
//! - Atari BASIC: the statement and operator tokens and the file layout as Jindrich Kubec's "Atari BASIC File
//!   Format Description" (gury.atari8.info) gives them. Listed only: Atari's tokeniser is its syntax checker,
//!   which chooses among several tokens for the same `=` and `(`, and builds the variable tables.

use crate::basic::Listing;
use crate::charset::{self, Charset};

// ---------- BBC BASIC ----------

/// The BASIC IV ROM's keyword table: keyword, token and flags, in the ROM's order.
/// Flags: bit 0 conditional (not a keyword when a name character follows), bit 1 middle of statement,
/// bit 2 start of statement, bit 3 FN or PROC (the name after it isn't tokenised), bit 4 line numbers follow,
/// bit 5 nothing more on the line is tokenised, bit 6 pseudo-variable (+&40 at the start of a statement).
const BBC_KEYWORDS: [(&str, u8, u8); 127] = [
    ("AND", 0x80, 0x00),
    ("ABS", 0x94, 0x00),
    ("ACS", 0x95, 0x00),
    ("ADVAL", 0x96, 0x00),
    ("ASC", 0x97, 0x00),
    ("ASN", 0x98, 0x00),
    ("ATN", 0x99, 0x00),
    ("AUTO", 0xC6, 0x10),
    ("BGET", 0x9A, 0x01),
    ("BPUT", 0xD5, 0x03),
    ("COLOUR", 0xFB, 0x02),
    ("CALL", 0xD6, 0x02),
    ("CHAIN", 0xD7, 0x02),
    ("CHR$", 0xBD, 0x00),
    ("CLEAR", 0xD8, 0x01),
    ("CLOSE", 0xD9, 0x03),
    ("CLG", 0xDA, 0x01),
    ("CLS", 0xDB, 0x01),
    ("COS", 0x9B, 0x00),
    ("COUNT", 0x9C, 0x01),
    ("COLOR", 0xFB, 0x02),
    ("DATA", 0xDC, 0x20),
    ("DEG", 0x9D, 0x00),
    ("DEF", 0xDD, 0x00),
    ("DELETE", 0xC7, 0x10),
    ("DIV", 0x81, 0x00),
    ("DIM", 0xDE, 0x02),
    ("DRAW", 0xDF, 0x02),
    ("ENDPROC", 0xE1, 0x01),
    ("END", 0xE0, 0x01),
    ("ENVELOPE", 0xE2, 0x02),
    ("ELSE", 0x8B, 0x14),
    ("EVAL", 0xA0, 0x00),
    ("ERL", 0x9E, 0x01),
    ("ERROR", 0x85, 0x04),
    ("EOF", 0xC5, 0x01),
    ("EOR", 0x82, 0x00),
    ("ERR", 0x9F, 0x01),
    ("EXP", 0xA1, 0x00),
    ("EXT", 0xA2, 0x01),
    ("EDIT", 0xCE, 0x10),
    ("FOR", 0xE3, 0x02),
    ("FALSE", 0xA3, 0x01),
    ("FN", 0xA4, 0x08),
    ("GOTO", 0xE5, 0x12),
    ("GET$", 0xBE, 0x00),
    ("GET", 0xA5, 0x00),
    ("GOSUB", 0xE4, 0x12),
    ("GCOL", 0xE6, 0x02),
    ("HIMEM", 0x93, 0x43),
    ("INPUT", 0xE8, 0x02),
    ("IF", 0xE7, 0x02),
    ("INKEY$", 0xBF, 0x00),
    ("INKEY", 0xA6, 0x00),
    ("INT", 0xA8, 0x00),
    ("INSTR(", 0xA7, 0x00),
    ("LIST", 0xC9, 0x10),
    ("LINE", 0x86, 0x00),
    ("LOAD", 0xC8, 0x02),
    ("LOMEM", 0x92, 0x43),
    ("LOCAL", 0xEA, 0x02),
    ("LEFT$(", 0xC0, 0x00),
    ("LEN", 0xA9, 0x00),
    ("LET", 0xE9, 0x04),
    ("LOG", 0xAB, 0x00),
    ("LN", 0xAA, 0x00),
    ("MID$(", 0xC1, 0x00),
    ("MODE", 0xEB, 0x02),
    ("MOD", 0x83, 0x00),
    ("MOVE", 0xEC, 0x02),
    ("NEXT", 0xED, 0x02),
    ("NEW", 0xCA, 0x01),
    ("NOT", 0xAC, 0x00),
    ("OLD", 0xCB, 0x01),
    ("ON", 0xEE, 0x02),
    ("OFF", 0x87, 0x00),
    ("OR", 0x84, 0x00),
    ("OPENIN", 0x8E, 0x00),
    ("OPENOUT", 0xAE, 0x00),
    ("OPENUP", 0xAD, 0x00),
    ("OSCLI", 0xFF, 0x02),
    ("PRINT", 0xF1, 0x02),
    ("PAGE", 0x90, 0x43),
    ("PTR", 0x8F, 0x43),
    ("PI", 0xAF, 0x01),
    ("PLOT", 0xF0, 0x02),
    ("POINT(", 0xB0, 0x00),
    ("PROC", 0xF2, 0x0A),
    ("POS", 0xB1, 0x01),
    ("RETURN", 0xF8, 0x01),
    ("REPEAT", 0xF5, 0x00),
    ("REPORT", 0xF6, 0x01),
    ("READ", 0xF3, 0x02),
    ("REM", 0xF4, 0x20),
    ("RUN", 0xF9, 0x01),
    ("RAD", 0xB2, 0x00),
    ("RESTORE", 0xF7, 0x12),
    ("RIGHT$(", 0xC2, 0x00),
    ("RND", 0xB3, 0x01),
    ("RENUMBER", 0xCC, 0x10),
    ("STEP", 0x88, 0x00),
    ("SAVE", 0xCD, 0x02),
    ("SGN", 0xB4, 0x00),
    ("SIN", 0xB5, 0x00),
    ("SQR", 0xB6, 0x00),
    ("SPC", 0x89, 0x00),
    ("STR$", 0xC3, 0x00),
    ("STRING$(", 0xC4, 0x00),
    ("SOUND", 0xD4, 0x02),
    ("STOP", 0xFA, 0x01),
    ("TAN", 0xB7, 0x00),
    ("THEN", 0x8C, 0x14),
    ("TO", 0xB8, 0x00),
    ("TAB(", 0x8A, 0x00),
    ("TRACE", 0xFC, 0x12),
    ("TIME", 0x91, 0x43),
    ("TRUE", 0xB9, 0x01),
    ("UNTIL", 0xFD, 0x02),
    ("USR", 0xBA, 0x00),
    ("VDU", 0xEF, 0x02),
    ("VAL", 0xBB, 0x00),
    ("VPOS", 0xBC, 0x01),
    ("WIDTH", 0xFE, 0x02),
    ("PTR", 0xCF, 0x00),
    ("TIME", 0xD1, 0x00),
    ("LOMEM", 0xD2, 0x00),
    ("HIMEM", 0xD3, 0x00),
];
/// The token before a line number written in three bytes.
const BBC_LINE_NUMBER: u8 = 0x8D;

/// A BBC BASIC token's keyword. PAGE's statement form, &D0, isn't in the ROM's table: it's PAGE plus &40.
fn bbc_keyword(token: u8) -> Option<&'static str> {
    if token == 0xD0 {
        return Some("PAGE");
    }
    BBC_KEYWORDS.iter().find(|(_, t, _)| *t == token).map(|(word, _, _)| *word)
}

/// The three bytes after &8D: the top two bits of each byte of the line number, as `00LlHh00` exclusive-ORed with
/// &54, then the low six bits of the low byte and of the high byte, each ORed with &40.
pub fn bbc_line_bytes(number: u16) -> [u8; 3] {
    let [low, high] = number.to_le_bytes();
    [((low & 0xC0) >> 2 | (high & 0xC0) >> 4) ^ 0x54, (low & 0x3F) | 0x40, (high & 0x3F) | 0x40]
}

pub fn bbc_line_number(bytes: [u8; 3]) -> u16 {
    let top = bytes[0] ^ 0x54;
    let low = (bytes[1] & 0x3F) | ((top << 2) & 0xC0);
    let high = (bytes[2] & 0x3F) | ((top << 4) & 0xC0);
    u16::from_le_bytes([low, high])
}

/// A character of BBC listing text: printable ASCII as itself, anything else, and the braces, as `{$xx}`.
fn bbc_char(byte: u8, out: &mut String) {
    match byte {
        0x20..=0x7E if byte != b'{' && byte != b'}' => out.push(byte as char),
        _ => out.push_str(&format!("{{${byte:02x}}}")),
    }
}

/// Whether bytes look like a BBC BASIC program: lines that each start with &0D, ending with &0D &FF.
pub fn looks_like_bbc(data: &[u8]) -> bool {
    let mut at = 0;
    while at + 1 < data.len() && data[at] == 0x0D {
        if data[at + 1] == 0xFF {
            return at > 0 || data.len() == 2;
        }
        let length = usize::from(*data.get(at + 3).unwrap_or(&0));
        if length < 4 {
            return false;
        }
        at += length;
    }
    false
}

/// BBC BASIC V's two-byte keywords (RISC OS): &C6 for functions, &C7 for commands and &C8 for statements, then a
/// byte from &8E, as the BBC BASIC Reference Manual tabulates them.
const BASIC_V_FUNCTIONS: [&str; 2] = ["SUM", "BEAT"];
const BASIC_V_COMMANDS: [&str; 18] = ["APPEND", "AUTO", "CRUNCH", "DELETE", "EDIT", "HELP", "LIST", "LOAD", "LVAR", "NEW", "OLD", "RENUMBER", "SAVE", "TEXTLOAD", "TEXTSAVE", "TWIN", "TWINO", "INSTALL"];
const BASIC_V_STATEMENTS: [&str; 22] = ["CASE", "CIRCLE", "FILL", "ORIGIN", "POINT", "RECTANGLE", "SWAP", "WHILE", "WAIT", "MOUSE", "QUIT", "SYS", "INSTALL", "LIBRARY", "TINT", "ELLIPSE", "BEATS", "TEMPO", "VOICES", "VOICE", "STEREO", "OVERLAY"];

/// A BASIC V keyword at a prefix byte and the byte after it.
fn basic_v_extended(prefix: u8, next: u8) -> Option<&'static str> {
    let table: &[&str] = match prefix {
        0xC6 => &BASIC_V_FUNCTIONS,
        0xC7 => &BASIC_V_COMMANDS,
        0xC8 => &BASIC_V_STATEMENTS,
        _ => return None,
    };
    table.get(usize::from(next.checked_sub(0x8E)?)).copied()
}

/// BASIC V's single-byte keywords where they differ from BASIC II's (whose &C9 to &CE are commands that a program
/// doesn't hold), and &7F for OTHERWISE.
fn basic_v_keyword(token: u8) -> Option<&'static str> {
    Some(match token {
        0x7F => "OTHERWISE",
        0xC9 => "WHEN",
        0xCA => "OF",
        0xCB => "ENDCASE",
        0xCC => "ELSE",
        0xCD => "ENDIF",
        0xCE => "ENDWHILE",
        _ => return None,
    })
}

/// Whether a program is RISC OS's BASIC V: it holds one of BASIC V's two-byte keywords outside a string, where
/// BASIC II would have LOAD, AUTO or DELETE, which a program doesn't hold, followed straight by a keyword.
fn is_basic_v(data: &[u8]) -> bool {
    let mut at = 0;
    while at + 3 < data.len() && data[at] == 0x0D && data[at + 1] != 0xFF {
        let end = (at + usize::from(data[at + 3])).min(data.len());
        if end <= at + 3 {
            return false;
        }
        let mut quoted = false;
        for pair in data[at + 4..end].windows(2) {
            match pair[0] {
                b'"' => quoted = !quoted,
                0xF4 | 0xDC if !quoted => break,
                prefix if !quoted && basic_v_extended(prefix, pair[1]).is_some() => return true,
                _ => {}
            }
        }
        at = end;
    }
    false
}

/// Lists a BBC BASIC program as LIST does with LISTO 0: the line number in five columns, then the line as stored.
/// A RISC OS program's BASIC V keywords are listed as BASIC V lists them.
pub fn list_bbc(data: &[u8]) -> Listing {
    let basic_v = is_basic_v(data);
    let mut listing = Listing { dialect: if basic_v { "BBC BASIC V" } else { "BBC BASIC" }, ..Listing::default() };
    let mut at = 0;
    loop {
        if at + 1 >= data.len() || data[at] != 0x0D {
            listing.notes.push(format!("The program stops making sense at byte {at}, so the listing stops there."));
            break;
        }
        if data[at + 1] == 0xFF {
            at += 2;
            break;
        }
        let (Some(&high), Some(&low), Some(&length)) = (data.get(at + 1), data.get(at + 2), data.get(at + 3)) else {
            listing.notes.push("The program ends partway through a line.".into());
            break;
        };
        let end = at + usize::from(length);
        if length < 4 || end > data.len() {
            listing.notes.push(format!("A line at byte {at} is cut short, so the listing stops there."));
            break;
        }
        let mut text = format!("{:>5}", u16::from_be_bytes([high, low]));
        let body = &data[at + 4..end];
        let (mut quoted, mut rest_plain) = (false, false);
        let mut i = 0;
        while i < body.len() {
            let byte = body[i];
            i += 1;
            if byte == b'"' && !rest_plain {
                quoted = !quoted;
            }
            if basic_v && !quoted && !rest_plain {
                if let Some(word) = body.get(i).and_then(|next| basic_v_extended(byte, *next)) {
                    text.push_str(word);
                    i += 1;
                    continue;
                }
                if let Some(word) = basic_v_keyword(byte) {
                    text.push_str(word);
                    continue;
                }
            }
            if quoted || rest_plain || byte < 0x80 {
                bbc_char(byte, &mut text);
            } else if byte == BBC_LINE_NUMBER && i + 3 <= body.len() {
                text.push_str(&bbc_line_number([body[i], body[i + 1], body[i + 2]]).to_string());
                i += 3;
            } else if let Some(word) = bbc_keyword(byte) {
                text.push_str(word);
                rest_plain = matches!(byte, 0xF4 | 0xDC);
            } else {
                bbc_char(byte, &mut text);
            }
        }
        listing.lines.push(text);
        at = end;
    }
    if at < data.len() {
        listing.notes.push(format!("{} byte(s) after the program aren't listed.", data.len() - at));
    }
    listing
}

fn bbc_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '`'
}

/// Tokenises one line's text after its number, as the ROM's tokeniser does.
fn bbc_crunch(text: &str) -> Result<Vec<u8>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut start = true;
    let mut line_numbers = false;
    let push_char = |out: &mut Vec<u8>, c: char| -> Result<(), String> {
        u8::try_from(c as u32).ok().filter(u8::is_ascii).map(|b| out.push(b)).ok_or_else(|| format!("'{c}' isn't in BBC BASIC's character set"))
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '{'
            && let Some(close) = chars[i..].iter().position(|ch| *ch == '}')
        {
            let inside: String = chars[i + 1..i + close].iter().collect();
            let byte = inside.strip_prefix('$').and_then(|hex| u8::from_str_radix(hex, 16).ok()).ok_or_else(|| format!("{{{inside}}} isn't a byte"))?;
            out.push(byte);
            i += close + 1;
            continue;
        }
        if c == '"' {
            let close = chars[i + 1..].iter().position(|ch| *ch == '"').map_or(chars.len(), |n| i + 1 + n + 1);
            for &ch in &chars[i..close.min(chars.len())] {
                push_char(&mut out, ch)?;
            }
            i = close;
            start = false;
            continue;
        }
        if line_numbers {
            if c.is_ascii_digit() {
                let digits: String = chars[i..].iter().take_while(|ch| ch.is_ascii_digit()).collect();
                let number: u16 = digits.parse().ok().filter(|n| *n <= 32767).ok_or_else(|| format!("{digits} is past the last line number there can be, 32767"))?;
                out.push(BBC_LINE_NUMBER);
                out.extend(bbc_line_bytes(number));
                i += digits.len();
                continue;
            }
            if c != ',' && c != ' ' {
                line_numbers = false;
            }
        }
        if c == ':' {
            out.push(b':');
            i += 1;
            start = true;
            line_numbers = false;
            continue;
        }
        if c == '*' && start {
            for &ch in &chars[i..] {
                push_char(&mut out, ch)?;
            }
            break;
        }
        if c == '&' {
            let digits = chars[i + 1..].iter().take_while(|ch| ch.is_ascii_hexdigit()).count();
            for &ch in &chars[i..=i + digits] {
                push_char(&mut out, ch)?;
            }
            i += 1 + digits;
            start = false;
            continue;
        }
        if c.is_ascii_digit() || c == '.' {
            let length = chars[i..].iter().take_while(|ch| ch.is_ascii_digit() || **ch == '.').count();
            for &ch in &chars[i..i + length] {
                push_char(&mut out, ch)?;
            }
            i += length;
            start = false;
            continue;
        }
        if c.is_ascii_uppercase() {
            // The ROM tries each keyword in turn; a full stop ends an abbreviation.
            let found = BBC_KEYWORDS.iter().find_map(|&(word, token, flags)| {
                let letters: Vec<char> = word.chars().collect();
                for (k, &letter) in letters.iter().enumerate() {
                    match chars.get(i + k) {
                        Some('.') if k > 0 => return Some((token, flags, k + 1, true)),
                        Some(&ch) if ch == letter => {}
                        _ => return None,
                    }
                }
                Some((token, flags, letters.len(), false))
            });
            if let Some((token, flags, used, abbreviated)) = found {
                let next_is_name = chars.get(i + used).is_some_and(|ch| bbc_name_char(*ch));
                if abbreviated || flags & 0x01 == 0 || !next_is_name {
                    out.push(if flags & 0x40 != 0 && start { token + 0x40 } else { token });
                    i += used;
                    if flags & 0x08 != 0 {
                        while chars.get(i).is_some_and(|ch| bbc_name_char(*ch)) {
                            push_char(&mut out, chars[i])?;
                            i += 1;
                        }
                    }
                    if flags & 0x10 != 0 {
                        line_numbers = true;
                    }
                    if flags & 0x20 != 0 {
                        for &ch in &chars[i..] {
                            push_char(&mut out, ch)?;
                        }
                        break;
                    }
                    if flags & 0x02 != 0 {
                        start = false;
                    }
                    if flags & 0x04 != 0 {
                        start = true;
                    }
                    continue;
                }
            }
        }
        if bbc_name_char(c) {
            // A name is taken whole, so a keyword inside it isn't tokenised.
            while chars.get(i).is_some_and(|ch| bbc_name_char(*ch)) {
                push_char(&mut out, chars[i])?;
                i += 1;
            }
            start = false;
            continue;
        }
        push_char(&mut out, c)?;
        if c != ' ' {
            start = false;
        }
        i += 1;
    }
    Ok(out)
}

/// Tokenises a listing into a BBC BASIC program, as SAVE writes it.
pub fn tokenise_bbc<S: AsRef<str>>(lines: &[S]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut previous: Option<u16> = None;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.as_ref().trim_start();
        if trimmed.trim().is_empty() {
            continue;
        }
        let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
        let number: u16 = trimmed[..digits].parse().map_err(|_| format!("Line {} doesn't start with a line number.", index + 1))?;
        if number > 32767 {
            return Err(format!("Line {}: {number} is past the last line number there can be, 32767.", index + 1));
        }
        if previous.is_some_and(|p| number <= p) {
            return Err(format!("Line {}: line numbers have to go up, and {number} comes after {}.", index + 1, previous.unwrap_or(0)));
        }
        previous = Some(number);
        let body = bbc_crunch(&trimmed[digits..]).map_err(|problem| format!("Line {} (BASIC line {number}): {problem}", index + 1))?;
        let length = u8::try_from(body.len() + 4).map_err(|_| format!("Line {} is longer than the 251 bytes a line can hold.", index + 1))?;
        out.push(0x0D);
        out.extend(number.to_be_bytes());
        out.push(length);
        out.extend(body);
    }
    out.extend([0x0D, 0xFF]);
    Ok(out)
}

// ---------- MSX-BASIC ----------

/// MSX-BASIC's one-byte intermediate codes, from table 2.20.
const MSX_CODES: [(u8, &str); 106] = [
    (0x81, "END"),
    (0x82, "FOR"),
    (0x83, "NEXT"),
    (0x84, "DATA"),
    (0x85, "INPUT"),
    (0x86, "DIM"),
    (0x87, "READ"),
    (0x88, "LET"),
    (0x89, "GOTO"),
    (0x8A, "RUN"),
    (0x8B, "IF"),
    (0x8C, "RESTORE"),
    (0x8D, "GOSUB"),
    (0x8E, "RETURN"),
    (0x8F, "REM"),
    (0x90, "STOP"),
    (0x91, "PRINT"),
    (0x92, "CLEAR"),
    (0x93, "LIST"),
    (0x94, "NEW"),
    (0x95, "ON"),
    (0x96, "WAIT"),
    (0x97, "DEF"),
    (0x98, "POKE"),
    (0x99, "CONT"),
    (0x9A, "CSAVE"),
    (0x9B, "CLOAD"),
    (0x9C, "OUT"),
    (0x9D, "LPRINT"),
    (0x9E, "LLIST"),
    (0x9F, "CLS"),
    (0xA0, "WIDTH"),
    (0xA1, "ELSE"),
    (0xA2, "TRON"),
    (0xA3, "TROFF"),
    (0xA4, "SWAP"),
    (0xA5, "ERASE"),
    (0xA6, "ERROR"),
    (0xA7, "RESUME"),
    (0xA8, "DELETE"),
    (0xA9, "AUTO"),
    (0xAA, "RENUM"),
    (0xAB, "DEFSTR"),
    (0xAC, "DEFINT"),
    (0xAD, "DEFSNG"),
    (0xAE, "DEFDBL"),
    (0xAF, "LINE"),
    (0xB0, "OPEN"),
    (0xB1, "FIELD"),
    (0xB2, "GET"),
    (0xB3, "PUT"),
    (0xB4, "CLOSE"),
    (0xB5, "LOAD"),
    (0xB6, "MERGE"),
    (0xB7, "FILES"),
    (0xB8, "LSET"),
    (0xB9, "RSET"),
    (0xBA, "SAVE"),
    (0xBB, "LFILES"),
    (0xBC, "CIRCLE"),
    (0xBD, "COLOR"),
    (0xBE, "DRAW"),
    (0xBF, "PAINT"),
    (0xC0, "BEEP"),
    (0xC1, "PLAY"),
    (0xC2, "PSET"),
    (0xC3, "PRESET"),
    (0xC4, "SOUND"),
    (0xC5, "SCREEN"),
    (0xC6, "VPOKE"),
    (0xC7, "SPRITE"),
    (0xC8, "VDP"),
    (0xC9, "BASE"),
    (0xCA, "CALL"),
    (0xCB, "TIME"),
    (0xCC, "KEY"),
    (0xCD, "MAX"),
    (0xCE, "MOTOR"),
    (0xCF, "BLOAD"),
    (0xD0, "BSAVE"),
    (0xD1, "DSKO$"),
    (0xD2, "SET"),
    (0xD3, "NAME"),
    (0xD4, "KILL"),
    (0xD5, "IPL"),
    (0xD6, "COPY"),
    (0xD7, "CMD"),
    (0xD8, "LOCATE"),
    (0xD9, "TO"),
    (0xDA, "THEN"),
    (0xDB, "TAB("),
    (0xDC, "STEP"),
    (0xDD, "USR"),
    (0xDE, "FN"),
    (0xDF, "SPC("),
    (0xE0, "NOT"),
    (0xE1, "ERL"),
    (0xE2, "ERR"),
    (0xE3, "STRING$"),
    (0xE4, "USING"),
    (0xE5, "INSTR"),
    (0xE7, "VARPTR"),
    (0xE8, "CSRLIN"),
    (0xE9, "ATTR$"),
    (0xEA, "DSKI$"),
    (0xEB, "OFF"),
];
/// The rest of the one-byte codes: INKEY$, POINT and the operators.
const MSX_OPERATORS: [(u8, &str); 18] = [
    (0xEC, "INKEY$"),
    (0xED, "POINT"),
    (0xEE, ">"),
    (0xEF, "="),
    (0xF0, "<"),
    (0xF1, "+"),
    (0xF2, "-"),
    (0xF3, "*"),
    (0xF4, "/"),
    (0xF5, "^"),
    (0xF6, "AND"),
    (0xF7, "OR"),
    (0xF8, "XOR"),
    (0xF9, "EQV"),
    (0xFA, "IMP"),
    (0xFB, "MOD"),
    (0xFC, "\\"),
    // The apostrophe comment: stored as :REM' (3A 8F E6), listed as just '.
    (0xE6, "'"),
];
/// The functions, which take two bytes, &FF and then these.
const MSX_FUNCTIONS: [(u8, &str); 46] = [
    (0x81, "LEFT$"),
    (0x82, "RIGHT$"),
    (0x83, "MID$"),
    (0x84, "SGN"),
    (0x85, "INT"),
    (0x86, "ABS"),
    (0x87, "SQR"),
    (0x88, "RND"),
    (0x89, "SIN"),
    (0x8A, "LOG"),
    (0x8B, "EXP"),
    (0x8C, "COS"),
    (0x8D, "TAN"),
    (0x8E, "ATN"),
    (0x8F, "FRE"),
    (0x90, "INP"),
    (0x91, "POS"),
    (0x92, "LEN"),
    (0x93, "STR$"),
    (0x94, "VAL"),
    (0x95, "ASC"),
    (0x96, "CHR$"),
    (0x97, "PEEK"),
    (0x98, "VPEEK"),
    (0x99, "SPACE$"),
    (0x9A, "OCT$"),
    (0x9B, "HEX$"),
    (0x9C, "LPOS"),
    (0x9D, "BIN$"),
    (0x9E, "CINT"),
    (0x9F, "CSNG"),
    (0xA0, "CDBL"),
    (0xA1, "FIX"),
    (0xA2, "STICK"),
    (0xA3, "STRIG"),
    (0xA4, "PDL"),
    (0xA5, "PAD"),
    (0xA6, "DSKF"),
    (0xA7, "FPOS"),
    (0xA8, "CVI"),
    (0xA9, "CVS"),
    (0xAA, "CVD"),
    (0xAB, "EOF"),
    (0xAC, "LOC"),
    (0xAD, "LOF"),
    (0xAE, "MKI$"),
];
const MSX_MORE_FUNCTIONS: [(u8, &str); 2] = [(0xAF, "MKS$"), (0xB0, "MKD$")];

fn msx_code(byte: u8) -> Option<&'static str> {
    MSX_CODES.iter().chain(&MSX_OPERATORS).find(|(b, _)| *b == byte).map(|(_, word)| *word)
}

fn msx_function(byte: u8) -> Option<&'static str> {
    MSX_FUNCTIONS.iter().chain(&MSX_MORE_FUNCTIONS).find(|(b, _)| *b == byte).map(|(_, word)| *word)
}

/// A number in MSX's Math-Pack form: a sign and excess-64 exponent byte, then packed decimal digits.
fn msx_float(bytes: &[u8]) -> String {
    let Some((&exponent, digits)) = bytes.split_first() else {
        return String::new();
    };
    let mut figures: String = digits.iter().map(|b| format!("{}{}", b >> 4, b & 0x0F)).collect();
    while figures.ends_with('0') {
        figures.pop();
    }
    if figures.is_empty() {
        return "0".to_string();
    }
    let sign = if exponent & 0x80 != 0 { "-" } else { "" };
    let point = i32::from(exponent & 0x7F) - 64;
    let length = figures.len() as i32;
    let text = if (1..=14).contains(&point) {
        if point >= length { format!("{figures}{}", "0".repeat((point - length) as usize)) } else { format!("{}.{}", &figures[..point as usize], &figures[point as usize..]) }
    } else if (-1..=0).contains(&point) {
        format!(".{}{figures}", "0".repeat((-point) as usize))
    } else {
        let rest = if figures.len() > 1 { format!(".{}", &figures[1..]) } else { String::new() };
        format!("{}{rest}E{:+03}", &figures[..1], point - 1)
    };
    format!("{sign}{text}")
}

/// Lists an MSX-BASIC program saved by SAVE: a byte &FF, then linked lines from &8001, each ending with &00.
pub fn list_msx(data: &[u8], set: &Charset) -> Listing {
    let set = if set.id.starts_with("msx") { set } else { charset::known("msx") };
    let mut listing = Listing { dialect: "MSX-BASIC", ..Listing::default() };
    if data.first() != Some(&0xFF) {
        listing.notes.push("An MSX-BASIC program starts with the byte &FF, and this doesn't.".into());
        return listing;
    }
    // Lines are found by their links, which are addresses from &8001, where the program starts in memory: a line
    // can't be ended at its first zero byte, because numbers in it can hold zeros.
    const START: usize = 0x8001;
    let memory = &data[1..];
    let mut at = 0;
    loop {
        if at + 2 > memory.len() {
            listing.notes.push("The program stops without its end marker.".into());
            break;
        }
        let link = usize::from(u16::from_le_bytes([memory[at], memory[at + 1]]));
        if link == 0 {
            at += 2;
            break;
        }
        let next = link.wrapping_sub(START);
        if next <= at + 4 || next > memory.len() || memory[next - 1] != 0 {
            listing.notes.push(format!("A line's link at byte {} doesn't point to the next line, so the listing stops there.", at + 1));
            break;
        }
        let number = u16::from_le_bytes([memory[at + 2], memory[at + 3]]);
        let body = &memory[at + 4..next - 1];
        listing.lines.push(format!("{number} {}", msx_line(body, set, &mut listing.notes)));
        at = next;
    }
    if at < memory.len() {
        listing.notes.push(format!("{} byte(s) after the program aren't listed.", memory.len() - at));
    }
    listing.notes.push("Numbers are written from their stored values, without a ! or # to say their precision.".into());
    listing.notes.dedup();
    listing
}

fn msx_line(body: &[u8], set: &Charset, notes: &mut Vec<String>) -> String {
    let mut text = String::new();
    let (mut quoted, mut rest_plain, mut data) = (false, false, false);
    let mut i = 0;
    let char_of = |byte: u8, text: &mut String| match set.entry(0, byte).and_then(|e| e.unicode).filter(|c| !c.is_control() && *c != '{' && *c != '}') {
        Some(c) => text.push(c),
        None => text.push_str(&format!("{{${byte:02x}}}")),
    };
    while i < body.len() {
        let byte = body[i];
        i += 1;
        if byte == b'"' {
            quoted = !quoted;
        }
        if quoted || rest_plain || (data && byte != b':') {
            char_of(byte, &mut text);
            continue;
        }
        data = false;
        match byte {
            // ELSE and the apostrophe are stored with a colon before them, which LIST doesn't show.
            b':' if body.get(i) == Some(&0xA1) => {}
            b':' if body.get(i) == Some(&0x8F) && body.get(i + 1) == Some(&0xE6) => i += 1,
            0x0B | 0x0C | 0x1C | 0x0E | 0x0D if i + 2 <= body.len() => {
                let value = u16::from_le_bytes([body[i], body[i + 1]]);
                i += 2;
                match byte {
                    0x0B => text.push_str(&format!("&O{value:o}")),
                    0x0C => text.push_str(&format!("&H{value:X}")),
                    0x0D => {
                        text.push_str(&format!("{{line at &H{value:04X}}}"));
                        notes.push("The program was saved after it had run, so some line numbers are only addresses.".into());
                    }
                    _ => text.push_str(&value.to_string()),
                }
            }
            0x0F if i < body.len() => {
                text.push_str(&body[i].to_string());
                i += 1;
            }
            0x11..=0x1A => text.push_str(&(byte - 0x11).to_string()),
            0x1D if i + 4 <= body.len() => {
                text.push_str(&msx_float(&body[i..i + 4]));
                i += 4;
            }
            0x1F if i + 8 <= body.len() => {
                text.push_str(&msx_float(&body[i..i + 8]));
                i += 8;
            }
            0xFF => match body.get(i).copied().and_then(msx_function) {
                Some(word) => {
                    text.push_str(word);
                    i += 1;
                }
                None => text.push_str("{$ff}"),
            },
            0x80..=0xFE => match msx_code(byte) {
                Some(word) => {
                    text.push_str(word);
                    rest_plain = matches!(byte, 0x8F | 0xE6);
                    data = byte == 0x84;
                }
                None => text.push_str(&format!("{{${byte:02x}}}")),
            },
            _ => char_of(byte, &mut text),
        }
    }
    text
}

// ---------- Color BASIC (Tandy Color Computer) and Dragon BASIC ----------

/// Extended and Disk Extended Color BASIC's (and the CoCo 3's Super Extended BASIC's) keywords, from &80.
const COCO_COMMANDS: [&str; 121] = [
    "FOR", "GO", "REM", "'", "ELSE", "IF", "DATA", "PRINT", "ON", "INPUT", "END", "NEXT", "DIM", "READ", "RUN", "RESTORE", "RETURN", "STOP", "POKE",
    "CONT", "LIST", "CLEAR", "NEW", "CLOAD", "CSAVE", "OPEN", "CLOSE", "LLIST", "SET", "RESET", "CLS", "MOTOR", "SOUND", "AUDIO", "EXEC", "SKIPF",
    "TAB(", "TO", "SUB", "THEN", "NOT", "STEP", "OFF", "+", "-", "*", "/", "^", "AND", "OR", ">", "=", "<", "DEL", "EDIT", "TRON", "TROFF", "DEF",
    "LET", "LINE", "PCLS", "PSET", "PRESET", "SCREEN", "PCLEAR", "COLOR", "CIRCLE", "PAINT", "GET", "PUT", "DRAW", "PCOPY", "PMODE", "PLAY", "DLOAD",
    "RENUM", "FN", "USING", "DIR", "DRIVE", "FIELD", "FILES", "KILL", "LOAD", "LSET", "MERGE", "RENAME", "RSET", "SAVE", "WRITE", "VERIFY", "UNLOAD",
    "DSKINI", "BACKUP", "COPY", "DSKI$", "DSKO$", "DOS", "WIDTH", "PALETTE", "HSCREEN", "LPOKE", "HCLS", "HCOLOR", "HPAINT", "HCIRCLE", "HLINE",
    "HGET", "HPUT", "HBUFF", "HPRINT", "ERR", "BRK", "LOCATE", "HSTAT", "HSET", "HRESET", "HDRAW", "CMP", "RGB", "ATTR",
];
/// Its functions, after a byte &FF, from &80.
const COCO_FUNCTIONS: [&str; 46] = [
    "SGN", "INT", "ABS", "USR", "RND", "SIN", "PEEK", "LEN", "STR$", "VAL", "ASC", "CHR$", "EOF", "JOYSTK", "LEFT$", "RIGHT$", "MID$", "POINT",
    "INKEY$", "MEM", "ATN", "COS", "TAN", "EXP", "FIX", "LOG", "POS", "SQR", "HEX$", "VARPTR", "INSTR", "TIMER", "PPOINT", "STRING$", "CVN", "FREE",
    "LOC", "LOF", "MKN$", "AS", "", "LPEEK", "BUTTON", "HPOINT", "ERNO", "ERLIN",
];
/// The Dragon's BASIC (and Dragon DOS's additions) has the same words in another order.
const DRAGON_COMMANDS: [&str; 104] = [
    "FOR", "GO", "REM", "'", "ELSE", "IF", "DATA", "PRINT", "ON", "INPUT", "END", "NEXT", "DIM", "READ", "LET", "RUN", "RESTORE", "RETURN", "STOP",
    "POKE", "CONT", "LIST", "CLEAR", "NEW", "DEF", "CLOAD", "CSAVE", "OPEN", "CLOSE", "LLIST", "SET", "RESET", "CLS", "MOTOR", "SOUND", "AUDIO",
    "EXEC", "SKIPF", "DEL", "EDIT", "TRON", "TROFF", "LINE", "PCLS", "PSET", "PRESET", "SCREEN", "PCLEAR", "COLOR", "CIRCLE", "PAINT", "GET", "PUT",
    "DRAW", "PCOPY", "PMODE", "PLAY", "DLOAD", "RENUM", "TAB(", "TO", "SUB", "FN", "THEN", "NOT", "STEP", "OFF", "+", "-", "*", "/", "^", "AND", "OR",
    ">", "=", "<", "USING", "AUTO", "BACKUP", "BEEP", "BOOT", "CHAIN", "COPY", "CREATE", "DIR", "DRIVE", "DSKINIT", "FREAD", "FWRITE", "ERROR", "KILL",
    "LOAD", "MERGE", "PROTECT", "WAIT", "RENAME", "SAVE", "SREAD", "SWRITE", "VERIFY", "FROM", "FLREAD", "SWAP",
];
const DRAGON_FUNCTIONS: [&str; 41] = [
    "SGN", "INT", "ABS", "POS", "RND", "SQR", "LOG", "EXP", "SIN", "COS", "TAN", "ATN", "PEEK", "LEN", "STR$", "VAL", "ASC", "CHR$", "EOF", "JOYSTK",
    "FIX", "HEX$", "LEFT$", "RIGHT$", "MID$", "POINT", "INKEY$", "MEM", "VARPTR", "INSTR", "TIMER", "PPOINT", "STRING$", "USR", "LOF", "FREE", "ERL",
    "ERR", "HIMEM", "LOC", "FRE$",
];

/// The lines of a Microsoft 6809 BASIC program: a link (unused here), a line number, then the line up to a zero
/// byte, which can only end it, numbers being kept as their digits. None unless the lines end where they should and
/// their numbers go up.
fn basic_6809_lines(program: &[u8]) -> Option<Vec<(u16, &[u8])>> {
    let mut lines = Vec::new();
    let mut at = 0;
    let mut last = None;
    loop {
        let link = u16::from_be_bytes([*program.get(at)?, *program.get(at + 1)?]);
        if link == 0 {
            return (!lines.is_empty()).then_some(lines);
        }
        let number = u16::from_be_bytes([*program.get(at + 2)?, *program.get(at + 3)?]);
        if number > 63999 || last.is_some_and(|l| number <= l) {
            return None;
        }
        last = Some(number);
        let end = at + 4 + program.get(at + 4..)?.iter().position(|&b| b == 0)?;
        lines.push((number, &program[at + 4..end]));
        at = end + 1;
    }
}

/// A CoCo Disk BASIC program: &FF and its length, then the lines.
pub fn looks_like_coco(data: &[u8]) -> bool {
    data.len() > 7 && data[0] == 0xFF && usize::from(u16::from_be_bytes([data[1], data[2]])) + 3 <= data.len() + 1 && basic_6809_lines(&data[3..]).is_some()
}

/// A Dragon DOS BASIC program: a 9-byte header, &55, type 1, load address, length, run address, &AA.
pub fn looks_like_dragon(data: &[u8]) -> bool {
    data.len() > 13 && data[0] == 0x55 && data[1] == 1 && data[8] == 0xAA && basic_6809_lines(&data[9..]).is_some()
}

fn list_6809(program: &[u8], dialect: &'static str, commands: &[&str], functions: &[&str]) -> Listing {
    let mut listing = Listing { dialect, ..Listing::default() };
    let Some(lines) = basic_6809_lines(program) else {
        listing.notes.push("The program's lines don't follow on from each other, so it can't be listed.".into());
        return listing;
    };
    for (number, body) in lines {
        let mut text = String::new();
        let (mut quoted, mut plain) = (false, false);
        let mut i = 0;
        while i < body.len() {
            let byte = body[i];
            i += 1;
            if byte == b'"' {
                quoted = !quoted;
            }
            if quoted || plain || byte < 0x80 {
                // The colon stored before ELSE and the apostrophe isn't listed.
                if !quoted && !plain && byte == b':' && matches!(body.get(i), Some(0x83 | 0x84)) {
                    continue;
                }
                let c = char::from(byte & 0x7F);
                if c.is_control() { text.push_str(&format!("{{${byte:02x}}}")) } else { text.push(c) }
                continue;
            }
            let word = if byte == 0xFF {
                let function = body.get(i).copied().unwrap_or(0);
                i += 1;
                functions.get(usize::from(function.wrapping_sub(0x80))).copied().filter(|w| !w.is_empty())
            } else {
                commands.get(usize::from(byte - 0x80)).copied()
            };
            match word {
                Some(word) => {
                    text.push_str(word);
                    // After REM or its apostrophe the rest of the line is as typed.
                    plain = matches!(byte, 0x82 | 0x83);
                }
                None => text.push_str(&format!("{{${byte:02x}}}")),
            }
        }
        listing.lines.push(format!("{number} {text}"));
    }
    listing
}

pub fn list_coco(data: &[u8]) -> Listing {
    list_6809(&data[3.min(data.len())..], "Color BASIC (Tandy Color Computer)", &COCO_COMMANDS, &COCO_FUNCTIONS)
}

pub fn list_dragon(data: &[u8]) -> Listing {
    list_6809(&data[9.min(data.len())..], "Dragon BASIC", &DRAGON_COMMANDS, &DRAGON_FUNCTIONS)
}

// ---------- Applesoft BASIC ----------

/// Applesoft's tokens, &80 to &EA, in the order of its ROM's table.
const APPLESOFT_TOKENS: [&str; 107] = [
    "END", "FOR", "NEXT", "DATA", "INPUT", "DEL", "DIM", "READ", "GR", "TEXT", "PR#", "IN#", "CALL", "PLOT", "HLIN", "VLIN", "HGR2", "HGR", "HCOLOR=",
    "HPLOT", "DRAW", "XDRAW", "HTAB", "HOME", "ROT=", "SCALE=", "SHLOAD", "TRACE", "NOTRACE", "NORMAL", "INVERSE", "FLASH", "COLOR=", "POP", "VTAB",
    "HIMEM:", "LOMEM:", "ONERR", "RESUME", "RECALL", "STORE", "SPEED=", "LET", "GOTO", "RUN", "IF", "RESTORE", "&", "GOSUB", "RETURN", "REM", "STOP",
    "ON", "WAIT", "LOAD", "SAVE", "DEF", "POKE", "PRINT", "CONT", "LIST", "CLEAR", "GET", "NEW", "TAB(", "TO", "FN", "SPC(", "THEN", "AT", "NOT", "STEP",
    "+", "-", "*", "/", "^", "AND", "OR", ">", "=", "<", "SGN", "INT", "ABS", "USR", "FRE", "SCRN(", "PDL", "POS", "SQR", "RND", "LOG", "EXP", "COS",
    "SIN", "TAN", "ATN", "PEEK", "LEN", "STR$", "VAL", "ASC", "CHR$", "LEFT$", "RIGHT$", "MID$",
];

/// An Applesoft program's lines, found by following their links from where Applesoft loads programs, &0801: each
/// link must lead to the next line's start, just after the zero that ends a line, and line numbers must go up.
fn applesoft_lines(data: &[u8]) -> Option<Vec<(u16, &[u8])>> {
    const START: usize = 0x0801;
    let mut lines = Vec::new();
    let mut at = 0;
    let mut last = None;
    loop {
        let link = usize::from(u16::from_le_bytes([*data.get(at)?, *data.get(at + 1)?]));
        if link == 0 {
            return (!lines.is_empty()).then_some(lines);
        }
        let next = link.checked_sub(START)?;
        if next <= at + 4 || next > data.len() || data[next - 1] != 0 {
            return None;
        }
        let number = u16::from_le_bytes([data[at + 2], data[at + 3]]);
        if number > 63999 || last.is_some_and(|l| number <= l) {
            return None;
        }
        last = Some(number);
        lines.push((number, &data[at + 4..next - 1]));
        at = next;
    }
}

pub fn looks_like_applesoft(data: &[u8]) -> bool {
    applesoft_lines(data).is_some_and(|lines| lines.iter().all(|(_, body)| body.iter().all(|&b| b <= 0xEA || b == 0)))
}

/// Lists an Applesoft BASIC program, as LIST shows it: keywords with a space either side.
pub fn list_applesoft(data: &[u8]) -> Listing {
    let mut listing = Listing { dialect: "Applesoft BASIC (Apple II)", ..Listing::default() };
    let Some(lines) = applesoft_lines(data) else {
        listing.notes.push("The program's line links don't lead from one line to the next from $0801, so it can't be listed.".into());
        return listing;
    };
    for (number, body) in lines {
        let mut text = String::new();
        let (mut quoted, mut plain) = (false, false);
        for &byte in body {
            let byte_char = char::from(byte & 0x7F);
            if quoted || plain || byte < 0x80 {
                if byte == b'"' {
                    quoted = !quoted;
                }
                if byte_char.is_control() { text.push_str(&format!("{{${byte:02x}}}")) } else { text.push(byte_char) }
                continue;
            }
            match APPLESOFT_TOKENS.get(usize::from(byte - 0x80)) {
                Some(word) => {
                    if !text.ends_with(' ') && !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(word);
                    text.push(' ');
                    // After REM the rest of the line is as typed; DATA too, as far as a colon, which is rare enough
                    // to let a DATA line be plain to its end.
                    plain = matches!(byte, 0xB2 | 0x83);
                }
                None => text.push_str(&format!("{{${byte:02x}}}")),
            }
        }
        listing.lines.push(format!("{number} {}", text.trim_end()));
    }
    listing
}

// ---------- Microsoft BASIC-80 (MBASIC) ----------

/// MBASIC 5.21's statement and operator tokens, as its own keyword table in MBASIC.COM has them.
const MBASIC_CODES: [(u8, &str); 100] = [
    (0x81, "END"), (0x82, "FOR"), (0x83, "NEXT"), (0x84, "DATA"), (0x85, "INPUT"), (0x86, "DIM"), (0x87, "READ"), (0x88, "LET"),
    (0x89, "GOTO"), (0x8A, "RUN"), (0x8B, "IF"), (0x8C, "RESTORE"), (0x8D, "GOSUB"), (0x8E, "RETURN"), (0x8F, "REM"), (0x90, "STOP"),
    (0x91, "PRINT"), (0x92, "CLEAR"), (0x93, "LIST"), (0x94, "NEW"), (0x95, "ON"), (0x96, "NULL"), (0x97, "WAIT"), (0x98, "DEF"),
    (0x99, "POKE"), (0x9A, "CONT"), (0x9D, "OUT"), (0x9E, "LPRINT"), (0x9F, "LLIST"), (0xA1, "WIDTH"), (0xA2, "ELSE"), (0xA3, "TRON"),
    (0xA4, "TROFF"), (0xA5, "SWAP"), (0xA6, "ERASE"), (0xA7, "EDIT"), (0xA8, "ERROR"), (0xA9, "RESUME"), (0xAA, "DELETE"), (0xAB, "AUTO"),
    (0xAC, "RENUM"), (0xAD, "DEFSTR"), (0xAE, "DEFINT"), (0xAF, "DEFSNG"), (0xB0, "DEFDBL"), (0xB1, "LINE"), (0xB4, "WHILE"), (0xB5, "WEND"),
    (0xB6, "CALL"), (0xB7, "WRITE"), (0xB8, "COMMON"), (0xB9, "CHAIN"), (0xBA, "OPTION"), (0xBB, "RANDOMIZE"), (0xBD, "SYSTEM"), (0xBF, "OPEN"),
    (0xC0, "FIELD"), (0xC1, "GET"), (0xC2, "PUT"), (0xC3, "CLOSE"), (0xC4, "LOAD"), (0xC5, "MERGE"), (0xC6, "FILES"), (0xC7, "NAME"),
    (0xC8, "KILL"), (0xC9, "LSET"), (0xCA, "RSET"), (0xCB, "SAVE"), (0xCC, "RESET"), (0xCE, "TO"), (0xCF, "THEN"), (0xD0, "TAB("),
    (0xD1, "STEP"), (0xD2, "USR"), (0xD3, "FN"), (0xD4, "SPC("), (0xD5, "NOT"), (0xD6, "ERL"), (0xD7, "ERR"), (0xD8, "STRING$"),
    (0xD9, "USING"), (0xDA, "INSTR"), (0xDB, "'"), (0xDC, "VARPTR"), (0xDD, "INKEY$"), (0xEF, ">"), (0xF0, "="), (0xF1, "<"),
    (0xF2, "+"), (0xF3, "-"), (0xF4, "*"), (0xF5, "/"), (0xF6, "^"), (0xF7, "AND"), (0xF8, "OR"), (0xF9, "XOR"),
    (0xFA, "EQV"), (0xFB, "IMP"), (0xFC, "MOD"), (0xFD, "\\"),
];
/// Its functions, which follow a byte &FF, numbered from &81.
const MBASIC_FUNCTIONS: [&str; 52] = [
    "LEFT$", "RIGHT$", "MID$", "SGN", "INT", "ABS", "SQR", "RND", "SIN", "LOG", "EXP", "COS", "TAN", "ATN", "FRE", "INP", "POS", "LEN", "STR$",
    "VAL", "ASC", "CHR$", "PEEK", "SPACE$", "OCT$", "HEX$", "LPOS", "CINT", "CSNG", "CDBL", "FIX", "", "", "", "", "", "", "", "", "", "",
    "CVI", "CVS", "CVD", "", "EOF", "LOC", "LOF", "MKI$", "MKS$", "MKD$", "",
];

/// Whether a program starting &FF is MBASIC's rather than MSX's: MSX programs start at &8001, MBASIC's lower.
pub fn looks_like_mbasic(data: &[u8]) -> bool {
    data.len() >= 3 && data[0] == 0xFF && u16::from_le_bytes([data[1], data[2]]) < 0x8000
}

/// A number in Microsoft Binary Format: the mantissa, lowest byte first, its top bit the sign; then an exponent
/// excess 128 (0 for zero).
fn mbf(bytes: &[u8]) -> String {
    let (&exponent, mantissa) = bytes.split_last().unwrap_or((&0, &[]));
    if exponent == 0 {
        return "0".into();
    }
    let top = mantissa.last().copied().unwrap_or(0);
    let mut value = 0f64;
    for (i, &b) in mantissa.iter().enumerate() {
        let b = if i == mantissa.len() - 1 { b | 0x80 } else { b };
        value += f64::from(b) * 256f64.powi(i as i32);
    }
    value *= 2f64.powi(i32::from(exponent) - 128 - 8 * mantissa.len() as i32);
    if top & 0x80 != 0 {
        value = -value;
    }
    let single = mantissa.len() == 3;
    let text = if value != 0.0 && (value.abs() >= 1e7 || value.abs() < 0.01) {
        let shown = if single { format!("{:E}", value as f32) } else { format!("{value:E}") };
        match shown.split_once('E') {
            Some((m, e)) if !e.starts_with('-') => format!("{m}E+{e}"),
            Some((m, e)) => format!("{m}E{e}"),
            None => shown,
        }
    } else if single {
        (value as f32).to_string()
    } else {
        value.to_string()
    };
    // LIST writes numbers below 1 without the 0 before the point.
    let text = text.replacen("0.", ".", usize::from(text.starts_with("0.") || text.starts_with("-0.")));
    if single { text } else { format!("{text}#") }
}

/// A Microsoft BASIC that keeps numbers in binary, as MBASIC 5 does: its name and its tokens. Functions follow a
/// byte &FF; ELSE and the apostrophe are stored with a colon before them (the apostrophe as :REM').
pub(crate) struct MsDialect {
    pub name: &'static str,
    pub code: fn(u8) -> Option<&'static str>,
    pub function: fn(u8) -> Option<&'static str>,
    pub data: u8,
    pub rem: u8,
    pub else_code: u8,
    pub apostrophe: u8,
}

const MBASIC: MsDialect = MsDialect {
    name: "Microsoft BASIC-80 (MBASIC)",
    code: |byte| MBASIC_CODES.iter().find(|(b, _)| *b == byte).map(|(_, word)| *word),
    function: |byte| byte.checked_sub(0x81).and_then(|f| MBASIC_FUNCTIONS.get(usize::from(f))).copied().filter(|w| !w.is_empty()),
    data: 0x84,
    rem: 0x8F,
    else_code: 0xA2,
    apostrophe: 0xDB,
};

/// Lists a Microsoft BASIC-80 (MBASIC) program as SAVE wrote it: a byte &FF, then linked lines, each ending with &00.
pub fn list_mbasic(data: &[u8], set: &Charset) -> Listing {
    list_microsoft(data, set, &MBASIC)
}

/// The lines of a program saved by a Microsoft BASIC that keeps its numbers in binary, found from their links.
pub(crate) struct MsLines {
    /// Where the program was in memory.
    pub start: usize,
    /// Each line's number and where its body is in the file, after the leading &FF.
    pub lines: Vec<(u16, std::ops::Range<usize>)>,
    /// Whether the links lead all the way to the program's end, rather than breaking off where it's damaged.
    pub whole: bool,
}

/// Finds a program's lines. The links are addresses where the program was in memory, which depends on the machine,
/// so the first line's end is found by trying each zero byte until the links lead through the whole program. If they
/// never do, the longest run of at least two lines they lead through is given, as what's left of a damaged program.
pub(crate) fn microsoft_lines(data: &[u8]) -> Option<MsLines> {
    let memory = data.get(1..).filter(|memory| memory.len() >= 2)?;
    let first_link = usize::from(u16::from_le_bytes([memory[0], memory[1]]));
    let walk = |start: usize| -> MsLines {
        let mut lines = Vec::new();
        let mut at = 0;
        loop {
            let (Some(&low), Some(&high)) = (memory.get(at), memory.get(at + 1)) else {
                return MsLines { start, lines, whole: false };
            };
            let link = usize::from(u16::from_le_bytes([low, high]));
            if link == 0 {
                return MsLines { start, lines, whole: true };
            }
            match link.checked_sub(start) {
                Some(next) if next > at + 4 && next <= memory.len() && memory[next - 1] == 0 => {
                    lines.push((u16::from_le_bytes([memory[at + 2], memory[at + 3]]), at + 5..next));
                    at = next;
                }
                _ => return MsLines { start, lines, whole: false },
            }
        }
    };
    let mut best: Option<MsLines> = None;
    for end in (5..memory.len().min(512)).filter(|&end| memory[end - 1] == 0 && first_link >= end) {
        let found = walk(first_link - end);
        if found.whole && !found.lines.is_empty() {
            return Some(found);
        }
        if found.lines.len() >= 2 && best.as_ref().is_none_or(|best| found.lines.len() > best.lines.len()) {
            best = Some(found);
        }
    }
    best
}

/// Lists a program in a Microsoft BASIC that keeps its numbers in binary, from its lines' links.
pub(crate) fn list_microsoft(data: &[u8], set: &Charset, dialect: &MsDialect) -> Listing {
    let mut listing = Listing { dialect: dialect.name, ..Listing::default() };
    if data.first() == Some(&0xFE) {
        listing.notes.push("This program was saved protected (SAVE \"name\",P), so it's scrambled and can't be listed.".into());
        return listing;
    }
    let Some(found) = microsoft_lines(data) else {
        listing.notes.push("The program's line links don't lead from one line to the next, so it can't be listed.".into());
        return listing;
    };
    for (number, body) in &found.lines {
        listing.lines.push(format!("{number} {}", microsoft_line(&data[body.clone()], set, dialect, &mut listing.notes)));
    }
    if !found.whole {
        let last = found.lines.last().map_or(0, |(number, _)| *number);
        listing.notes.push(format!("The program is damaged after line {last}: the next line's link doesn't lead to a line, so the listing stops there."));
    }
    listing.notes.push("Numbers are written from their stored values; a double-precision one has a # after it.".into());
    listing.notes.dedup();
    listing
}

fn microsoft_line(body: &[u8], set: &Charset, dialect: &MsDialect, notes: &mut Vec<String>) -> String {
    let mut text = String::new();
    let (mut quoted, mut rest_plain, mut data) = (false, false, false);
    let mut i = 0;
    let char_of = |byte: u8, text: &mut String| match set.entry(0, byte).and_then(|e| e.unicode).filter(|c| !c.is_control() && *c != '{' && *c != '}') {
        Some(c) => text.push(c),
        None => text.push_str(&format!("{{${byte:02x}}}")),
    };
    while i < body.len() {
        let byte = body[i];
        i += 1;
        if byte == b'"' {
            quoted = !quoted;
        }
        if quoted || rest_plain || (data && byte != b':') {
            char_of(byte, &mut text);
            continue;
        }
        data = false;
        match byte {
            // ELSE and the apostrophe are stored with a colon before them (and the apostrophe a REM), which LIST doesn't show.
            b':' if body.get(i) == Some(&dialect.else_code) => {}
            b':' if body.get(i) == Some(&dialect.rem) && body.get(i + 1) == Some(&dialect.apostrophe) => i += 1,
            0x0B | 0x0C | 0x1C | 0x0E | 0x0D if i + 2 <= body.len() => {
                let value = u16::from_le_bytes([body[i], body[i + 1]]);
                i += 2;
                match byte {
                    0x0B => text.push_str(&format!("&O{value:o}")),
                    0x0C => text.push_str(&format!("&H{value:X}")),
                    0x1C => text.push_str(&(value as i16).to_string()),
                    0x0D => {
                        text.push_str(&format!("{{line at &H{value:04X}}}"));
                        notes.push("The program was saved after it had run, so some line numbers are only addresses.".into());
                    }
                    _ => text.push_str(&value.to_string()),
                }
            }
            0x0F if i < body.len() => {
                text.push_str(&body[i].to_string());
                i += 1;
            }
            0x11..=0x1A => text.push_str(&(byte - 0x11).to_string()),
            0x1D if i + 4 <= body.len() => {
                text.push_str(&mbf(&body[i..i + 4]));
                i += 4;
            }
            0x1F if i + 8 <= body.len() => {
                text.push_str(&mbf(&body[i..i + 8]));
                i += 8;
            }
            0xFF => match body.get(i).copied().and_then(dialect.function) {
                Some(word) => {
                    text.push_str(word);
                    i += 1;
                }
                None => text.push_str("{$ff}"),
            },
            0x80..=0xFE => match (dialect.code)(byte) {
                Some(word) => {
                    text.push_str(word);
                    rest_plain = byte == dialect.rem || byte == dialect.apostrophe;
                    data = byte == dialect.data;
                }
                None => text.push_str(&format!("{{${byte:02x}}}")),
            },
            _ => char_of(byte, &mut text),
        }
    }
    text
}

// ---------- Atari BASIC ----------

/// Atari BASIC's statement tokens, 0 to &37. &36 is the LET that isn't written, &37 a line with a syntax error.
const ATARI_STATEMENTS: [&str; 56] = [
    "REM", "DATA", "INPUT", "COLOR", "LIST", "ENTER", "LET", "IF", "FOR", "NEXT", "GOTO", "GO TO", "GOSUB", "TRAP", "BYE", "CONT", "COM", "CLOSE", "CLR", "DEG", "DIM", "END", "NEW", "OPEN", "LOAD", "SAVE", "STATUS", "NOTE",
    "POINT", "XIO", "ON", "POKE", "PRINT", "RAD", "READ", "RESTORE", "RETURN", "RUN", "STOP", "POP", "?", "GET", "PUT", "GRAPHICS", "PLOT", "POSITION", "DOS", "DRAWTO", "SETCOLOR", "LOCATE", "SOUND", "LPRINT", "CSAVE", "CLOAD", "", "ERROR -",
];
/// Its operator and function tokens, from &12; &0E and &0F begin numbers and strings.
const ATARI_OPERATORS: [&str; 67] = [
    ",", "$", ":", ";", "", "GOTO", "GOSUB", "TO", "STEP", "THEN", "#", "<=", "<>", ">=", "<", ">", "=", "^", "*", "+", "-", "/", "NOT", "OR", "AND", "(", ")", "=", "=", "<=", "<>", ">=", "<", ">", "=", "+", "-", "(",
    "(", "(", "(", "(", ",", "STR$", "CHR$", "USR", "ASC", "VAL", "LEN", "ADR", "ATN", "COS", "PEEK", "SIN", "RND", "FRE", "EXP", "LOG", "CLOG", "SQR", "SGN", "ABS", "INT", "PADDLE", "STICK", "PTRIG", "STRIG",
];
const ATARI_EOL: u8 = 0x16;

/// Atari's floating point: an excess-64 exponent of 100 with the sign in bit 7, then five bytes of packed decimal.
fn atari_float(bytes: &[u8]) -> String {
    let Some((&exponent, mantissa)) = bytes.split_first() else {
        return String::new();
    };
    if mantissa.iter().all(|b| *b == 0) {
        return "0".to_string();
    }
    let mut figures: String = mantissa.iter().map(|b| format!("{}{}", b >> 4, b & 0x0F)).collect();
    // The first byte is the whole part in hundreds, so the point sits after the first two figures.
    let mut point = (i32::from(exponent & 0x7F) - 64) * 2 + 2;
    while figures.starts_with('0') {
        figures.remove(0);
        point -= 1;
    }
    while figures.ends_with('0') {
        figures.pop();
    }
    let sign = if exponent & 0x80 != 0 { "-" } else { "" };
    let length = figures.len() as i32;
    let text = if (1..=10).contains(&point) {
        if point >= length { format!("{figures}{}", "0".repeat((point - length) as usize)) } else { format!("{}.{}", &figures[..point as usize], &figures[point as usize..]) }
    } else if (-1..=0).contains(&point) {
        format!("0.{}{figures}", "0".repeat((-point) as usize))
    } else {
        let rest = if figures.len() > 1 { format!(".{}", &figures[1..]) } else { String::new() };
        format!("{}{rest}E{:+03}", &figures[..1], point - 1)
    };
    format!("{sign}{text}")
}

/// Whether bytes look like a SAVEd Atari BASIC program: a header of seven pointers, in order, after a LOMEM of 0.
pub fn looks_like_atari(data: &[u8]) -> bool {
    if data.len() < 14 || data[0] != 0 || data[1] != 0 {
        return false;
    }
    let word = |at: usize| usize::from(u16::from_le_bytes([data[at], data[at + 1]]));
    let pointers = [word(2), word(4), word(6), word(8), word(10), word(12)];
    pointers.windows(2).all(|pair| pair[0] <= pair[1]) && pointers[5] - pointers[0] + 14 <= data.len() + 256
}

/// Lists an Atari BASIC program from its variable name table and its statement table.
pub fn list_atari(data: &[u8]) -> Listing {
    let mut listing = Listing { dialect: "Atari BASIC", ..Listing::default() };
    if !looks_like_atari(data) {
        listing.notes.push("This doesn't have an Atari BASIC program's header.".into());
        return listing;
    }
    let word = |at: usize| usize::from(u16::from_le_bytes([data[at], data[at + 1]]));
    let base = word(2);
    let place = |pointer: usize| (pointer + 14).saturating_sub(base).min(data.len());
    let (names_at, names_end, statements_at, statements_end) = (place(word(2)), place(word(4)), place(word(8)), place(word(12)));
    let atascii = charset::known("atascii");
    let char_of = |byte: u8, text: &mut String| match atascii.entry(0, byte).filter(|e| !e.inverse).and_then(|e| e.unicode).filter(|c| !c.is_control() && *c != '{' && *c != '}') {
        Some(c) => text.push(c),
        None => text.push_str(&format!("{{${byte:02x}}}")),
    };
    // Each name ends with its last character's top bit set. An array's name ends in ( and a string's in $.
    let mut names: Vec<String> = Vec::new();
    let mut current = String::new();
    for &byte in &data[names_at..names_end.max(names_at)] {
        char_of(byte & 0x7F, &mut current);
        if byte & 0x80 != 0 {
            names.push(std::mem::take(&mut current));
        }
    }
    let mut at = statements_at;
    while at + 3 <= statements_end {
        let number = u16::from_le_bytes([data[at], data[at + 1]]);
        let length = usize::from(data[at + 2]);
        // Line 32768 is the direct command SAVE was typed as, not part of the program.
        if number >= 32768 || length < 3 || at + length > data.len() {
            break;
        }
        let line = &data[at..at + length];
        let mut text = format!("{number} ");
        let mut s = 3;
        while s + 2 <= line.len() {
            let next = usize::from(line[s]).clamp(s + 2, line.len());
            let token = line[s + 1];
            if s > 3 {
                text.push(':');
            }
            let statement = ATARI_STATEMENTS.get(usize::from(token)).copied().unwrap_or("?");
            text.push_str(statement);
            let operands = &line[s + 2..next];
            if matches!(token, 0x00 | 0x01 | 0x37) {
                // REM, DATA and an erroneous line keep their text as typed, up to the end-of-line byte.
                if token != 0x37 {
                    text.push(' ');
                }
                for &byte in operands.iter().take_while(|b| **b != 0x9B) {
                    char_of(byte, &mut text);
                }
            } else {
                if !statement.is_empty() && !operands.first().is_some_and(|b| matches!(*b, 0x14 | ATARI_EOL)) {
                    text.push(' ');
                }
                atari_operands(operands, &names, &char_of, &mut text);
            }
            s = next;
        }
        listing.lines.push(text.trim_end().to_string());
        at += length;
    }
    if names.iter().any(|name| name.chars().all(|c| !c.is_ascii_graphic())) {
        listing.notes.push("The variable names have been overwritten, as an unlistable program's are.".into());
    }
    listing
}

fn atari_operands(operands: &[u8], names: &[String], char_of: &dyn Fn(u8, &mut String), text: &mut String) {
    let mut i = 0;
    while i < operands.len() {
        let byte = operands[i];
        i += 1;
        match byte {
            0x0E if i + 6 <= operands.len() => {
                text.push_str(&atari_float(&operands[i..i + 6]));
                i += 6;
            }
            0x0F if i < operands.len() => {
                let length = usize::from(operands[i]);
                text.push('"');
                for &b in operands.get(i + 1..i + 1 + length).unwrap_or(&[]) {
                    char_of(b, text);
                }
                text.push('"');
                i += 1 + length;
            }
            0x14 | ATARI_EOL => {}
            0x80..=0xFF => text.push_str(names.get(usize::from(byte & 0x7F)).map_or("?", String::as_str)),
            0x12..=0x54 => {
                let word = ATARI_OPERATORS[usize::from(byte - 0x12)];
                // An array's name already ends in its bracket, so the bracket token after it isn't written twice.
                if word == "(" && text.ends_with('(') {
                    continue;
                }
                let spaced = word.starts_with(|c: char| c.is_ascii_alphabetic()) && !matches!(byte, 0x3D..=0x54);
                if spaced {
                    if !text.ends_with(' ') {
                        text.push(' ');
                    }
                    text.push_str(word);
                    text.push(' ');
                } else {
                    text.push_str(word);
                }
            }
            _ => text.push_str(&format!("{{${byte:02x}}}")),
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn coco_and_dragon_basic_list_with_their_own_tokens() {
        // 10 PRINT INT(RND(6)):' DICE
        let line: Vec<u8> = [&[0x87, b' ', 0xFF, 0x81, b'(', 0xFF, 0x84, b'(', b'6', b')', b')', b':', 0x83][..], b" DICE", &[0]].concat();
        let mut program = vec![0x26, 0x10, 0, 10];
        program.extend(&line);
        program.extend([0, 0]);
        let mut coco = vec![0xFF];
        coco.extend((program.len() as u16).to_be_bytes());
        coco.extend(&program);
        assert!(looks_like_coco(&coco));
        assert_eq!(list_coco(&coco).lines, ["10 PRINT INT(RND(6))' DICE"]);
        // These few tokens are the same on the Dragon; many others (LET, RUN, the functions after RND) aren't.
        let mut dragon = vec![0x55, 1, 0x1E, 0x01, 0, program.len() as u8, 0x8B, 0x8D, 0xAA];
        dragon.extend(&program);
        assert!(looks_like_dragon(&dragon));
        assert_eq!(list_dragon(&dragon).lines, ["10 PRINT INT(RND(6))' DICE"]);
        assert!(!looks_like_coco(&[0xFF, 0x01, 0x80, 0x0A, 0x00, 0, 0, 0]), "an MSX program isn't one");
    }


    #[test]
    fn applesoft_lists_from_its_links() {
        // 10 HOME : PRINT "HI"   20 GOTO 10
        let line10 = [0x97, b':', 0xBA, b'"', b'H', b'I', b'"', 0];
        let line20 = [0xAB, b'1', b'0', 0];
        let mut program = Vec::new();
        let second = 0x0801 + 4 + line10.len();
        program.extend((second as u16).to_le_bytes());
        program.extend(10u16.to_le_bytes());
        program.extend(line10);
        program.extend(((second + 4 + line20.len()) as u16).to_le_bytes());
        program.extend(20u16.to_le_bytes());
        program.extend(line20);
        program.extend([0, 0]);
        assert!(looks_like_applesoft(&program));
        assert_eq!(list_applesoft(&program).lines, ["10 HOME : PRINT \"HI\"", "20 GOTO 10"]);
        assert!(!looks_like_applesoft(&[0xFF, 0x01, 0x80, 0x0A, 0x00]), "an MSX program isn't one");
    }


    #[test]
    fn mbasic_lists_with_its_own_tokens_and_binary_numbers() {
        // 10 PRINT INT(RND(1)*6)+1:' A DIE  ;  20 X=3.5#:GOTO 10  (the program loaded at &4A01)
        let line10: Vec<u8> = [&[0x91, b' ', 0xFF, 0x85, b'(', 0xFF, 0x88, b'(', 0x12, b')', 0xF4, 0x17, b')', 0xF2, 0x12, b':', 0x8F, 0xDB][..], b" A DIE", &[0]].concat();
        let mut line20 = vec![b'X', 0xF0, 0x1F];
        line20.extend([0, 0, 0, 0, 0, 0, 0x60, 0x82]); // 3.5 as a double
        line20.extend([b':', 0x89, 0x0E, 10, 0, 0]);
        let start = 0x4A01usize;
        let first = start + 4 + line10.len();
        let second = first + 4 + line20.len();
        let mut program = vec![0xFF];
        program.extend((first as u16).to_le_bytes());
        program.extend(10u16.to_le_bytes());
        program.extend(&line10);
        program.extend((second as u16).to_le_bytes());
        program.extend(20u16.to_le_bytes());
        program.extend(&line20);
        program.extend([0, 0]);
        assert!(looks_like_mbasic(&program));
        let listing = list_mbasic(&program, charset::known("cp437"));
        assert_eq!(listing.lines, ["10 PRINT INT(RND(1)*6)+1' A DIE", "20 X=3.5#:GOTO10"]);
        assert_eq!(mbf(&[0, 0, 0x20, 0x82]), "2.5");
        assert_eq!(mbf(&[0, 0, 0, 0x80]), ".5");
    }

    /// Lists every tokenised BASIC program in the folder LIMONITER_BASIC names, as a .bas would be, to read; skipped
    /// without it.
    #[test]
    fn real_basic_programs() {
        let Some(folder) = std::env::var_os("LIMONITER_BASIC") else {
            return;
        };
        for item in std::fs::read_dir(folder).unwrap().flatten() {
            let data = std::fs::read(item.path()).unwrap();
            let listing = crate::basic::list(&data, "bas", charset::known("cp437"));
            eprintln!("---- {}", listing.dialect);
            eprintln!("==== {} ({} lines) {:?}", item.path().display(), listing.lines.len(), listing.notes);
            for line in listing.lines.iter().take(25) {
                eprintln!("{line}");
            }
        }
    }

    use super::*;

    #[test]
    fn basic_v_programs_list_their_two_byte_keywords() {
        let line = |number: u8, body: &[u8]| [&[0x0D, 0, number, 4 + body.len() as u8][..], body].concat();
        // SYS "OS_Exit", CASE x% OF, WHEN 1, OTHERWISE, ENDCASE, WHILE TRUE, ENDWHILE, VOICES 4.
        let program = [
            line(10, b"\xC8\x99\"OS_Exit\""),
            line(20, b"\xC8\x8Ex%\xCA"),
            line(30, b"\xC91:\xF1\"\xC8\x99\""),
            line(40, b"\x7F"),
            line(50, b"\xCB"),
            line(60, b"\xC8\x95\xB9:\xCE"),
            line(70, b"\xC8\xA0 4"),
            vec![0x0D, 0xFF],
        ]
        .concat();
        let listing = list_bbc(&program);
        assert_eq!(listing.dialect, "BBC BASIC V");
        assert_eq!(listing.lines, ["   10SYS\"OS_Exit\"", "   20CASEx%OF", "   30WHEN1:PRINT\"{$c8}{$99}\"", "   40OTHERWISE", "   50ENDCASE", "   60WHILETRUE:ENDWHILE", "   70VOICES 4"], "a keyword's bytes in a string stay bytes");
        // BASIC II's program, with no two-byte keywords, lists as BASIC II, &C9 to &CE and all.
        let old = [line(10, b"\xF1\"HI\""), vec![0x0D, 0xFF]].concat();
        assert_eq!(list_bbc(&old).dialect, "BBC BASIC");
    }

    #[test]
    fn bbc_line_numbers_go_in_three_bytes_both_ways() {
        // xania.org's worked example, line 139, and line 10.
        assert_eq!(bbc_line_bytes(139), [0x74, 0x4B, 0x40]);
        assert_eq!(bbc_line_bytes(10), [0x54, 0x4A, 0x40]);
        for number in [0, 1, 10, 139, 255, 256, 1000, 32767] {
            assert_eq!(bbc_line_number(bbc_line_bytes(number)), number);
        }
    }

    #[test]
    fn bbc_basic_tokenises_as_the_rom_does_and_lists_back_the_same() {
        let text = ["   10 MODE 7", "   20 PAGE=&1900:PRINT PAGE", "   30 FOR I%=1 TO 10:PRINT \"HELLO\";I%:NEXT", "   40 IF X THEN 10 ELSE GOTO 20", "   50 REM PRINT is not tokenised here", "   60 DATA PRINT,1", "   70 *FX 200,3", "   80 TIMER=1:TIME=0", "   90 PROCdraw:DEF PROCdraw:ENDPROC", "  100 ON A GOTO 10,20,30"];
        let program = tokenise_bbc(&text).unwrap();
        assert_eq!(&program[..4], &[0x0D, 0x00, 0x0A, 8], "a line: &0D, its number high byte first, its length");
        assert_eq!(&program[4..7], &[b' ', 0xEB, b' '], "MODE");
        assert!(program.ends_with(&[0x0D, 0xFF]));
        let listing = list_bbc(&program);
        assert_eq!(listing.lines, text.iter().map(|l| l.to_string()).collect::<Vec<_>>());
        assert_eq!(tokenise_bbc(&listing.lines).unwrap(), program, "listing and tokenising undo each other");
        let line = |n: usize| tokenise_bbc(&[text[n]]).unwrap();
        assert!(line(1).windows(1).any(|w| w == [0xD0]) && line(1).contains(&0x90), "PAGE= at the start of a statement is &D0, and &90 after PRINT");
        assert!(line(3).contains(&0x8D), "the line number after THEN goes in three bytes");
        assert!(!line(4)[4..].contains(&0xF1), "nothing after REM is tokenised");
        let timer = line(7);
        assert!(timer.windows(5).any(|w| w == b"TIMER"), "TIME is conditional, so TIMER is a name");
        assert!(timer.contains(&0xD1), "but TIME= is the statement form");
        assert!(line(8).windows(4).any(|w| w == b"draw"), "a PROC's name isn't tokenised");
        assert!(tokenise_bbc(&["10 P.\"HI\""]).unwrap().contains(&0xF1), "P. is PRINT, as the ROM's order says");
        assert!(!tokenise_bbc(&["10 print"]).unwrap().contains(&0xF1), "keywords are capitals only");
        assert!(looks_like_bbc(&program) && !looks_like_bbc(b"10 PRINT"));
    }

    #[test]
    fn bbc_names_starting_with_keywords_follow_the_rom() {
        // TO isn't conditional, so TOTAL is TO and then TAL; FALSE is, so FALSEHOOD is a name.
        let body = |text: &str| tokenise_bbc(&[text]).unwrap()[4..].strip_suffix(&[0x0D, 0xFF]).unwrap().to_vec();
        assert_eq!(body("1 TOTAL"), [b' ', 0xB8, b'T', b'A', b'L']);
        assert_eq!(body("1 FALSEHOOD"), b" FALSEHOOD");
        assert_eq!(body("1 A=&DEF"), b" A=&DEF", "hex isn't tokenised");
        assert!(tokenise_bbc(&["10 A", "5 B"]).unwrap_err().contains("go up"));
        let broken = [&[0x0D, 0x00][..], &[0x0D, 0x00, 0x0A, 0x02], &[0x0D, 0x00, 0x0A, 0x40, 0x41]];
        for data in broken {
            let _ = list_bbc(data);
        }
    }

    #[test]
    fn msx_programs_list_from_the_handbooks_codes() {
        // 10 PRINT "HI";A:ELSE '  comment ; 20 GOTO 10 ; 30 X=&HFF+12+300+1.5
        let mut data = vec![0xFF];
        let mut line = |number: u16, body: &[u8]| {
            // Each line's link is the address of the next, counting from &8001 for the byte after the &FF.
            let next = 0x8001 + data.len() - 1 + 4 + body.len() + 1;
            data.extend((next as u16).to_le_bytes());
            data.extend(number.to_le_bytes());
            data.extend_from_slice(body);
            data.push(0);
        };
        line(10, b"\x91 \"HI\";A :\xa1 :\x8f\xe6 comment");
        line(20, &[0x89, b' ', 0x0E, 10, 0]);
        line(30, &[b'X', 0xEF, 0x0C, 0xFF, 0x00, 0xF1, 0x0F, 12, 0xF1, 0x1C, 0x2C, 0x01, 0xF1, 0x1D, 0x41, 0x15, 0x00, 0x00, 0xF1, 0x13, 0xF1, 0xFF, 0x96, b'(', 0x0F, 65, b')']);
        data.extend([0, 0]);
        let listing = list_msx(&data, charset::known("msx"));
        assert_eq!(listing.lines[0], "10 PRINT \"HI\";A ELSE ' comment", "the colons stored before ELSE and ' aren't shown");
        assert_eq!(listing.lines[1], "20 GOTO 10");
        assert_eq!(listing.lines[2], "30 X=&HFF+12+300+1.5+2+CHR$(65)");
        assert_eq!(msx_float(&[0x41, 0x31, 0x41, 0x59]), "3.14159");
        assert_eq!(msx_float(&[0x40, 0x50, 0x00, 0x00]), ".5");
        assert_eq!(msx_float(&[0x50, 0x10, 0x00, 0x00]), "1E+15");
        let _ = list_msx(&[0xFF, 1, 2, 3], charset::known("msx"));
    }

    fn atari_program(names: &[u8], lines: &[&[u8]]) -> Vec<u8> {
        let values = vec![0u8; 8 * names.iter().filter(|b| **b & 0x80 != 0).count()];
        let statements: Vec<u8> = lines.concat();
        let base = 0x0100usize;
        let vnt_end = base + names.len();
        let vvt = vnt_end;
        let stmtab = vvt + values.len();
        let starp = stmtab + statements.len();
        let mut data = vec![0, 0];
        for pointer in [base, vnt_end, vvt, stmtab, starp, starp] {
            data.extend((pointer as u16).to_le_bytes());
        }
        data.extend_from_slice(names);
        data.extend(values);
        data.extend(statements);
        data
    }

    #[test]
    fn atari_programs_list_from_their_tables() {
        // The format description's own example: 10 A=10 and 20 PRINT A;
        let data = atari_program(&[b'A' | 0x80], &[&[0x0A, 0x00, 0x0F, 0x0F, 0x36, 0x80, 0x2D, 0x0E, 0x40, 0x10, 0x00, 0x00, 0x00, 0x00, 0x16], &[0x14, 0x00, 0x08, 0x08, 0x20, 0x80, 0x15, 0x16]]);
        assert!(looks_like_atari(&data));
        let listing = list_atari(&data);
        assert_eq!(listing.lines, vec!["10 A=10".to_string(), "20 PRINT A;".to_string()]);
        // FOR I=1 TO 10, an array and a string, and REM.
        let names = [b'I' | 0x80, b'B', b'(' | 0x80, b'N', b'$' | 0x80];
        let one = [0x0E, 0x40, 0x01, 0, 0, 0, 0];
        let ten = [0x0E, 0x40, 0x10, 0, 0, 0, 0];
        let mut for_line = vec![0x1E, 0x00, 0, 0, 0x08, 0x80, 0x2D];
        for_line.extend(one);
        for_line.push(0x19);
        for_line.extend(ten);
        for_line.push(0x16);
        for_line[2] = for_line.len() as u8;
        for_line[3] = for_line.len() as u8;
        let mut array = vec![0x28, 0x00, 0, 0, 0x36, 0x81, 0x38];
        array.extend(one);
        array.extend([0x2C, 0x2D, 0x0F, 2, b'H', b'I', 0x16]);
        array[2] = array.len() as u8;
        array[3] = array.len() as u8;
        let rem = [0x32, 0x00, 0x0A, 0x0A, 0x00, b'N', b'O', b'T', b'E', 0x9B];
        let data = atari_program(&names, &[&for_line, &array, &rem]);
        let listing = list_atari(&data);
        assert_eq!(listing.lines, vec!["30 FOR I=1 TO 10".to_string(), "40 B(1)=\"HI\"".to_string(), "50 REM NOTE".to_string()]);
        assert_eq!(atari_float(&[0x3F, 0x50, 0, 0, 0, 0]), "0.5");
        assert_eq!(atari_float(&[0x41, 0x01, 0x23, 0, 0, 0]), "123");
        assert_eq!(atari_float(&[0xC0, 0x02, 0, 0, 0, 0]), "-2");
        assert!(!looks_like_atari(b"10 PRINT"));
        let _ = list_atari(&[0, 0, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    }
}
