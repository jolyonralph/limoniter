//! Yet more tokenised BASICs, listed only: the TRS-80's, the ZX81's, the SAM Coupé's and the Oric's.
//!
//! - TRS-80 Level II BASIC, and Disk BASIC on the Model I and III: the tokens are the Level II ROM's, &80 to &FB,
//!   as trs80-tool (github.com/lkesteloot/trs80) and the Level II reference manual list them. Numbers are kept as
//!   their digits, so a line ends at its first zero byte. On disk a program starts &FF; on tape &D3 &D3 &D3 and a
//!   one-letter name. Each line is a link to the next (an address in memory), a line number, then the line.
//! - TRS-80 Model 4 BASIC (TRSDOS 6 and LS-DOS 6): a Microsoft BASIC 5 that keeps numbers in binary, as MBASIC and
//!   MSX-BASIC do, with its own token numbers, read out of the keyword table in LS-DOS 6.3's BASIC/CMD. Which of the
//!   three a program is, is told from its tokens here, and the program then listed by its own lister.
//! - ZX81 BASIC: `.p` files, which are the ZX81's memory from &4009 as SAVE wrote it to tape, so the system
//!   variables say where the program, the display file and the variables are. Lines are a line number (high byte
//!   first), a length, then the line in the ZX81's own character set, keywords &C0 to &FF; a number is its digits and
//!   then &7E and its five-byte form, as on the Spectrum. The ROM disassembly (Logan and O'Hara's, and the annotated
//!   one in github.com/Toysoft/ZX-81-ROMS) gives the keyword table and LIST's rules for spaces.
//! - SAM BASIC, with MasterDOS's and MasterBASIC's keywords: a file from a SAM disk, its 9-byte header first. Lines
//!   are as the Spectrum's (number high byte first, a length, the line, &0D), keywords single bytes from &85 or &FF
//!   and a second byte, and a number's five-byte form after &0E. The keyword table and how LIST spaces each keyword
//!   are from the SAM ROM's tables as sambasicvscode (github.com/spectecjr/sambasicvscode) sets them out.
//! - Oric BASIC (Oric-1 and Atmos): a program from a tape, as it was in memory from &0501, lines linked as
//!   Microsoft's 6502 BASICs link them, numbers as digits, keywords &80 to &F6 from the Atmos manual's table.

use crate::basic::Listing;
use crate::basic_more::{self, MsDialect};
use crate::charset::{self, Charset};

/// Lists a program in one of these BASICs, if its bytes (or, where they say nothing, its ending) show it's one.
pub fn list_by_bytes(data: &[u8], extension: &str, set: &Charset) -> Option<Listing> {
    if is_zx81_ending(extension) {
        Some(list_zx81(data))
    } else if looks_like_sam_basic(data) {
        Some(list_sam(data))
    } else if extension == "tap" && crate::media::oric::looks_like(data) {
        let program = crate::media::oric::first_program(data);
        Some(program.map_or_else(|| Listing { dialect: "Oric BASIC", notes: vec!["There's no BASIC program on this Oric tape.".into()], ..Listing::default() }, |program| list_oric(&program, set)))
    } else if looks_like_oric(data) {
        Some(list_oric(data, set))
    } else if looks_like_trs80(data) {
        Some(list_trs80(data, set))
    } else {
        match microsoft_flavour(data)? {
            Flavour::Model4 => Some(list_trs80_model4(data, set)),
            Flavour::Msx => Some(basic_more::list_msx(data, set)),
            Flavour::Mbasic => Some(basic_more::list_mbasic(data, set)),
        }
    }
}

/// Whether a file ending (and its bytes, when there are some) names a program in one of these BASICs.
pub fn is_program_file(extension: &str, data: Option<&[u8]>) -> bool {
    // A .p is also Pascal or Prolog source; a ZX81 program's system variables say what it is.
    is_zx81_ending(extension) && data.is_none_or(|data| zx81_file(data).is_ok()) || extension == "bas" && data.is_some_and(|data| looks_like_sam_basic(data) || looks_like_oric(data))
}

