//! QDOS's calls, for hover in 68000 source for the Sinclair QL: the traps by the key in d0 (TRAP #1 the manager's,
//! TRAP #2 opening and closing channels, TRAP #3 input and output on one), and the vectored routines called
//! through the word at their vector (move.w UT.CON,a2 then jsr (a2)).
//!
//! The keys, names and vectors are the QL Technical Guide's (sections 18.14 and 18.15, checked against each trap's
//! own page in sections 13 to 15); the descriptions are Limoniter's own, from the Guide's.

use crate::amiga_source::{evaluate, split, text_of};
use crate::asm::Assembly;
use std::collections::HashMap;

/// A QDOS trap: which TRAP, the key in d0, its name and what it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Call {
    pub trap: u8,
    pub key: u8,
    pub name: &'static str,
    pub about: &'static str,
}

const fn key(trap: u8, key: u8, name: &'static str, about: &'static str) -> Call {
    Call { trap, key, name, about }
}

pub const CALLS: &[Call] = &[
    key(1, 0x00, "MT.INF", "get system information: the current job's ID, QDOS's version and where the system variables are"),
    key(1, 0x01, "MT.CJOB", "create a job"),
    key(1, 0x02, "MT.JINF", "get information on a job"),
    key(1, 0x04, "MT.RJOB", "remove an inactive job"),
    key(1, 0x05, "MT.FRJOB", "force remove a job, whether it's active or not (d1 = -1 for this one, d3 the error code)"),
    key(1, 0x06, "MT.FREE", "find the largest space there is for a job"),
    key(1, 0x07, "MT.TRAPV", "set a job's trap redirection vectors"),
    key(1, 0x08, "MT.SUSJB", "suspend a job"),
    key(1, 0x09, "MT.RELJB", "release a suspended job"),
    key(1, 0x0A, "MT.ACTIV", "activate a job"),
    key(1, 0x0B, "MT.PRIOR", "set a job's priority"),
    key(1, 0x0C, "MT.ALLOC", "allocate part of a heap"),
    key(1, 0x0D, "MT.LNKFR", "release part of a heap"),
    key(1, 0x0E, "MT.ALRES", "allocate resident procedure area"),
    key(1, 0x0F, "MT.RERES", "release the resident procedure area"),
    key(1, 0x10, "MT.DMODE", "set or read the display mode"),
    key(1, 0x11, "MT.IPCOM", "send a command to the IPC"),
    key(1, 0x12, "MT.BAUD", "set the serial ports' baud rate"),
    key(1, 0x13, "MT.RCLCK", "read the clock"),
    key(1, 0x14, "MT.SCLCK", "set the clock"),
    key(1, 0x15, "MT.ACLCK", "adjust the clock"),
    key(1, 0x16, "MT.ALBAS", "allocate SuperBASIC area"),
    key(1, 0x17, "MT.REBAS", "release SuperBASIC area"),
    key(1, 0x18, "MT.ALCHP", "allocate space in the common heap"),
    key(1, 0x19, "MT.RECHP", "release space in the common heap"),
    key(1, 0x1A, "MT.LXINT", "link in an external interrupt handler"),
    key(1, 0x1B, "MT.RXINT", "remove an external interrupt handler"),
    key(1, 0x1C, "MT.LPOLL", "link in a polled task"),
    key(1, 0x1D, "MT.RPOLL", "remove a polled task"),
    key(1, 0x1E, "MT.LSCHD", "link in a scheduler task"),
    key(1, 0x1F, "MT.RSCHD", "remove a scheduler task"),
    key(1, 0x20, "MT.LIOD", "link in an I/O driver"),
    key(1, 0x21, "MT.RIOD", "remove an I/O driver"),
    key(1, 0x22, "MT.LDD", "link in a directory driver"),
    key(1, 0x23, "MT.RDD", "remove a directory driver"),
    key(2, 0x01, "IO.OPEN", "open a channel: d1 the job (-1 for this one), d3 how (0 old, 1 shared, 2 new, 3 overwrite, 4 directory), a0 the name (a word count, then the letters); gives the channel in a0"),
    key(2, 0x02, "IO.CLOSE", "close the channel in a0"),
    key(2, 0x03, "IO.FORMT", "format a medium"),
    key(2, 0x04, "IO.DELET", "delete a file"),
    key(3, 0x00, "IO.PEND", "check for pending input"),
    key(3, 0x01, "IO.FBYTE", "fetch a byte into d1 (d3 the timeout, -1 to wait as long as it takes)"),
    key(3, 0x02, "IO.FLINE", "fetch a line of bytes, ending with a line feed, to (a1)"),
    key(3, 0x03, "IO.FSTRG", "fetch a string of bytes to (a1)"),
    key(3, 0x04, "IO.EDLIN", "edit a line"),
    key(3, 0x05, "IO.SBYTE", "send the byte in d1"),
    key(3, 0x07, "IO.SSTRG", "send a string of bytes: d2 how many, d3 the timeout, a1 where they are"),
    key(3, 0x09, "SD.EXTOP", "an external operation on a window, the routine at (a2)"),
    key(3, 0x0A, "SD.PXENQ", "a window's size and cursor position, in pixels"),
    key(3, 0x0B, "SD.CHENQ", "a window's size and cursor position, in characters"),
    key(3, 0x0C, "SD.BORDR", "set the window's border: d1 its colour, d2 its width"),
    key(3, 0x0D, "SD.WDEF", "define a window"),
    key(3, 0x0E, "SD.CURE", "enable the cursor"),
    key(3, 0x0F, "SD.CURS", "suppress the cursor"),
    key(3, 0x10, "SD.POS", "set the cursor's position"),
    key(3, 0x11, "SD.TAB", "tabulate: set the cursor's column"),
    key(3, 0x12, "SD.NL", "newline"),
    key(3, 0x13, "SD.PCOL", "previous column"),
    key(3, 0x14, "SD.NCOL", "next column"),
    key(3, 0x15, "SD.PROW", "previous row"),
    key(3, 0x16, "SD.NROW", "next row"),
    key(3, 0x17, "SD.PIXP", "set the cursor's position in pixels"),
    key(3, 0x18, "SD.SCROL", "scroll the whole window"),
    key(3, 0x19, "SD.SCRTP", "scroll the top of the window"),
    key(3, 0x1A, "SD.SCRBT", "scroll the bottom of the window"),
    key(3, 0x1B, "SD.PAN", "pan the window"),
    key(3, 0x1E, "SD.PANLN", "pan the cursor's line"),
    key(3, 0x1F, "SD.PANRT", "pan the right of the cursor's line"),
    key(3, 0x20, "SD.CLEAR", "clear the whole window to its paper colour"),
    key(3, 0x21, "SD.CLRTP", "clear the top of the window"),
    key(3, 0x22, "SD.CLRBT", "clear the bottom of the window"),
    key(3, 0x23, "SD.CLRLN", "clear the cursor's line"),
    key(3, 0x24, "SD.CLRRT", "clear to the right of the cursor"),
    key(3, 0x25, "SD.FOUNT", "set the founts"),
    key(3, 0x26, "SD.RECOL", "recolour a window"),
    key(3, 0x27, "SD.SETPA", "set the paper colour (d1)"),
    key(3, 0x28, "SD.SETST", "set the strip colour (d1)"),
    key(3, 0x29, "SD.SETIN", "set the ink colour (d1)"),
    key(3, 0x2A, "SD.SETFL", "set flashing on or off"),
    key(3, 0x2B, "SD.SETUL", "set underlining on or off"),
    key(3, 0x2C, "SD.SETMD", "set the writing mode"),
    key(3, 0x2D, "SD.SETSZ", "set the characters' size"),
    key(3, 0x2E, "SD.FILL", "fill a block"),
    key(3, 0x2F, "SD.DONL", "do a pending newline"),
    key(3, 0x30, "SD.POINT", "plot a point"),
    key(3, 0x31, "SD.LINE", "draw a line"),
    key(3, 0x32, "SD.ARC", "draw an arc"),
    key(3, 0x33, "SD.ELIPS", "draw an ellipse"),
    key(3, 0x34, "SD.SCALE", "set the graphics scale"),
    key(3, 0x35, "SD.FLOOD", "set the fill mode"),
    key(3, 0x36, "SD.GCUR", "set the text cursor by graphics coordinates"),
    key(3, 0x40, "FS.CHECK", "check all pending operations on a file"),
    key(3, 0x41, "FS.FLUSH", "flush a file's buffers"),
    key(3, 0x42, "FS.POSAB", "set the file's position, from its start"),
    key(3, 0x43, "FS.POSRE", "move the file's position, from where it is"),
    key(3, 0x45, "FS.MDINF", "information about the medium"),
    key(3, 0x46, "FS.HEADS", "set the file's header"),
    key(3, 0x47, "FS.HEADR", "read the file's header"),
    key(3, 0x48, "FS.LOAD", "load a file into memory"),
    key(3, 0x49, "FS.SAVE", "save memory as a file"),
];

