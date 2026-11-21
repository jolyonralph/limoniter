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
    /// What a language server says, for a file it's looking after: whether one is, its problems by line, the words
    /// it offered for the place they were asked for (line, where the word starts), and what it said about a place.
    pub lsp: bool,
    pub diagnostics: Vec<(usize, String)>,
    pub lsp_completion: Option<(usize, usize, Vec<String>)>,
    /// Set when new words come, so the list opens with them.
    pub lsp_fresh: bool,
    pub lsp_hover: Option<(usize, usize, String)>,
    /// The server's problems as it gave them, for asking it how to fix them.
    pub lsp_problems: Vec<serde_json::Value>,
    /// The signature of the call being typed: its line, its words, and the parameter being typed in them.
    pub lsp_signature: Option<(usize, String, Option<(usize, usize)>)>,
    /// Inline hints (types, parameters' names) by line, and the version they're for.
    pub lsp_hints: Vec<(usize, String)>,
    pub lsp_hints_version: u64,
    /// What the last build task said about this file, by line.
    pub build_problems: Vec<(usize, String)>,
    /// For Amiga code: the system's functions, from the NDK.
    pub amiga: Option<Arc<crate::amiga_ndk::Index>>,
    /// For 68000 source: what it does with the Amiga's custom chips and its copper lists.
    pub hardware: Option<Arc<crate::amiga_source::Source>>,
    /// For 68000 source: what it does with the Atari ST's hardware and which TOS functions it calls.
    pub atari: Option<Arc<crate::st_source::Source>>,
    /// For 68000 source for the Sinclair QL: which QDOS traps and vectors it calls.
    pub ql: Option<Arc<crate::ql_source::Source>>,
    /// For 6502 source: what it does with the Commodore 64's chips and which KERNAL routines it calls.
    pub c64: Option<Arc<crate::c64_source::Source>>,
    /// Boriel's ZX Basic: its words are described on hover, and its comments start with ' rather than ;.
    pub boriel: bool,
    /// Turbo Rascal (TRSE): its built-ins are described on hover, and its comments start with //.
    pub trse: bool,
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
        // KickAssembler's and 64tass's labels have other names in the assembler: .inner for a scope's own, loop__m2
        // for a multi label.
        if let Some(found) = assembly.uses.iter().chain(&assembly.definitions).find(|p| p.file == self.file && p.line == line && crate::asm::c64_syntax::written_name(&p.written) == word.trim_start_matches('!')) {
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
    /// timing when the word is its instruction; or what a language server said about it.
    pub fn describe(&self, line: usize, text: &str, col: usize) -> Option<String> {
        if let Some((l, c, words)) = &self.lsp_hover
            && *l == line
            && word_at(text, col).zip(word_at(text, *c)).is_some_and(|(a, b)| a.0 == b.0)
        {
            return Some(words.clone());
        }
        // TRSE's built-ins.
        if self.trse {
            return word_at(text, col).filter(|_| !crate::trse::in_comment_or_string(text, col)).and_then(|(_, _, word)| crate::trse::describe(&word));
        }
        // Boriel's ZX Basic's words and its library's.
        if self.boriel {
            return word_at(text, col).filter(|_| !crate::boriel::in_comment_or_string(text, col)).and_then(|(_, _, word)| crate::boriel::describe(&word));
        }
        // The Commodore 64's chips and KERNAL, for 6502 source.
        if let Some(c64) = self.c64.as_ref().filter(|s| s.c64)
            && !in_comment_or_string(text, col)
            && let Some(words) = c64.describe(line, text, col)
        {
            return Some(words);
        }
        // QDOS's traps and vectors, for the QL's code, which gets nothing of the ST's or the Amiga's.
        let ql = self.ql.as_ref().filter(|ql| ql.ql);
        if let Some(ql) = ql
            && !in_comment_or_string(text, col)
            && let Some(words) = ql.describe(line, text, col)
        {
            return Some(words);
        }
        // The Atari ST's hardware and TOS calls; ST code gets nothing of the Amiga's.
        let atari = self.atari.as_ref().filter(|st| st.atari && ql.is_none());
        if let Some(st) = atari
            && !in_comment_or_string(text, col)
            && let Some(words) = st.describe(line, text, col)
        {
            return Some(words);
        }
        // An Amiga system function, or a structure's field or a flag from the NDK's headers, unless the program
        // has a label of that name of its own (one it has from INCLUDEing the header is the NDK's).
        if let Some(ndk) = self.amiga.as_ref().filter(|_| atari.is_none() && ql.is_none())
            && !(self.cpu.is_some() && in_comment_or_string(text, col))
            && let Some((_, _, word)) = word_at(text, col)
            && (word.starts_with("_LVO") || self.assembly.as_ref().and_then(|a| a.symbols.get(&word)).is_none_or(|v| ndk.headers.get(&word).is_some_and(|n| n.value as i32 == *v as i32)))
            && let Some(words) = ndk.describe(&word)
        {
            return Some(words);
        }
        // The Amiga's custom chips: a register, a value written to one, or a copper instruction.
        if let Some(hardware) = self.hardware.as_ref().filter(|_| atari.is_none() && ql.is_none())
            && !in_comment_or_string(text, col)
            && let Some(words) = hardware.describe(line, text, col)
        {
            return Some(words);
        }
        if self.lsp && self.assembly.is_none() {
            return None;
        }
        if in_comment_or_string(text, col) {
            return None;
        }
        let assembly = self.assembly.as_ref()?;
        if let Some(name) = self.symbol_at(line, text, col) {
            let value = assembly.symbols.get(&name)?;
            let place = self.definition(&name).map(|d| {
                let file = assembly.files.get(d.file).filter(|_| d.file != self.file).and_then(|f| f.file_name()).map(|f| format!(" of {}", f.to_string_lossy())).unwrap_or_default();
                crate::i18n::tf!("\nDefined on line {}{file}", d.line + 1)
            });
            let negative = if *value < 0 { format!(" ({value})") } else { String::new() };
            // A KickAssembler multi label or a 64tass anonymous one by the name it was written with.
            let shown = crate::asm::c64_syntax::written_name(&name);
            return Some(format!("{shown} = {} (${:04X}){negative}{}", value, value & 0xFFFF_FFFF, place.unwrap_or_default()));
        }
        let (_, _, word) = word_at(text, col)?;
        if !self.words.iter().any(|w| w.eq_ignore_ascii_case(&word)) {
            return None;
        }
        let placed = assembly.lines.iter().find(|p| p.file == self.file && p.line == line && !p.bytes.is_empty())?;
        let bytes: Vec<String> = placed.bytes.iter().take(8).map(|b| format!("{b:02X}")).collect();
        let more = if placed.bytes.len() > 8 { " \u{2026}" } else { "" };
        let time = self.cpu.and_then(|cpu| cpu.isa().cycles_at(&placed.bytes, placed.address)).map(|c| format!(", {c} {}", if self.cpu == Some(Cpu::Z80) { "T-states" } else { "cycles" })).unwrap_or_default();
        // The 6809's and 6309's in the mode the source says, with the other mode's and why they may vary.
        let six309 = (self.cpu == Some(Cpu::M6809)).then(|| crate::m6809_timing::line(assembly, self.file, line)).flatten();
        let time = match six309 {
            Some((_, mode, timing)) => format!(", {}", crate::m6809_timing::words(timing, mode)),
            None if self.cpu == Some(Cpu::M6809) => String::new(),
            // The 68000's and 68008's, for the processor the source is for; none for the 68010 and later.
            None if self.cpu == Some(Cpu::M68k) => crate::m68k_timing::line(assembly, self.file, line).map(|(_, cpu, t)| format!(", {}", crate::m68k_timing::words(t, cpu))).unwrap_or_default(),
            None => time,
        };
        // On the 6502, what may add a cycle: an index crossing a page, a branch taken.
        let why = if let Some((_, mode, _)) = six309 {
            crate::m6809_timing::more(&placed.bytes, mode)
        } else if self.cpu == Some(Cpu::M6502) && crate::m6502_timing::is_instruction(&placed.bytes) {
            crate::m6502_timing::explain(&placed.bytes, Some(placed.address)).and_then(|w| w.split_once(" (").map(|(_, why)| format!("\n({why}"))).unwrap_or_default()
        } else {
            String::new()
        };
        Some(format!("${:04X}: {}{more} ({} byte{}{time}){why}", placed.address, bytes.join(" "), placed.bytes.len(), if placed.bytes.len() == 1 { "" } else { "s" }))
    }

    /// What can finish the word being typed on a line: labels (a local label only in its own part of the program),
    /// and the language's words, best first.
    pub fn completions(&self, line: usize, text: &str, col: usize) -> Vec<String> {
        // In ST code's comments, the TOS functions' names, to say which a call is.
        if self.atari.as_ref().is_some_and(|st| st.atari)
            && !self.ql.as_ref().is_some_and(|ql| ql.ql)
            && in_comment_or_string(text, col)
            && let Some((_, prefix)) = prefix_at(text, col)
        {
            return crate::st_source::Source::function_names(&prefix);
        }
        // Boriel's ZX Basic's comments start with ', and its semicolons are PRINT's.
        if self.boriel && crate::boriel::in_comment_or_string(text, col) {
            return Vec::new();
        }
        // Turbo Rascal's comments start with //, and its semicolons end statements.
        if self.trse && crate::trse::in_comment_or_string(text, col) {
            return Vec::new();
        }
        // C's semicolons end statements, not lines: only assembler and BASIC have comments that way.
        if !self.boriel && !self.trse && (self.amiga.is_none() || self.cpu.is_some()) && in_comment_or_string(text, col) {
            return Vec::new();
        }
        let Some((word_start, prefix)) = prefix_at(text, col) else {
            return Vec::new();
        };
        let lower = prefix.to_lowercase();
        let mut found: Vec<String> = Vec::new();
        // A language server's words, for where they were asked: those that go with what's been typed since.
        if let Some((l, start, words)) = &self.lsp_completion
            && *l == line
            && *start == word_start
        {
            found.extend(words.iter().filter(|w| w.to_lowercase().starts_with(&lower) && **w != prefix).cloned());
        }
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
        // Amiga system functions: in assembler as _LVO names (after JSR or wherever they're typed), in C by name.
        if let Some(ndk) = &self.amiga
            && (self.cpu.is_none() || prefix.starts_with("_LVO"))
        {
            found.extend(ndk.complete(&prefix));
        }
        // And in an operand, the headers' fields and flags: LN_ offers LN_SUCC and the rest.
        if let Some(ndk) = &self.amiga
            && operand
            && !prefix.starts_with("_LVO")
        {
            let names: Vec<String> = ndk.complete_header_names(&prefix).into_iter().filter(|n| !found.contains(n)).collect();
            found.extend(names);
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
            // The 6309's.
            "adcd", "adcr", "adde", "addf", "addr", "addw", "aim", "andd", "andr", "asld", "asrd", "band", "beor", "bieor", "biand", "bior", "bitd", "bitmd", "bor", "clrd", "clre", "clrf", "clrw", "cmpe", "cmpf", "cmpr", "cmpw", "comd", "come", "comf", "comw", "decd", "dece", "decf", "decw", "divd", "divq", "eim", "eord", "eorr", "incd", "ince", "incf", "incw", "ldbt", "lde", "ldf", "ldmd", "ldq", "ldw", "lsld", "lsrd", "lsrw", "muld", "negd", "oim", "ord", "orr", "pshsw", "pshuw", "pulsw", "puluw", "rold", "rolw", "rord", "rorw", "sbcd", "sbcr", "sexw", "stbt", "ste", "stf", "stq", "stw", "sube", "subf", "subr", "subw", "tfm", "tim", "tstd", "tste", "tstf", "tstw", "pragma",
        ],
        Cpu::M68k => &[
            "abcd", "add", "adda", "addi", "addq", "addx", "and", "andi", "asl", "asr", "bcc", "bchg", "bclr", "bcs", "beq", "bge", "bgt", "bhi", "ble", "bls", "blt", "bmi", "bne", "bpl", "bra", "bset", "bsr", "btst", "bvc", "bvs", "chk", "clr", "cmp", "cmpa", "cmpi", "cmpm", "dbf", "dbra", "dbeq", "dbne", "divs", "divu", "eor", "eori", "exg", "ext", "illegal", "jmp", "jsr", "lea", "link", "lsl", "lsr", "move", "movea", "movem", "moveq", "muls", "mulu", "nbcd", "neg", "negx", "nop", "not", "or", "ori", "pea", "reset", "rol", "ror", "roxl", "roxr", "rte", "rtr", "rts", "sbcd", "seq", "sne", "stop", "sub", "suba", "subi", "subq", "subx", "swap", "tas", "trap", "trapv", "tst", "unlk",
            // The 68010's and 68020's.
            "movec", "moves", "rtd", "bkpt", "bfchg", "bfclr", "bfexts", "bfextu", "bfffo", "bfins", "bfset", "bftst", "cas", "cas2", "chk2", "cmp2", "divsl", "divul", "extb", "pack", "unpk", "rtm", "callm", "trapeq", "trapne", "trapcc", "trapcs", "machine",
        ],
        Cpu::X86 => &[
            "aaa", "aad", "aam", "aas", "adc", "add", "and", "bound", "bsf", "bsr", "bswap", "bt", "btc", "btr", "bts", "call", "cbw", "cdq", "clc", "cld", "cli", "cmc", "cmp", "cmpsb", "cmpsw", "cmpsd", "cmpxchg", "cpuid", "cwd", "cwde", "daa", "das", "dec", "div", "enter", "hlt", "idiv", "imul", "in", "inc", "insb", "insw", "int", "into", "iret", "ja", "jae", "jb", "jbe", "jc", "jcxz", "je", "jecxz", "jg", "jge", "jl", "jle", "jmp", "jna", "jnc", "jne", "jnz", "jo", "jno", "jp", "jpe", "jpo", "js", "jns", "jz", "lahf", "lds", "lea", "leave", "les", "lfs", "lgs", "lodsb", "lodsw", "lodsd", "loop", "loope", "loopne", "lss", "mov", "movsb", "movsw", "movsd", "movsx", "movzx", "mul", "neg", "nop", "not", "or", "out", "outsb", "outsw", "pop", "popa", "popad", "popf", "push", "pusha", "pushad", "pushf", "rcl", "rcr", "rep", "repe", "repne", "ret", "retf", "rol", "ror", "sahf", "sal", "sar", "sbb", "scasb", "scasw", "scasd", "sete", "setne", "shl", "shld", "shr", "shrd", "stc", "std", "sti", "stosb", "stosw", "stosd", "sub", "test", "xchg", "xlatb", "xor",
            // NASM's own words.
            "bits", "cpu", "times", "resb", "resw", "resd", "dq",
            // The x87's commonest.
            "fld", "fst", "fstp", "fild", "fist", "fistp", "fadd", "faddp", "fsub", "fsubp", "fmul", "fmulp", "fdiv", "fdivp", "fcom", "fcomp", "fxch", "fsqrt", "fabs", "fchs", "fldz", "fld1", "fldpi", "finit", "fninit", "fwait",
        ],
        Cpu::Sm83 => &[
            "adc", "add", "and", "bit", "call", "ccf", "cp", "cpl", "daa", "dec", "di", "ei", "halt", "inc", "jp", "jr", "ld", "ldh", "ldi", "ldd", "nop", "or", "pop", "push", "res", "ret", "reti", "rl", "rla", "rlc", "rlca", "rr", "rra", "rrc", "rrca", "rst", "sbc", "scf", "set", "sla", "sra", "srl", "stop", "sub", "swap", "xor",
            "section", "def", "export",
        ],
        Cpu::Spc700 => &[
            "adc", "addw", "and", "and1", "asl", "bbc", "bbs", "bcc", "bcs", "beq", "bmi", "bne", "bpl", "bra", "brk", "bvc", "bvs", "call", "cbne", "clr1", "clrc", "clrp", "clrv", "cmp", "cmpw", "daa", "das", "dbnz", "dec", "decw", "di", "div", "ei", "eor", "eor1", "inc", "incw", "jmp", "lsr", "mov", "mov1", "movw", "mul", "nop", "not1", "notc", "or", "or1", "pcall", "pop", "push", "ret", "reti", "rol", "ror", "sbc", "set1", "setc", "setp", "sleep", "stop", "subw", "tcall", "tclr1", "tset1", "xcn",
        ],
        Cpu::Arm => &[
            "adc", "add", "adr", "and", "b", "beq", "bne", "bcs", "bcc", "bmi", "bpl", "bvs", "bvc", "bhi", "bls", "bge", "blt", "bgt", "ble", "bic", "bl", "cdp", "cmn", "cmp", "eor", "ldc", "ldmfd", "ldmia", "ldmib", "ldmda", "ldmdb", "ldr", "ldrb", "mcr", "mla", "mov", "movs", "mrc", "mul", "mvn", "nop", "orr", "rsb", "rsc", "sbc", "stc", "stmfd", "stmia", "stmib", "stmda", "stmdb", "str", "strb", "sub", "subs", "swi", "swp", "swpb", "teq", "teqp", "tst",
            "dcb", "dcw", "dcd", "equb", "equw", "equd", "equs", "align",
        ],
    };
    let mut words: Vec<String> = instructions.iter().chain(crate::asm::DIRECTIVES).map(|w| w.to_string()).collect();
    // The 68881's and 68882's, and their own directives.
    if cpu == Cpu::M68k {
        words.extend(crate::asm::m68k::fpu::mnemonics());
        words.extend(["fpu", "dc.s", "dc.d", "dc.x", "dc.p"].map(String::from));
        // The MMUs', and the 68040's, 68060's and CPU32's own.
        words.extend(crate::asm::m68k::mmu::mnemonics());
        words.extend(crate::asm::m68k::later::mnemonics());
        words.push("pmmu".into());
    }
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
                    assert!(cpu.isa().is_instruction(&word) || cpu.isa().is_directive(&word), "{cpu:?}: {word}");
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

    #[test]
    fn amiga_functions_are_described_and_offered_from_the_ndk() {
        let sfd = "==libname exec.library\n==bias 192\nAPTR\tAllocMem( ULONG byteSize, ULONG requirements ) (d0,d1)\n==end\n";
        let mut index = crate::amiga_ndk::Index::default();
        for f in crate::amiga_ndk::read_sfd(sfd) {
            index.functions.insert(f.name.clone(), f);
        }
        let ndk = Arc::new(index);
        let asm = CodeIntel { amiga: Some(ndk.clone()), ..CodeIntel::for_cpu(Cpu::M68k) };
        let hover = asm.describe(0, "  jsr _LVOAllocMem(a6)", 10).unwrap();
        assert!(hover.contains("_LVOAllocMem = -192") && hover.contains("byteSize in d0"), "{hover}");
        assert!(asm.describe(0, "  nop ; AllocMem", 13).is_none(), "nothing in a comment");
        assert_eq!(asm.completions(0, "  jsr _LVOAll", 13), ["_LVOAllocMem"]);
        assert!(asm.completions(0, "  All", 5).iter().all(|w| w != "AllocMem"), "assembler gets them as _LVO names");
        let c = CodeIntel { amiga: Some(ndk.clone()), ..CodeIntel::default() };
        assert!(c.describe(0, "x = 1; p = AllocMem(64, 0);", 13).unwrap().starts_with("APTR AllocMem"), "a semicolon isn't a comment in C");
        assert_eq!(c.completions(0, "x = 1; p = Allo", 15), ["AllocMem"]);
    }

    #[test]
    fn amiga_structures_fields_are_described_and_offered_from_the_ndk_s_headers() {
        use crate::amiga_ndk::headers::{Name, What};
        let mut index = crate::amiga_ndk::Index::default();
        let field = |value, kind: &str, size| Name { value, what: What::Field { structure: "LN".into(), kind: kind.into(), size }, header: "exec/nodes.i".into() };
        index.headers.insert("LN_NAME".into(), field(10, "APTR", 4));
        index.headers.insert("LN_PRI".into(), field(9, "BYTE", 1));
        index.headers.insert("LN_SIZE".into(), Name { value: 14, what: What::Size { structure: "LN".into() }, header: "exec/nodes.i".into() });
        let ndk = Arc::new(index);
        let asm = CodeIntel { amiga: Some(ndk.clone()), ..CodeIntel::for_cpu(Cpu::M68k) };
        assert_eq!(asm.describe(0, "  move.l LN_NAME(a1),a0", 12).as_deref(), Some("LN_NAME: offset 10 in LN (APTR, 4 bytes)\nexec/nodes.i"));
        assert_eq!(asm.completions(0, "  move.l LN_", 12), ["LN_NAME", "LN_PRI", "LN_SIZE"]);
        assert!(asm.completions(0, "  LN_", 5).is_empty(), "not where the instruction goes");
        // The program's own LN_SIZE, of another value, is its own.
        let assembly = crate::assemblers::assemble_own(Cpu::M68k, Path::new("t.s"), "LN_SIZE equ 3\n  dc.b LN_SIZE\n").assembly;
        let own = CodeIntel { assembly: Some(Arc::new(assembly)), amiga: Some(ndk), ..CodeIntel::for_cpu(Cpu::M68k) };
        assert!(own.describe(1, "  dc.b LN_SIZE", 10).unwrap().starts_with("LN_SIZE = 3"));
    }
}