fn is_zx81_ending(extension: &str) -> bool {
    matches!(extension, "p" | "81" | "p81")
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
/// Below &8000 only, where TRS-80 BASIC keeps a program: an MSX program with no numbers in it would pass otherwise.
pub fn looks_like_trs80(data: &[u8]) -> bool {
    if data.first() != Some(&0xFF) || basic_more::looks_like_coco(data) {
        return false;
    }
    let (lines, whole) = trs80_lines(&data[1..]);
    let binary = lines.iter().filter(|(_, body)| has_binary_codes(body)).count();
    if lines.is_empty() || binary * 20 > lines.len() {
        return false;
    }
    // TRS-80 BASIC keeps programs low in memory, from &42E9 on a Model I with Level II; an MSX's are from &8001.
    let start = u16::from_le_bytes([data[1], data[2]]).wrapping_sub(lines[0].1.len() as u16 + 5);
    if start >= 0x8000 {
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

/// The Microsoft BASICs that keep numbers in binary and save a program as &FF and its lines.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flavour {
    Mbasic,
    Msx,
    Model4,
}

/// Which Microsoft BASIC a program starting &FF is, of those that keep their numbers in binary: MBASIC, MSX-BASIC or
/// TRS-80 Model 4 BASIC. Their tokens differ, so it's told by the = in its assignments (a statement that starts with
/// a name, or FOR and a name), which is &F0 in MBASIC, &EF in MSX-BASIC and &F1 in Model 4 BASIC: elsewhere the same
/// bytes are other operators, such as MSX-BASIC's + at &F1. A program without such an = is told by where it was in
/// memory: an MSX's from &8001, or &C001 with 16K; a Model 4's elsewhere above &8000; MBASIC's
/// lower. None if the lines don't link up as such a program's do.
pub fn microsoft_flavour(data: &[u8]) -> Option<Flavour> {
    // A Color Computer's program starts &FF too, then its length.
    if data.first() != Some(&0xFF) || basic_more::looks_like_coco(data) {
        return None;
    }
    let found = basic_more::microsoft_lines(data)?;
    let (mut msx, mut mbasic, mut model4) = (0, 0, 0);
    for (_, body) in &found.lines {
        let body = &data[body.clone()];
        // Each statement that starts with a name, or with FOR (&82 in all three) and a name, is an assignment, so
        // the next token is the =.
        let mut at = 0;
        while at < body.len() {
            let statement = &body[at..];
            let name = statement.iter().position(|&b| b != b' ' && b != 0x82).unwrap_or(statement.len());
            let after = statement[name..].iter().skip_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'$' | b'%' | b'!' | b'#'));
            if statement.get(name).is_some_and(u8::is_ascii_alphabetic) {
                match after.copied().find(|&b| b != b' ') {
                    Some(0xEF) => msx += 1,
                    Some(0xF0) => mbasic += 1,
                    Some(0xF1) => model4 += 1,
                    _ => {}
                }
            }
            // On to the next statement: past a colon outside a string, unless a REM or DATA makes the rest plain.
            let mut quoted = false;
            let mut next = None;
            for (i, &b) in statement.iter().enumerate() {
                match b {
                    b'"' => quoted = !quoted,
                    b':' if !quoted => {
                        next = Some(i + 1);
                        break;
                    }
                    0x8F | 0x84 if !quoted => break,
                    _ => {}
                }
            }
            match next {
                Some(next) => at += next,
                None => break,
            }
        }
    }
    let by_place = match found.start {
        0x8001 | 0xC001 => Flavour::Msx,
        0x8000.. => Flavour::Model4,
        _ => Flavour::Mbasic,
    };
    Some(if msx > mbasic.max(model4) {
        Flavour::Msx
    } else if mbasic > msx.max(model4) {
        Flavour::Mbasic
    } else if model4 > msx.max(mbasic) {
        Flavour::Model4
    } else {
        by_place
    })
}

/// Lists a Model 4 BASIC program, as SAVE writes it to disk.
pub fn list_trs80_model4(data: &[u8], set: &Charset) -> Listing {
    let set = if set.id.starts_with("trs80-m") { set } else { charset::known("trs80-m3") };
    basic_more::list_microsoft(data, set, &MODEL4)
}

// ---------- ZX81 ----------

/// The ZX81's keywords for codes &C0 to &FF, in the ROM's order: &C0 is the "" that stands for a quote inside a
/// string, &C3 is unused.
const ZX81_TOKENS: [&str; 64] = [
    "\"\"", "AT", "TAB", "?", "CODE", "VAL", "LEN", "SIN", "COS", "TAN", "ASN", "ACS", "ATN", "LN", "EXP", "INT", "SQR", "SGN", "ABS", "PEEK",
    "USR", "STR$", "CHR$", "NOT", "**", "OR", "AND", "<=", ">=", "<>", "THEN", "TO", "STEP", "LPRINT", "LLIST", "STOP", "SLOW", "FAST", "NEW",
    "SCROLL", "CONT", "DIM", "REM", "FOR", "GOTO", "GOSUB", "INPUT", "LOAD", "LIST", "LET", "PAUSE", "NEXT", "POKE", "PRINT", "PLOT", "RUN", "SAVE",
    "RAND", "IF", "CLS", "UNPLOT", "CLEAR", "RETURN", "COPY",
];
/// The functions at &40 to &42, which have no spaces round them.
const ZX81_FUNCTIONS: [&str; 3] = ["RND", "INKEY$", "PI"];

/// Where a ZX81 program file's memory starts: a `.p` file is the RAM from the system variable VERSN at &4009 up to
/// E_LINE, so the system variables come first, then the program at &407D, the display file and the variables.
const ZX81_FILE_START: usize = 0x4009;
const ZX81_PROGRAM: usize = 0x407D;

/// The parts of a ZX81 program file: the program's lines and the display file, as places in the file.
struct Zx81File {
    program: std::ops::Range<usize>,
    display: std::ops::Range<usize>,
}

/// Finds a `.p` file's program and display file from its system variables, D_FILE, VARS and E_LINE, which must be
/// in order, after the program's start and within the file. A `.p81` file has the program's name first, in the
/// ZX81's characters with the last one inverse; that is skipped first.
fn zx81_file(data: &[u8]) -> Result<(usize, Zx81File), String> {
    let try_at = |skip: usize| -> Result<Zx81File, String> {
        let file = data.get(skip..).unwrap_or_default();
        let word = |address: usize| file.get(address - ZX81_FILE_START..address - ZX81_FILE_START + 2).map(|b| usize::from(u16::from_le_bytes([b[0], b[1]])));
        let (Some(d_file), Some(vars), Some(e_line)) = (word(0x400C), word(0x4010), word(0x4014)) else {
            return Err("The file is too short to hold the ZX81's system variables.".into());
        };
        if !(ZX81_PROGRAM <= d_file && d_file < vars && vars < e_line) {
            return Err("The system variables at the start don't say where a ZX81 program and its screen are.".into());
        }
        let end = (vars - ZX81_FILE_START).min(file.len());
        if d_file - ZX81_FILE_START >= end {
            return Err("The file stops before the program's screen.".into());
        }
        Ok(Zx81File { program: skip + ZX81_PROGRAM - ZX81_FILE_START..skip + d_file - ZX81_FILE_START, display: skip + d_file - ZX81_FILE_START..skip + end })
    };
    let plain = try_at(0);
    if plain.is_ok() {
        return plain.map(|file| (0, file));
    }
    // A name of up to 127 characters, ending with an inverse one.
    match data.iter().take(128).position(|&b| b >= 0x80).map(|last| try_at(last + 1).map(|file| (last + 1, file))) {
        Some(Ok(found)) => Ok(found),
        _ => plain.map(|file| (0, file)),
    }
}

