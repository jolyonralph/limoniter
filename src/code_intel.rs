//! What the editor knows about the code in front, to help write it: for assembler source, the program as
//! Limoniter's own assembler last saw it (each label's value, where it's defined and used, each line's bytes),
//! and for any language it knows, its words. From these come completion, hover, going to a definition, finding
//! uses and renaming.

use crate::asm::{Assembly, Place};
use crate::assemblers::Cpu;
use std::sync::Arc;

/// What's known about a document's code.
#[derive(Clone, Debug, Default)]
pub struct CodeIntel {
    /// The program, as last assembled quietly, and the document's version then.
    pub assembly: Option<Arc<Assembly>>,
    pub version: u64,
    /// Which of the assembly's files the document is: 0 unless it's INCLUDEd by the one assembled.
    pub file: usize,
    pub cpu: Option<Cpu>,
    /// Words of the language: instructions and directives, or a BASIC's keywords.
    pub words: Arc<Vec<String>>,
    /// Whether the words are offered in capitals, as BASIC keywords are.
    pub upper: bool,
}

impl PartialEq for CodeIntel {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version && self.file == other.file && self.cpu == other.cpu && Arc::ptr_eq(&self.words, &other.words)
    }
}

impl Eq for CodeIntel {}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '@' | '?' | '$')
}

/// The word around a column of a line, by characters: its start, end and text. A word can be a label with its
/// local prefix (.loop, @loop), but a trailing dot or a size (.b, .w, .l after a 68000 instruction) stays part of it.
pub fn word_at(line: &str, col: usize) -> Option<(usize, usize, String)> {
    let chars: Vec<char> = line.chars().collect();
    let at = col.min(chars.len());
    let mut start = at;
    while start > 0 && word_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = at;
    while end < chars.len() && word_char(chars[end]) {
        end += 1;
    }
    // $ starts a hex number, not a label.
    while start < end && chars[start] == '$' {
        start += 1;
    }
    (start < end).then(|| (start, end, chars[start..end].iter().collect()))
}

/// The word being typed: from the start of the word to the cursor.
pub fn prefix_at(line: &str, col: usize) -> Option<(usize, String)> {
    let chars: Vec<char> = line.chars().collect();
    let at = col.min(chars.len());
    let mut start = at;
    while start > 0 && word_char(chars[start - 1]) {
        start -= 1;
    }
    (start < at).then(|| (start, chars[start..at].iter().collect()))
}

/// Whether a column is in a comment or a string, where nothing is offered.
fn in_comment_or_string(line: &str, col: usize) -> bool {
    let mut quote: Option<char> = None;
    for (i, c) in line.chars().enumerate() {
        if i >= col {
            break;
        }
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == ';' => return true,
            None if c == '"' => quote = Some(c),
            None => {}
        }
    }
    quote.is_some()
}

impl CodeIntel {
    /// For a BASIC's keywords.
    pub fn keywords(words: Vec<String>) -> Self {
        Self { words: Arc::new(words), upper: true, ..Self::default() }
    }

    /// For assembler source for a processor: its instructions and the assembler's directives.
    pub fn for_cpu(cpu: Cpu) -> Self {
        Self { cpu: Some(cpu), words: Arc::new(words_for(cpu)), ..Self::default() }
    }

    /// The label a word on a line of the document stands for, in full.
    pub fn symbol_at(&self, line: usize, text: &str, col: usize) -> Option<String> {
        let assembly = self.assembly.as_ref()?;
        let (_, _, word) = word_at(text, col)?;
        let word = word.trim_end_matches(':').to_string();
        let here = |p: &&Place| p.file == self.file && p.line == line && p.written == word;
        if let Some(found) = assembly.uses.iter().find(here).or_else(|| assembly.definitions.iter().find(here)) {
            return Some(found.name.clone());
        }
        // Somewhere that wasn't assembled, such as in an IF that was false: the label if there is one by that name.
        assembly.symbols.contains_key(&word).then_some(word)
    }

    pub fn definition(&self, name: &str) -> Option<&Place> {
        let assembly = self.assembly.as_ref()?;
        assembly.definitions.iter().find(|d| d.name == name).or_else(|| assembly.macros.iter().find(|m| m.name == name.to_ascii_lowercase()))
    }

