//! ZX81 programs made from text: a BASIC listing tokenised as the ZX81 keeps it, and machine code put in a REM, each
//! made into a `.p` file (the ZX81's memory from &4009, as SAVE writes it) that starts as it's loaded.
//!
//! A listing is read as `basic_extra::list_zx81` writes one, so the two undo each other: keywords spelt out (the
//! ZX81 typed them with one key), an inverse character with % before it, and a code with no character as {$xx}.
//! As on the ZX81, a number is kept as its digits and then &7E and its five-byte form.

use crate::basic_extra::{ZX81_FUNCTIONS, ZX81_TOKENS};

const NEWLINE: u8 = 0x76;
const NUMBER: u8 = 0x7E;
const REM: u8 = 0xEA;
const RAND: u8 = 0xF9;
const USR: u8 = 0xD4;
/// Where the program's lines start, and where code in a first line's REM starts: 16514.
const PROGRAM: u16 = 0x407D;
pub const REM_CODE: u16 = 0x4082;

/// The five bytes the ZX81 keeps after a number: an exponent byte and four of mantissa, the sign in the mantissa's
/// top bit (the ZX81 has no short form for whole numbers, as the Spectrum has).
pub fn number(value: f64) -> Result<[u8; 5], String> {
    if value == 0.0 {
        return Ok([0; 5]);
    }
    let negative = value < 0.0;
    let mut mantissa = value.abs();
    let mut exponent = 0i32;
    while mantissa >= 1.0 {
        mantissa /= 2.0;
        exponent += 1;
    }
    while mantissa < 0.5 {
        mantissa *= 2.0;
        exponent -= 1;
    }
    let mut bits = (mantissa * 4_294_967_296.0).round() as u64;
    if bits >= 1 << 32 {
        bits >>= 1;
        exponent += 1;
    }
    let byte = exponent + 128;
    if !(1..=255).contains(&byte) {
        return Err(crate::i18n::tf!("{value} is too big or too small for the ZX81"));
    }
    let bits = bits as u32;
    let first = ((bits >> 24) as u8 & 0x7F) | if negative { 0x80 } else { 0 };
    Ok([byte as u8, first, (bits >> 16) as u8, (bits >> 8) as u8, bits as u8])
}

/// A keyword or function spelt out at `at`, and its code and length. A keyword made of letters has to stand as a
/// word: not straight after a letter or digit, nor straight before a letter (so TOTAL is a name, not TO).
fn keyword_at(chars: &[char], at: usize) -> Option<(u8, usize)> {
    let starts_word = at == 0 || !chars[at - 1].is_ascii_alphanumeric();
    let mut best: Option<(u8, usize)> = None;
    let words = ZX81_TOKENS.iter().enumerate().map(|(i, w)| (0xC0 + i as u8, *w)).chain(ZX81_FUNCTIONS.iter().enumerate().map(|(i, w)| (0x40 + i as u8, *w)));
    for (code, word) in words {
        // &C0 is "" inside a string, and &C3 is unused.
        if code == 0xC0 || code == 0xC3 {
            continue;
        }
        let length = word.chars().count();
        if best.is_some_and(|(_, l)| l >= length) || at + length > chars.len() {
            continue;
        }
        if !chars[at..at + length].iter().zip(word.chars()).all(|(a, b)| a.to_ascii_uppercase() == b) {
            continue;
        }
        let alphabetic = word.starts_with(|c: char| c.is_ascii_alphabetic());
        if alphabetic && (!starts_word || (word.ends_with(|c: char| c.is_ascii_alphabetic()) && chars.get(at + length).is_some_and(char::is_ascii_alphabetic))) {
            continue;
        }
        best = Some((code, length));
    }
    best
}

/// A number written at `at` (digits, a point, more digits, and an exponent), and how many characters it takes.
fn number_at(chars: &[char], at: usize) -> Option<(f64, usize)> {
    let mut i = at;
    let digits = |i: &mut usize| {
        let from = *i;
        while chars.get(*i).is_some_and(char::is_ascii_digit) {
            *i += 1;
        }
        *i > from
    };
    let whole = digits(&mut i);
    if chars.get(i) == Some(&'.') {
        let mut j = i + 1;
        if digits(&mut j) || whole {
            i = j;
        }
    }
    if i == at || (!whole && i == at + 1) {
        return None;
    }
    if matches!(chars.get(i), Some('E' | 'e')) {
        let mut j = i + 1;
        if matches!(chars.get(j), Some('+' | '-')) {
            j += 1;
        }
        if digits(&mut j) {
            i = j;
        }
    }
    let text: String = chars[at..i].iter().collect();
    text.parse::<f64>().ok().map(|value| (value, i - at))
}

