//! OpticBASIC, the BASIC of OpticOS (a clean-room replacement ROM for the 48K Spectrum, github.com/brixham99/OpticOS):
//! programs with no line numbers, listed and tokenised exactly as the ROM does it, and put on tape as its SAVE does.
//!
//! A program is lines of `len, tokens…, $0D, len`, `len` counting the whole line, ended by a 0. Keywords are one
//! byte from $80 (in the ROM's table order); `$01 type hash length name` is an identifier (type 0 number, 1 integer,
//! 2 string, +4 when a bracket follows; the name lower case, without its % or $; the hash the type plus the name's
//! bytes, low 5 bits); `$02`, `$03` and `$04` are a whole number written in decimal, hex and binary; `$05` a 5-byte
//! float; `$06 length chars` a string; `$07 lo hi` a link slot the ROM fills in when the program runs; `$08`–`$0C`
//! are <=, >=, <>, << and >>; `$0E` a ' comment, its text kept as typed. On tape it's a type-0 ("Program:") header
//! and a data block starting with the signature "OB" 1 0.
//!
//! The rules here follow the ROM's own tokenise.asm and list.asm; tests check them against the programs the ROM
//! itself saved (vendor/opticos/examples).

/// The keywords, from token $80, in the ROM's order.
pub const KEYWORDS: [&str; 114] = [
    "PRINT", "LET", "IF", "ELSEIF", "ELSE", "ENDIF", "WHILE", "WEND", "REPEAT", "UNTIL", "FOR", "NEXT", "DEF", "ENDPROC", "ENDFN", "RETURN", "LOCAL", "TRY", "CATCH", "ENDTRY", "ERROR", "END", "STOP", "REM", "DIM", "CLS", "RUN", "LIST", "NEW", "BORDER", "OVER", "INVERSE",
    "BRIGHT", "FLASH", "PAPER", "INK", "POKE", "WAIT", "RANDOMIZE", "LOAD", "AUTO", "INPUT", "DATA", "READ", "RESTORE", "SAVE", "VERIFY", "PLOT", "DRAW", "BEEP", "OUT", "DPOKE", "CALL", "SWAP", "CASE", "WHEN", "OTHERWISE", "ENDCASE", "EXIT", "CLEAR", "THEN", "TO", "STEP", "PROC",
    "FN", "AND", "OR", "XOR", "NOT", "DIV", "MOD", "AT", "TAB", "CODE", "OF", "SQR", "SIN", "COS", "TAN", "ATN", "EXP", "LN", "LOG", "INT", "ABS", "SGN", "PI", "RND", "LEN", "LEFT$", "RIGHT$", "MID$", "CHR$", "ASC", "STR$", "VAL",
    "PEEK", "ERR", "TRUE", "FALSE", "INKEY$", "FRAMES", "MAX", "MIN", "INSTR", "STRING$", "HEX$", "UPPER$", "ERR$", "IN", "USR", "DPEEK", "KEY", "FREE",
];

/// The tape signature at the start of a program's data.
pub const SIGNATURE: [u8; 4] = [b'O', b'B', 1, 0];

const T: u8 = 0x80;
fn token(name: &str) -> u8 {
    T + KEYWORDS.iter().position(|k| *k == name).expect("a keyword") as u8
}

/// Keywords whose statements get a link slot after them.
const LINKED: [&str; 12] = ["IF", "ELSEIF", "ELSE", "WHILE", "WEND", "UNTIL", "TRY", "CATCH", "DEF", "CASE", "WHEN", "OTHERWISE"];
const COMMENT: u8 = 0x0E;
const PAIRS: [&str; 5] = ["<=", ">=", "<>", "<<", ">>"];

/// Whether bytes are an OpticBASIC program as saved: the signature, then lines.
pub fn is_program(bytes: &[u8]) -> bool {
    bytes.starts_with(&SIGNATURE)
}

// ---------- Numbers ----------