/// How a ZX81 character is written in a listing: its character from the ZX81 set, an inverse one with % before it
/// (the ZX81 has no % of its own, so this is what zxtext2p and other tools write), and an inverse graphic as the
/// block graphic it looks like. Codes with no character are {$xx}.
fn zx81_char(byte: u8, out: &mut String) {
    let set = charset::known("zx81");
    match set.entry(0, byte).and_then(|e| e.unicode.map(|c| (c, e.inverse))) {
        Some((c, true)) if byte > 0x8A => {
            out.push('%');
            out.push(c);
        }
        Some((c, _)) => out.push(c),
        None => out.push_str(&format!("{{${byte:02x}}}")),
    }
}

/// A ZX81 line as LIST shows it, following the ROM's own rules for spaces: a keyword from STEP's group on (the
/// commands, THEN, TO, the operators AND, OR and the comparisons) gets a space before it unless one was just
/// printed after a keyword, and every keyword from &C0 on that ends in a letter, a digit or $ gets one after it.
/// A number's hidden five-byte form after &7E isn't shown, and LIST stops at the line's first NEWLINE, &76.
fn zx81_line(body: &[u8], hidden: &mut bool) -> String {
    let mut text = String::new();
    // Set after a keyword's trailing space, so the next keyword doesn't add another. LIST starts a line with it set,
    // having put a space after the line number.
    let mut suppress = true;
    let mut i = 0;
    while i < body.len() {
        let byte = body[i];
        i += 1;
        match byte {
            0x76 => {
                *hidden |= i < body.len();
                break;
            }
            0x7E => i += 5,
            0x40..=0x42 => {
                text.push_str(ZX81_FUNCTIONS[usize::from(byte - 0x40)]);
                suppress = false;
            }
            0xC0..=0xFF => {
                let index = usize::from(byte - 0xC0);
                let word = ZX81_TOKENS[index];
                if index >= 0x18 && !suppress {
                    text.push(' ');
                }
                text.push_str(word);
                suppress = false;
                if word.ends_with(|c: char| c == '$' || c.is_ascii_alphanumeric()) {
                    text.push(' ');
                    suppress = true;
                }
            }
            _ => {
                zx81_char(byte, &mut text);
                // Printing a space leaves the keyword's spacing as it was; anything else lets the next keyword
                // have its space before it.
                if byte != 0 {
                    suppress = false;
                }
            }
        }
    }
    text
}

/// Lists a ZX81 program from a `.p` (or `.81` or `.p81`) file, with the screen it was saved with.
pub fn list_zx81(data: &[u8]) -> Listing {
    let mut listing = Listing { dialect: "ZX81 BASIC", ..Listing::default() };
    let (skip, file) = match zx81_file(data) {
        Ok(found) => found,
        Err(problem) => {
            listing.notes.push(problem);
            return listing;
        }
    };
    if skip > 0 {
        let mut name = String::new();
        for &b in &data[..skip] {
            zx81_char(b & 0x7F, &mut name);
        }
        listing.info.push(("Name".into(), name));
    }
    let program = &data[file.program];
    let mut at = 0;
    let mut hidden = false;
    while at < program.len() {
        // As LIST does, a line number from 16384 up is taken to be past the program's end.
        if program[at] >= 0x40 {
            listing.notes.push(format!("{} bytes after the last line, before the screen, aren't BASIC lines (usually machine code).", program.len() - at));
            break;
        }
        let Some(head) = program.get(at..at + 4) else {
            listing.notes.push("The program ends partway through a line's number and length.".into());
            break;
        };
        let number = u16::from_be_bytes([head[0], head[1]]);
        let length = usize::from(u16::from_le_bytes([head[2], head[3]]));
        let Some(body) = program.get(at + 4..at + 4 + length) else {
            listing.notes.push(format!("Line {number} says it's longer than what's left of the program, so the listing stops there."));
            break;
        };
        listing.lines.push(format!("{number} {}", zx81_line(body, &mut hidden).trim_end()));
        at += 4 + length;
    }
    if hidden {
        listing.notes.push("Some lines hold more after a NEWLINE byte, usually machine code in a REM, which LIST doesn't show.".into());
    }
    // The display file: a NEWLINE, then 24 rows, each ending with NEWLINE (shorter when the ZX81 had little memory).
    let display = &data[file.display];
    let rows: Vec<String> = display
        .get(1..)
        .unwrap_or_default()
        .split(|&b| b == 0x76)
        .take(24)
        .map(|row| {
            let mut text = String::new();
            for &b in row {
                // Inverse letters can't be shown as such in plain text, so the screen shows them plain.
                zx81_char(if b > 0x8A && b < 0xC0 { b & 0x3F } else { b }, &mut text);
            }
            text.trim_end().to_string()
        })
        .collect();
    if rows.iter().any(|row| !row.is_empty()) {
        listing.screen = rows;
    }
    // The ZX81 has no % of its own, so one in the listing is always an inverse character's mark.
    if listing.lines.iter().any(|line| line.contains('%')) {
        listing.notes.push("Inverse characters are written with % before them.".into());
    }
    if !listing.screen.is_empty() && display.iter().any(|&b| (0x8B..0xC0).contains(&b)) {
        listing.notes.push("Inverse letters and signs on the screen are shown plain.".into());
    }
    listing
}

