//! ZX80 programs made from text: a BASIC listing tokenised as the ZX80 keeps it, made into a `.o` file (the ZX80's
//! memory from &4000, as SAVE writes it).
//!
//! A listing is read as `basic_extra::list_zx80` writes one, so the two undo each other: keywords spelt out (the
//! ZX80 typed them with one key), an inverse character with % before it, and a code with no character as {$xx}.
//! The ZX80 keeps a line as its number (high byte first), its characters and tokens, and NEWLINE; a number is only
//! its digits. The signs ; , ( ) - + * / = > < and ** are tokens wherever they are, strings and REMs too, because
//! the ZX80's keyboard types them only as tokens (from the key table at &006C in the ROM). A string is between two
//! &01 bytes, the ZX80's quote.

use crate::basic_extra::{ZX80_FIRST_TOKEN, ZX80_TOKENS};

const NEWLINE: u8 = 0x76;
const QUOTE: u8 = 0x01;
const REM: u8 = 0xFE;
/// The program's lines start after the forty system variables.
const PROGRAM: u16 = 0x4028;

/// The tokens, as they're spelt in a listing (and a few other ways they're often written), and their codes.
fn tokens() -> impl Iterator<Item = (u8, &'static str)> {
    let table = ZX80_TOKENS.iter().enumerate().map(|(i, w)| (ZX80_FIRST_TOKEN + i as u8, *w)).filter(|(code, w)| *w != "?" && *code != ZX80_FIRST_TOKEN);
    table.chain([(0xEC, "GOTO"), (0xFB, "GOSUB"), (0xEF, "RANDOMIZE"), (0xF9, "CONT")])
}

/// A token spelt out at `at`, and its code and length; only signs if `signs_only`. A token made of letters has to
/// stand as a word: not straight after a letter or digit, nor straight before a letter (so TOTAL is a name, not TO).
fn token_at(chars: &[char], at: usize, signs_only: bool) -> Option<(u8, usize)> {
    let starts_word = at == 0 || !chars[at - 1].is_ascii_alphanumeric();
    let mut best: Option<(u8, usize)> = None;
    for (code, word) in tokens() {
        let length = word.chars().count();
        let alphabetic = word.starts_with(|c: char| c.is_ascii_alphabetic());
        if (signs_only && alphabetic) || best.is_some_and(|(_, l)| l >= length) || at + length > chars.len() {
            continue;
        }
        if !chars[at..at + length].iter().zip(word.chars()).all(|(a, b)| a.to_ascii_uppercase() == b) {
            continue;
        }
        if alphabetic && (!starts_word || chars.get(at + length).is_some_and(char::is_ascii_alphabetic)) {
            continue;
        }
        best = Some((code, length));
    }
    best
}

/// A character in the ZX80's set: a sign or letter (small letters as capitals, as the ZX80 has none), or a block
/// graphic.
fn char_code(c: char) -> Option<u8> {
    let set = crate::charset::known("zx80");
    let table = set.tables.first()?;
    table.byte_of(c).or_else(|| table.byte_of(c.to_ascii_uppercase())).filter(|b| *b < 0x40 || *b == 0x80 || (0x82..0x8C).contains(b))
}

/// Tokenises one line's text after its number.
fn line_body(text: &str, number: u16) -> Result<Vec<u8>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut in_string = false;
    let mut in_rem = false;
    let bad = |c: char| crate::i18n::tf!("Line {number}: the ZX80 has no {c} character.");
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
        if c == '"' {
            if !in_rem {
                in_string = !in_string;
            }
            out.push(QUOTE);
            i += 1;
            continue;
        }
        // In a string or a REM, only the signs are tokens, and a space is kept.
        if in_string || in_rem {
            if let Some((code, length)) = token_at(&chars, i, true) {
                out.push(code);
                i += length;
            } else {
                out.push(char_code(c).ok_or_else(|| bad(c))?);
                i += 1;
            }
            continue;
        }
        // Spaces round keywords are LIST's; the ZX80 doesn't need them.
        if c == ' ' {
            i += 1;
            continue;
        }
        if let Some((code, length)) = token_at(&chars, i, false) {
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
        out.push(char_code(c).ok_or_else(|| bad(c))?);
        i += 1;
    }
    out.push(NEWLINE);
    Ok(out)
}

/// A listing tokenised as the ZX80's program lines: each its number (high byte first), its codes and NEWLINE.
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
            return Err(crate::i18n::tf!("Line {number}: the ZX80's lines are numbered from 1 to 9999."));
        }
        if last.is_some_and(|l| number <= l) {
            return Err(crate::i18n::tf!("Line {number} comes after line {}: the numbers have to go up.", last.unwrap_or_default()));
        }
        last = Some(number);
        program.extend(number.to_be_bytes());
        program.extend(line_body(&line[digits.len()..], number)?);
    }
    if program.is_empty() {
        return Err(crate::i18n::tr("There are no lines to tokenise.").into());
    }
    Ok(program)
}