/// A vectored routine: the vector's address, its name and what it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Vector {
    pub address: u16,
    pub name: &'static str,
    pub about: &'static str,
}

const fn vector(address: u16, name: &'static str, about: &'static str) -> Vector {
    Vector { address, name, about }
}

pub const VECTORS: &[Vector] = &[
    vector(0xC0, "MM.ALCHP", "allocate common heap space"),
    vector(0xC2, "MM.RECHP", "release common heap space"),
    vector(0xC4, "UT.WINDW", "set up a window by a name given in a0, with the border and colours in the block at a1"),
    vector(0xC6, "UT.CON", "set up a console window, as the block at a1 describes it (border, colours, size and place); gives the channel in a0"),
    vector(0xC8, "UT.SCR", "set up a screen window, as the block at a1 describes it; gives the channel in a0"),
    vector(0xCA, "UT.ERR0", "write an error message to channel 0"),
    vector(0xCC, "UT.ERR", "write an error message to a channel"),
    vector(0xCE, "UT.MINT", "write a number as text to a channel"),
    vector(0xD0, "UT.MTEXT", "send a message to a channel"),
    vector(0xD2, "UT.LINK", "link an item into a list"),
    vector(0xD4, "UT.UNLNK", "unlink an item from a list"),
    vector(0xD8, "MM.ALLOC", "allocate an area in a heap"),
    vector(0xDA, "MM.LNKFR", "link free space back into a heap"),
    vector(0xDC, "IO.QSET", "set up a queue"),
    vector(0xDE, "IO.QTEST", "test a queue's state"),
    vector(0xE0, "IO.QIN", "put a byte into a queue"),
    vector(0xE2, "IO.QOUT", "take a byte out of a queue"),
    vector(0xE4, "IO.QEOF", "put an end of file mark into a queue"),
    vector(0xE6, "UT.CSTR", "compare two strings"),
    vector(0xE8, "IO.SERQ", "direct queue handling"),
    vector(0xEA, "IO.SERIO", "general I/O handling"),
    vector(0xEC, "CN.DATE", "the date and time as text"),
    vector(0xEE, "CN.DAY", "the day of the week as text"),
    vector(0xF0, "CN.FTOD", "a floating point number as text"),
    vector(0xF2, "CN.ITOD", "a word integer as text"),
    vector(0xF4, "CN.ITOBB", "a byte as binary text"),
    vector(0xF6, "CN.ITOBW", "a word as binary text"),
    vector(0xF8, "CN.ITOBL", "a long as binary text"),
    vector(0xFA, "CN.ITOHB", "a byte as hex text"),
    vector(0xFC, "CN.ITOHW", "a word as hex text"),
    vector(0xFE, "CN.ITOHL", "a long as hex text"),
    vector(0x100, "CN.DTOF", "text to a floating point number"),
    vector(0x102, "CN.DTOI", "text to an integer"),
    vector(0x104, "CN.BTOIB", "binary text to a byte"),
    vector(0x106, "CN.BTOIW", "binary text to a word"),
    vector(0x108, "CN.BTOIL", "binary text to a long"),
    vector(0x10A, "CN.HTOIB", "hex text to a byte"),
    vector(0x10C, "CN.HTOIW", "hex text to a word"),
    vector(0x10E, "CN.HTOIL", "hex text to a long"),
    vector(0x110, "BP.INIT", "add machine code procedures and functions to SuperBASIC"),
    vector(0x112, "CA.GTINT", "get word integer parameters onto the arithmetic stack"),
    vector(0x114, "CA.GTFP", "get floating point parameters"),
    vector(0x116, "CA.GTSTR", "get string parameters"),
    vector(0x118, "CA.GTLIN", "get long integer parameters"),
    vector(0x11A, "BV.CHRIX", "reserve space on the arithmetic stack"),
    vector(0x11C, "RI.EXEC", "do an arithmetic operation"),
    vector(0x11E, "RI.EXECB", "do a list of arithmetic operations"),
    vector(0x120, "BP.LET", "assign the value on the stack to a variable"),
    vector(0x122, "IO.NAME", "decode a device's name"),
];

