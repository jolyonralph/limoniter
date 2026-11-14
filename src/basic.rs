//! Tokenised BASIC: listing a saved program as the machine's LIST would, and tokenising a listing back into a
//! program the machine (or an emulator) can load.
//!
//! - Commodore BASIC V2 (C64, VIC-20, PET): `.PRG` files and PC64's `.P00`. Keywords from the C64 Programmer's
//!   Reference Guide's token table, codes in strings written as `{clr}`, `{red}` and so on, as `petcat` does.
//! - Sinclair ZX Spectrum BASIC, 48K and 128K: programs inside `.TAP` and `.TZX` tapes and `+3DOS` files. The
//!   keyword names are the Character table's own, so the two can't disagree.
//! - ZX Spectrum Next's NextBASIC: `+3DOS` `.bas` files, with the keywords NextZXOS adds from 0x81, integer
//!   expressions after `%`, `$` hex and `@` binary numbers, `@` labels, `;` comments and dot commands. The rules
//!   follow NextBASIC's own documentation and, where it doesn't say, programs saved by NextZXOS itself.
//!
//! Listing and tokenising are made to undo each other: tokenising a listing gives back the program's bytes.
//! 48K keywords are stored without the spaces LIST puts round them; NextBASIC keeps a space typed before a keyword
//! and drops the one after it. Every number in a standard expression is followed by its hidden five-byte form,
//! which is what the ROM uses when the line runs; NextBASIC's integer expressions have none.

use crate::charset::{self, Charset};

/// What a listing found, ready to show or to open as text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Listing {
    pub dialect: &'static str,
    pub info: Vec<(String, String)>,
    pub lines: Vec<String>,
    pub notes: Vec<String>,
    /// What the program's screen showed when it was saved, row by row, for machines that save it with the program.
    pub screen: Vec<String>,
}

// ---------- Commodore ----------

/// BASIC V2's keywords, for tokens 0x80 to 0xCB, in the ROM's own order. The order matters when tokenising:
/// the ROM tries them in turn, so INPUT# is found before INPUT and GOTO before GO.
const CBM_KEYWORDS: [&str; 76] = [
    "END", "FOR", "NEXT", "DATA", "INPUT#", "INPUT", "DIM", "READ", "LET", "GOTO", "RUN", "IF", "RESTORE", "GOSUB", "RETURN", "REM", "STOP", "ON", "WAIT", "LOAD", "SAVE", "VERIFY", "DEF", "POKE", "PRINT#", "PRINT",
    "CONT", "LIST", "CLR", "CMD", "SYS", "OPEN", "CLOSE", "GET", "NEW", "TAB(", "TO", "FN", "SPC(", "THEN", "NOT", "STEP", "+", "-", "*", "/", "^", "AND", "OR", ">", "=", "<", "SGN", "INT", "ABS", "USR", "FRE", "POS",
    "SQR", "RND", "LOG", "EXP", "COS", "SIN", "TAN", "ATN", "PEEK", "LEN", "STR$", "VAL", "ASC", "CHR$", "LEFT$", "RIGHT$", "MID$", "GO",
];

/// Names for PETSCII's control codes in a listing, as the `petcat` tool writes them.
const CBM_CODES: [(u8, &str); 39] = [
    (0x05, "wht"),
    (0x07, "bell"),
    (0x08, "dish"),
    (0x09, "ensh"),
    (0x0D, "cr"),
    (0x0E, "swlc"),
    (0x11, "down"),
    (0x12, "rvon"),
    (0x13, "home"),
    (0x14, "del"),
    (0x1C, "red"),
    (0x1D, "rght"),
    (0x1E, "grn"),
    (0x1F, "blu"),
    (0x81, "orng"),
    (0x85, "f1"),
    (0x86, "f3"),
    (0x87, "f5"),
    (0x88, "f7"),
    (0x89, "f2"),
    (0x8A, "f4"),
    (0x8B, "f6"),
    (0x8C, "f8"),
    (0x8D, "sret"),
    (0x8E, "swuc"),
    (0x90, "blk"),
    (0x91, "up"),
    (0x92, "rvof"),
    (0x93, "clr"),
    (0x94, "inst"),
    (0x95, "brn"),
    (0x96, "lred"),
    (0x97, "gry1"),
    (0x98, "gry2"),
    (0x99, "lgrn"),
    (0x9A, "lblu"),
    (0x9B, "gry3"),
    (0x9C, "pur"),
    (0x9E, "yel"),
];
const CBM_CYAN: (u8, &str) = (0x9F, "cyn");
const CBM_LEFT: (u8, &str) = (0x9D, "left");

fn cbm_code_name(byte: u8) -> Option<&'static str> {
    CBM_CODES.iter().chain([&CBM_CYAN, &CBM_LEFT]).find(|(b, _)| *b == byte).map(|(_, name)| *name)
}

fn cbm_code_byte(name: &str) -> Option<u8> {
    let lower = name.to_lowercase();
    if let Some(hex) = lower.strip_prefix('$') {
        return u8::from_str_radix(hex, 16).ok();
    }
    CBM_CODES.iter().chain([&CBM_CYAN, &CBM_LEFT]).find(|(_, n)| *n == lower).map(|(b, _)| *b)
}

/// The Commodore machines a program can be tokenised for, each with its BASIC and where its programs load.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CbmMachine {
    C64,
    Vic20,
    /// The PET and CBM, with BASIC 4.0.
    Pet,
    /// The Commodore 16 and Plus/4, with BASIC 3.5.
    Plus4,
    /// The Commodore 128, with BASIC 7.0.
    C128,
}

impl CbmMachine {
    pub const ALL: [CbmMachine; 5] = [CbmMachine::C64, CbmMachine::Vic20, CbmMachine::Pet, CbmMachine::Plus4, CbmMachine::C128];