/// A `.o` file of program lines and no variables, with the system variables as a ZX80 has them after the lines
/// were typed in and SAVE was typed (as the real ROM leaves them, SAVE caught on the built-in ZX80).
pub fn o_file(lines: &[u8]) -> Vec<u8> {
    let vars = PROGRAM + lines.len() as u16;
    let e_line = vars + 1;
    let first_line = lines.get(0..2).map_or(0, |b| u16::from_be_bytes([b[0], b[1]]));
    let mut file = Vec::new();
    let word = |file: &mut Vec<u8>, w: u16| file.extend(w.to_le_bytes());
    file.push(0xFF); // ERR_NR: no report
    file.push(0x88); // FLAGS: syntax off, K cursor
    word(&mut file, 0xFFFE); // PPC: a command, not a line, is running
    word(&mut file, e_line + 1); // P_PTR: the cursor, after SAVE in the edit line
    word(&mut file, first_line); // E_PPC: the line with the > cursor
    word(&mut file, vars); // VARS
    word(&mut file, e_line); // E_LINE
    word(&mut file, e_line + 2); // D_FILE, after the edit line, made again once loaded
    word(&mut file, e_line + 3); // DF_EA
    word(&mut file, e_line + 3); // DF_END
    file.push(2); // DF_SZ: the lines at the bottom
    word(&mut file, 0); // S_TOP: the listing from the start
    word(&mut file, 0); // X_PTR: no syntax error
    word(&mut file, 0); // OLDPPC
    file.push(0); // FLAGX
    word(&mut file, 0x07B0); // T_ADDR: in the ROM's syntax table, where SAVE left it
    word(&mut file, 0); // SEED
    word(&mut file, 0); // FRAMES
    word(&mut file, 0); // DEST
    word(&mut file, 0); // RESULT
    file.extend([0x21, 0x17]); // S_POSN: column and line of the next PRINT
    word(&mut file, e_line + 1); // CH_ADD
    debug_assert_eq!(file.len(), usize::from(PROGRAM - crate::zx80::FILE_START));
    file.extend(lines);
    file.push(0x80); // the end of the variables
    file
}

/// A BASIC listing as a `.o` file.
pub fn program<S: AsRef<str>>(lines: &[S]) -> Result<Vec<u8>, String> {
    tokenise(lines).map(|lines| o_file(&lines))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listing_tokenises_and_lists_back_the_same() {
        let text = ["10 REM HI (THERE)", "20 LET TOTAL=15*(A+2)", "30 PRINT \"A;B %X\";CHR$(38)", "40 IF NOT A=B AND C>1 THEN GO TO 10", "50 FOR I=1 TO 10", "60 NEXT I", "70 PRINT \"▘▝▄\"", "80 GO SUB 2**3", "90 RANDOMISE USR(16424)"];
        let lines = tokenise(&text).unwrap();
        // 10 REM: number, the REM and its letters as the ZX80's characters, with ( a token even here.
        assert_eq!(&lines[..12], &[0, 10, REM, 0x2D, 0x2E, 0x00, 0xDA, 0x39, 0x2D, 0x2A, 0x37, 0x2A]);
        let file = o_file(&lines);
        let listing = crate::basic_extra::list_zx80(&file);
        assert_eq!(listing.lines, text.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "{:?}", listing.notes);
        assert_eq!(tokenise(&listing.lines).unwrap(), lines, "listing and tokenising undo each other");
        assert!(listing.notes.iter().any(|n| n.contains('%')));
    }

    #[test]
    fn lines_are_their_number_codes_and_newline() {
        let lines = tokenise(&["10 PRINT \"HI\"", "20 GOTO 10"]).unwrap();
        assert_eq!(lines, [0, 10, 0xF4, QUOTE, 0x2D, 0x2E, QUOTE, NEWLINE, 0, 20, 0xEC, 0x1D, 0x1C, NEWLINE], "numbers are only their digits; GOTO is GO TO");
        let file = o_file(&lines);
        assert_eq!(file.len(), 0x28 + lines.len() + 1);
        assert_eq!(crate::basic_extra::zx80_length(&file), Ok(file.len()));
        assert_eq!(u16::from_le_bytes([file[8], file[9]]), 0x4028 + lines.len() as u16, "VARS just after the lines");
    }

    #[test]
    fn mistakes_are_explained() {
        assert!(tokenise(&["PRINT 1"]).unwrap_err().contains("no line number"));
        assert!(tokenise(&["20 CLS", "10 CLS"]).unwrap_err().contains("go up"));
        assert!(tokenise(&["10 PRINT \"{\""]).unwrap_err().contains("no {"));
        assert!(tokenise(&["10000 CLS"]).unwrap_err().contains("1 to 9999"));
        assert!(tokenise(&["10 PRINT 1&2"]).unwrap_err().contains("no &"));
    }

    #[test]
    fn an_object_file_isnt_taken_for_a_zx80_program() {
        assert!(crate::basic_extra::zx80_length(b"\x7FELF\x02\x01\x01\0\0\0\0\0\0\0\0\0").is_err());
        assert!(!crate::basic::is_program_file("o", Some(b"\x7FELF\x02\x01\x01\0\0\0\0\0\0\0\0\0")));
        assert!(!crate::basic::is_program_file("o", None), "a .o is taken to be a ZX80 program only when its bytes say so");
        let file = program(&["10 PRINT 1"]).unwrap();
        assert!(crate::basic::is_program_file("o", Some(&file)) && crate::basic::is_program_file("80", Some(&file)));
        assert_eq!(crate::basic::list(&file, "o", crate::charset::utf8()).lines, ["10 PRINT 1"]);
    }
}