    /// Where a label is defined and used, in file and line order.
    pub fn places(&self, name: &str) -> Vec<Place> {
        let Some(assembly) = self.assembly.as_ref() else {
            return Vec::new();
        };
        let mut places: Vec<Place> = assembly.definitions.iter().chain(&assembly.uses).filter(|p| p.name == name).cloned().collect();
        places.sort_by_key(|p| (p.file, p.line));
        places.dedup_by(|a, b| a.file == b.file && a.line == b.line && a.written == b.written);
        places
    }

    /// What a word on a line is, for its hover: a label's value and where it's defined, or the line's bytes and
    /// timing when the word is its instruction.
    pub fn describe(&self, line: usize, text: &str, col: usize) -> Option<String> {
        if in_comment_or_string(text, col) {
            return None;
        }
        let assembly = self.assembly.as_ref()?;
        if let Some(name) = self.symbol_at(line, text, col) {
            let value = assembly.symbols.get(&name)?;
            let place = self.definition(&name).map(|d| {
                let file = assembly.files.get(d.file).filter(|_| d.file != self.file).and_then(|f| f.file_name()).map(|f| format!(" of {}", f.to_string_lossy())).unwrap_or_default();
                format!("\nDefined on line {}{file}", d.line + 1)
            });
            let negative = if *value < 0 { format!(" ({value})") } else { String::new() };
            return Some(format!("{name} = {} (${:04X}){negative}{}", value, value & 0xFFFF_FFFF, place.unwrap_or_default()));
        }
        let (_, _, word) = word_at(text, col)?;
        if !self.words.iter().any(|w| w.eq_ignore_ascii_case(&word)) {
            return None;
        }
        let placed = assembly.lines.iter().find(|p| p.file == self.file && p.line == line && !p.bytes.is_empty())?;
        let bytes: Vec<String> = placed.bytes.iter().take(8).map(|b| format!("{b:02X}")).collect();
        let more = if placed.bytes.len() > 8 { " \u{2026}" } else { "" };
        let time = self.cpu.and_then(|cpu| cpu.isa().cycles(&placed.bytes)).map(|c| format!(", {c} {}", if self.cpu == Some(Cpu::Z80) { "T-states" } else { "cycles" })).unwrap_or_default();
        Some(format!("${:04X}: {}{more} ({} byte{}{time})", placed.address, bytes.join(" "), placed.bytes.len(), if placed.bytes.len() == 1 { "" } else { "s" }))
    }

    /// What can finish the word being typed on a line: labels (a local label only in its own part of the program),
    /// and the language's words, best first.
    pub fn completions(&self, line: usize, text: &str, col: usize) -> Vec<String> {
        if in_comment_or_string(text, col) {
            return Vec::new();
        }
        let Some((_, prefix)) = prefix_at(text, col) else {
            return Vec::new();
        };
        let lower = prefix.to_lowercase();
        let mut found: Vec<String> = Vec::new();
        // In assembler source, after the instruction come its operands: labels, not more instructions.
        let start = col - prefix.chars().count();
        let before: String = text.chars().take(start).collect();
        let mut words = before.split_whitespace();
        if !text.starts_with(char::is_whitespace) {
            words.next();
        }
        let operand = self.cpu.is_some() && words.next().is_some();
        if let Some(assembly) = &self.assembly {
            // The label this line is under, whose local labels can be reached from here.
            let scope = assembly.definitions.iter().rfind(|d| d.file == self.file && d.line <= line && d.written == d.name).map(|d| d.name.clone()).unwrap_or_default();
            for d in &assembly.definitions {
                let offered = if d.written == d.name {
                    Some(d.name.as_str())
                } else if d.name == format!("{scope}{}", d.written) {
                    Some(d.written.as_str())
                } else {
                    None
                };
                if let Some(name) = offered.filter(|n| n.to_lowercase().starts_with(&lower) && **n != prefix) {
                    found.push(name.to_string());
                }
            }
            for m in assembly.macros.iter().filter(|_| !operand) {
                if m.name.starts_with(&lower) && m.name != prefix {
                    found.push(m.name.clone());
                }
            }
        }
        let typed_upper = prefix.chars().any(|c| c.is_ascii_uppercase()) && !prefix.chars().any(|c| c.is_ascii_lowercase());
        for word in self.words.iter().filter(|_| !operand) {
            if word.to_lowercase().starts_with(&lower) && word.to_lowercase() != lower {
                found.push(if self.upper || typed_upper { word.to_uppercase() } else { word.to_lowercase() });
            }
        }
        found.dedup();
        found.truncate(40);
        found
    }
}

