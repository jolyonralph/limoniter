//! Yet more tokenised BASICs, listed only: the TRS-80's, the ZX81's, the SAM Coupé's and the Oric's.
//!
//! - TRS-80 Level II BASIC, and Disk BASIC on the Model I and III: the tokens are the Level II ROM's, &80 to &FB,
//!   as trs80-tool (github.com/lkesteloot/trs80) and the Level II reference manual list them. Numbers are kept as
//!   their digits, so a line ends at its first zero byte. On disk a program starts &FF; on tape &D3 &D3 &D3 and a
//!   one-letter name. Each line is a link to the next (an address in memory), a line number, then the line.
//! - TRS-80 Model 4 BASIC (TRSDOS 6 and LS-DOS 6): a Microsoft BASIC 5 that keeps numbers in binary, as MBASIC does,
//!   with its own token numbers, read out of the keyword table in LS-DOS 6.3's BASIC/CMD.

use crate::basic::Listing;
use crate::basic_more::{self, MsDialect};
use crate::charset::{self, Charset};

/// Lists a program in one of these BASICs, if its bytes (or, where they say nothing, its ending) show it's one.
pub fn list_by_bytes(data: &[u8], _extension: &str, set: &Charset) -> Option<Listing> {
    if looks_like_trs80(data) {
        Some(list_trs80(data, set))
    } else if looks_like_trs80_model4(data) {
        Some(list_trs80_model4(data, set))
    } else {
        None
    }
}

/// Whether a file ending (and its bytes, when there are some) names a program in one of these BASICs.
pub fn is_program_file(_extension: &str, _data: Option<&[u8]>) -> bool {
    false
}

// ---------- TRS-80 Level II and Disk BASIC ----------

/// Level II and Disk BASIC's tokens, from &80.
const TRS80_TOKENS: [&str; 124] = [
    "END", "FOR", "RESET", "SET", "CLS", "CMD", "RANDOM", "NEXT", "DATA", "INPUT", "DIM", "READ", "LET", "GOTO", "RUN", "IF", "RESTORE", "GOSUB",
    "RETURN", "REM", "STOP", "ELSE", "TRON", "TROFF", "DEFSTR", "DEFINT", "DEFSNG", "DEFDBL", "LINE", "EDIT", "ERROR", "RESUME", "OUT", "ON", "OPEN",
    "FIELD", "GET", "PUT", "CLOSE", "LOAD", "MERGE", "NAME", "KILL", "LSET", "RSET", "SAVE", "SYSTEM", "LPRINT", "DEF", "POKE", "PRINT", "CONT", "LIST",
    "LLIST", "DELETE", "AUTO", "CLEAR", "CLOAD", "CSAVE", "NEW", "TAB(", "TO", "FN", "USING", "VARPTR", "USR", "ERL", "ERR", "STRING$", "INSTR",
    "POINT", "TIME$", "MEM", "INKEY$", "THEN", "NOT", "STEP", "+", "-", "*", "/", "[", "AND", "OR", ">", "=", "<", "SGN", "INT", "ABS", "FRE",
    "INP", "POS", "SQR", "RND", "LOG", "EXP", "COS", "SIN", "TAN", "ATN", "PEEK", "CVI", "CVS", "CVD", "EOF", "LOC", "LOF", "MKI$", "MKS$", "MKD$",
    "CINT", "CSNG", "CDBL", "FIX", "LEN", "STR$", "VAL", "ASC", "CHR$", "LEFT$", "RIGHT$", "MID$", "'",
];
const TRS80_DATA: u8 = 0x88;
const TRS80_REM: u8 = 0x93;
const TRS80_ELSE: u8 = 0x95;
const TRS80_APOSTROPHE: u8 = 0xFB;