/// A character in the ZX81's set: a sign or letter (small letters as capitals, as the ZX81 has none), or a block
/// graphic.
fn char_code(c: char) -> Option<u8> {
    let set = crate::charset::known("zx81");
    let table = set.tables.first()?;
    table.byte_of(c).or_else(|| table.byte_of(c.to_ascii_uppercase())).filter(|b| *b < 0x40 || (0x80..0x8B).contains(b))
}

/// Tokenises one line's text after its number.
fn line_body(text: &str, number: u16) -> Result<Vec<u8>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut in_string = false;
    let mut in_rem = false;
    let bad = |c: char| crate::i18n::tf!("Line {number}: the ZX81 has no {c} character.");
    while i < chars.len() {
        let c = chars[i];
        // {$xx}: a code with no character of its own.
        if c == '{'
            && chars.get(i + 1) == Some(&'$')
            && let Some(close) = chars[i..].iter().position(|&c| c == '}')
            && let Ok(byte) = u8::from_str_radix(&chars[i + 2..i + close].iter().collect::<String>(), 16)
        {
            out.push(byte);
            i += close + 1;
            continue;
        }
        // An inverse character.
        if c == '%' && i + 1 < chars.len() {
            let code = char_code(chars[i + 1]).ok_or_else(|| bad(chars[i + 1]))?;
            out.push(code | 0x80);
            i += 2;
            continue;
        }
        if in_rem {
            out.push(char_code(c).ok_or_else(|| bad(c))?);
            i += 1;
            continue;
        }
        if in_string {
            if c == '"' {
                if chars.get(i + 1) == Some(&'"') {
                    out.push(0xC0);
                    i += 2;
                    continue;
                }
                in_string = false;
            }
            out.push(char_code(c).ok_or_else(|| bad(c))?);
            i += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push(0x0B);
            i += 1;
            continue;
        }
        // Spaces round keywords are LIST's; the ZX81 doesn't keep them.
        if c == ' ' {
            i += 1;
            continue;
        }
        if let Some((code, length)) = keyword_at(&chars, i) {
            out.push(code);
            i += length;
            if code == REM {
                in_rem = true;
                if chars.get(i) == Some(&' ') {
                    i += 1;
                }
            }
            continue;
        }
        let in_name = i > 0 && chars[i - 1].is_ascii_alphanumeric() && out.last().is_some_and(|&b| (0x1C..0x40).contains(&b));
        if !in_name
            && (c.is_ascii_digit() || c == '.')
            && let Some((value, length)) = number_at(&chars, i)
        {
            for d in &chars[i..i + length] {
                out.push(char_code(*d).ok_or_else(|| bad(*d))?);
            }
            out.push(NUMBER);
            out.extend(self::number(value).map_err(|e| crate::i18n::tf!("Line {number}: {e}"))?);
            i += length;
            continue;
        }
        out.push(char_code(c).ok_or_else(|| bad(c))?);
        i += 1;
    }
    out.push(NEWLINE);
    Ok(out)
}

/// A listing tokenised as the ZX81's program lines: each its number (high byte first), its length and its codes.
pub fn tokenise<S: AsRef<str>>(lines: &[S]) -> Result<Vec<u8>, String> {
    let mut program = Vec::new();
    let mut last: Option<u16> = None;
    for (index, line) in lines.iter().enumerate() {
        let line = line.as_ref().trim();
        if line.is_empty() {
            continue;
        }
        let digits: String = line.chars().take_while(char::is_ascii_digit).collect();
        let Ok(number) = digits.parse::<u16>() else {
            return Err(crate::i18n::tf!("Line {} of the text has no line number.", index + 1));
        };
        if number == 0 || number > 9999 {
            return Err(crate::i18n::tf!("Line {number}: the ZX81's lines are numbered from 1 to 9999."));
        }
        if last.is_some_and(|l| number <= l) {
            return Err(crate::i18n::tf!("Line {number} comes after line {}: the numbers have to go up.", last.unwrap_or_default()));
        }
        last = Some(number);
        let body = line_body(&line[digits.len()..], number)?;
        program.extend(number.to_be_bytes());
        program.extend((body.len() as u16).to_le_bytes());
        program.extend(body);
    }
    if program.is_empty() {
        return Err(crate::i18n::tr("There are no lines to tokenise.").into());
    }
    Ok(program)
}