    pub fn name(self) -> &'static str {
        match self {
            CbmMachine::C64 => "C64 (BASIC 2.0)",
            CbmMachine::Vic20 => "VIC-20 (BASIC 2.0)",
            CbmMachine::Pet => "PET and CBM (BASIC 4.0)",
            CbmMachine::Plus4 => "C16 and Plus/4 (BASIC 3.5)",
            CbmMachine::C128 => "C128 (BASIC 7.0)",
        }
    }

    fn dialect(self) -> &'static str {
        match self {
            CbmMachine::C64 | CbmMachine::Vic20 => "Commodore BASIC 2.0",
            CbmMachine::Pet => "Commodore BASIC 4.0",
            CbmMachine::Plus4 => "Commodore BASIC 3.5",
            CbmMachine::C128 => "Commodore BASIC 7.0",
        }
    }

    /// Where its BASIC programs start in memory, unexpanded.
    pub fn load_address(self) -> u16 {
        match self {
            CbmMachine::C64 => 0x0801,
            CbmMachine::Vic20 | CbmMachine::Plus4 => 0x1001,
            CbmMachine::Pet => 0x0401,
            CbmMachine::C128 => 0x1C01,
        }
    }

    /// The machine a document's PETSCII set is for.
    pub fn of(set: &Charset) -> Self {
        if set.id.ends_with("vic20") {
            CbmMachine::Vic20
        } else if set.id.ends_with("-pet") {
            CbmMachine::Pet
        } else {
            CbmMachine::C64
        }
    }

    fn petscii(self) -> &'static Charset {
        charset::known(match self {
            CbmMachine::Vic20 => "petscii-vic20",
            CbmMachine::Pet => "petscii-pet",
            _ => "petscii-c64",
        })
    }

    /// The keywords this BASIC adds to 2.0's, each with its bytes.
    fn extra_keywords(self) -> Vec<(&'static str, Vec<u8>)> {
        let single = |table: &[&'static str], first: u8| table.iter().enumerate().map(move |(i, w)| (*w, vec![first + i as u8])).collect::<Vec<_>>();
        match self {
            CbmMachine::C64 | CbmMachine::Vic20 => Vec::new(),
            CbmMachine::Pet => single(&BASIC4, 0xCC),
            CbmMachine::Plus4 => single(&BASIC35, 0xCC),
            CbmMachine::C128 => {
                // 7.0 takes 3.5's keywords, except that &CE becomes the first byte of two-byte tokens, as does &FE.
                let mut words: Vec<(&'static str, Vec<u8>)> = single(&BASIC35, 0xCC).into_iter().filter(|(_, bytes)| bytes[0] != 0xCE).collect();
                words.extend(BASIC7_CE.iter().map(|(b, w)| (*w, vec![0xCE, *b])));
                words.extend(BASIC7_FE.iter().map(|(b, w)| (*w, vec![0xFE, *b])));
                words
            }
        }
    }
}

/// BASIC 4.0's added keywords, &CC to &DA, from the C64-Wiki's "BASIC token" table.
const BASIC4: [&str; 15] = ["CONCAT", "DOPEN", "DCLOSE", "RECORD", "HEADER", "COLLECT", "BACKUP", "COPY", "APPEND", "DSAVE", "DLOAD", "CATALOG", "RENAME", "SCRATCH", "DIRECTORY"];
/// BASIC 3.5's, &CC to &FD.
const BASIC35: [&str; 50] = [
    "RGR", "RCLR", "RLUM", "JOY", "RDOT", "DEC", "HEX$", "ERR$", "INSTR", "ELSE", "RESUME", "TRAP", "TRON", "TROFF", "SOUND", "VOL", "AUTO", "PUDEF", "GRAPHIC", "PAINT", "CHAR", "BOX", "CIRCLE", "GSHAPE", "SSHAPE", "DRAW", "LOCATE",
    "COLOR", "SCNCLR", "SCALE", "HELP", "DO", "LOOP", "EXIT", "DIRECTORY", "DSAVE", "DLOAD", "HEADER", "SCRATCH", "COLLECT", "COPY", "RENAME", "BACKUP", "DELETE", "RENUMBER", "KEY", "MONITOR", "USING", "UNTIL", "WHILE",
];
/// BASIC 7.0's two-byte tokens after &CE.
const BASIC7_CE: [(u8, &str); 9] = [(0x02, "POT"), (0x03, "BUMP"), (0x04, "PEN"), (0x05, "RSPPOS"), (0x06, "RSPRITE"), (0x07, "RSPCOLOR"), (0x08, "XOR"), (0x09, "RWINDOW"), (0x0A, "POINTER")];
/// And after &FE.
const BASIC7_FE: [(u8, &str); 35] = [
    (0x02, "BANK"),
    (0x03, "FILTER"),
    (0x04, "PLAY"),
    (0x05, "TEMPO"),
    (0x06, "MOVSPR"),
    (0x07, "SPRITE"),
    (0x08, "SPRCOLOR"),
    (0x09, "RREG"),
    (0x0A, "ENVELOPE"),
    (0x0B, "SLEEP"),
    (0x0C, "CATALOG"),
    (0x0D, "DOPEN"),
    (0x0E, "APPEND"),
    (0x0F, "DCLOSE"),
    (0x10, "BSAVE"),
    (0x11, "BLOAD"),
    (0x12, "RECORD"),
    (0x13, "CONCAT"),
    (0x14, "DVERIFY"),
    (0x15, "DCLEAR"),
    (0x16, "SPRSAV"),
    (0x17, "COLLISION"),
    (0x18, "BEGIN"),
    (0x19, "BEND"),
    (0x1A, "WINDOW"),
    (0x1B, "BOOT"),
    (0x1C, "WIDTH"),
    (0x1D, "SPRDEF"),
    (0x1E, "QUIT"),
    (0x1F, "STASH"),
    (0x21, "FETCH"),
    (0x23, "SWAP"),
    (0x24, "OFF"),
    (0x25, "FAST"),
    (0x26, "SLOW"),
];

/// One character of a listing, from a PETSCII byte that isn't a keyword.
fn cbm_char(set: &Charset, byte: u8, out: &mut String) {
    let glyph = if (0x20..0x80).contains(&byte) || byte >= 0xA1 { set.entry(0, byte).and_then(|e| e.unicode) } else { None };
    match glyph.filter(|c| *c != '{' && *c != '}') {
        Some(c) => out.push(c),
        None => match cbm_code_name(byte) {
            Some(name) => out.push_str(&format!("{{{name}}}")),
            None => out.push_str(&format!("{{${byte:02x}}}")),
        },
    }
}

/// Which machine a program is for, from where it loads and whether it uses keywords 2.0 hasn't got.
fn cbm_machine_of(load: u16, memory: &[u8], set: &Charset) -> CbmMachine {
    let extended = || {
        let mut quoted = false;
        memory.iter().any(|b| {
            quoted ^= *b == b'"';
            !quoted && (0xCC..=0xFE).contains(b)
        })
    };
    match load {
        0x1C01 => CbmMachine::C128,
        // The C16 and Plus/4 load where an unexpanded VIC-20 does; their programs use 3.5's keywords.
        0x1001 if extended() => CbmMachine::Plus4,
        0x1001 | 0x1201 => CbmMachine::Vic20,
        0x0401 if set.id.ends_with("vic20") => CbmMachine::Vic20,
        0x0401 => CbmMachine::Pet,
        _ => CbmMachine::C64,
    }
}

/// Lists a Commodore BASIC program: two bytes of load address, then linked lines. The machine, and so the BASIC,
/// is told from where it loads.
pub fn list_commodore(data: &[u8], set: &Charset) -> Listing {
    let mut listing = Listing::default();
    let mut data = data;
    if data.starts_with(b"C64File\0") && data.len() >= 26 {
        let name: String = data[8..24].iter().take_while(|b| **b != 0).map(|b| *b as char).collect();
        listing.info.push(("Name".into(), name));
        data = &data[26..];
    }
    if data.len() < 2 {
        listing.dialect = CbmMachine::C64.dialect();
        listing.notes.push("The file is too short to be a program.".into());
        return listing;
    }
    let load = u16::from_le_bytes([data[0], data[1]]);
    let memory = &data[2..];
    let machine = cbm_machine_of(load, memory, set);
    listing.dialect = machine.dialect();
    let set = if set.id.starts_with("petscii-") { charset::known(&set.id) } else { machine.petscii() };
    listing.info.push(("Loads at".into(), format!("${load:04X}, as a {} program", machine.name())));
    let two_byte = |first: u8, second: u8| {
        let table: &[(u8, &str)] = if first == 0xCE { &BASIC7_CE } else { &BASIC7_FE };
        table.iter().find(|(b, _)| *b == second).map(|(_, w)| *w)
    };
    let mut at = 0;
    let mut previous: Option<u16> = None;
    loop {
        if at + 2 > memory.len() {
            listing.notes.push("The program stops without its end marker.".into());
            break;
        }
        if memory[at] == 0 && memory[at + 1] == 0 {
            at += 2;
            break;
        }
        let Some(end) = memory.get(at + 4..).and_then(|rest| rest.iter().position(|b| *b == 0)).map(|n| at + 4 + n) else {
            listing.notes.push(format!("A line starting at byte {} doesn't end, so the listing stops there.", at + 2));
            break;
        };
        let number = u16::from_le_bytes([memory[at + 2], memory[at + 3]]);
        if number > 63999 || previous.is_some_and(|p| number <= p) {
            listing.notes.push(format!("What follows byte {} doesn't look like BASIC lines, so it isn't listed.", at + 2));
            at = memory.len();
            break;
        }
        previous = Some(number);
        let mut text = format!("{number} ");
        let mut quoted = false;
        let body = &memory[at + 4..end];
        let mut i = 0;
        while i < body.len() {
            let byte = body[i];
            i += 1;
            match byte {
                b'"' => {
                    quoted = !quoted;
                    text.push('"');
                }
                0x80..=0xCB if !quoted => text.push_str(CBM_KEYWORDS[usize::from(byte - 0x80)]),
                0xFF if !quoted => text.push('\u{3C0}'),
                0xCE | 0xFE if !quoted && machine == CbmMachine::C128 => match body.get(i).and_then(|second| two_byte(byte, *second)) {
                    Some(word) => {
                        text.push_str(word);
                        i += 1;
                    }
                    None => text.push_str(&format!("{{${byte:02x}}}")),
                },
                0xCC..=0xDA if !quoted && machine == CbmMachine::Pet => text.push_str(BASIC4[usize::from(byte - 0xCC)]),
                0xCC..=0xFD if !quoted && matches!(machine, CbmMachine::Plus4 | CbmMachine::C128) => text.push_str(BASIC35[usize::from(byte - 0xCC)]),
                0xCC..=0xFE if !quoted => text.push_str(&format!("{{${byte:02x}}}")),
                _ => cbm_char(set, byte, &mut text),
            }
        }
        listing.lines.push(text);
        at = end + 1;
    }
    if at < memory.len() {
        listing.notes.push(format!("{} byte(s) after the program, usually machine code, aren't listed.", memory.len() - at));
    }
    if listing.lines.is_empty() && listing.notes.is_empty() {
        listing.notes.push("The program has no lines.".into());
    }
    listing
}

/// Tokenises a listing into a program file for a Commodore machine, load address first.
pub fn tokenise_commodore<S: AsRef<str>>(lines: &[S], machine: CbmMachine) -> Result<Vec<u8>, String> {
    let set = machine.petscii();
    let extra = machine.extra_keywords();
    let load = machine.load_address();
    let mut out = load.to_le_bytes().to_vec();
    let mut address = load;
    let mut previous: Option<u32> = None;
    for (index, line) in lines.iter().enumerate() {
        let Some((number, rest)) = split_number(line.as_ref(), index, previous, 63999)? else {
            continue;
        };
        previous = Some(number);
        let body = cbm_crunch(rest.trim_start(), set, &extra).map_err(|problem| format!("Line {} (BASIC line {number}): {problem}", index + 1))?;
        address = address.checked_add(u16::try_from(body.len() + 5).map_err(|_| format!("Line {} is too long.", index + 1))?).ok_or("The program is too big for the machine's memory.")?;
        out.extend_from_slice(&address.to_le_bytes());
        out.extend_from_slice(&(number as u16).to_le_bytes());
        out.extend_from_slice(&body);
        out.push(0);
    }
    out.extend_from_slice(&[0, 0]);
    Ok(out)
}

/// The ROM's CRUNCH: keywords anywhere outside quotes, nothing after REM, and DATA left alone up to a colon.
/// 2.0's keywords are tried in the ROM's order; a later BASIC's own are tried after them, longest first, so that
/// DOPEN isn't taken for DO.
fn cbm_crunch(text: &str, set: &Charset, extra: &[(&'static str, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut extra: Vec<&(&'static str, Vec<u8>)> = extra.iter().collect();
    extra.sort_by_key(|(word, _)| std::cmp::Reverse(word.len()));
    let matches = |i: usize, word: &str| {
        let word: Vec<char> = word.chars().collect();
        chars.len() >= i + word.len() && chars[i..i + word.len()].iter().zip(&word).all(|(a, b)| a.to_ascii_uppercase() == *b)
    };
    let mut out = Vec::new();
    let mut i = 0;
    let (mut quoted, mut rem, mut data) = (false, false, false);
    while i < chars.len() {
        let c = chars[i];
        if c == '{'
            && let Some(close) = chars[i..].iter().position(|ch| *ch == '}')
        {
            let name: String = chars[i + 1..i + close].iter().collect();
            out.push(cbm_code_byte(&name).ok_or_else(|| format!("{{{name}}} isn't a code this knows"))?);
            i += close + 1;
            continue;
        }
        if c == '"' {
            quoted = !quoted;
        }
        if c == ':' && !quoted {
            data = false;
        }
        if !quoted && !rem && !data {
            if c == '?' {
                out.push(0x99);
                i += 1;
                continue;
            }
            if c == '\u{3C0}' {
                out.push(0xFF);
                i += 1;
                continue;
            }
            if let Some((token, word)) = CBM_KEYWORDS.iter().enumerate().find(|(_, word)| matches(i, word)) {
                out.push(0x80 + token as u8);
                i += word.chars().count();
                rem |= *word == "REM";
                data |= *word == "DATA";
                continue;
            }
            if let Some((word, bytes)) = extra.iter().find(|(word, _)| matches(i, word)) {
                out.extend_from_slice(bytes);
                i += word.chars().count();
                continue;
            }
        }
        // Typing a small letter on a Commodore in its capitals mode gives the capital.
        let typed = if c.is_ascii_lowercase() { c.to_ascii_uppercase() } else { c };
        let byte = set.table(0).and_then(|t| t.byte_of(typed)).filter(|b| *b >= 0x20).ok_or_else(|| format!("'{c}' has no PETSCII code"))?;
        out.push(byte);
        i += 1;
    }
    Ok(out)
}

/// A listing line's number and what follows it. Blank lines are skipped; anything else must have a number.
fn split_number(line: &str, index: usize, previous: Option<u32>, most: u32) -> Result<Option<(u32, &str)>, String> {
    let trimmed = line.trim_start();
    if trimmed.trim().is_empty() {
        return Ok(None);
    }
    let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
    let number: u32 = trimmed[..digits].parse().map_err(|_| format!("Line {} doesn't start with a line number.", index + 1))?;
    if number > most {
        return Err(format!("Line {}: {number} is past the last line number there can be, {most}.", index + 1));
    }
    if previous.is_some_and(|p| number <= p) {
        return Err(format!("Line {}: line numbers have to go up, and {number} comes after {}.", index + 1, previous.unwrap_or(0)));
    }
    Ok(Some((number, &trimmed[digits..])))
}

// ---------- ZX Spectrum and Spectrum Next ----------

/// Which Sinclair BASIC a program is in. They share the 48K's tokens from 0xA5 up; the 128K adds SPECTRUM and
/// PLAY, and NextBASIC adds keywords from 0x81, in the codes the 48K used for block graphics and UDGs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZxDialect {
    /// 48K and 128K BASIC. Keywords are single keypresses, stored with no spaces round them.
    Spectrum,
    /// NextBASIC, as NextZXOS 2.10 has it. A space typed before a keyword is kept; the one after it isn't.
    Next,
}

impl ZxDialect {
    pub fn name(self) -> &'static str {
        match self {
            ZxDialect::Spectrum => "Sinclair ZX Spectrum BASIC",
            ZxDialect::Next => "ZX Spectrum Next BASIC (NextBASIC)",
        }
    }
}