// ---------- SAM BASIC ----------

/// What LIST puts round a SAM BASIC keyword: a space before (unless one was just printed), after (if the word ends
/// in a letter or $), both, or neither.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Spacing {
    B,
    L,
    T,
    N,
}
use Spacing::{B, L, N, T};

/// SAM BASIC's one-byte keywords, with MasterDOS's and MasterBASIC's from &F7.
const SAM_KEYWORDS: [(u8, &str, Spacing); 120] = [
    (0x85, "USING", B), (0x86, "WRITE", B), (0x87, "AT", B), (0x88, "TAB", B), (0x89, "OFF", B), (0x8A, "WHILE", B),
    (0x8B, "UNTIL", B), (0x8C, "LINE", B), (0x8D, "THEN", B), (0x8E, "TO", B), (0x8F, "STEP", B), (0x90, "DIR", B),
    (0x91, "FORMAT", B), (0x92, "ERASE", B), (0x93, "MOVE", B), (0x94, "SAVE", B), (0x95, "LOAD", B),
    (0x96, "MERGE", B), (0x97, "VERIFY", B), (0x98, "OPEN", B), (0x99, "CLOSE", B), (0x9A, "CIRCLE", B),
    (0x9B, "PLOT", B), (0x9C, "LET", B), (0x9D, "BLITZ", B), (0x9E, "BORDER", B), (0x9F, "CLS", B),
    (0xA0, "PALETTE", B), (0xA1, "PEN", B), (0xA2, "PAPER", B), (0xA3, "FLASH", B), (0xA4, "BRIGHT", B),
    (0xA5, "INVERSE", B), (0xA6, "OVER", B), (0xA7, "FATPIX", B), (0xA8, "CSIZE", B), (0xA9, "BLOCKS", B),
    (0xAA, "MODE", B), (0xAB, "GRAB", B), (0xAC, "PUT", B), (0xAD, "BEEP", B), (0xAE, "SOUND", B), (0xAF, "NEW", B),
    (0xB0, "RUN", B), (0xB1, "STOP", B), (0xB2, "CONTINUE", B), (0xB3, "CLEAR", B), (0xB4, "GO TO", B),
    (0xB5, "GO SUB", B), (0xB6, "RETURN", B), (0xB7, "REM", B), (0xB8, "READ", B), (0xB9, "DATA", B),
    (0xBA, "RESTORE", B), (0xBB, "PRINT", B), (0xBC, "LPRINT", B), (0xBD, "LIST", B), (0xBE, "LLIST", B),
    (0xBF, "DUMP", B), (0xC0, "FOR", B), (0xC1, "NEXT", B), (0xC2, "PAUSE", B), (0xC3, "DRAW", B),
    (0xC4, "DEFAULT", B), (0xC5, "DIM", B), (0xC6, "INPUT", B), (0xC7, "RANDOMIZE", B), (0xC8, "DEF FN", B),
    (0xC9, "DEF KEYCODE", B), (0xCA, "DEF PROC", B), (0xCB, "END PROC", B), (0xCC, "RENUM", B), (0xCD, "DELETE", B),
    (0xCE, "REF", B), (0xCF, "COPY", B), (0xD1, "KEYIN", B), (0xD2, "LOCAL", B), (0xD3, "LOOP IF", B),
    (0xD4, "DO", B), (0xD5, "LOOP", B), (0xD6, "EXIT IF", B), (0xD7, "IF", B), (0xD8, "IF", B), (0xD9, "ELSE", B),
    (0xDA, "ELSE", B), (0xDB, "END IF", B), (0xDC, "KEY", B), (0xDD, "ON ERROR", B), (0xDE, "ON", B),
    (0xDF, "GET", B), (0xE0, "OUT", B), (0xE1, "POKE", B), (0xE2, "DPOKE", B), (0xE3, "RENAME", B),
    (0xE4, "CALL", B), (0xE5, "ROLL", B), (0xE6, "SCROLL", B), (0xE7, "SCREEN", B), (0xE8, "DISPLAY", B),
    (0xE9, "BOOT", B), (0xEA, "LABEL", B), (0xEB, "FILL", B), (0xEC, "WINDOW", B), (0xED, "AUTO", B),
    (0xEE, "POP", B), (0xEF, "RECORD", B), (0xF0, "DEVICE", B), (0xF1, "PROTECT", B), (0xF2, "HIDE", B),
    (0xF3, "ZAP", B), (0xF4, "POW", B), (0xF5, "BOOM", B), (0xF6, "ZOOM", B), (0xF7, "BACKUP", B), (0xF8, "TIME", B),
    (0xF9, "DATE", B), (0xFA, "ALTER", B), (0xFB, "SORT", B), (0xFC, "JOIN", B), (0xFD, "EDIT", B),
];
/// Its functions and operators, stored as &FF and then these; MasterDOS's and MasterBASIC's below &3B.
const SAM_FUNCTIONS: [(u8, &str, Spacing); 83] = [
    (0x26, "EXIT PROC", L), (0x27, "EXIT DO", L), (0x28, "EXIT FOR", L), (0x29, "LOCN", L), (0x2A, "RESERVED", L),
    (0x2B, "EQU", L), (0x2C, "TICS", L), (0x2D, "SHIFT$", L), (0x2E, "SVAL$", L), (0x2F, "USING$", L),
    (0x30, "TIME$", L), (0x31, "DATE$", L), (0x32, "INP$", L), (0x33, "DIR$", L), (0x34, "FSTAT", L),
    (0x35, "DSTAT", L), (0x36, "FPAGES", L), (0x37, "SCRAD", L), (0x38, "INARRAY", L), (0x3B, "PI", N),
    (0x3C, "RND", N), (0x3D, "POINT", N), (0x3E, "FREE", N), (0x3F, "LENGTH", N), (0x40, "ITEM", N),
    (0x41, "ATTR", N), (0x42, "FN", T), (0x43, "BIN", T), (0x44, "XMOUSE", N), (0x45, "YMOUSE", N),
    (0x46, "XPEN", N), (0x47, "YPEN", N), (0x48, "RAMTOP", N), (0x4A, "INSTR", N), (0x4B, "INKEY$", N),
    (0x4C, "SCREEN$", N), (0x4D, "MEM$", N), (0x4F, "PATH$", N), (0x50, "STRING$", N), (0x53, "SIN", T),
    (0x54, "COS", T), (0x55, "TAN", T), (0x56, "ASN", T), (0x57, "ACS", T), (0x58, "ATN", T), (0x59, "LN", T),
    (0x5A, "EXP", T), (0x5B, "ABS", T), (0x5C, "SGN", T), (0x5D, "SQR", T), (0x5E, "INT", T), (0x5F, "USR", T),
    (0x60, "IN", T), (0x61, "PEEK", T), (0x62, "DPEEK", T), (0x63, "DVAR", T), (0x64, "SVAR", T),
    (0x65, "BUTTON", T), (0x66, "EOF", T), (0x67, "PTR", T), (0x68, "XVAR", L), (0x69, "UDG", T), (0x6A, "NVAL", L),
    (0x6B, "LEN", T), (0x6C, "CODE", T), (0x6D, "VAL$", T), (0x6E, "VAL", T), (0x6F, "TRUNC$", T), (0x70, "CHR$", T),
    (0x71, "STR$", T), (0x72, "BIN$", T), (0x73, "HEX$", T), (0x74, "USR$", T), (0x76, "NOT", T), (0x7A, "MOD", B),
    (0x7B, "DIV", B), (0x7C, "BOR", B), (0x7E, "BAND", B), (0x7F, "OR", B), (0x80, "AND", B), (0x81, "<>", N),
    (0x82, "<=", N), (0x83, ">=", N),
];
const SAM_REM: u8 = 0xB7;