/// A Level II program's lines, after its &FF: each one's number and body, and whether they run to the program's end
/// rather than breaking off where it's damaged. The links are where the next line was in memory; as numbers are kept
/// as digits, each line ends at its first zero, and every link must then be the same distance from where the next
/// line is in the file. (The line numbers needn't go up: a program can hide machine code in a line numbered out of
/// order.)
fn trs80_lines(program: &[u8]) -> (Vec<(u16, &[u8])>, bool) {
    let mut lines = Vec::new();
    let mut at = 0;
    let mut base = None;
    loop {
        let Some(link) = program.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]])) else {
            return (lines, false);
        };
        if link == 0 {
            return (lines, true);
        }
        let Some(number) = program.get(at + 2..at + 4).map(|b| u16::from_le_bytes([b[0], b[1]])) else {
            return (lines, false);
        };
        let Some(end) = program[at + 4..].iter().position(|&b| b == 0) else {
            return (lines, false);
        };
        let next = at + 4 + end + 1;
        let here = link.wrapping_sub(next as u16);
        if *base.get_or_insert(here) != here {
            return (lines, false);
        }
        lines.push((number, &program[at + 4..at + 4 + end]));
        at = next;
    }
}

/// Whether a program is Level II or Disk BASIC's: a byte &FF, then lines that end at their first zero, as only a
/// BASIC that keeps its numbers as digits has them. Hardly any line may have the codes &0B to &1F that start a
/// binary number in MBASIC, Model 4 or MSX BASIC, outside its strings, REMs and DATA (a line feed, &0A, is often
/// typed to break up a long line). A damaged program is believed if it isn't whole as one of those either.
pub fn looks_like_trs80(data: &[u8]) -> bool {
    if data.first() != Some(&0xFF) {
        return false;
    }
    let (lines, whole) = trs80_lines(&data[1..]);
    let binary = lines.iter().filter(|(_, body)| has_binary_codes(body)).count();
    if lines.is_empty() || binary * 20 > lines.len() {
        return false;
    }
    whole || lines.len() >= 2 && basic_more::microsoft_lines(data).is_none_or(|found| !found.whole)
}

/// Whether a line has a byte that can't be in a Level II line outside its strings, REM and DATA.
fn has_binary_codes(body: &[u8]) -> bool {
    let (mut quoted, mut data) = (false, false);
    for &b in body {
        match b {
            b'"' => quoted = !quoted,
            _ if quoted => {}
            TRS80_REM => return false,
            TRS80_DATA => data = true,
            b':' => data = false,
            _ if data => {}
            0x0B..0x20 | 0xFC.. => return true,
            _ => {}
        }
    }
    false
}

/// Lists a Level II or Disk BASIC program, as saved to disk (starting &FF) or to tape (&D3 &D3 &D3 and a name).
pub fn list_trs80(data: &[u8], set: &Charset) -> Listing {
    // The Model III's set, unless the Model I's is chosen: a Model I without lower case shows it as capitals.
    let set = if set.id.starts_with("trs80-m") { set } else { charset::known("trs80-m3") };
    let mut listing = Listing { dialect: "TRS-80 Level II BASIC", ..Listing::default() };
    let program = if data.starts_with(&[0xD3, 0xD3, 0xD3]) && data.len() > 4 {
        listing.info.push(("Name on the tape".into(), char::from(data[3]).to_string()));
        &data[4..]
    } else if data.first() == Some(&0xFF) {
        &data[1..]
    } else {
        listing.notes.push("A TRS-80 BASIC program starts with the byte &FF, or on tape &D3 three times, and this doesn't.".into());
        return listing;
    };
    let (lines, whole) = trs80_lines(program);
    if lines.is_empty() {
        listing.notes.push("The program's lines don't link up as a Level II BASIC program's do, so it can't be listed.".into());
        return listing;
    }
    for (number, body) in &lines {
        listing.lines.push(format!("{number} {}", trs80_line(body, set)));
    }
    if !whole {
        let last = lines.last().map_or(0, |(number, _)| *number);
        listing.notes.push(format!("The program is damaged after line {last}: the next line's link doesn't lead to a line, so the listing stops there."));
    }
    listing
}