/// A name as QDOS has it, whether written mt.frjob, MT.FRJOB or mt_frjob.
fn same_name(word: &str, name: &str) -> bool {
    word.len() == name.len() && word.chars().zip(name.chars()).all(|(a, b)| a.eq_ignore_ascii_case(&b) || (a == '_' && b == '.'))
}

pub fn call_named(word: &str) -> Option<&'static Call> {
    CALLS.iter().find(|c| same_name(word, c.name))
}

pub fn vector_named(word: &str) -> Option<&'static Vector> {
    VECTORS.iter().find(|v| same_name(word, v.name))
}

pub fn call(trap: u8, key: i64) -> Option<&'static Call> {
    CALLS.iter().find(|c| c.trap == trap && i64::from(c.key) == key)
}

impl Call {
    pub fn describe(&self) -> String {
        crate::i18n::tf!("{}: TRAP #{} with d0 = ${:02X}, to {}.", self.name, self.trap, self.key, self.about)
    }
}

impl Vector {
    pub fn describe(&self) -> String {
        crate::i18n::tf!("{}: the routine whose address is the word at ${:X}, to {}. It's called as move.w {},a2 then jsr (a2).", self.name, self.address, self.about, self.name.to_ascii_lowercase())
    }
}

/// What each trap is for, when its key isn't known.
fn trap_about(trap: i64) -> String {
    match trap {
        1 => crate::i18n::tr("TRAP #1: QDOS's manager traps (jobs, memory, the clock), the key in d0.").into(),
        2 => crate::i18n::tr("TRAP #2: QDOS's traps that open and close channels, the key in d0.").into(),
        3 => crate::i18n::tr("TRAP #3: QDOS's input and output on the channel in a0, the key in d0 and the timeout in d3.").into(),
        4 => crate::i18n::tr("TRAP #4: makes the next TRAP #2 or #3's addresses relative to a6, as SuperBASIC's procedures need.").into(),
        _ => crate::i18n::tf!("TRAP #{trap}: not one QDOS uses for its calls (1 the manager's, 2 opening channels, 3 input and output, 4 a6-relative)."),
    }
}