/// A SAM BASIC program's lines, after its 9-byte file header: a line number (high byte first), a length (counting
/// the &0D that ends the line), the line. A line number from &FF00 ends the program. None if the lines don't fit
/// together that way, with whether they reached the end.
fn sam_lines(program: &[u8]) -> Option<(Vec<(u16, &[u8])>, bool)> {
    let mut lines = Vec::new();
    let mut at = 0;
    loop {
        let Some(&high) = program.get(at) else {
            return (!lines.is_empty()).then_some((lines, false));
        };
        if high == 0xFF {
            return Some((lines, true));
        }
        let (Some(&low), Some(length)) = (program.get(at + 1), program.get(at + 2..at + 4)) else {
            return (!lines.is_empty()).then_some((lines, false));
        };
        let length = usize::from(u16::from_le_bytes([length[0], length[1]]));
        match program.get(at + 4..at + 4 + length) {
            Some(body) if body.last() == Some(&0x0D) => lines.push((u16::from_be_bytes([high, low]), &body[..length - 1])),
            _ => return (!lines.is_empty()).then_some((lines, false)),
        }
        at += 4 + length;
    }
}

/// Whether a file is a SAM BASIC program as SAMDOS saves it: its header says type 16 and a length the file holds,
/// and the program's lines run, each ending &0D, to the &FF that ends them (or at least two of them do, before damage).
pub fn looks_like_sam_basic(data: &[u8]) -> bool {
    if data.len() < 10 || data[0] != 16 {
        return false;
    }
    let length = usize::from(data[7] & 0x1F) * 16384 + usize::from(u16::from_le_bytes([data[1], data[2]]) & 0x3FFF);
    length + 9 <= data.len() && sam_lines(&data[9..]).is_some_and(|(lines, whole)| whole && !lines.is_empty() || lines.len() >= 2)
}

/// SAM BASIC's colour and position codes, which take one or two bytes after them.
fn sam_control(byte: u8) -> Option<(&'static str, usize)> {
    Some(match byte {
        0x10 => ("PEN", 1),
        0x11 => ("PAPER", 1),
        0x12 => ("FLASH", 1),
        0x13 => ("BRIGHT", 1),
        0x14 => ("INVERSE", 1),
        0x15 => ("OVER", 1),
        0x16 => ("AT", 2),
        0x17 => ("TAB", 2),
        _ => return None,
    })
}