/// NextBASIC's added keywords, from "NextBASIC New Commands and Features" (NextZXOS 2.10, updated 7 May 2026),
/// with the 128K's two. 0x83 is IF as it's stored after ELSE, in a long-form ELSE IF.
const NEXT_KEYWORDS: [(u8, &str); 36] = [
    (0x81, "TIME"),
    (0x82, "PRIVATE"),
    (0x83, "IF"),
    (0x84, "ENDIF"),
    (0x85, "EXIT"),
    (0x86, "REF"),
    (0x87, "PEEK$"),
    (0x88, "REG"),
    (0x89, "DPOKE"),
    (0x8A, "DPEEK"),
    (0x8B, "MOD"),
    (0x8C, "<<"),
    (0x8D, ">>"),
    (0x8E, "UNTIL"),
    (0x8F, "ERROR"),
    (0x90, "ON"),
    (0x91, "DEFPROC"),
    (0x92, "ENDPROC"),
    (0x93, "PROC"),
    (0x94, "LOCAL"),
    (0x95, "DRIVER"),
    (0x96, "WHILE"),
    (0x97, "REPEAT"),
    (0x98, "ELSE"),
    (0x99, "REMOUNT"),
    (0x9A, "BANK"),
    (0x9B, "TILE"),
    (0x9C, "LAYER"),
    (0x9D, "PALETTE"),
    (0x9E, "SPRITE"),
    (0x9F, "PWD"),
    (0xA0, "CD"),
    (0xA1, "MKDIR"),
    (0xA2, "RMDIR"),
    (0xA3, "SPECTRUM"),
    (0xA4, "PLAY"),
];
const ZX_IF: u8 = 0xFA;
const ZX_ELSE_IF: u8 = 0x83;
const ZX_ELSE: u8 = 0x98;
const ZX_THEN: u8 = 0xCB;
const ZX_REM: u8 = 0xEA;
const ZX_BIN: u8 = 0xC4;
const ZX_DEF_FN: u8 = 0xCE;

/// The keywords an integer expression may go on through: its operators and integer functions.
const ZX_INTEGER_WORDS: [&str; 20] = ["MOD", "<<", ">>", "AND", "OR", "NOT", "IN", "REG", "PEEK", "DPEEK", "USR", "BANK", "RND", "BIN", "SPRITE", "INPUT", "AT", "OVER", "CONTINUE", "<>"];

fn zx() -> &'static Charset {
    charset::known("zx-spectrum")
}

/// The keyword a token stands for in a dialect. The 48K's come from the Character table, so they can't disagree.
fn zx_keyword(byte: u8, dialect: ZxDialect) -> Option<&'static str> {
    if byte >= 0xA5 {
        return zx().entry(0, byte).and_then(|e| e.name.strip_prefix("BASIC keyword "));
    }
    let next_only = dialect == ZxDialect::Next || matches!(byte, 0xA3 | 0xA4);
    NEXT_KEYWORDS.iter().find(|(b, _)| *b == byte && next_only).map(|(_, word)| *word)
}