/// A Level II line as LIST shows it: tokens as their keywords; strings, REMs and DATA as typed.
fn trs80_line(body: &[u8], set: &Charset) -> String {
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
            // ELSE is stored with a colon before it, and the apostrophe as :REM', neither of which LIST shows.
            b':' if body.get(i) == Some(&TRS80_ELSE) => {}
            b':' if body.get(i) == Some(&TRS80_REM) && body.get(i + 1) == Some(&TRS80_APOSTROPHE) => i += 1,
            // The power operator is the character at &5B, which the Model I shows as an up arrow, the III as [.
            0xD1 => char_of(0x5B, &mut text),
            0x80..=0xFB => {
                text.push_str(TRS80_TOKENS[usize::from(byte - 0x80)]);
                rest_plain = matches!(byte, TRS80_REM | TRS80_APOSTROPHE);
                data = byte == TRS80_DATA;
            }
            _ => char_of(byte, &mut text),
        }
    }
    text
}

// ---------- TRS-80 Model 4 BASIC ----------

/// Model 4 BASIC's statement and operator tokens, from the keyword table in LS-DOS 6.3's BASIC/CMD.
const MODEL4_CODES: [(u8, &str); 103] = [
    (0x81, "END"), (0x82, "FOR"), (0x83, "NEXT"), (0x84, "DATA"), (0x85, "INPUT"), (0x86, "DIM"), (0x87, "READ"), (0x88, "LET"),
    (0x89, "GOTO"), (0x8A, "RUN"), (0x8B, "IF"), (0x8C, "RESTORE"), (0x8D, "GOSUB"), (0x8E, "RETURN"), (0x8F, "REM"), (0x90, "STOP"),
    (0x91, "PRINT"), (0x92, "CLEAR"), (0x93, "LIST"), (0x94, "NEW"), (0x95, "ON"), (0x96, "WAIT"), (0x97, "DEF"), (0x98, "POKE"),
    (0x99, "CONT"), (0x9C, "OUT"), (0x9D, "LPRINT"), (0x9E, "LLIST"), (0x9F, "CLS"), (0xA1, "WIDTH"), (0xA2, "ELSE"), (0xA3, "TRON"),
    (0xA4, "TROFF"), (0xA5, "SWAP"), (0xA6, "ERASE"), (0xA7, "EDIT"), (0xA8, "ERROR"), (0xA9, "RESUME"), (0xAA, "DELETE"), (0xAB, "AUTO"),
    (0xAC, "RENUM"), (0xAD, "DEFSTR"), (0xAE, "DEFINT"), (0xAF, "DEFSNG"), (0xB0, "DEFDBL"), (0xB1, "LINE"), (0xB4, "WHILE"), (0xB5, "WEND"),
    (0xB6, "CALL"), (0xB7, "WRITE"), (0xB8, "COMMON"), (0xB9, "CHAIN"), (0xBA, "OPTION"), (0xBB, "RANDOM"), (0xBD, "SYSTEM"), (0xBF, "OPEN"),
    (0xC0, "FIELD"), (0xC1, "GET"), (0xC2, "PUT"), (0xC3, "CLOSE"), (0xC4, "LOAD"), (0xC5, "MERGE"), (0xC7, "NAME"), (0xC8, "KILL"),
    (0xC9, "LSET"), (0xCA, "RSET"), (0xCB, "SAVE"), (0xCD, "SOUND"), (0xCF, "TO"), (0xD0, "THEN"), (0xD1, "TAB("), (0xD2, "STEP"),
    (0xD3, "USR"), (0xD4, "FN"), (0xD5, "SPC("), (0xD6, "NOT"), (0xD7, "ERL"), (0xD8, "ERR"), (0xD9, "STRING$"), (0xDA, "USING"),
    (0xDB, "INSTR"), (0xDC, "'"), (0xDD, "VARPTR"), (0xDE, "DATE$"), (0xDF, "ERRS$"), (0xE0, "INKEY$"), (0xE1, "MEM"), (0xE2, "TIME$"),
    (0xF0, ">"), (0xF1, "="), (0xF2, "<"), (0xF3, "+"), (0xF4, "-"), (0xF5, "*"), (0xF6, "/"), (0xF7, "^"),
    (0xF8, "AND"), (0xF9, "OR"), (0xFA, "XOR"), (0xFB, "EQV"), (0xFC, "IMP"), (0xFD, "MOD"), (0xFE, "\\"),
];
/// Its functions, which follow a byte &FF, from &81.
const MODEL4_FUNCTIONS: [(u8, &str); 41] = [
    (0x81, "LEFT$"), (0x82, "RIGHT$"), (0x83, "MID$"), (0x84, "SGN"), (0x85, "INT"), (0x86, "ABS"), (0x87, "SQR"), (0x88, "RND"),
    (0x89, "SIN"), (0x8A, "LOG"), (0x8B, "EXP"), (0x8C, "COS"), (0x8D, "TAN"), (0x8E, "ATN"), (0x8F, "FRE"), (0x90, "INP"),
    (0x91, "POS"), (0x92, "LEN"), (0x93, "STR$"), (0x94, "VAL"), (0x95, "ASC"), (0x96, "CHR$"), (0x97, "PEEK"), (0x98, "SPACE$"),
    (0x99, "OCT$"), (0x9A, "HEX$"), (0x9B, "LPOS"), (0x9C, "CINT"), (0x9D, "CSNG"), (0x9E, "CDBL"), (0x9F, "FIX"), (0xAA, "CVI"),
    (0xAB, "CVS"), (0xAC, "CVD"), (0xAD, "EOF"), (0xAE, "LOC"), (0xAF, "LOF"), (0xB0, "MKI$"), (0xB1, "MKS$"), (0xB2, "MKD$"),
    (0xB3, "ROW"),
];