/// A `.p` file of program lines, with an empty screen (a full display file, as a 16K ZX81 has) and no variables,
/// whose system variables say to run from the first line once it's loaded.
pub fn p_file(lines: &[u8]) -> Vec<u8> {
    let d_file = PROGRAM + lines.len() as u16;
    let display_len = 1 + 24 * 33;
    let vars = d_file + display_len as u16;
    let e_line = vars + 1;
    let first_line = lines.get(0..2).map_or(0, |b| u16::from_be_bytes([b[0], b[1]]));
    let mut file = Vec::new();
    let word = |file: &mut Vec<u8>, w: u16| file.extend(w.to_le_bytes());
    file.push(0); // VERSN
    word(&mut file, first_line); // E_PPC
    word(&mut file, d_file); // D_FILE
    word(&mut file, d_file + 1); // DF_CC
    word(&mut file, vars); // VARS
    word(&mut file, 0); // DEST
    word(&mut file, e_line); // E_LINE
    word(&mut file, e_line); // CH_ADD
    word(&mut file, 0); // X_PTR
    word(&mut file, e_line + 1); // STKBOT
    word(&mut file, e_line + 1); // STKEND
    file.push(0); // BERG
    word(&mut file, 0x405D); // MEM: the calculator's memory, MEMBOT
    file.push(0);
    file.push(2); // DF_SZ: the lines at the bottom
    word(&mut file, 0); // S_TOP
    word(&mut file, 0xFFFF); // LAST_K
    file.push(0xFF); // DEBOUNCE
    file.push(55); // MARGIN: a 50 Hz television's
    word(&mut file, PROGRAM); // NXTLIN: run from the first line
    word(&mut file, 0xFFFE); // OLDPPC
    file.push(0); // FLAGX
    word(&mut file, 0); // STRLEN
    word(&mut file, 0x0C8D); // T_ADDR
    word(&mut file, 0); // SEED
    word(&mut file, 0xFFFF); // FRAMES
    word(&mut file, 0); // COORDS
    file.push(0xBC); // PR_CC
    file.extend([0x21, 0x18]); // S_POSN
    file.push(0x40); // CDFLAG: SLOW
    file.extend([0; 32]); // PRBUFF
    file.push(NEWLINE);
    file.extend([0; 30]); // MEMBOT
    file.extend([0; 2]);
    debug_assert_eq!(file.len(), usize::from(PROGRAM - crate::zx81::FILE_START));
    file.extend(lines);
    file.push(NEWLINE);
    for _ in 0..24 {
        file.extend([0; 32]);
        file.push(NEWLINE);
    }
    file.push(0x80); // the end of the variables
    file.push(NEWLINE); // the line being edited, empty
    file
}

/// A BASIC listing as a `.p` file that runs once loaded.
pub fn program<S: AsRef<str>>(lines: &[S]) -> Result<Vec<u8>, String> {
    tokenise(lines).map(|lines| p_file(&lines))
}

/// Machine code assembled to go at `low`, run from `start`, as a `.p` file: in a REM on line 1 (from 16514, its
/// first byte's place, padded with spaces up to `low`), with line 2 RAND USR start to run it.
pub fn machine_code(code: &[u8], low: u16, start: u16) -> Result<Vec<u8>, String> {
    if low < REM_CODE {
        return Err(crate::i18n::tf!("ZX81 machine code goes in a REM from 16514 ($4082); this starts at {low} (${low:04X}). Put ORG 16514 at its top."));
    }
    let padding = usize::from(low - REM_CODE);
    let mut rem = vec![REM];
    rem.extend(std::iter::repeat_n(0, padding));
    rem.extend(code);
    rem.push(NEWLINE);
    if usize::from(REM_CODE) + padding + code.len() > 0x8000 - 1200 {
        return Err(crate::i18n::tf!("{} bytes of machine code won't fit in a 16K ZX81 with its screen.", code.len()));
    }
    let mut lines = Vec::new();
    lines.extend(1u16.to_be_bytes());
    lines.extend((rem.len() as u16).to_le_bytes());
    lines.extend(rem);
    let mut run = vec![RAND, USR];
    for d in start.to_string().chars() {
        run.push(char_code(d).expect("a digit"));
    }
    run.push(NUMBER);
    run.extend(number(f64::from(start))?);
    run.push(NEWLINE);
    lines.extend(2u16.to_be_bytes());
    lines.extend((run.len() as u16).to_le_bytes());
    lines.extend(run);
    Ok(p_file(&lines))
}