/// The controls that carry their own values in a line, and how many.
fn zx_control(byte: u8) -> Option<(&'static str, usize)> {
    Some(match byte {
        0x10 => ("INK", 1),
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

/// A byte that isn't a keyword, as listing text: a character where it has one, else an escape.
fn zx_char(byte: u8, out: &mut String) {
    match byte {
        0x90..=0xA4 => out.push_str(&format!("{{UDG-{}}}", (b'A' + byte - 0x90) as char)),
        _ => match zx().entry(0, byte).and_then(|e| e.unicode).filter(|c| !c.is_control() && *c != '{' && *c != '}') {
            Some(c) => out.push(c),
            None => out.push_str(&format!("{{${byte:02x}}}")),
        },
    }
}

/// NextBASIC if a byte only Next has as a keyword is used as one, outside strings and comments.
fn zx_dialect_of(program: &[u8]) -> ZxDialect {
    let mut found = ZxDialect::Spectrum;
    zx_walk(program, |_, body| {
        let (mut quoted, mut i) = (false, 0);
        while i < body.len() {
            match body[i] {
                0x0E => i += 5,
                b'"' => quoted = !quoted,
                ZX_REM if !quoted => return,
                0x81..=0xA2 if !quoted => found = ZxDialect::Next,
                b if !quoted && zx_control(b).is_some() => i += zx_control(b).map_or(0, |c| c.1),
                _ => {}
            }
            i += 1;
        }
    });
    found
}

/// Goes through a program's lines: number, then the bytes up to its ENTER. Stops where lines stop making sense.
fn zx_walk(program: &[u8], mut each: impl FnMut(u16, &[u8])) -> Option<usize> {
    let mut at = 0;
    while at + 4 <= program.len() {
        let number = u16::from_be_bytes([program[at], program[at + 1]]);
        let length = usize::from(u16::from_le_bytes([program[at + 2], program[at + 3]]));
        if number > 9999 {
            return Some(at);
        }
        let end = (at + 4 + length).min(program.len());
        let body = &program[at + 4..end];
        each(number, body.strip_suffix(&[0x0D]).unwrap_or(body));
        at = end;
    }
    None
}

/// Lists the program part of a Spectrum BASIC block, telling 48K BASIC and NextBASIC apart.
fn zx_lines(program: &[u8], listing: &mut Listing) {
    let dialect = zx_dialect_of(program);
    listing.dialect = dialect.name();
    let mut lines = Vec::new();
    let stopped = zx_walk(program, |number, body| lines.push(zx_line(number, body, dialect)));
    listing.lines.extend(lines);
    if let Some(at) = stopped {
        listing.notes.push(format!("What follows byte {at} doesn't look like a BASIC line, so it isn't listed."));
    }
}

fn zx_line(number: u16, body: &[u8], dialect: ZxDialect) -> String {
    let mut text = format!("{number:>4} ");
    let mut i = 0;
    let mut quoted = false;
    let mut comment = false;
    let mut statement_start = true;
    // In a 48K REM, after a colon: the editor's back in keyword mode, and TR-DOS reads what follows as commands.
    let mut rem_commands: Option<bool> = None;
    let mut in_rem = false;
    while i < body.len() {
        let byte = body[i];
        i += 1;
        if let Some(quoted_here) = rem_commands.as_mut() {
            match byte {
                b'"' => {
                    *quoted_here = !*quoted_here;
                    text.push('"');
                }
                _ if *quoted_here => zx_char(byte, &mut text),
                _ => match zx_keyword(byte, dialect) {
                    // Exactly one space each side, which tokenising takes off again, so spaces typed there are kept.
                    Some(word) if word.starts_with(|c: char| c.is_ascii_alphabetic()) => text.push_str(&format!(" {word} ")),
                    Some(word) => text.push_str(word),
                    // Letters that would be taken for a keyword start with an escape, so they stay letters.
                    None if byte.is_ascii_uppercase() && zx_rem_keyword_at(&body[i - 1..], body.get(i.wrapping_sub(2)).copied()) => text.push_str(&format!("{{${byte:02x}}}")),
                    None => zx_char(byte, &mut text),
                },
            }
            continue;
        }
        if comment {
            if byte == b':' && dialect == ZxDialect::Spectrum && in_rem {
                rem_commands = Some(false);
            }
            zx_char(byte, &mut text);
            continue;
        }
        match byte {
            0x0E => i += 5,
            b'"' => {
                quoted = !quoted;
                text.push('"');
            }
            _ if zx_control(byte).is_some() => {
                let (name, count) = zx_control(byte).unwrap_or(("", 0));
                let values: Vec<String> = body.get(i..i + count).unwrap_or(&[]).iter().map(u8::to_string).collect();
                text.push_str(&format!("{{{name} {}}}", values.join(",")));
                i += count;
            }
            // In strings, codes are characters: Next's keyword codes are UDGs there, as in the 48K.
            _ if quoted => zx_char(byte, &mut text),
            b';' if dialect == ZxDialect::Next && statement_start => {
                text.push(';');
                comment = true;
            }
            _ => match zx_keyword(byte, dialect) {
                Some(word) => {
                    let alphabetic = word.starts_with(|c: char| c.is_ascii_alphabetic());
                    // The 48K's LIST puts a space before a keyword; on the Next any space before one is in the line.
                    if alphabetic && dialect == ZxDialect::Spectrum && !text.ends_with(' ') {
                        text.push(' ');
                    }
                    text.push_str(word);
                    if alphabetic {
                        text.push(' ');
                    }
                    comment = byte == ZX_REM;
                    in_rem = comment;
                    statement_start = matches!(byte, ZX_THEN | ZX_ELSE);
                    continue;
                }
                None => zx_char(byte, &mut text),
            },
        }
        if byte == b':' && !quoted {
            statement_start = true;
        } else if byte != b' ' {
            statement_start = false;
        }
    }
    text.trim_end().to_string()
}

/// A Spectrum tape header: what the block after it holds.
struct ZxHeader {
    kind: u8,
    name: String,
    length: u16,
    start: u16,
    program: u16,
}

fn zx_header(body: &[u8]) -> Option<ZxHeader> {
    (body.len() >= 17).then(|| ZxHeader {
        kind: body[0],
        name: body[1..11].iter().map(|b| zx().entry(0, *b).and_then(|e| e.unicode).filter(|c| !c.is_control()).unwrap_or('?')).collect::<String>().trim_end().to_string(),
        length: u16::from_le_bytes([body[11], body[12]]),
        start: u16::from_le_bytes([body[13], body[14]]),
        program: u16::from_le_bytes([body[15], body[16]]),
    })
}

/// What one block of a tape adds to the listing; `header` carries a header on to the block it describes.
fn zx_block(block: &[u8], header: &mut Option<ZxHeader>, listing: &mut Listing) {
    if block.len() < 2 {
        return;
    }
    let (flag, body) = (block[0], &block[1..block.len() - 1]);
    if flag == 0x00
        && let Some(found) = zx_header(body)
    {
        let kind = match found.kind {
            0 => "Program",
            1 => "Number array",
            2 => "Character array",
            _ => "Bytes",
        };
        let detail = match found.kind {
            0 if found.start < 10000 => format!("{}, runs from line {}", found.name, found.start),
            0 => found.name.clone(),
            3 => format!("{}, {} bytes at {}", found.name, found.length, found.start),
            _ => format!("{}, {} bytes", found.name, found.length),
        };
        listing.info.push((kind.into(), detail));
        *header = Some(found);
        return;
    }
    if let Some(found) = header.take().filter(|found| found.kind == 0) {
        if !listing.lines.is_empty() {
            listing.lines.push(String::new());
        }
        let program = &body[..usize::from(found.program).min(body.len())];
        zx_lines(program, listing);
        if body.len() > program.len() {
            listing.notes.push(format!("{}'s variables, saved after its lines, aren't listed.", found.name));
        }
    }
}

/// The data blocks on a tape, each a flag byte, its data and a checksum, and why the tape ended early if it did.
/// A `.TAP` file is just its blocks, each after a two-byte length. A `.TZX` file has standard and turbo data
/// blocks among blocks about timing and the tape itself; those are stepped over, and an unknown one ends the
/// tape, as its length can't be known.
pub fn tape_blocks(data: &[u8]) -> (Vec<&[u8]>, Option<String>) {
    let (places, problem) = tape_block_places(data);
    (places.into_iter().map(|(_, start, length)| &data[start..start + length]).collect(), problem)
}

/// Where each data block is on a tape: where the whole block starts (its length, or its TZX block ID), where its
/// flag byte is, and the length from the flag to the checksum; and why the tape ended early if it did.
pub fn tape_block_places(data: &[u8]) -> (Vec<(usize, usize, usize)>, Option<String>) {
    let mut blocks = Vec::new();
    if !data.starts_with(b"ZXTape!\x1A") {
        let mut at = 0;
        while at + 2 <= data.len() {
            let length = usize::from(u16::from_le_bytes([data[at], data[at + 1]]));
            if data.len() < at + 2 + length {
                return (blocks, Some("The tape ends partway through a block.".into()));
            }
            blocks.push((at, at + 2, length));
            at += 2 + length;
        }
        if at < data.len() {
            return (blocks, Some("The tape ends partway through a block.".into()));
        }
        return (blocks, None);
    }
    let mut at = 10;
    let word = |at: usize| data.get(at..at + 2).map_or(0, |b| usize::from(u16::from_le_bytes([b[0], b[1]])));
    let three = |at: usize| data.get(at..at + 3).map_or(0, |b| usize::from(b[0]) | usize::from(b[1]) << 8 | usize::from(b[2]) << 16);
    let four = |at: usize| data.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let count = |at: usize| data.get(at).map_or(0, |n| usize::from(*n));
    while at < data.len() {
        let id = data[at];
        let body = at + 1;
        let (block, next) = match id {
            0x10 => (Some((body + 4, word(body + 2))), body + 4 + word(body + 2)),
            0x11 => (Some((body + 18, three(body + 15))), body + 18 + three(body + 15)),
            0x14 => (Some((body + 10, three(body + 7))), body + 10 + three(body + 7)),
            0x12 => (None, body + 4),
            0x13 => (None, body + 1 + 2 * count(body)),
            0x15 => (None, body + 8 + three(body + 5)),
            0x20 | 0x23 | 0x24 => (None, body + 2),
            0x21 | 0x30 => (None, body + 1 + count(body)),
            0x22 | 0x25 | 0x27 => (None, body),
            0x2A => (None, body + 4),
            0x2B => (None, body + 5),
            0x31 => (None, body + 2 + count(body + 1)),
            0x32 => (None, body + 2 + word(body)),
            0x33 => (None, body + 1 + 3 * count(body)),
            0x35 => (None, body + 20 + four(body + 16)),
            0x5A => (None, body + 9),
            _ => return (blocks, Some(format!("The tape has a kind of block (0x{id:02X}) this doesn't read, so it stops there."))),
        };
        if let Some((start, length)) = block {
            if data.len() < start + length {
                return (blocks, Some("The tape ends partway through a block.".into()));
            }
            blocks.push((at, start, length));
        }
        at = next;
    }
    (blocks, None)
}

/// Lists the BASIC on a tape, `.TAP` or `.TZX`.
fn list_tape(data: &[u8]) -> Listing {
    let mut listing = Listing { dialect: ZxDialect::Spectrum.name(), ..Listing::default() };
    let mut header = None;
    let (blocks, problem) = tape_blocks(data);
    for block in blocks {
        zx_block(block, &mut header, &mut listing);
    }
    listing.notes.extend(problem);
    finish_zx(listing)
}

/// Lists the BASIC in a `.TAP` file: blocks of a two-byte length, a flag, the data and a checksum.
pub fn list_tap(data: &[u8]) -> Listing {
    list_tape(data)
}

/// Lists the BASIC in a `.TZX` file.
pub fn list_tzx(data: &[u8]) -> Listing {
    list_tape(data)
}

/// Lists a `+3DOS` file, as the +3 and the Next save to disk: a 128-byte header, whose bytes 15 to 22 are the
/// tape header's, then the data.
pub fn list_plus3(data: &[u8]) -> Listing {
    let mut listing = Listing { dialect: ZxDialect::Spectrum.name(), ..Listing::default() };
    if data.len() < 128 {
        listing.notes.push("The +3DOS header is cut short.".into());
        return listing;
    }
    let (kind, program) = (data[15], usize::from(u16::from_le_bytes([data[20], data[21]])));
    if kind != 0 {
        listing.notes.push("This +3DOS file holds data, not a BASIC program.".into());
        return listing;
    }
    let start = u16::from_le_bytes([data[18], data[19]]);
    if start < 10000 {
        listing.info.push(("Runs from line".into(), start.to_string()));
    }
    let body = &data[128..];
    zx_lines(&body[..program.min(body.len())], &mut listing);
    finish_zx(listing)
}

fn finish_zx(mut listing: Listing) -> Listing {
    if listing.lines.is_empty() && !listing.notes.iter().any(|n| n.contains("stops") || n.contains("partway")) {
        listing.notes.push("There's no BASIC program here.".into());
    }
    listing
}

/// The five bytes the Spectrum keeps after a number in a line: a small whole number in its short form, anything
/// else as a floating point number with an exponent byte and four of mantissa.
pub fn zx_number(value: f64) -> Result<[u8; 5], String> {
    if value == 0.0 || (value.fract() == 0.0 && value.abs() <= 65535.0) {
        let whole = value as i64;
        let (sign, bits) = if whole < 0 { (0xFF, (whole + 65536) as u16) } else { (0x00, whole as u16) };
        let [low, high] = bits.to_le_bytes();
        return Ok([0, sign, low, high, 0]);
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
        return Err(format!("{value} is too big or too small for the Spectrum"));
    }
    let bits = bits as u32;
    let first = ((bits >> 24) as u8 & 0x7F) | if negative { 0x80 } else { 0 };
    Ok([byte as u8, first, (bits >> 16) as u8, (bits >> 8) as u8, bits as u8])
}

/// A dialect's keywords, longest first, as text to match: GO TO and the like can be typed with or without their
/// space. NextBASIC also takes CONT and RAND for CONTINUE and RANDOMIZE. ELSE IF's IF is chosen by its place.
/// A keyword's token and its letters.
type Keywords = Vec<(u8, Vec<char>)>;

/// The keywords of a BASIC, for completing them as they're typed: Spectrum or NextBASIC, or Commodore BASIC V2.
pub fn keywords(spectrum: Option<ZxDialect>) -> Vec<String> {
    match spectrum {
        Some(dialect) => {
            let mut words: Vec<String> = zx_keywords(dialect).iter().map(|(_, w)| w.iter().collect::<String>()).filter(|w| w.starts_with(|c: char| c.is_ascii_alphabetic()) && w.len() > 1).collect();
            words.sort();
            words.dedup();
            words
        }
        None => CBM_KEYWORDS.iter().filter(|w| w.starts_with(|c: char| c.is_ascii_alphabetic()) && w.len() > 1).map(|w| w.to_string()).collect(),
    }
}

fn zx_keywords(dialect: ZxDialect) -> &'static [(u8, Vec<char>)] {
    static WORDS: std::sync::OnceLock<[Keywords; 2]> = std::sync::OnceLock::new();
    let lists = WORDS.get_or_init(|| {
        let build = |dialect: ZxDialect| {
            let mut words: Keywords = (0x81..=0xFF).filter(|b| *b != ZX_ELSE_IF).filter_map(|b| zx_keyword(b, dialect).map(|w| (b, w.chars().collect()))).collect();
            if dialect == ZxDialect::Next {
                words.push((0xE8, "CONT".chars().collect()));
                words.push((0xF9, "RAND".chars().collect()));
            }
            words.sort_by_key(|(_, word)| std::cmp::Reverse(word.iter().filter(|c| **c != ' ').count()));
            words
        };
        [build(ZxDialect::Spectrum), build(ZxDialect::Next)]
    });
    &lists[usize::from(dialect == ZxDialect::Next)]
}

/// Whether a keyword is written at `at`, and how many characters it takes there.
fn zx_keyword_at(chars: &[char], at: usize, dialect: ZxDialect) -> Option<(u8, usize)> {
    let letter = |c: &char| c.is_ascii_alphabetic();
    // Just after a name, such as the "a1" of "a1TO", isn't a keyword's place; just after a number is.
    let mut back = at;
    while back > 0 && chars[back - 1].is_ascii_digit() {
        back -= 1;
    }
    let after_name = back > 0 && (letter(&chars[back - 1]) || chars[back - 1] == '@');
    for (token, word) in zx_keywords(dialect) {
        let mut i = at;
        let mut fits = true;
        for &wanted in word {
            if wanted == ' ' {
                while chars.get(i) == Some(&' ') {
                    i += 1;
                }
                continue;
            }
            if chars.get(i).is_some_and(|c| c.to_ascii_uppercase() == wanted) {
                i += 1;
            } else {
                fits = false;
                break;
            }
        }
        if !fits {
            continue;
        }
        // A keyword made of letters can't be part of a longer name, such as "printer" or "lenx".
        let starts_word = word.first().is_some_and(letter);
        let ends_word = word.last().is_some_and(letter);
        if starts_word && (after_name || (at > 0 && (letter(&chars[at - 1]) || chars[at - 1] == '@'))) {
            continue;
        }
        if ends_word && chars.get(i).is_some_and(letter) {
            continue;
        }
        return Some((*token, i - at));
    }
    None
}

/// A number written at `at`, and how many characters it takes: decimal, or with NextBASIC $ for hex and @ for
/// binary, fractions allowed. `binary` reads the digits after BIN.
fn zx_number_at(chars: &[char], at: usize, binary: bool, dialect: ZxDialect) -> Option<(f64, usize)> {
    let radix_number = |start: usize, radix: u32| {
        let mut i = start;
        let mut value = 0f64;
        while let Some(d) = chars.get(i).and_then(|c| c.to_digit(radix)) {
            value = value * f64::from(radix) + f64::from(d);
            i += 1;
        }
        let whole = i > start;
        if chars.get(i) == Some(&'.') && chars.get(i + 1).and_then(|c| c.to_digit(radix)).is_some() {
            i += 1;
            let mut scale = 1.0 / f64::from(radix);
            while let Some(d) = chars.get(i).and_then(|c| c.to_digit(radix)) {
                value += f64::from(d) * scale;
                scale /= f64::from(radix);
                i += 1;
            }
        } else if !whole {
            return None;
        }
        Some((value, i - at))
    };
    if binary {
        return radix_number(at, 2);
    }
    if dialect == ZxDialect::Next {
        match chars[at] {
            '$' => return radix_number(at + 1, 16),
            '@' if chars.get(at + 1).is_some_and(|c| matches!(c, '0' | '1')) => return radix_number(at + 1, 2),
            _ => {}
        }
    }
    let mut i = at;
    let digits = |i: &mut usize| {
        let from = *i;
        while chars.get(*i).is_some_and(char::is_ascii_digit) {
            *i += 1;
        }
        *i > from
    };
    let whole = digits(&mut i);
    let mut fraction = false;
    if chars.get(i) == Some(&'.') {
        i += 1;
        fraction = digits(&mut i);
    }
    if !whole && !fraction {
        return None;
    }
    if matches!(chars.get(i), Some('e' | 'E')) {
        let mut j = i + 1;
        if matches!(chars.get(j), Some('+' | '-')) {
            j += 1;
        }
        if digits(&mut j) {
            i = j;
        }
    }
    let text: String = chars[at..i].iter().collect();
    text.parse().ok().map(|value| (value, i - at))
}

/// Whether THEN comes before the end of the statement that starts at `at`.
fn zx_has_then(chars: &[char], at: usize, dialect: ZxDialect) -> bool {
    let mut quoted = false;
    let mut i = at;
    while i < chars.len() {
        match chars[i] {
            '"' => quoted = !quoted,
            ':' if !quoted => return false,
            _ if !quoted => {
                if let Some((token, used)) = zx_keyword_at(chars, i, dialect) {
                    if token == ZX_THEN {
                        return true;
                    }
                    i += used;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    false
}

/// Tokenises one line's text, after its line number.
fn zx_crunch(text: &str, dialect: ZxDialect) -> Result<Vec<u8>, String> {
    let next = dialect == ZxDialect::Next;
    let chars: Vec<char> = text.chars().collect();
    let set = zx();
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0;
    let mut quoted = false;
    let mut after_bin = false;
    let mut statement_start = true;
    let mut last_keyword: Option<u8> = None;
    // DEF FN's parameters each have room for a value after them.
    let mut def_fn: Option<bool> = None;
    // NextBASIC: after % numbers are integers, with no hidden form, until the integer expression ends.
    let mut integer: Option<usize> = None;
    let mut depth = 0usize;
    let mut condition = false;
    let raw = zx_raw;
    while i < chars.len() {
        let c = chars[i];
        if c == '{'
            && let Some(close) = chars[i..].iter().position(|ch| *ch == '}')
        {
            let inside: String = chars[i + 1..i + close].iter().collect();
            out.extend(zx_escape(&inside)?);
            i += close + 1;
            statement_start = false;
            continue;
        }
        if c == '"' {
            quoted = !quoted;
        }
        if !quoted {
            // NextBASIC's ; comment, and a dot command, which its own program reads: both are kept as typed.
            if next && statement_start && c == ';' {
                return raw(&chars[i..], &mut out).map(|()| out);
            }
            if next && statement_start && c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_alphabetic) {
                let mut end = i;
                let mut in_quotes = false;
                while end < chars.len() && (in_quotes || chars[end] != ':') {
                    in_quotes ^= chars[end] == '"';
                    end += 1;
                }
                raw(&chars[i..end], &mut out)?;
                i = end;
                continue;
            }
            if let Some((mut token, used)) = zx_keyword_at(&chars, i, dialect) {
                if next {
                    // Only the space after a keyword goes; one typed before it stays in the line.
                    if token == ZX_IF && last_keyword == Some(ZX_ELSE) && !zx_has_then(&chars, i, dialect) {
                        token = ZX_ELSE_IF;
                    }
                } else {
                    // The 48K's LIST puts spaces round keywords, so the ones next to a keyword aren't part of the line.
                    while out.last() == Some(&b' ') {
                        out.pop();
                    }
                }
                out.push(token);
                i += used;
                let word = zx_keyword(token, dialect).unwrap_or("");
                if !next {
                    while chars.get(i) == Some(&' ') {
                        i += 1;
                    }
                } else if chars.get(i) == Some(&' ') && word.starts_with(|c: char| c.is_ascii_alphabetic()) {
                    i += 1;
                }
                after_bin = token == ZX_BIN;
                if token == ZX_DEF_FN {
                    def_fn = Some(false);
                }
                if integer.is_some() && !ZX_INTEGER_WORDS.contains(&word) {
                    integer = None;
                }
                match token {
                    ZX_IF | ZX_ELSE_IF | 0x96 | 0x8E => condition = true,
                    ZX_THEN => condition = false,
                    _ => {}
                }
                statement_start = matches!(token, ZX_THEN | ZX_ELSE);
                last_keyword = Some(token);
                if token == ZX_REM {
                    // REM: the rest of the line is kept as it is, but on the 48K a colon puts the editor back in
                    // keyword mode, so keywords in capitals after one are tokens (TR-DOS's commands are kept so).
                    match chars[i..].iter().position(|c| *c == ':').filter(|_| !next) {
                        Some(colon) => {
                            raw(&chars[i..=i + colon], &mut out)?;
                            zx_rem_commands(&chars[i + colon + 1..], &mut out)?;
                        }
                        None => raw(&chars[i..], &mut out)?,
                    }
                    break;
                }
                continue;
            }
            // A label is a name, not a number or keywords.
            if next && c == '@' && chars.get(i + 1).is_some_and(char::is_ascii_alphabetic) {
                let end = (i + 1..chars.len()).find(|&j| !(chars[j].is_ascii_alphanumeric() || chars[j] == '_' || chars[j] == '$')).unwrap_or(chars.len());
                raw(&chars[i..end], &mut out)?;
                i = end;
                statement_start = false;
                continue;
            }
            let in_name = i > 0 && (chars[i - 1].is_ascii_alphanumeric() || chars[i - 1] == '$' || chars[i - 1] == '_');
            let starts_number = c.is_ascii_digit() || c == '.' || after_bin || (next && matches!(c, '$' | '@'));
            if !in_name
                && starts_number
                && let Some((value, used)) = zx_number_at(&chars, i, after_bin, dialect)
            {
                raw(&chars[i..i + used], &mut out)?;
                i += used;
                if integer.is_none() {
                    // The ROM reads spaces after a number as part of it, so its hidden form comes after them. On the
                    // 48K, spaces before a keyword aren't kept at all, so there they're left to be dropped.
                    let spaces = chars[i..].iter().take_while(|c| **c == ' ').count();
                    if next || zx_keyword_at(&chars, i + spaces, dialect).is_none() {
                        out.extend(std::iter::repeat_n(b' ', spaces));
                        i += spaces;
                    }
                    out.push(0x0E);
                    out.extend(zx_number(value)?);
                }
                after_bin = false;
                statement_start = false;
                continue;
            }
            after_bin = false;
            match c {
                '%' if next => integer = Some(depth),
                '(' => depth += 1,
                ')' => {
                    depth = depth.saturating_sub(1);
                    if integer.is_some_and(|from| depth < from) {
                        integer = None;
                    }
                }
                ',' | ';' => integer = None,
                '=' if !condition => integer = None,
                ':' => {
                    integer = None;
                    condition = false;
                    last_keyword = None;
                }
                _ => {}
            }
            match (def_fn, c) {
                (Some(false), '(') => def_fn = Some(true),
                (Some(true), ')') => def_fn = None,
                (Some(true), _) if c.is_ascii_alphabetic() => {
                    out.push(zx_byte(set, c)?);
                    i += 1;
                    if chars.get(i) == Some(&'$') {
                        out.push(b'$');
                        i += 1;
                    }
                    out.extend([0x0E, 0, 0, 0, 0, 0]);
                    continue;
                }
                _ => {}
            }
        }
        out.push(zx_byte(set, c)?);
        i += 1;
        if c == ':' && !quoted {
            statement_start = true;
        } else if c != ' ' {
            statement_start = false;
        }
    }
    Ok(out)
}

/// A 48K REM's text after a colon: keywords in capitals are tokens, outside quotes, with the one space the listing
/// puts each side of them taken off; everything else is kept as typed.
fn zx_rem_commands(chars: &[char], out: &mut Vec<u8>) -> Result<(), String> {
    let mut i = 0;
    let mut quoted = false;
    while i < chars.len() {
        let c = chars[i];
        if c == '{'
            && let Some(close) = chars[i..].iter().position(|ch| *ch == '}')
        {
            out.extend(zx_escape(&chars[i + 1..i + close].iter().collect::<String>())?);
            i += close + 1;
            continue;
        }
        quoted ^= c == '"';
        if !quoted
            && let Some((token, used)) = zx_keyword_at(chars, i, ZxDialect::Spectrum)
            && !chars[i..i + used].iter().any(char::is_ascii_lowercase)
        {
            // The one space LIST puts each side of a keyword here; any more were typed.
            let alphabetic = c.is_ascii_alphabetic();
            if alphabetic && out.last() == Some(&b' ') {
                out.pop();
            }
            out.push(token);
            i += used;
            if alphabetic && chars.get(i) == Some(&' ') {
                i += 1;
            }
            continue;
        }
        out.push(zx_byte(zx(), c)?);
        i += 1;
    }
    Ok(())
}

/// Whether plain bytes in a REM's commands spell a keyword in capitals, as `zx_rem_commands` would take one.
fn zx_rem_keyword_at(bytes: &[u8], before: Option<u8>) -> bool {
    let mut chars: Vec<char> = before.map(char::from).into_iter().collect();
    let start = chars.len();
    chars.extend(bytes.iter().take_while(|b| (0x20..0x80).contains(*b)).map(|b| char::from(*b)));
    zx_keyword_at(&chars, start, ZxDialect::Spectrum).is_some_and(|(_, used)| !chars[start..start + used].iter().any(char::is_ascii_lowercase))
}

/// Text kept as it's typed, in a REM, a ; comment or a dot command, with its `{...}` escapes still read.
fn zx_raw(chars: &[char], out: &mut Vec<u8>) -> Result<(), String> {
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{'
            && let Some(close) = chars[i..].iter().position(|ch| *ch == '}')
        {
            out.extend(zx_escape(&chars[i + 1..i + close].iter().collect::<String>())?);
            i += close + 1;
            continue;
        }
        out.push(zx_byte(zx(), chars[i])?);
        i += 1;
    }
    Ok(())
}

fn zx_byte(set: &Charset, c: char) -> Result<u8, String> {
    // Listings made for zmakebas and bas2tap have the Spectrum's codes for £ and ↑ as ASCII has them.
    match c {
        '`' => return Ok(0x60),
        '^' => return Ok(0x5E),
        _ => {}
    }
    set.table(0).and_then(|t| t.byte_of(c)).filter(|b| *b >= 0x20 && *b < 0xA5).ok_or_else(|| format!("'{c}' isn't in the Spectrum's character set"))
}

/// `{INK 2}`, `{AT 3,4}`, `{UDG-A}` and `{$xx}`, as a listing writes them, and bas2tap's `{A}`, `{-1}`, `{+8}`,
/// `{(C)}` and `{XX}`.
pub(crate) fn zx_escape(inside: &str) -> Result<Vec<u8>, String> {
    if let Some(code) = crate::zx_graphics::bas2tap_escape(inside) {
        return Ok(vec![code]);
    }
    if let Some(hex) = inside.strip_prefix('$') {
        return u8::from_str_radix(hex, 16).map(|b| vec![b]).map_err(|_| format!("{{{inside}}} isn't a byte"));
    }
    if let Some(letter) = inside.strip_prefix("UDG-").and_then(|l| l.chars().next()).filter(|l| ('A'..='U').contains(l)) {
        return Ok(vec![0x90 + (letter as u8 - b'A')]);
    }
    let (name, values) = inside.split_once(' ').unwrap_or((inside, ""));
    let control = (0x10..=0x17).find(|b| zx_control(*b).is_some_and(|(n, _)| n.eq_ignore_ascii_case(name)));
    let Some(byte) = control else {
        return Err(format!("{{{inside}}} isn't a code this knows"));
    };
    let count = zx_control(byte).map_or(0, |(_, n)| n);
    let values: Vec<u8> = values.split(',').filter(|v| !v.trim().is_empty()).map(|v| v.trim().parse::<u8>()).collect::<Result<_, _>>().map_err(|_| format!("{{{inside}}} needs numbers from 0 to 255"))?;
    if values.len() != count {
        return Err(format!("{{{inside}}} needs {count} value(s)"));
    }
    Ok(std::iter::once(byte).chain(values).collect())
}

/// Tokenises a listing into a Spectrum program's bytes, as they'd be in memory.
pub fn tokenise_spectrum<S: AsRef<str>>(lines: &[S], dialect: ZxDialect) -> Result<Vec<u8>, String> {
    // A listing written for zmakebas has its escapes put in Limoniter's own way first.
    let rewritten: Option<Vec<String>> = crate::zx_graphics::uses_zmakebas(lines).then(|| crate::zx_graphics::join_zmakebas(lines).iter().map(|l| crate::zx_graphics::from_zmakebas(l)).collect());
    let lines: Vec<&str> = match &rewritten {
        Some(rewritten) => rewritten.iter().map(String::as_str).collect(),
        None => lines.iter().map(AsRef::as_ref).collect(),
    };
    let mut out = Vec::new();
    let mut previous = None;
    for (index, line) in lines.iter().enumerate() {
        let Some((number, rest)) = split_number(line, index, previous, 9999)? else {
            continue;
        };
        previous = Some(number);
        let rest = rest.strip_prefix(' ').unwrap_or(rest);
        let rest = if dialect == ZxDialect::Spectrum { rest.trim_start() } else { rest };
        let mut body = zx_crunch(rest, dialect)
            .map_err(|problem| format!("Line {} (BASIC line {number}): {problem}", index + 1))?;
        body.push(0x0D);
        out.extend_from_slice(&(number as u16).to_be_bytes());
        out.extend_from_slice(&u16::try_from(body.len()).map_err(|_| format!("Line {} is too long.", index + 1))?.to_le_bytes());
        out.extend(body);
    }
    if dialect == ZxDialect::Spectrum && out.len() > 41_000 {
        return Err("The program is bigger than a 48K Spectrum has room for.".into());
    }
    Ok(out)
}

/// A tape block: its length, the flag, the bytes and a checksum that makes them all add up, by XOR, to zero.
pub fn tap_block(flag: u8, bytes: &[u8]) -> Vec<u8> {
    let mut out = ((bytes.len() + 2) as u16).to_le_bytes().to_vec();
    out.push(flag);
    out.extend_from_slice(bytes);
    out.push(bytes.iter().fold(flag, |sum, b| sum ^ b));
    out
}

/// A `.TAP` file holding a program, as SAVE "name" would write it. `run_from` is SAVE ... LINE's line.
pub fn spectrum_tape(program: &[u8], name: &str, run_from: Option<u16>) -> Vec<u8> {
    let mut header = vec![0u8];
    let mut title: Vec<u8> = name.chars().filter_map(|c| zx_byte(zx(), c).ok()).take(10).collect();
    title.resize(10, b' ');
    header.extend(title);
    let length = program.len() as u16;
    header.extend(length.to_le_bytes());
    header.extend(run_from.unwrap_or(0x8000).to_le_bytes());
    header.extend(length.to_le_bytes());
    let mut out = tap_block(0x00, &header);
    out.extend(tap_block(0xFF, program));
    out
}

/// A `+3DOS` file holding a program, as the +3 and the Next save one to disk. The last header byte is a checksum
/// of the 127 before it.
pub fn plus3dos_file(program: &[u8], run_from: Option<u16>) -> Vec<u8> {
    plus3dos_basic(program, program.len(), run_from)
}

/// A BASIC program with the variables saved after it, as a +3DOS file: the header gives both lengths.
pub fn plus3dos_basic(program_and_variables: &[u8], program_length: usize, run_from: Option<u16>) -> Vec<u8> {
    let program = program_and_variables;
    let mut header = b"PLUS3DOS\x1A\x01\x00".to_vec();
    header.extend(((program.len() + 128) as u32).to_le_bytes());
    header.push(0);
    header.extend((program.len() as u16).to_le_bytes());
    header.extend(run_from.unwrap_or(0x8000).to_le_bytes());
    header.extend((program_length.min(program.len()) as u16).to_le_bytes());
    header.resize(127, 0);
    let sum = header.iter().fold(0u8, |sum, b| sum.wrapping_add(*b));
    header.push(sum);
    header.extend_from_slice(program);
    header
}

/// Lists whatever tokenised BASIC a file holds, telling the kind from its first bytes or else its name.
pub fn list(data: &[u8], extension: &str, set: &Charset) -> Listing {
    use crate::basic_more;
    if let Some(listing) = crate::basic_extra::list_by_bytes(data, extension, set) {
        listing
    } else if data.starts_with(b"PLUS3DOS") {
        list_plus3(data)
    } else if data.starts_with(b"ZXTape!\x1A") {
        list_tzx(data)
    } else if extension == "tap" {
        list_tap(data)
    } else if matches!(extension, "prg" | "p00") {
        list_commodore(data, set)
    } else if basic_more::looks_like_bbc(data) {
        basic_more::list_bbc(data)
    } else if basic_more::looks_like_dragon(data) {
        basic_more::list_dragon(data)
    } else if basic_more::looks_like_coco(data) {
        basic_more::list_coco(data)
    } else if basic_more::looks_like_applesoft(data) {
        basic_more::list_applesoft(data)
    } else if basic_more::looks_like_mbasic(data) || data.first() == Some(&0xFE) {
        basic_more::list_mbasic(data, set)
    } else if data.first() == Some(&0xFF) {
        basic_more::list_msx(data, set)
    } else if basic_more::looks_like_atari(data) {
        basic_more::list_atari(data)
    } else {
        list_commodore(data, set)
    }
}

/// Whether a file ending names a tokenised BASIC program. A `.bas` (or a BBC program's `.bbc`) only does when its
/// bytes say so, a +3DOS header, a BBC, MSX or Atari program's start, because a `.bas` is as often a listing kept
/// as text. With no bytes to go by, it's taken to be one, and the preview says if it isn't.
pub fn is_program_file(extension: &str, data: Option<&[u8]>) -> bool {
    use crate::basic_more;
    if crate::basic_extra::is_program_file(extension, data) {
        return true;
    }
    let tokenised = |data: &[u8]| data.starts_with(b"PLUS3DOS") || basic_more::looks_like_bbc(data) || matches!(data.first(), Some(0xFF | 0xFE)) || basic_more::looks_like_atari(data) || basic_more::looks_like_applesoft(data) || basic_more::looks_like_dragon(data);
    // A .prg is also a Clipper or FoxPro source; a Commodore one starts with a load address, low byte 1.
    let commodore = |data: &[u8]| data.starts_with(b"C64File\0") || (data.len() >= 2 && data[0] == 0x01 && matches!(data[1], 0x04 | 0x08 | 0x10 | 0x12 | 0x1C));
    match extension {
        "prg" => data.is_none_or(commodore),
        "p00" | "tap" | "tzx" => true,
        "bas" | "bbc" => data.is_none_or(tokenised),
        _ => false,
    }
}

/// The machine a listing can be tokenised back for, from what it was listed as and the file it came from: a
/// Commodore program says which machine by where it loads. None when that isn't clear, such as a 48K Spectrum
/// program from a +3DOS file, which could go back as a tape or a +3DOS file.
pub fn machine_of(dialect: &str, extension: &str, bytes: &[u8]) -> Option<Machine> {
    if dialect == ZxDialect::Next.name() {
        return Some(Machine::Next);
    }
    if dialect == ZxDialect::Spectrum.name() {
        return matches!(extension, "tap" | "tzx").then_some(Machine::Spectrum);
    }
    if dialect == "BBC BASIC" {
        return Some(Machine::Bbc);
    }
    // A .p00 file has its own header before the load address.
    let load = if extension == "p00" { bytes.get(26..28) } else { bytes.get(0..2) }.map(|b| u16::from_le_bytes([b[0], b[1]]))?;
    CbmMachine::ALL.into_iter().find(|machine| machine.dialect() == dialect && machine.load_address() == load).map(Machine::Commodore)
}

/// Which machine a listing is tokenised for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Machine {
    /// A 48K or 128K Spectrum, saved as a tape.
    Spectrum,
    /// A Spectrum Next, saved as a +3DOS `.bas` file.
    Next,
    Commodore(CbmMachine),
    /// A BBC Micro or Master, saved as SAVE writes a program.
    Bbc,
}

impl Machine {
    /// The file ending of the program it loads.
    pub fn ending(self) -> &'static str {
        match self {
            Machine::Spectrum => "tap",
            Machine::Next => "bas",
            Machine::Commodore(_) => "prg",
            Machine::Bbc => "bbc",
        }
    }

    /// What its program is, in words.
    pub fn kind(self) -> &'static str {
        match self {
            Machine::Spectrum => "Spectrum tape",
            Machine::Next => "Spectrum Next program",
            Machine::Commodore(_) => "Commodore program",
            Machine::Bbc => "BBC BASIC program",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_commodore_program_lists_and_tokenises_back_to_the_same_bytes() {
        let set = charset::known("petscii-c64");
        let text = ["10 PRINT\"{clr}{red}HELLO\"", "20 FORI=1TO10:?I:NEXT", "30 REM PRINT IS NOT TOKENISED HERE", "40 DATA PRINT,1,2:PRINT", "50 GOTO10"];
        let program = tokenise_commodore(&text, CbmMachine::C64).unwrap();
        assert_eq!(&program[..2], &[0x01, 0x08], "loads at $0801");
        // 10 PRINT"{clr}{red}HELLO"
        assert_eq!(&program[6..17], &[0x99, b'"', 0x93, 0x1C, b'H', b'E', b'L', b'L', b'O', b'"', 0]);
        let listing = list_commodore(&program, set);
        assert_eq!(listing.lines[0], "10 PRINT\"{clr}{red}HELLO\"");
        assert_eq!(listing.lines[1], "20 FORI=1TO10:PRINTI:NEXT", "? is typed for PRINT and listed as it");
        assert_eq!(listing.lines[2], "30 REM PRINT IS NOT TOKENISED HERE");
        assert_eq!(listing.lines[3], "40 DATA PRINT,1,2:PRINT");
        assert_eq!(tokenise_commodore(&listing.lines, CbmMachine::C64).unwrap(), program, "listing and tokenising undo each other");
        // The first line's link points at the second line, 15 bytes on from $0801.
        assert_eq!(u16::from_le_bytes([program[2], program[3]]), 0x0801 + 15);
    }

    #[test]
    fn commodore_typing_quirks() {
        let set = CbmMachine::C64;
        let program = tokenise_commodore(&["10 print \"hi\""], set).unwrap();
        assert_eq!(&program[6..12], &[0x99, b' ', b'"', b'H', b'I', b'"'], "small letters are typed as capitals, and spaces are kept");
        assert!(tokenise_commodore(&["PRINT 1"], set).unwrap_err().contains("line number"));
        assert!(tokenise_commodore(&["20 A=1", "10 B=2"], set).unwrap_err().contains("go up"));
        assert!(tokenise_commodore(&["10 PRINT \"{nonsense}\""], set).unwrap_err().contains("nonsense"));
        assert_eq!(&tokenise_commodore(&["1 X"], CbmMachine::Vic20).unwrap()[..2], &[0x01, 0x10], "the VIC-20 loads at $1001");
    }

    #[test]
    fn machine_code_after_a_basic_stub_is_noted_not_listed() {
        let set = charset::known("petscii-c64");
        let mut program = tokenise_commodore(&["10 SYS2061"], CbmMachine::C64).unwrap();
        program.extend([0xA9, 0x00, 0x8D, 0x20, 0xD0, 0x60]);
        let listing = list_commodore(&program, set);
        assert_eq!(listing.lines, vec!["10 SYS2061".to_string()]);
        assert!(listing.notes.iter().any(|n| n.contains("6 byte")));
        for broken in [&[][..], &[0x01], &[0x01, 0x08, 0x10], &[0x01, 0x08, 0x09, 0x08, 0x0A, 0x00, 0x99]] {
            let _ = list_commodore(broken, set);
        }
    }

    #[test]
    fn later_commodore_basics_have_their_own_keywords() {
        // C128 BASIC 7.0: DO, LOOP and two-byte tokens; DOPEN isn't taken for DO, nor PEN for a part of it.
        let program = tokenise_commodore(&["10 DO:DOPEN#1,\"F\":SPRITE 1,1:A=POT(1):LOOP", "20 COLOR 1,2"], CbmMachine::C128).unwrap();
        assert_eq!(&program[..2], &[0x01, 0x1C], "the C128 loads at $1C01");
        assert_eq!(&program[6..10], &[0xEB, b':', 0xFE, 0x0D], "DO, then DOPEN as &FE &0D");
        assert!(program.windows(2).any(|w| w == [0xFE, 0x07]), "SPRITE");
        assert!(program.windows(2).any(|w| w == [0xCE, 0x02]), "POT");
        let listing = list_commodore(&program, charset::utf8());
        assert_eq!(listing.dialect, "Commodore BASIC 7.0");
        assert_eq!(listing.lines[0], "10 DO:DOPEN#1,\"F\":SPRITE 1,1:A=POT(1):LOOP");
        assert_eq!(tokenise_commodore(&listing.lines, CbmMachine::C128).unwrap(), program);
        // Plus/4 BASIC 3.5 loads where the VIC-20 does, and is told apart by its keywords.
        let plus4 = tokenise_commodore(&["10 GRAPHIC 1:SCNCLR:CIRCLE 1,160,100,50"], CbmMachine::Plus4).unwrap();
        assert_eq!(&plus4[..2], &[0x01, 0x10]);
        let listing = list_commodore(&plus4, charset::utf8());
        assert_eq!(listing.dialect, "Commodore BASIC 3.5");
        assert_eq!(listing.lines[0], "10 GRAPHIC 1:SCNCLR:CIRCLE 1,160,100,50");
        assert_eq!(list_commodore(&tokenise_commodore(&["10 PRINT"], CbmMachine::Vic20).unwrap(), charset::utf8()).dialect, "Commodore BASIC 2.0");
        // PET BASIC 4.0's disk commands, and a C64 that doesn't know them keeps them as letters.
        let pet = tokenise_commodore(&["10 DIRECTORY:DLOAD\"X\""], CbmMachine::Pet).unwrap();
        assert_eq!(&pet[6..7], &[0xDA]);
        assert_eq!(list_commodore(&pet, charset::utf8()).lines[0], "10 DIRECTORY:DLOAD\"X\"");
        assert!(!tokenise_commodore(&["10 GRAPHIC 1"], CbmMachine::C64).unwrap().contains(&0xDE));
    }

    fn spectrum(lines: &[&str]) -> Vec<u8> {
        tokenise_spectrum(lines, ZxDialect::Spectrum).unwrap()
    }

    fn next(lines: &[&str]) -> Vec<u8> {
        tokenise_spectrum(lines, ZxDialect::Next).unwrap()
    }

    /// A line's bytes after its number and length, with each hidden number written as `#`.
    fn shown(program: &[u8]) -> String {
        let mut out = String::new();
        let mut i = 4;
        while i < program.len() {
            match program[i] {
                0x0E => {
                    out.push('#');
                    i += 5;
                }
                0x0D => break,
                b if (0x20..0x7F).contains(&b) => out.push(b as char),
                b => out.push_str(&format!("{{{b:02x}}}")),
            }
            i += 1;
        }
        out
    }

    #[test]
    fn a_48k_rem_after_a_colon_holds_keywords_as_trdos_commands_do() {
        // Sgt. Pepper's Beta Disk lines: RANDOMIZE USR 15614 runs the REM's commands.
        let program = spectrum(&["4500 RANDOMIZE USR 15614: REM : SAVE a$ LINE 4000", "4510 REM : LOAD b$ CODE s,l"]);
        assert!(shown(&program).contains("{ea}:{f8}a${ca}4000"), "keywords in capitals are tokens, numbers have no hidden form: {}", shown(&program));
        assert_eq!(shown(&spectrum(&["4510 REM : LOAD b$ CODE s,l"])), "{ea}:{ef}b${af}s,l");
        let listing = list_plus3(&plus3dos_file(&program, None));
        assert_eq!(listing.lines, ["4500 RANDOMIZE USR 15614: REM : SAVE a$ LINE 4000", "4510 REM : LOAD b$ CODE s,l"]);
        // Before a colon, in small letters, or in quotes, text stays text.
        assert_eq!(shown(&spectrum(&["10 REM PRINT: print \"PRINT\""])), "{ea}PRINT: print \"PRINT\"");
        // Letters that would be taken for a keyword are listed with an escape, and come back as letters.
        let letters = [0x00, 0x14, 0x09, 0x00, 0xEA, b':', b'S', b'T', b'O', b'P', b' ', b'1', 0x0D];
        let listed = list_plus3(&plus3dos_file(&letters, None)).lines;
        assert_eq!(listed, ["  20 REM :{$53}TOP 1"]);
        assert_eq!(spectrum(&["20 REM :{$53}TOP 1"]), letters, "it tokenises back to the same bytes");
        // NextBASIC's REM is kept as typed.
        assert!(tokenise_spectrum(&["10 REM : LOAD x"], ZxDialect::Next).unwrap().ends_with(b": LOAD x\r"));
    }

    #[test]
    fn spectrum_keywords_lose_their_spaces_and_numbers_get_their_hidden_form() {
        assert_eq!(spectrum(&["10 print \"hello\""]), vec![0x00, 0x0A, 0x09, 0x00, 0xF5, b'"', b'h', b'e', b'l', b'l', b'o', b'"', 0x0D], "PRINT is one byte, with no spaces");
        let goto = spectrum(&["20 GOTO 10"]);
        assert_eq!(&goto[4..], &[0xEC, b'1', b'0', 0x0E, 0, 0, 10, 0, 0, 0x0D], "GO TO, then 10 and its five bytes");
        assert_eq!(spectrum(&["20 GO TO 10"]), goto, "with or without the space");
        assert_eq!(shown(&spectrum(&["30 LET printer=lenx"])), "{f1}printer=lenx", "a keyword inside a name is just letters");
        assert_eq!(shown(&spectrum(&["40 REM print 10"])), "{ea}print 10");
        assert_eq!(shown(&spectrum(&["45 REM {$f5}{UDG-A}"])), "{ea}{f5}{90}", "escapes are read in a REM too");
        assert_eq!(shown(&spectrum(&["50 IF a<=b THEN STOP"])), "{fa}a{c7}b{cb}{e2}");
        assert_eq!(shown(&spectrum(&["60 FOR i=1TO 10"])), "{eb}i=1#{cc}10#", "a keyword straight after a number");
        assert_eq!(shown(&spectrum(&["70 PLAY \"a\""])), "{a4}\"a\"", "the 128K's PLAY");
    }

    #[test]
    fn spectrum_numbers_in_five_bytes() {
        assert_eq!(zx_number(10.0).unwrap(), [0, 0, 10, 0, 0]);
        assert_eq!(zx_number(65535.0).unwrap(), [0, 0, 0xFF, 0xFF, 0]);
        assert_eq!(zx_number(-1.0).unwrap(), [0, 0xFF, 0xFF, 0xFF, 0]);
        assert_eq!(zx_number(0.5).unwrap(), [0x80, 0x00, 0, 0, 0]);
        assert_eq!(zx_number(65536.0).unwrap(), [0x91, 0x00, 0, 0, 0]);
        assert_eq!(zx_number(-0.75).unwrap(), [0x80, 0xC0, 0, 0, 0]);
        assert!(zx_number(1e300).is_err());
        let program = spectrum(&["10 PRINT 1.5;BIN 101;.25;1e3"]);
        let hidden = |value: f64| [vec![0x0E], zx_number(value).unwrap().to_vec()].concat();
        let mut expected = vec![0xF5, b'1', b'.', b'5'];
        expected.extend(hidden(1.5));
        expected.extend([b';', 0xC4, b'1', b'0', b'1']);
        expected.extend(hidden(5.0));
        expected.extend(b";.25");
        expected.extend(hidden(0.25));
        expected.extend(b";1e3");
        expected.extend(hidden(1000.0));
        expected.push(0x0D);
        assert_eq!(&program[4..], &expected[..]);
    }

    #[test]
    fn a_spectrum_tape_lists_and_tokenises_back_to_the_same_bytes() {
        let text = ["  10 BORDER 1: PAPER 1: INK 7: CLS", "  20 FOR i=1 TO 10", "  30 PRINT AT i,i;\"{INK 2}Hi\"", "  40 NEXT i", "  50 DEF FN s(x,y$)=x*2", "  60 GO SUB 100: STOP"];
        let program = spectrum(&text);
        let tape = spectrum_tape(&program, "demo", Some(10));
        assert_eq!(u16::from_le_bytes([tape[16], tape[17]]), 10, "the header says to run from line 10");
        let listing = list_tap(&tape);
        assert_eq!(listing.dialect, ZxDialect::Spectrum.name());
        assert_eq!(listing.lines, text.iter().map(|l| l.to_string()).collect::<Vec<_>>());
        assert!(listing.info.iter().any(|(k, v)| k == "Program" && v.starts_with("demo") && v.contains("line 10")));
        assert_eq!(tokenise_spectrum(&listing.lines, ZxDialect::Spectrum).unwrap(), program);
        // DEF FN's parameters each have five empty bytes after them.
        assert_eq!(&spectrum(&["50 DEF FN s(x,y$)=x"])[4..], &[0xCE, b's', b'(', b'x', 0x0E, 0, 0, 0, 0, 0, b',', b'y', b'$', 0x0E, 0, 0, 0, 0, 0, b')', b'=', b'x', 0x0D]);
        // Each block's checksum makes its bytes XOR to zero, so a Spectrum will load it.
        let first = usize::from(u16::from_le_bytes([tape[0], tape[1]]));
        assert_eq!(tape[2..2 + first].iter().fold(0, |a, b| a ^ b), 0);
        assert_eq!(tape[4 + first..].iter().fold(0, |a, b| a ^ b), 0);
    }

    #[test]
    fn plus3dos_and_tzx_hold_programs_too() {
        let program = spectrum(&["10 PRINT 1"]);
        let plus3 = plus3dos_file(&program, None);
        assert_eq!(plus3[127], plus3[..127].iter().fold(0u8, |a, b| a.wrapping_add(*b)), "the header's checksum");
        assert_eq!(u32::from_le_bytes([plus3[11], plus3[12], plus3[13], plus3[14]]) as usize, plus3.len());
        assert_eq!(list(&plus3, "bas", charset::utf8()).lines, vec!["  10 PRINT 1".to_string()]);
        let tape = spectrum_tape(&program, "x", None);
        let mut tzx = b"ZXTape!\x1A\x01\x14".to_vec();
        tzx.extend([0x30, 2, b'h', b'i']);
        let mut at = 0;
        while at < tape.len() {
            let length = usize::from(u16::from_le_bytes([tape[at], tape[at + 1]]));
            tzx.extend([0x10, 0xE8, 0x03]);
            tzx.extend(&tape[at..at + 2 + length]);
            at += 2 + length;
        }
        assert_eq!(list(&tzx, "tzx", charset::utf8()).lines, vec!["  10 PRINT 1".to_string()]);
        for broken in [&b"ZXTape!\x1A\x01\x14\x99"[..], b"PLUS3DOS", &[0x13, 0x00, 0x00], &[0xFF; 40]] {
            let _ = list(broken, "tap", charset::utf8());
        }
        assert!(is_program_file("bas", Some(&plus3)));
        assert!(!is_program_file("bas", Some(b"10 PRINT 1\r\n")), "a .bas that's a text listing isn't a program file");
    }

    #[test]
    fn spectrum_errors_say_which_line() {
        assert!(tokenise_spectrum(&["10 PRINT \"\u{263A}\""], ZxDialect::Spectrum).unwrap_err().contains("Line 1"));
        assert!(tokenise_spectrum(&["10000 PRINT"], ZxDialect::Spectrum).unwrap_err().contains("9999"));
        assert!(tokenise_spectrum(&["10 PRINT {INK}"], ZxDialect::Spectrum).unwrap_err().contains("1 value"));
        assert_eq!(spectrum(&["10 PRINT {AT 1,2}{UDG-A}\u{A3}"])[4..], [0xF5, 0x16, 1, 2, 0x90, 0x60, 0x0D]);
    }

    #[test]
    fn spaces_after_a_number_come_before_its_hidden_form() {
        assert_eq!(shown(&next(&["10 IF 1 < 2 THEN STOP"])), "{fa}1 #< 2 #{cb}{e2}", "as NextZXOS saves it");
        assert_eq!(shown(&spectrum(&["10 PRINT 1 ;2"])), "{f5}1 #;2#");
        assert_eq!(shown(&spectrum(&["10 FOR i=1 TO 2"])), "{eb}i=1#{cc}2#", "on the 48K a space before a keyword isn't kept");
    }

    // These follow programs saved by NextZXOS itself (txt2bas's test fixtures ifelse208.bas, simple208.bas,
    // tyvarian.bas and def-fn-bytes.bas), byte for byte.
    #[test]
    fn nextbasic_keeps_spaces_before_keywords_and_stores_else_if_as_its_own_if() {
        assert_eq!(shown(&next(&["100 IF x > 7 : PRINT \" x > 7 \""])), "{fa}x > 7 #: {f5}\" x > 7 \"", "a long-form IF, with no THEN, is the usual IF");
        assert_eq!(shown(&next(&["120 ELSE IF x > 3 : PRINT \"x\""])), "{98}{83}x > 3 #: {f5}\"x\"", "ELSE IF's IF is 0x83");
        assert_eq!(shown(&next(&["20 ELSE IF 1 < 2:PRINT \"ELSE\""])), "{98}{83}1 #< 2#:{f5}\"ELSE\"");
        assert_eq!(shown(&next(&["10 IF x=0 THEN PRINT 1: ELSE IF x=1 THEN PRINT 2"])), "{fa}x=0 #{cb}{f5}1#: {98}{fa}x=1 #{cb}{f5}2#", "short-form ELSE IF has THEN, so it's the usual IF");
        assert_eq!(shown(&next(&["115 ENDIF"])), "{84}");
        assert_eq!(shown(&next(&["10 DEF FN r(x)=INT (x+0.5)"])), "{ce}r(x#)={ba}(x+0.5#)");
        assert_eq!(shown(&next(&["20 PRINT FN r(2.2)"])), "{f5}{a8}r(2.2#)");
    }

    #[test]
    fn nextbasic_integer_expressions_have_no_hidden_numbers() {
        assert_eq!(shown(&next(&["30 REG %127,0"])), "{88}%127,0#", "the comma ends the integer expression");
        assert_eq!(shown(&next(&["50 %o=%REG 127"])), "%o=%{88}127");
        assert_eq!(shown(&next(&["60 IF %o = 0 THEN STOP"])), "{fa}%o = 0 {cb}{e2}", "= in a condition stays integer, THEN ends it");
        assert_eq!(shown(&next(&["10 FOR %i=%$c9 TO 220"])), "{eb}%i=%$c9 {cc}220#", "TO starts a standard expression");
        assert_eq!(shown(&next(&["20 LET a=(%x<<8)+1"])), "{f1}a=(%x{8c}8)+1#", "the closing bracket ends it");
        assert_eq!(shown(&next(&["30 LET a=$ff+@101"])), "{f1}a=$ff#+@101#", "hex and binary in a standard expression get hidden numbers");
    }

    #[test]
    fn nextbasic_labels_comments_procedures_and_dot_commands() {
        assert_eq!(shown(&next(&["100 RETURN: @alice: GO TO @bob"])), "{fe}: @alice: {ec}@bob", "labels are names");
        assert_eq!(shown(&next(&["1010 ; LD BC,1234"])), "; LD BC,1234", "a ; comment is kept as typed");
        assert_eq!(shown(&next(&["70 IF %o = 1THEN CD \"c:/\": .nexload Tyvarian.nex"])), "{fa}%o = 1{cb}{a0}\"c:/\": .nexload Tyvarian.nex", "a dot command isn't tokenised");
        assert_eq!(shown(&next(&["10 DEFPROC go(a): LOCAL b: ENDPROC"])), "{91}go(a): {94}b: {92}");
        assert_eq!(shown(&next(&["10 CONT: RAND"])), "{e8}: {f9}", "CONT and RAND are short for CONTINUE and RANDOMIZE");
        assert_eq!(shown(&next(&["10 SPRITE 1,0,0,0,1: LAYER 2,1"])), "{9e}1#,0#,0#,0#,1#: {9c}2#,1#");
        assert_eq!(shown(&next(&["10 REPEAT : WHILE a<10: a=a+1: REPEAT UNTIL a MOD 3"])), "{97}: {96}a<10#: a=a+1#: {97}{8e}a {8b}3#");
    }

    #[test]
    fn a_next_program_lists_as_nextbasic_and_tokenises_back_to_the_same_bytes() {
        let text = ["  10 BORDER 1: LAYER 2,1", "  20 FOR %i=%$c9 TO 220", "  30 IF %i MOD 2 = 0:PRINT %i", "  40 ELSE IF %i > 210:PRINT \"big\"", "  50 ENDIF", "  60 NEXT %i", "  70 ; a comment", "  80 PRINT \"{UDG-A}\""];
        let program = next(&text);
        let file = plus3dos_file(&program, None);
        let listing = list(&file, "bas", charset::utf8());
        assert_eq!(listing.dialect, ZxDialect::Next.name());
        assert_eq!(listing.lines, text.iter().map(|l| l.to_string()).collect::<Vec<_>>());
        assert_eq!(tokenise_spectrum(&listing.lines, ZxDialect::Next).unwrap(), program);
        // A 48K program with block graphics in its strings isn't taken for NextBASIC.
        let old = plus3dos_file(&spectrum(&["10 PRINT \"\u{2588}{UDG-B}\""]), None);
        assert_eq!(list(&old, "bas", charset::utf8()).dialect, ZxDialect::Spectrum.name());
    }
}