/// A number as the ROM keeps it: five bytes, Sinclair's layout (an exponent byte, then the mantissa with its
/// sign in the top bit), or the small-integer form `00 sign lo hi 00`.
pub fn float_bytes(value: f64) -> [u8; 5] {
    if value == 0.0 {
        return [0; 5];
    }
    let negative = value < 0.0;
    let mut magnitude = value.abs();
    let mut exponent = 0i32;
    while magnitude >= 1.0 {
        magnitude /= 2.0;
        exponent += 1;
    }
    while magnitude < 0.5 {
        magnitude *= 2.0;
        exponent -= 1;
    }
    let mut mantissa = (magnitude * 4_294_967_296.0).round() as u64;
    if mantissa >= 1 << 32 {
        mantissa >>= 1;
        exponent += 1;
    }
    let mantissa = (mantissa as u32 & 0x7FFF_FFFF) | if negative { 0x8000_0000 } else { 0 };
    let [a, b, c, d] = mantissa.to_be_bytes();
    [(exponent + 128).clamp(1, 255) as u8, a, b, c, d]
}

pub fn float_value(bytes: &[u8; 5]) -> f64 {
    if bytes[0] == 0 {
        let whole = f64::from(u16::from_le_bytes([bytes[2], bytes[3]]));
        return if bytes[1] == 0xFF { whole - 65536.0 } else { whole };
    }
    let mantissa = u32::from_be_bytes([bytes[1] | 0x80, bytes[2], bytes[3], bytes[4]]);
    let value = f64::from(mantissa) / 4_294_967_296.0 * 2f64.powi(i32::from(bytes[0]) - 128);
    if bytes[1] & 0x80 != 0 { -value } else { value }
}

/// A float as LIST shows it: up to nine significant figures, trailing zeros dropped, E for the very large or small.
fn shown_float(value: f64) -> String {
    if value == 0.0 {
        return "0".into();
    }
    let exponent = value.abs().log10().floor() as i32;
    if !(-5..9).contains(&exponent) {
        let text = format!("{:.8E}", value);
        let (mantissa, power) = text.split_once('E').unwrap_or((&text, "0"));
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        return format!("{mantissa}E{power}");
    }
    let decimals = (8 - exponent).max(0) as usize;
    let text = format!("{value:.decimals$}");
    let text = if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_string() } else { text };
    // A leading zero is left off, as the Spectrum does: .5 rather than 0.5.
    text.replacen("0.", ".", if text.starts_with("0.") || text.starts_with("-0.") { 1 } else { 0 })
}

// ---------- Listing ----------

/// The length of the item at the start of `rest` (a token and what follows it), to step over it.
fn item_length(rest: &[u8]) -> usize {
    match rest.first() {
        Some(1) => 4 + rest.get(3).map_or(0, |n| usize::from(*n)),
        Some(2..=4 | 7) => 3,
        Some(5) => 6,
        Some(6) => 2 + rest.get(1).map_or(0, |n| usize::from(*n)),
        Some(&COMMENT) => rest.len(),
        Some(t) if *t == token("REM") => rest.len(),
        _ => 1,
    }
}

/// The items of a line's tokens, each as its bytes.
fn items(tokens: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < tokens.len() {
        let length = item_length(&tokens[at..]).max(1).min(tokens.len() - at);
        out.push(&tokens[at..at + length]);
        at += length;
    }
    out
}

/// Whether a line's tokens hold a keyword or character, as the ROM looks for THEN after IF.
fn has(tokens: &[u8], byte: u8) -> bool {
    items(tokens).iter().any(|item| item.len() == 1 && item[0] == byte)
}