/// A SAM BASIC line as LIST shows it, with the keywords spaced as the ROM's print routine spaces them.
fn sam_line(body: &[u8]) -> String {
    let set = charset::known("zx-spectrum");
    // LIST has put a space after the line number, so the first keyword needs none before it.
    let mut text = String::from(" ");
    let (mut quoted, mut rest_plain) = (false, false);
    let mut i = 0;
    let char_of = |byte: u8, text: &mut String| match set.entry(0, byte).and_then(|e| e.unicode).filter(|c| !c.is_control() && *c != '{' && *c != '}').filter(|_| (0x20..0x90).contains(&byte)) {
        Some(c) => text.push(c),
        None => text.push_str(&format!("{{${byte:02x}}}")),
    };
    while i < body.len() {
        let byte = body[i];
        i += 1;
        if byte == b'"' {
            quoted = !quoted;
        }
        // Colour and position codes, written as the Spectrum's are: {PEN 2}, {AT 1,5}.
        if let Some((name, count)) = sam_control(byte).filter(|(_, count)| i + count <= body.len()) {
            let values: Vec<String> = body[i..i + count].iter().map(u8::to_string).collect();
            text.push_str(&format!("{{{name} {}}}", values.join(",")));
            i += count;
            continue;
        }
        if quoted || rest_plain {
            char_of(byte, &mut text);
            continue;
        }
        // A number's digits are there to be read; &0E and the five bytes after it are its value, which LIST
        // doesn't show. The same follows a procedure's name where it's called, and DEF FN's parameters.
        if byte == 0x0E {
            i += 5;
            continue;
        }
        let word = if byte == 0xFF {
            body.get(i).and_then(|&second| SAM_FUNCTIONS.iter().find(|(b, _, _)| *b == second)).inspect(|_| i += 1)
        } else {
            SAM_KEYWORDS.iter().find(|(b, _, _)| *b == byte)
        };
        match word {
            Some(&(_, word, spacing)) => {
                if matches!(spacing, B | L) && !text.ends_with(' ') {
                    text.push(' ');
                }
                text.push_str(word);
                if matches!(spacing, B | T) && word.ends_with(|c: char| c == '$' || c.is_ascii_alphabetic()) {
                    text.push(' ');
                }
                rest_plain = byte == SAM_REM;
            }
            None => char_of(byte, &mut text),
        }
    }
    text.split_off(1)
}

/// Lists a SAM BASIC program, as SAMDOS or MasterDOS saves it, its 9-byte header first.
pub fn list_sam(data: &[u8]) -> Listing {
    let mut listing = Listing { dialect: "SAM BASIC", ..Listing::default() };
    let Some((lines, whole)) = data.get(9..).and_then(sam_lines) else {
        listing.notes.push("The file doesn't hold SAM BASIC lines after its header.".into());
        return listing;
    };
    for (number, body) in &lines {
        listing.lines.push(format!("{number} {}", sam_line(body).trim_end()));
    }
    if !whole {
        let last = lines.last().map_or(0, |(number, _)| *number);
        listing.notes.push(format!("The program breaks off after line {last}, before the byte that ends it."));
    }
    listing
}

// ---------- Oric BASIC ----------

/// The Oric-1's and Atmos's BASIC keywords, from &80 to &F6.
const ORIC_TOKENS: [&str; 119] = [
    "END", "EDIT", "STORE", "RECALL", "TRON", "TROFF", "POP", "PLOT", "PULL", "LORES", "DOKE", "REPEAT", "UNTIL", "FOR", "LLIST", "LPRINT", "NEXT",
    "DATA", "INPUT", "DIM", "CLS", "READ", "LET", "GOTO", "RUN", "IF", "RESTORE", "GOSUB", "RETURN", "REM", "HIMEM", "GRAB", "RELEASE", "TEXT",
    "HIRES", "SHOOT", "EXPLODE", "ZAP", "PING", "SOUND", "MUSIC", "PLAY", "CURSET", "CURMOV", "DRAW", "CIRCLE", "PATTERN", "FILL", "CHAR", "PAPER",
    "INK", "STOP", "ON", "WAIT", "CLOAD", "CSAVE", "DEF", "POKE", "PRINT", "CONT", "LIST", "CLEAR", "GET", "CALL", "!", "NEW", "TAB(", "TO", "FN",
    "SPC(", "@", "AUTO", "ELSE", "THEN", "NOT", "STEP", "+", "-", "*", "/", "^", "AND", "OR", ">", "=", "<", "SGN", "INT", "ABS", "USR", "FRE", "POS",
    "HEX$", "&", "SQR", "RND", "LN", "EXP", "COS", "SIN", "TAN", "ATN", "PEEK", "DEEK", "LOG", "LEN", "STR$", "VAL", "ASC", "CHR$", "PI", "TRUE",
    "FALSE", "KEY$", "SCRN", "POINT", "LEFT$", "RIGHT$", "MID$",
];
const ORIC_REM: u8 = 0x9D;
const ORIC_DATA: u8 = 0x91;
/// Where an Oric keeps a BASIC program, and so where a program's links count from.
const ORIC_START: u16 = 0x0501;

/// An Oric program's lines: a link to the next line (its address, the program being at &0501), the line number,
/// the line, a zero. As numbers are kept as digits, a line ends at its first zero, and each link must be where
/// the next line then is. Whether they run to the zero link that ends the program, too.
fn oric_lines(program: &[u8]) -> (Vec<(u16, &[u8])>, bool) {
    let mut lines = Vec::new();
    let mut at = 0;
    loop {
        let Some(link) = program.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]])) else {
            return (lines, false);
        };
        if link == 0 {
            return (lines, true);
        }
        let (Some(number), Some(end)) = (program.get(at + 2..at + 4), program.get(at + 4..).and_then(|rest| rest.iter().position(|&b| b == 0))) else {
            return (lines, false);
        };
        let next = at + 4 + end + 1;
        if usize::from(link) != usize::from(ORIC_START) + next {
            return (lines, false);
        }
        lines.push((u16::from_le_bytes([number[0], number[1]]), &program[at + 4..at + 4 + end]));
        at = next;
    }
}

/// Whether bytes are an Oric BASIC program as it was in memory: lines linked from &0501, to the end.
pub fn looks_like_oric(data: &[u8]) -> bool {
    let (lines, whole) = oric_lines(data);
    whole && !lines.is_empty()
}