/// A line of the source, split.
#[derive(Clone, Debug, Default)]
struct Line {
    mnemonic: String,
    operands: Vec<String>,
}

/// What a QL program's source calls.
#[derive(Clone, Debug, Default)]
pub struct Source {
    /// Whether it's the QL's: it says so, or it's for the 68008.
    pub ql: bool,
    lines: Vec<Line>,
    symbols: HashMap<String, i64>,
}

impl Source {
    pub fn scan(text: &str, assembly: Option<&Assembly>) -> Source {
        let mut source = Source { ql: crate::ql::says_ql(text.lines()), ..Source::default() };
        if let Some(assembly) = assembly {
            source.symbols = assembly.symbols.iter().map(|(k, v)| (k.to_ascii_lowercase(), *v)).collect();
        }
        for text_line in text.lines() {
            let parts = split(text_line);
            let line = Line { mnemonic: parts.mnemonic.map(|m| text_of(text_line, m).to_ascii_lowercase()).unwrap_or_default(), operands: parts.operands.iter().map(|s| text_of(text_line, *s)).collect() };
            if line.mnemonic == "machine" && line.operands.first().is_some_and(|o| o.trim().trim_start_matches(|c: char| c.is_ascii_alphabetic()) == "68008") {
                source.ql = true;
            }
            source.lines.push(line);
        }
        source
    }

    fn eval(&self, expr: &str) -> Option<i64> {
        let lookup = |name: &str| self.symbols.get(&name.to_ascii_lowercase()).copied().or_else(|| call_named(name).map(|c| i64::from(c.key)));
        evaluate(expr.trim(), &lookup)
    }

    /// The key put in d0 before the trap on `line`, looking back to the last trap or jump.
    fn key_before(&self, line: usize) -> Option<Option<i64>> {
        for n in (line.saturating_sub(20)..line).rev() {
            let l = &self.lines[n];
            let base = l.mnemonic.split('.').next().unwrap_or_default();
            if matches!(base, "trap" | "rts" | "bra" | "jmp" | "jsr" | "bsr") {
                return None;
            }
            if let [from, to] = &l.operands[..]
                && to.trim().eq_ignore_ascii_case("d0")
                && matches!(base, "moveq" | "move")
            {
                return Some(from.trim().strip_prefix('#').and_then(|e| self.eval(e)));
            }
        }
        None
    }