/// The program's lines, as the ROM lists them: indented two spaces a block, with its spacing.
pub fn list(program: &[u8]) -> Result<Vec<String>, String> {
    let body = program.strip_prefix(&SIGNATURE[..]).unwrap_or(program);
    let mut lines = Vec::new();
    let mut level: usize = 0;
    let mut at = 0;
    while at < body.len() && body[at] != 0 {
        let length = usize::from(body[at]);
        if length < 3 || at + length > body.len() || body[at + length - 1] != body[at] || body[at + length - 2] != 0x0D {
            return Err(format!("Line {} of the program is damaged, so the listing stops there.", lines.len() + 1));
        }
        let tokens = &body[at + 1..at + length - 2];
        let first = tokens.first().copied().unwrap_or(0);
        let named = |name: &str| first == token(name);
        let opens = if named("IF") {
            !has(tokens, token("THEN"))
        } else if named("FOR") {
            !has(tokens, token("NEXT"))
        } else if named("DEF") {
            !has(tokens, b'=')
        } else {
            ["WHILE", "REPEAT", "TRY", "CASE", "ELSEIF", "WHEN", "OTHERWISE"].iter().any(|k| named(k))
        };
        let closes = ["WHEN", "OTHERWISE", "ENDCASE", "ELSE", "CATCH", "ELSEIF", "ENDIF", "WEND", "UNTIL", "NEXT", "ENDPROC", "ENDFN", "ENDTRY"].iter().any(|k| named(k));
        if closes {
            level = level.saturating_sub(1);
        }
        let mut text = " ".repeat(level * 2);
        level += usize::from(opens || named("ELSE") || named("CATCH"));
        text.push_str(&detokenise(tokens));
        lines.push(text);
        at += length;
    }
    Ok(lines)
}

/// A line's tokens as text, spaced as the ROM's LIST spaces them.
pub fn detokenise(tokens: &[u8]) -> String {
    const OWED: char = '\u{1}';
    let mut out = String::new();
    // The last character shown, or OWED when a keyword is owed a space after it.
    let mut last = ' ';
    let separate = |out: &mut String, last: char| {
        if last == OWED || last.is_ascii_alphanumeric() || last == '_' || matches!(last, '"' | ')' | '$' | '%') {
            out.push(' ');
        }
    };
    for item in items(tokens) {
        match item[0] {
            t @ 0x80..=0xFF => {
                separate(&mut out, last);
                let name = KEYWORDS.get(usize::from(t - T)).copied().unwrap_or("?");
                out.push_str(name);
                if t == token("REM") {
                    out.push_str(&String::from_utf8_lossy(&item[1..]));
                    break;
                }
                last = if t < token("SQR") { OWED } else { name.chars().last().unwrap_or(' ') };
            }
            1 => {
                separate(&mut out, last);
                let kind = item.get(1).copied().unwrap_or(0);
                out.push_str(&String::from_utf8_lossy(item.get(4..).unwrap_or_default()));
                match kind & 3 {
                    1 => out.push('%'),
                    2 => out.push('$'),
                    _ => {}
                }
                last = 'a';
            }
            2..=4 => {
                separate(&mut out, last);
                let value = u16::from_le_bytes([item.get(1).copied().unwrap_or(0), item.get(2).copied().unwrap_or(0)]);
                match item[0] {
                    2 => out.push_str(&value.to_string()),
                    3 => out.push_str(&format!("${value:X}")),
                    _ => out.push_str(&format!("%{value:b}")),
                }
                last = '0';
            }
            5 => {
                separate(&mut out, last);
                let mut bytes = [0u8; 5];
                for (slot, byte) in bytes.iter_mut().zip(&item[1..]) {
                    *slot = *byte;
                }
                let shown = shown_float(float_value(&bytes));
                last = shown.chars().last().unwrap_or('0');
                out.push_str(&shown);
            }
            6 => {
                separate(&mut out, last);
                out.push('"');
                for c in item.get(2..).unwrap_or_default() {
                    out.push(char::from(*c));
                    if *c == b'"' {
                        out.push('"');
                    }
                }
                out.push('"');
                last = '"';
            }
            7 => {}
            8..=0x0C => {
                let pair = PAIRS[usize::from(item[0] - 8)];
                out.push_str(pair);
                last = pair.chars().last().unwrap_or(' ');
            }
            COMMENT => {
                out.push('\'');
                out.push_str(&String::from_utf8_lossy(&item[1..]));
                break;
            }
            c => {
                let c = char::from(c);
                if last == OWED && !matches!(c, ':' | ',' | ';' | ')') {
                    out.push(' ');
                }
                out.push(c);
                last = c;
            }
        }
    }
    out
}

// ---------- Tokenising ----------