/// A processor's instructions, and the directives Limoniter's assembler knows.
fn words_for(cpu: Cpu) -> Vec<String> {
    let instructions: &[&str] = match cpu {
        Cpu::Z80 => &[
            "adc", "add", "and", "bit", "call", "ccf", "cp", "cpd", "cpdr", "cpi", "cpir", "cpl", "daa", "dec", "di", "djnz", "ei", "ex", "exx", "halt", "im", "in", "inc", "ind", "indr", "ini", "inir", "jp", "jr", "ld", "ldd", "lddr", "ldi", "ldir", "neg", "nop", "or", "otdr", "otir", "out", "outd", "outi", "pop", "push", "res", "ret", "reti", "retn", "rl", "rla", "rlc", "rlca", "rld", "rr", "rra", "rrc", "rrca", "rrd", "rst", "sbc", "scf", "set", "sla", "sll", "sra", "srl", "sub", "xor",
            // The Next's.
            "ldix", "ldws", "ldirx", "lddx", "lddrx", "ldpirx", "outinb", "mul", "swapnib", "mirror", "nextreg", "pixeldn", "pixelad", "setae", "test", "bsla", "bsra", "bsrl", "bsrf", "brlc",
        ],
        Cpu::M6502 => &[
            "adc", "and", "asl", "bcc", "bcs", "beq", "bit", "bmi", "bne", "bpl", "brk", "bvc", "bvs", "clc", "cld", "cli", "clv", "cmp", "cpx", "cpy", "dec", "dex", "dey", "eor", "inc", "inx", "iny", "jmp", "jsr", "lda", "ldx", "ldy", "lsr", "nop", "ora", "pha", "php", "pla", "plp", "rol", "ror", "rti", "rts", "sbc", "sec", "sed", "sei", "sta", "stx", "sty", "tax", "tay", "tsx", "txa", "txs", "tya",
        ],
        Cpu::M6809 => &[
            "abx", "adca", "adcb", "adda", "addb", "addd", "anda", "andb", "andcc", "asl", "asla", "aslb", "asr", "asra", "asrb", "bcc", "bcs", "beq", "bge", "bgt", "bhi", "bhs", "bita", "bitb", "ble", "blo", "bls", "blt", "bmi", "bne", "bpl", "bra", "brn", "bsr", "bvc", "bvs", "clr", "clra", "clrb", "cmpa", "cmpb", "cmpd", "cmps", "cmpu", "cmpx", "cmpy", "com", "coma", "comb", "cwai", "daa", "dec", "deca", "decb", "eora", "eorb", "exg", "inc", "inca", "incb", "jmp", "jsr", "lbcc", "lbcs", "lbeq", "lbge", "lbgt", "lbhi", "lbhs", "lble", "lblo", "lbls", "lblt", "lbmi", "lbne", "lbpl", "lbra", "lbrn", "lbsr", "lbvc", "lbvs", "lda", "ldb", "ldd", "lds", "ldu", "ldx", "ldy", "leas", "leau", "leax", "leay", "lsl", "lsla", "lslb", "lsr", "lsra", "lsrb", "mul", "neg", "nega", "negb", "nop", "ora", "orb", "orcc", "pshs", "pshu", "puls", "pulu", "rol", "rola", "rolb", "ror", "rora", "rorb", "rti", "rts", "sbca", "sbcb", "sex", "sta", "stb", "std", "sts", "stu", "stx", "sty", "suba", "subb", "subd", "swi", "swi2", "swi3", "sync", "tfr", "tst", "tsta", "tstb",
        ],
        Cpu::M68k => &[
            "abcd", "add", "adda", "addi", "addq", "addx", "and", "andi", "asl", "asr", "bcc", "bchg", "bclr", "bcs", "beq", "bge", "bgt", "bhi", "ble", "bls", "blt", "bmi", "bne", "bpl", "bra", "bset", "bsr", "btst", "bvc", "bvs", "chk", "clr", "cmp", "cmpa", "cmpi", "cmpm", "dbf", "dbra", "dbeq", "dbne", "divs", "divu", "eor", "eori", "exg", "ext", "illegal", "jmp", "jsr", "lea", "link", "lsl", "lsr", "move", "movea", "movem", "moveq", "muls", "mulu", "nbcd", "neg", "negx", "nop", "not", "or", "ori", "pea", "reset", "rol", "ror", "roxl", "roxr", "rte", "rtr", "rts", "sbcd", "seq", "sne", "stop", "sub", "suba", "subi", "subq", "subx", "swap", "tas", "trap", "trapv", "tst", "unlk",
        ],
    };
    let mut words: Vec<String> = instructions.iter().chain(crate::asm::DIRECTIVES).map(|w| w.to_string()).collect();
    words.sort();
    words.dedup();
    words
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn intel(source: &str) -> CodeIntel {
        let assembly = crate::assemblers::assemble_own(Cpu::Z80, Path::new("t.asm"), source).assembly;
        CodeIntel { assembly: Some(Arc::new(assembly)), ..CodeIntel::for_cpu(Cpu::Z80) }
    }

    const SOURCE: &str = "  org 32768\nmain: ld b,8\n.loop: djnz .loop\n  jp main\nother: ld a,2\n.loop: ret\ncount equ 3\n  ld a,count ; main\n";

    #[test]
    fn every_instruction_offered_is_one_the_assembler_knows() {
        for cpu in Cpu::ALL {
            for word in words_for(cpu) {
                if !crate::asm::DIRECTIVES.contains(&word.as_str()) {
                    assert!(cpu.isa().is_instruction(&word), "{cpu:?}: {word}");
                }
            }
        }
    }

    #[test]
    fn a_word_is_known_for_the_label_it_is() {
        let intel = intel(SOURCE);
        assert_eq!(word_at("  djnz .loop", 9), Some((7, 12, ".loop".into())));
        assert_eq!(word_at("  ld a,$ff", 9), Some((8, 10, "ff".into())));
        assert_eq!(intel.symbol_at(2, ".loop: djnz .loop", 14).as_deref(), Some("main.loop"));
        assert_eq!(intel.symbol_at(5, ".loop: ret", 2).as_deref(), Some("other.loop"), "the same written name, another label");
        assert_eq!(intel.symbol_at(3, "  jp main", 6).as_deref(), Some("main"));
        assert_eq!(intel.definition("main.loop").map(|d| d.line), Some(2));
        let places: Vec<usize> = intel.places("main").iter().map(|p| p.line).collect();
        assert_eq!(places, [1, 3]);
    }

    #[test]
    fn hover_says_what_a_label_is_and_what_a_line_became() {
        let intel = intel(SOURCE);
        assert_eq!(intel.describe(3, "  jp main", 6).as_deref(), Some("main = 32768 ($8000)\nDefined on line 2"));
        assert_eq!(intel.describe(1, "main: ld b,8", 7).as_deref(), Some("$8000: 06 08 (2 bytes, 7 T-states)"));
        assert_eq!(intel.describe(7, "  ld a,count ; main", 17), None, "nothing in a comment");
    }

    #[test]
    fn completion_offers_labels_in_reach_and_the_language_s_words() {
        let intel = intel(SOURCE);
        let offered = intel.completions(7, "  ld a,co", 9);
        assert_eq!(offered.first().map(String::as_str), Some("count"));
        assert_eq!(offered, ["count"], "after an instruction, only labels");
        let locals = intel.completions(3, "  jr .l", 7);
        assert_eq!(locals, [".loop"], "main's .loop, not other's: one offered");
        assert!(intel.completions(3, "  DJ", 4).contains(&"DJNZ".to_string()), "typed in capitals, offered in capitals");
        let basic = CodeIntel::keywords(crate::basic::keywords(Some(crate::basic::ZxDialect::Spectrum)));
        assert!(basic.completions(0, "10 RAN", 6).contains(&"RANDOMIZE".to_string()));
        assert!(basic.completions(0, "10 PRINT \"ran", 13).is_empty(), "nothing in a string");
    }
}