    /// What's under the cursor: a trap, with the call its key in d0 makes; a QDOS name; or a vector being read.
    pub fn describe(&self, line: usize, text: &str, col: usize) -> Option<String> {
        if !self.ql {
            return None;
        }
        let parts = split(text);
        let on_mnemonic = parts.mnemonic.is_some_and(|(s, e)| s <= col && col <= e);
        let operand = parts.operands.iter().position(|(s, e)| *s <= col && col <= *e);
        let here = self.lines.get(line)?;
        if here.mnemonic == "trap" && (on_mnemonic || operand.is_some()) {
            let trap = here.operands.first().and_then(|o| o.trim().strip_prefix('#')).and_then(|e| self.eval(e))?;
            let Some(key) = self.key_before(line) else {
                return Some(format!("{}\n{}", trap_about(trap), crate::i18n::tr("No key is put in d0 just before it here.")));
            };
            let Some(key) = key else {
                return Some(format!("{}\n{}", trap_about(trap), crate::i18n::tr("The key in d0 isn't known until it runs.")));
            };
            return Some(match u8::try_from(trap).ok().and_then(|t| call(t, key)) {
                Some(found) => found.describe(),
                None => format!("{}\n{}", trap_about(trap), crate::i18n::tf!("${key:02X} isn't a key Limoniter knows for it.")),
            });
        }
        let (_, _, word) = crate::code_intel::word_at(text, col)?;
        let word = word.trim_start_matches('#');
        if let Some(found) = call_named(word) {
            return Some(found.describe());
        }
        if let Some(found) = vector_named(word) {
            return Some(found.describe());
        }
        // A vector read by its address: move.w $C6,a2.
        if operand == Some(0)
            && here.mnemonic.starts_with("move")
            && let Some(address) = self.eval(word.trim_end_matches(".w").trim_end_matches(".W"))
            && let Some(found) = VECTORS.iter().find(|v| i64::from(v.address) == address)
            && here.operands.get(1).is_some_and(|o| o.trim().to_ascii_lowercase().starts_with('a'))
        {
            return Some(found.describe());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traps_are_named_by_the_key_in_d0() {
        let source = Source::scan(crate::ql::STARTER, None);
        assert!(source.ql);
        let lines: Vec<&str> = crate::ql::STARTER.lines().collect();
        let at = |word: &str, nth: usize| {
            let line = lines.iter().enumerate().filter(|(_, l)| l.contains(word)).nth(nth).unwrap().0;
            (line, lines[line].find(word).unwrap() + 1)
        };
        let (line, col) = at("trap    #2", 0);
        assert!(source.describe(line, lines[line], col).unwrap().starts_with("IO.OPEN: TRAP #2 with d0 = $01"));
        let (line, col) = at("trap    #1", 0);
        assert!(source.describe(line, lines[line], col).unwrap().starts_with("MT.FRJOB: TRAP #1 with d0 = $05"));
        let (line, col) = at("trap    #3", 2);
        assert!(source.describe(line, lines[line], col).unwrap().starts_with("IO.SSTRG: TRAP #3 with d0 = $07"));
        // A name, where it's defined or used.
        let (line, col) = at("mt.frjob equ", 0);
        assert!(source.describe(line, lines[line], col).unwrap().contains("force remove a job"));
        let (line, col) = at("#sd.clear", 0);
        assert!(source.describe(line, lines[line], col + 2).unwrap().starts_with("SD.CLEAR: TRAP #3 with d0 = $20"));
    }

    #[test]
    fn vectors_and_unknown_keys_are_explained() {
        let text = "        machine 68008\n        move.w  $c6,a2\n        jsr     (a2)\n        moveq   #$7f,d0\n        trap    #3\n        trap    #4\n        move.w  ut.scr,a2\n";
        let source = Source::scan(text, None);
        assert!(source.ql, "MACHINE 68008 is the QL's");
        let lines: Vec<&str> = text.lines().collect();
        let said = source.describe(4, lines[4], 9); assert!(said.as_deref().is_some_and(|s| s.contains("$7F isn't a key")), "{said:?}");
        assert!(source.describe(4, lines[4], 9).unwrap().contains("$7F isn't a key"));
        assert!(source.describe(5, lines[5], 9).unwrap().starts_with("TRAP #4: makes the next"));
        assert!(source.describe(6, lines[6], 18).unwrap().starts_with("UT.SCR"));
        assert_eq!(call_named("MT_FRJOB").map(|c| c.key), Some(5));
        // Other machines' code gets none of this.
        assert!(Source::scan("        moveq #5,d0\n        trap #1\n", None).describe(1, "        trap #1", 9).is_none());
    }

    /// Each key is the Guide's, no two calls share one, and no two names are the same.
    #[test]
    fn every_call_and_vector_is_known_once() {
        for (n, a) in CALLS.iter().enumerate() {
            assert!(CALLS[n + 1..].iter().all(|b| (a.trap, a.key) != (b.trap, b.key) && a.name != b.name), "{}", a.name);
        }
        for (n, a) in VECTORS.iter().enumerate() {
            assert!(a.address.is_multiple_of(2));
            assert!(VECTORS[n + 1..].iter().all(|b| a.address != b.address && a.name != b.name), "{}", a.name);
        }
        assert_eq!(call(3, 0x47).unwrap().name, "FS.HEADR");
        assert_eq!(vector_named("ut.con").unwrap().address, 0xC6);
    }
}