/// A listing tokenised as the ROM tokenises what's typed, a line at a time: the program's bytes, ended by 0, without
/// the tape signature. Blank lines are left out, as the editor doesn't keep them.
pub fn tokenise<S: AsRef<str>>(lines: &[S]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let text = line.as_ref();
        if text.trim().is_empty() {
            continue;
        }
        let tokens = tokenise_line(text).map_err(|why| format!("Line {}: {why}", index + 1))?;
        let length = tokens.len() + 3;
        if length > 255 {
            return Err(format!("Line {} is too long for OpticBASIC: a line can be 252 bytes once tokenised.", index + 1));
        }
        out.push(length as u8);
        out.extend(tokens);
        out.push(0x0D);
        out.push(length as u8);
    }
    out.push(0);
    if out.len() > 40_000 {
        return Err("The program is bigger than a 48K Spectrum has room for.".into());
    }
    Ok(out)
}

/// One line's tokens, without its length bytes and $0D.
pub fn tokenise_line(text: &str) -> Result<Vec<u8>, String> {
    let chars: Vec<char> = text.chars().collect();
    let byte = |c: char| -> Result<u8, String> { u8::try_from(u32::from(c)).ok().filter(|b| *b >= 0x20 && *b < 0x80).ok_or_else(|| format!("{c} isn't a character OpticBASIC has")) };
    let mut out = Vec::new();
    let mut at = 0;
    let mut statement_start = true;
    while at < chars.len() {
        let c = chars[at];
        at += 1;
        if c == ' ' {
            continue;
        }
        let starting = std::mem::replace(&mut statement_start, false);
        match c {
            ':' => {
                out.push(b':');
                statement_start = true;
            }
            '\'' if starting => {
                out.push(COMMENT);
                for c in &chars[at..] {
                    out.push(byte(*c)?);
                }
                break;
            }
            '"' => {
                let mut text = Vec::new();
                loop {
                    let Some(c) = chars.get(at).copied() else {
                        return Err("a string isn't closed with \"".into());
                    };
                    at += 1;
                    if c == '"' {
                        if chars.get(at) == Some(&'"') {
                            at += 1;
                        } else {
                            break;
                        }
                    }
                    text.push(byte(c)?);
                }
                if text.len() > 255 {
                    return Err("a string can be 255 characters".into());
                }
                out.push(6);
                out.push(text.len() as u8);
                out.extend(text);
            }
            '?' => out.push(token("PRINT")),
            '<' | '>' => {
                let next = chars.get(at).copied().unwrap_or(' ');
                let pair: String = [c, next].iter().collect();
                match PAIRS.iter().position(|p| *p == pair) {
                    Some(n) => {
                        out.push(8 + n as u8);
                        at += 1;
                    }
                    None => out.push(c as u8),
                }
            }
            '&' | '$' | '%' => {
                let (kind, radix) = if c == '%' { (4, 2) } else { (3, 16) };
                let start = at;
                while chars.get(at).is_some_and(|d| d.is_digit(radix)) {
                    at += 1;
                }
                if at == start {
                    return Err(format!("{c} needs {} digits after it", if radix == 2 { "binary" } else { "hex" }));
                }
                let digits: String = chars[start..at].iter().collect();
                let value = u32::from_str_radix(&digits, radix).map_err(|_| format!("{c}{digits} isn't a number"))? & 0xFFFF;
                out.push(kind);
                out.extend((value as u16).to_le_bytes());
            }
            '0'..='9' | '.' => {
                let start = at - 1;
                while chars.get(at).is_some_and(|d| d.is_ascii_digit() || *d == '.') {
                    at += 1;
                }
                if chars.get(at).is_some_and(|e| *e == 'E' || *e == 'e') && chars.get(at + 1).is_some_and(|d| d.is_ascii_digit() || *d == '+' || *d == '-') {
                    at += 2;
                    while chars.get(at).is_some_and(char::is_ascii_digit) {
                        at += 1;
                    }
                }
                let text: String = chars[start..at].iter().collect();
                let value: f64 = text.parse().map_err(|_| format!("{text} isn't a number"))?;
                let bytes = float_bytes(value);
                // A whole number from 0 to 65535 is kept as two bytes.
                if value.fract() == 0.0 && (0.0..=65535.0).contains(&value) {
                    out.push(2);
                    out.extend((value as u16).to_le_bytes());
                } else {
                    out.push(5);
                    out.extend(bytes);
                }
            }
            c if c.is_ascii_alphabetic() => {
                let start = at - 1;
                while chars.get(at).is_some_and(|d| d.is_ascii_alphanumeric() || *d == '_') {
                    at += 1;
                }
                let mut word: String = chars[start..at].iter().collect();
                if chars.get(at) == Some(&'$') {
                    word.push('$');
                    at += 1;
                }
                if word.chars().count() > 31 {
                    return Err(format!("{word} is longer than the 31 characters a name can have"));
                }
                let upper = word.to_ascii_uppercase();
                if let Some(index) = KEYWORDS.iter().position(|k| *k == upper) {
                    let t = T + index as u8;
                    out.push(t);
                    if upper == "REM" {
                        for c in &chars[at..] {
                            out.push(byte(*c)?);
                        }
                        break;
                    }
                    if LINKED.contains(&upper.as_str()) {
                        out.extend([7, 0, 0]);
                    }
                    continue;
                }
                // An identifier: its type from the $ or % after it and a bracket following.
                let (name, mut kind) = match word.strip_suffix('$') {
                    Some(name) => (name.to_string(), 2u8),
                    None => (word.clone(), 0),
                };
                if kind == 0 && chars.get(at) == Some(&'%') {
                    kind = 1;
                    at += 1;
                }
                if chars.get(at) == Some(&'(') {
                    kind |= 4;
                }
                let lower = name.to_ascii_lowercase();
                let hash = lower.bytes().fold(kind, u8::wrapping_add) & 31;
                out.extend([1, kind, hash, lower.len() as u8]);
                out.extend(lower.bytes());
            }
            c => out.push(byte(c)?),
        }
    }
    Ok(out)
}