/// The ZX81 machine code starter for File, New as: code in line 1's REM that prints with the ROM's own routine.
pub const STARTER: &str = "; machine: zx81\n        org 16514           ; in line 1's REM, where ZX81 machine code is kept\nstart:  ld hl,message\nnext:   ld a,(hl)\n        cp $ff\n        ret z               ; back to BASIC\n        rst $10             ; the ROM prints the character in A\n        inc hl\n        jr next\nmessage:\n        db $2d,$2a,$31,$31,$34,$ff  ; HELLO, in the ZX81's own characters\n";

/// Whether Z80 code assembled to these addresses is for the ZX81: at 16514, where a first line's REM keeps it, or at
/// &4009, a whole `.p` file made by the source itself.
pub fn is_zx81_code(low: i64) -> bool {
    low == i64::from(REM_CODE) || low == i64::from(crate::zx81::FILE_START)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_kept_in_full_floating_point() {
        assert_eq!(number(0.0).unwrap(), [0; 5]);
        assert_eq!(number(1.0).unwrap(), [0x81, 0x00, 0, 0, 0]);
        assert_eq!(number(10.0).unwrap(), [0x84, 0x20, 0, 0, 0]);
        assert_eq!(number(-1.0).unwrap(), [0x81, 0x80, 0, 0, 0]);
        assert_eq!(number(0.5).unwrap(), [0x80, 0x00, 0, 0, 0]);
        assert_eq!(number(16514.0).unwrap(), [0x8F, 0x01, 0x04, 0, 0]);
    }

    #[test]
    fn a_listing_tokenises_and_lists_back_the_same() {
        let text = ["10 REM HI THERE", "20 LET TOTAL=1.5*PI", "30 PRINT AT 2,3;\"A\"\"B %X\";TAB 4;CHR$ 38", "40 IF INKEY$ <>\"\" THEN GOTO 10", "50 FOR I=1 TO 10 STEP 2", "60 NEXT I", "70 PRINT \"▘▝▄\""];
        let lines = tokenise(&text).unwrap();
        // 10 REM: number, length, the REM and its letters as the ZX81's characters.
        assert_eq!(&lines[..6], &[0, 10, 10, 0, REM, 0x2D]);
        let file = p_file(&lines);
        let listing = crate::basic_extra::list_zx81(&file);
        assert_eq!(listing.lines, text.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "{:?}", listing.notes);
        assert_eq!(tokenise(&listing.lines).unwrap(), lines, "listing and tokenising undo each other");
    }

    #[test]
    fn numbers_in_lines_carry_their_hidden_form() {
        let lines = tokenise(&["10 GOTO 20"]).unwrap();
        // GOTO, 2, 0, then &7E and twenty's five bytes.
        assert_eq!(&lines[4..], &[0xEC, 0x1E, 0x1C, NUMBER, 0x85, 0x20, 0, 0, 0, NEWLINE]);
        let lines = tokenise(&["10 LET A1=2E3"]).unwrap();
        assert_eq!(&lines[4..], &[0xF1, 0x26, 0x1D, 0x14, 0x1E, 0x2A, 0x1F, NUMBER, 0x8B, 0x7A, 0, 0, 0, NEWLINE], "A1 is a name; 2E3 is a number");
    }

    #[test]
    fn mistakes_are_explained() {
        assert!(tokenise(&["PRINT 1"]).unwrap_err().contains("no line number"));
        assert!(tokenise(&["20 CLS", "10 CLS"]).unwrap_err().contains("go up"));
        assert!(tokenise(&["10 PRINT \"{\""]).unwrap_err().contains("no {"));
        assert!(tokenise(&["0 CLS"]).unwrap_err().contains("1 to 9999"));
    }

    #[test]
    fn machine_code_goes_in_a_rem_and_is_run_by_rand_usr() {
        let file = machine_code(&[0x3E, 0x01, 0xC9], REM_CODE, REM_CODE).unwrap();
        let at = usize::from(REM_CODE - crate::zx81::FILE_START);
        assert_eq!(&file[at..at + 3], &[0x3E, 0x01, 0xC9]);
        let listing = crate::basic_extra::list_zx81(&file);
        assert_eq!(listing.lines[1], "2 RAND USR 16514");
        assert!(machine_code(&[0xC9], 0x4000, 0x4000).unwrap_err().contains("16514"));
        let padded = machine_code(&[0xC9], REM_CODE + 4, REM_CODE + 4).unwrap();
        assert_eq!(padded[at + 4], 0xC9, "code further on is put where it was assembled to go");
    }
}