const MODEL4: MsDialect = MsDialect {
    name: "TRS-80 Model 4 BASIC",
    code: |byte| MODEL4_CODES.iter().find(|(b, _)| *b == byte).map(|(_, word)| *word),
    function: |byte| MODEL4_FUNCTIONS.iter().find(|(b, _)| *b == byte).map(|(_, word)| *word),
    data: 0x84,
    rem: 0x8F,
    else_code: 0xA2,
    apostrophe: 0xDC,
};

/// Whether a program starting &FF is Model 4 BASIC's, rather than MBASIC's or MSX-BASIC's, which keep their numbers
/// the same way. Model 4 programs sit above &8000, where MBASIC's never reach and MSX's start at exactly &8001.
/// Lower down, it's told from MBASIC by its commonest operator, =, which is &F1 in Model 4 BASIC and &F0 in MBASIC
/// (where &F1 is <, and in Model 4 BASIC &F0 is >).
pub fn looks_like_trs80_model4(data: &[u8]) -> bool {
    let Some(found) = basic_more::microsoft_lines(data) else {
        return false;
    };
    if found.start >= 0x8000 {
        return found.start != 0x8001;
    }
    let (mut model4, mut mbasic) = (0, 0);
    for (_, body) in &found.lines {
        let body = &data[body.clone()];
        let mut i = 0;
        while let Some(&byte) = body.get(i) {
            i += match byte {
                b'"' => body[i + 1..].iter().position(|&b| b == b'"').map_or(body.len(), |end| end + 2),
                // REM, and the apostrophe in either BASIC: the rest of the line is as typed.
                0x8F | 0xDB | 0xDC => body.len(),
                0x0B..=0x0E | 0x1C => 3,
                0x0F | 0xFF => 2,
                0x1D => 5,
                0x1F => 9,
                0xF1 => {
                    model4 += 1;
                    1
                }
                0xF0 => {
                    mbasic += 1;
                    1
                }
                _ => 1,
            };
        }
    }
    model4 > mbasic
}