/// A program on a tape, as the ROM's SAVE writes it: a "Program:" header naming it, with the line of a DEF PROC to
/// run when it loads (`auto`, an offset into the program) or none, and the data block of the signature, the
/// lines and the 0 that ends them.
pub fn tape(program: &[u8], name: &str, auto: Option<u16>) -> Vec<u8> {
    let mut data = SIGNATURE.to_vec();
    data.extend_from_slice(program);
    let mut header = vec![0u8];
    let mut title: Vec<u8> = name.bytes().filter(|b| (0x20..0x80).contains(b)).take(10).collect();
    title.resize(10, b' ');
    header.extend(title);
    let length = data.len() as u16;
    header.extend(length.to_le_bytes());
    header.extend(auto.unwrap_or(0x8000).to_le_bytes());
    header.extend(length.to_le_bytes());
    let mut out = crate::basic::tap_block(0x00, &header);
    out.extend(crate::basic::tap_block(0xFF, &data));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The programs on the tape the ROM saved, by their names there, and the sources they were typed from.
    fn examples() -> Vec<(String, Vec<u8>, &'static str)> {
        let tape = include_bytes!("../vendor/opticos/examples/opticos_examples.tap");
        let sources: [(&str, &str); 11] = [
            ("examples", include_str!("../vendor/opticos/examples/menu.bas")),
            ("times", include_str!("../vendor/opticos/examples/times.bas")),
            ("guess", include_str!("../vendor/opticos/examples/guess.bas")),
            ("fact", include_str!("../vendor/opticos/examples/fact.bas")),
            ("sort", include_str!("../vendor/opticos/examples/sort.bas")),
            ("errors", include_str!("../vendor/opticos/examples/errors.bas")),
            ("ball", include_str!("../vendor/opticos/examples/ball.bas")),
            ("tree", include_str!("../vendor/opticos/examples/tree.bas")),
            ("colour", include_str!("../vendor/opticos/examples/colour.bas")),
            ("hilo", include_str!("../vendor/opticos/examples/hilo.bas")),
            ("flag", include_str!("../vendor/opticos/examples/flag.bas")),
        ];
        let (blocks, _) = crate::basic::tape_blocks(tape);
        blocks
            .chunks(2)
            .map(|pair| {
                let name = String::from_utf8_lossy(&pair[0][2..12]).trim_end().to_string();
                let data = pair[1][1..pair[1].len() - 1].to_vec();
                let source = sources.iter().find(|(n, _)| *n == name).map(|(_, s)| *s).expect("a source for each");
                (name, data, source)
            })
            .collect()
    }

    /// A program with its link slots emptied, as they are before it's first run (the ROM fills them then, and
    /// keeps them filled in what it saves).
    fn unlinked(program: &[u8]) -> Vec<u8> {
        let mut out = program.to_vec();
        let mut at = 0;
        while at < out.len() && out[at] != 0 {
            let length = usize::from(out[at]);
            let mut i = at + 1;
            while i < at + length - 2 {
                let step = item_length(&out[i..at + length - 2]).max(1);
                if out[i] == 7 {
                    out[i + 1] = 0;
                    out[i + 2] = 0;
                }
                i += step;
            }
            at += length;
        }
        out
    }

    #[test]
    fn programs_tokenise_as_the_rom_tokenised_them() {
        for (name, data, source) in examples() {
            let lines: Vec<&str> = source.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
            let ours = tokenise(&lines).unwrap();
            assert!(is_program(&data));
            let theirs_unlinked = unlinked(&data[4..]);
            if ours != theirs_unlinked {
                // Say which line first differs.
                let theirs = list(&data).unwrap_or_default();
                let at = ours.iter().zip(&theirs_unlinked).position(|(a, b)| a != b).unwrap_or(0);
                panic!("{name}: differs at byte {at}; the ROM's listing is {theirs:?}");
            }
        }
    }

    #[test]
    fn programs_list_as_their_sources_and_tokenise_back() {
        for (name, data, source) in examples() {
            let listed = list(&data).unwrap();
            // The listing tokenises back to the same bytes.
            assert_eq!(tokenise(&listed).unwrap(), unlinked(&data[4..]), "{name}");
            // Its words are the source's, whatever the spacing.
            let squash = |text: &str| text.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase();
            let source_lines: Vec<String> = source.lines().map(squash).filter(|l| !l.is_empty()).collect();
            let listed_lines: Vec<String> = listed.iter().map(|l| squash(l)).collect();
            assert_eq!(listed_lines.len(), source_lines.len(), "{name}");
        }
        let fact = examples().into_iter().find(|(n, _, _)| n == "fact").unwrap().1;
        assert_eq!(list(&fact).unwrap()[..4], ["' Recursive functions", "DEF FN fact(n)", "  IF n<=1 THEN RETURN 1", "  RETURN n*fact(n-1)"]);
    }

    #[test]
    fn numbers_keep_the_roms_forms() {
        assert_eq!(float_bytes(1.0), [0x81, 0, 0, 0, 0]);
        assert_eq!(float_bytes(0.5), [0x80, 0, 0, 0, 0]);
        assert_eq!(float_bytes(10.0), [0x84, 0x20, 0, 0, 0]);
        assert_eq!(float_value(&float_bytes(-2.75)), -2.75);
        assert_eq!(tokenise_line("x = 1.5").unwrap(), [1, 0, 24, 1, b'x', b'=', 5, 0x81, 0x40, 0, 0, 0]);
        assert_eq!(detokenise(&tokenise_line("a% = $C000 + %101 : PRINT 70000").unwrap()), "a%=$C000+%101:PRINT 70000");
    }

    #[test]
    fn a_program_goes_on_tape_as_save_writes_it() {
        let (name, data, source) = examples().into_iter().find(|(n, _, _)| n == "times").unwrap();
        let lines: Vec<&str> = source.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        let tape = tape(&tokenise(&lines).unwrap(), &name, None);
        let (blocks, _) = crate::basic::tape_blocks(&tape);
        assert_eq!(blocks[1][5..blocks[1].len() - 1], unlinked(&data[4..])[..]);
        assert_eq!(&blocks[0][1..18], &[0, b't', b'i', b'm', b'e', b's', b' ', b' ', b' ', b' ', b' ', data.len() as u8, (data.len() >> 8) as u8, 0, 0x80, data.len() as u8, (data.len() >> 8) as u8]);
    }
}