/// Lists an Oric BASIC program, as LIST shows it: keywords written out where they're stored, with no spaces put in.
pub fn list_oric(data: &[u8], set: &Charset) -> Listing {
    let set = if set.id == "oric" { set } else { charset::known("oric") };
    let mut listing = Listing { dialect: "Oric BASIC", ..Listing::default() };
    let (lines, whole) = oric_lines(data);
    if lines.is_empty() {
        listing.notes.push("The program's lines don't link up as an Oric program's do, from &0501, so it can't be listed.".into());
        return listing;
    }
    let char_of = |byte: u8, text: &mut String| match set.entry(0, byte).and_then(|e| e.unicode).filter(|c| !c.is_control() && *c != '{' && *c != '}') {
        Some(c) => text.push(c),
        None => text.push_str(&format!("{{${byte:02x}}}")),
    };
    for (number, body) in &lines {
        let mut text = String::new();
        let (mut quoted, mut plain) = (false, false);
        for &byte in *body {
            if byte == b'"' {
                quoted = !quoted;
            }
            match ORIC_TOKENS.get(usize::from(byte.wrapping_sub(0x80))).filter(|_| byte >= 0x80 && !quoted && !plain) {
                Some(word) => {
                    text.push_str(word);
                    plain = matches!(byte, ORIC_REM | ORIC_DATA);
                }
                None => char_of(byte, &mut text),
            }
        }
        listing.lines.push(format!("{number} {text}"));
    }
    if !whole {
        let last = lines.last().map_or(0, |(number, _)| *number);
        listing.notes.push(format!("The program is damaged after line {last}: the next line's link doesn't lead to a line, so the listing stops there."));
    }
    listing
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
    fn model_4_basic_is_told_from_mbasic_and_msx_by_its_equals() {
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
        assert!(microsoft_flavour(&high) == Some(Flavour::Model4) && !looks_like_trs80(&high));
        assert_eq!(list_trs80_model4(&high, charset::utf8()).lines, ["10 FOR I=1 TO 2:PRINT CHR$(65):NEXT"]);
        let low = program(0x6000, line);
        assert_eq!(microsoft_flavour(&low), Some(Flavour::Model4), "its = says Model 4");
        let mbasic = program(0x6000, b"\x82 I\xF0\x12 \xCE \x13:\x83");
        assert_eq!(microsoft_flavour(&mbasic), Some(Flavour::Mbasic));
        let msx = program(0x8001, line);
        assert_eq!(microsoft_flavour(&msx), Some(Flavour::Model4), "an = of Model 4's wherever it is");
        let msx = program(0xC001, b"\x82 I\xEF\x12 \xD9 \x13:\x83");
        assert_eq!(microsoft_flavour(&msx), Some(Flavour::Msx), "an MSX with 16K");
        assert_eq!(crate::basic::list(&msx, "bas", charset::utf8()).lines, ["10 FOR I=1 TO 2:NEXT"]);
        assert_eq!(microsoft_flavour(&program(0x8001, b"\x91")), Some(Flavour::Msx), "no =, but where MSX programs are");
    }

    /// A `.p` file: the system variables from &4009, the program's lines, a collapsed display file, no variables.
    fn zx81_p(lines: &[(u16, &[u8])], screen: &[&[u8]]) -> Vec<u8> {
        let mut program = Vec::new();
        for (number, body) in lines {
            program.extend(number.to_be_bytes());
            program.extend((body.len() as u16).to_le_bytes());
            program.extend_from_slice(body);
        }
        let mut display = vec![0x76];
        for row in 0..24 {
            display.extend_from_slice(screen.get(row).copied().unwrap_or_default());
            display.push(0x76);
        }
        let d_file = (ZX81_PROGRAM + program.len()) as u16;
        let vars = d_file + display.len() as u16;
        let mut file = vec![0u8; ZX81_PROGRAM - ZX81_FILE_START];
        file[3..5].copy_from_slice(&d_file.to_le_bytes());
        file[7..9].copy_from_slice(&vars.to_le_bytes());
        file[11..13].copy_from_slice(&(vars + 1).to_le_bytes());
        file.extend(program);
        file.extend(display);
        file.push(0x80);
        file
    }

    #[test]
    fn a_zx81_program_lists_with_the_roms_spaces_and_its_screen() {
        let file = zx81_p(
            &[
                (10, b"\xF5\x0B\x2D\x2E\x0B\x76"),
                (20, b"\xEC\x1D\x1C\x7E\x84\x20\x00\x00\x00\x76"),
                (30, b"\xFA\x26\x14\x1D\x7E\x81\x00\x00\x00\x00\xDE\xE3\x76"),
                (40, b"\xEA\xA6\x00\x01\x76\xC9\x76"),
                (50, b"\xF5\x40\x17\x1E\x7E\x82\x00\x00\x00\x00\xD8\x1F\x7E\x82\x40\x00\x00\x00\x76"),
                (60, b"\xF5\xD5\x0B\xC0\x0B\x76"),
            ],
            &[b"\x2D\x2E\x00\xAD\x80"],
        );
        assert!(is_program_file("p", Some(&file)) && !is_program_file("p", Some(b"program x; begin end.")));
        let listing = crate::basic::list(&file, "p", charset::utf8());
        assert_eq!(listing.dialect, "ZX81 BASIC");
        assert_eq!(
            listing.lines,
            ["10 PRINT \"HI\"", "20 GOTO 10", "30 IF A=1 THEN STOP", "40 REM %A \u{2598}", "50 PRINT RND*2 **3", "60 PRINT STR$ \"\"\"\""]
        );
        assert!(listing.notes.iter().any(|n| n.contains("machine code")), "line 40 goes on after its NEWLINE");
        assert!(listing.notes.iter().any(|n| n.contains('%')));
        assert_eq!(listing.screen.len(), 24);
        assert_eq!(listing.screen[0], "HI H\u{2588}", "inverse letters shown plain, an inverse space as a block");
        // The same program with a name before it, as a .p81 file.
        let mut named = vec![0x33, 0x2E, 0xB2];
        named.extend(&file);
        let listing = crate::basic::list(&named, "p81", charset::utf8());
        assert_eq!(listing.lines[0], "10 PRINT \"HI\"");
        assert_eq!(listing.info, [("Name".to_string(), "NIM".to_string())]);
    }

    #[test]
    fn a_damaged_zx81_file_says_so() {
        assert!(list_zx81(&[0u8; 8]).notes[0].contains("too short"));
        let file = zx81_p(&[(10, b"\xF5\x76")], &[]);
        let mut bad = file.clone();
        bad[3] = 0;
        bad[4] = 0;
        assert!(list_zx81(&bad).notes[0].contains("system variables"));
        let mut long = file.clone();
        long[ZX81_PROGRAM - ZX81_FILE_START + 2] = 0xFF;
        assert!(list_zx81(&long).notes[0].contains("longer"));
        for cut in 0..file.len() {
            let _ = list_zx81(&file[..cut]);
        }
    }

    /// A SAM BASIC file: its 9-byte header, then the lines and the &FF that ends them.
    fn sam_file(lines: &[(u16, &[u8])]) -> Vec<u8> {
        let mut program = Vec::new();
        for (number, body) in lines {
            program.extend(number.to_be_bytes());
            program.extend((body.len() as u16 + 1).to_le_bytes());
            program.extend_from_slice(body);
            program.push(0x0D);
        }
        program.push(0xFF);
        let mut file = vec![16, program.len() as u8, (program.len() >> 8) as u8, 0x00, 0x80, 0, 0, 0, 1];
        file.extend(program);
        file
    }

    #[test]
    fn a_sam_basic_program_lists_with_the_roms_spacing() {
        let number = |digits: &[u8], value: u8| [digits, &[0x0E, 0, 0, value, 0, 0]].concat();
        let line10 = [&[0xBB, 0x87][..], &number(b"1", 1), b",", &number(b"2", 2), b";\"\x10\x02HI\":", &[0xB4], &number(b"10", 10)].concat();
        let line20 = [&b"\x9Ca=\xFF\x3C:\x9Cb$=\xFF\x70"[..], &number(b"65", 65), b"\xFF\x80", &number(b"1", 1)].concat();
        let file = sam_file(&[(10, &line10), (20, &line20), (30, b"\xB7\xBB stays")]);
        assert!(looks_like_sam_basic(&file) && is_program_file("bas", Some(&file)));
        let listing = crate::basic::list(&file, "bas", charset::utf8());
        assert_eq!(listing.dialect, "SAM BASIC");
        assert_eq!(listing.lines, ["10 PRINT AT 1,2;\"{PEN 2}HI\": GO TO 10", "20 LET a=RND: LET b$=CHR$ 65 AND 1", "30 REM {$bb} stays"]);
        let mut cut = file.clone();
        cut.truncate(9 + line10.len() + 5 + line20.len() + 5 + 2);
        cut[1] = 0;
        cut[2] = 0;
        let listing = list_sam(&cut);
        assert_eq!(listing.lines.len(), 2);
        assert!(listing.notes[0].contains("breaks off"));
        for end in 0..file.len() {
            let _ = list_sam(&file[..end]);
            let _ = looks_like_sam_basic(&file[..end]);
        }
    }

    /// Real tapes, disks and programs from the TOSEC collections and the TRS-80 File Cabinet disks, where they're
    /// kept beside the source: each machine's programs list in its own BASIC, and nothing in them panics.
    #[test]
    fn real_media_of_these_machines_list_their_programs() {
        let root = std::path::Path::new("../testmedia");
        if !root.is_dir() {
            return;
        }
        let mut found: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for folder in ["trs80", "zx81", "msx", "sam-coupe", "oric"] {
            let Ok(items) = std::fs::read_dir(root.join(folder)) else {
                continue;
            };
            for path in items.flatten().map(|item| item.path()) {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                let ending = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
                let bytes = std::fs::read(&path).unwrap();
                if crate::basic::is_program_file(&ending, Some(&bytes)) && !crate::media::is_image(&name, &bytes, bytes.len() as u64) {
                    *found.entry(crate::basic::list(&bytes, &ending, charset::utf8()).dialect).or_default() += 1;
                    continue;
                }
                let Ok(volume) = crate::media::open(std::sync::Arc::new(bytes), &name) else {
                    continue;
                };
                let mut entries = volume.list(None).unwrap();
                for dir in entries.clone().iter().filter(|e| e.is_dir) {
                    entries.extend(volume.list(Some(dir)).unwrap());
                }
                for entry in entries.iter().filter(|e| !e.is_dir) {
                    let Ok(data) = volume.read(entry) else {
                        continue;
                    };
                    let ending = entry.open_name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
                    if crate::basic::is_program_file(&ending, Some(&data)) {
                        let listing = crate::basic::list(&data, &ending, charset::utf8());
                        if !listing.lines.is_empty() {
                            *found.entry(listing.dialect).or_default() += 1;
                        }
                    }
                }
            }
        }
        for dialect in ["TRS-80 Level II BASIC", "TRS-80 Model 4 BASIC", "ZX81 BASIC", "MSX-BASIC", "SAM BASIC", "Oric BASIC"] {
            assert!(found.get(dialect).is_some_and(|n| *n > 0), "no {dialect} programs were listed: {found:?}");
        }
    }
}