/// Lists a Model 4 BASIC program, as SAVE writes it to disk.
pub fn list_trs80_model4(data: &[u8], set: &Charset) -> Listing {
    let set = if set.id.starts_with("trs80-m") { set } else { charset::known("trs80-m3") };
    basic_more::list_microsoft(data, set, &MODEL4)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A program as Level II saves it: each line a link to the next, its number, its tokens, a zero.
    fn level2(base: u16, lines: &[(u16, &[u8])]) -> Vec<u8> {
        let mut out = vec![0xFF];
        let mut address = base;
        for (number, body) in lines {
            address += 4 + body.len() as u16 + 1;
            out.extend(address.to_le_bytes());
            out.extend(number.to_le_bytes());
            out.extend_from_slice(body);
            out.push(0);
        }
        out.extend([0, 0]);
        out
    }

    #[test]
    fn a_level_ii_program_lists_with_its_tokens_else_and_apostrophe() {
        let program = level2(
            0x6A00,
            &[
                (10, b"\xB8 100:\xB2\"PRINT \x80\""),
                (20, b"\x8F A\xD510 \xCA \xB2\"BIG\" :\x95 \xB2\"SMALL\""),
                (30, b"\x88 1,2,3:\xB2 X :\x93\xFB ALL DONE"),
                (40, b"A\xD5B\xD12:\x93 PRINT STAYS"),
            ],
        );
        assert!(looks_like_trs80(&program));
        let listing = list_trs80(&program, charset::utf8());
        assert_eq!(listing.dialect, "TRS-80 Level II BASIC");
        // &80 in a string is a block graphic, the blank one.
        let blank = charset::known("trs80-m3").entry(0, 0x80).and_then(|e| e.unicode).unwrap();
        assert_eq!(listing.lines, [format!("10 CLEAR 100:PRINT\"PRINT {blank}\"").as_str(), "20 IF A=10 THEN PRINT\"BIG\" ELSE PRINT\"SMALL\"", "30 DATA 1,2,3:PRINT X ' ALL DONE", "40 A=B[2:REM PRINT STAYS"]);
        assert_eq!(list_trs80(&program, charset::known("trs80-m1")).lines[3], "40 A=B\u{2191}2:REM PRINT STAYS", "the Model I shows an arrow");
    }

    #[test]
    fn a_tape_program_lists_too_and_damage_is_said() {
        let mut tape = vec![0xD3, 0xD3, 0xD3, b'A'];
        tape.extend(&level2(0x42E8, &[(10, b"\xB2 1")])[1..]);
        let listing = list_trs80(&tape, charset::utf8());
        assert_eq!(listing.lines, ["10 PRINT 1"]);
        assert_eq!(listing.info[0].1, "A");
        assert!(list_trs80(&tape[..10], charset::utf8()).notes[0].contains("don't link up"));
        assert!(list_trs80(b"10 PRINT", charset::utf8()).notes[0].contains("&FF"));
        // A link that doesn't match where the next line is.
        let mut wrong = level2(0x6A00, &[(10, b"\xB2 1"), (20, b"\xB2 2")]);
        wrong[1] ^= 0x10;
        assert!(!looks_like_trs80(&wrong));
    }

    #[test]
    fn model_4_basic_is_told_from_mbasic_and_msx() {
        // 10 FOR I=1 TO 2:PRINT CHR$(65):NEXT, numbers as Microsoft BASIC keeps them.
        let line = b"\x82 I\xF1\x12 \xCF \x13:\x91 \xFF\x96(\x0F\x41):\x83";
        let program = |base: u16, line: &[u8]| {
            let mut out = vec![0xFF];
            out.extend((base + 4 + line.len() as u16 + 1).to_le_bytes());
            out.extend(10u16.to_le_bytes());
            out.extend_from_slice(line);
            out.extend([0, 0, 0]);
            out
        };
        let high = program(0x8400, line);
        assert!(looks_like_trs80_model4(&high) && !looks_like_trs80(&high));
        assert_eq!(list_trs80_model4(&high, charset::utf8()).lines, ["10 FOR I=1 TO 2:PRINT CHR$(65):NEXT"]);
        let low = program(0x6000, line);
        assert!(looks_like_trs80_model4(&low), "the = after FOR's variable says Model 4");
        let mbasic = program(0x6000, b"\x82 I\xF0\x12 \xCE \x13:\x83");
        assert!(!looks_like_trs80_model4(&mbasic));
        let msx = program(0x8001, line);
        assert!(!looks_like_trs80_model4(&msx), "MSX programs start at &8001");
    }
}
