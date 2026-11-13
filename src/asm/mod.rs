//! Limoniter's own assemblers. The part every processor shares is here: reading the source a line at a time
//! (labels, local labels, comments, several statements to a line), includes, macros, repeats and conditions, the
//! data directives, and passes until every label has settled. Each processor (`Isa`) only turns one instruction
//! into its bytes. The syntax follows what's usual for each machine, sjasmplus's for the Z80 above all.

pub mod device;
pub mod expr;
pub mod m6502;
pub mod m6809;
pub mod m68k;
pub mod output;
pub mod z80;

use expr::{Scope, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// A processor's instructions.
pub trait Isa {
    /// Its name, for messages.
    fn name(&self) -> &'static str;
    /// Whether words are stored high byte first.
    fn big_endian(&self) -> bool {
        false
    }
    /// Whether a word is one of its instructions.
    fn is_instruction(&self, word: &str) -> bool;
    /// An instruction's bytes. `mnemonic` is in lower case.
    fn encode(&self, mnemonic: &str, operands: &str, line: &mut Line) -> Result<Vec<u8>, String>;
    /// Whether a colon can separate statements on a line (it can on the Z80, as sjasmplus allows).
    fn colon_separates(&self) -> bool {
        false
    }
    /// What starts a local label, which belongs to the label before it.
    fn local_prefixes(&self) -> &'static [char] {
        &['.']
    }
    /// Whether an instruction's operand ends at a space, and what follows is a comment (Motorola's way).
    fn operand_ends_at_space(&self) -> bool {
        false
    }
    /// How many clock cycles an instruction takes, as words to show ("11" or "7/12").
    fn cycles(&self, _bytes: &[u8]) -> Option<String> {
        None
    }
}

/// Where a problem is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub file: usize,
    /// From 0.
    pub line: usize,
    pub message: String,
}

/// What one source line became.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    pub file: usize,
    pub line: usize,
    pub address: i64,
    pub bytes: Vec<u8>,
}

/// A run of bytes at an address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub address: i64,
    pub bytes: Vec<u8>,
}

/// Where a label is defined or used: the file and line, and the label as it's written there (a local label is
/// written .loop and known as main.loop).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub name: String,
    pub written: String,
    pub file: usize,
    pub line: usize,
}

/// A SAVESNA, SAVETAP, SAVENEX or SAVEBIN line: what the source asks to be written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Save {
    pub kind: String,
    pub file: String,
    pub start: Option<i64>,
}

/// The whole program assembled.
#[derive(Clone, Debug, Default)]
pub struct Assembly {
    /// The files it was made from; the first is the one assembled.
    pub files: Vec<PathBuf>,
    pub chunks: Vec<Chunk>,
    pub lines: Vec<Placed>,
    pub symbols: BTreeMap<String, i64>,
    pub problems: Vec<Problem>,
    /// Where it starts, if the source says (END label, ENT, or a SAVE line's start).
    pub entry: Option<i64>,
    /// The machine it's for, if the source says (DEVICE).
    pub device: Option<String>,
    pub saves: Vec<Save>,
    /// What DISPLAY lines printed.
    pub notes: Vec<String>,
    /// Whether ORG put it at an address, rather than wherever it's loaded.
    pub absolute: bool,
    /// Where each label is defined, in the order they are.
    pub definitions: Vec<Place>,
    /// Where labels are used.
    pub uses: Vec<Place>,
    /// Where each macro is defined: its name, file and line.
    pub macros: Vec<Place>,
    /// The files the source asked to be written (SAVEBIN, SAVENEX, SAVESNA, SAVETAP, SAVE3DOS, OUTPUT), in order.
    pub outputs: Vec<device::Output>,
    /// What's worth saying but doesn't stop the program being made, such as a value cut to fit a byte, as
    /// sjasmplus warns of it.
    pub warnings: Vec<Problem>,
    /// sjasmplus's RELOCATE blocks: where each is put, whether it's HIGH, and how far DISP moves its addresses.
    pub relocations: Vec<(i64, i64, bool, i64)>,
}

impl Assembly {
    pub fn ok(&self) -> bool {
        self.problems.is_empty()
    }

    pub fn lowest(&self) -> Option<i64> {
        self.chunks.iter().filter(|c| !c.bytes.is_empty()).map(|c| c.address).min()
    }

    pub fn highest(&self) -> Option<i64> {
        self.chunks.iter().filter(|c| !c.bytes.is_empty()).map(|c| c.address + c.bytes.len() as i64).max()
    }

    /// The bytes from `start` to `end`, with anything not assembled as zero.
    pub fn image(&self, start: i64, end: i64) -> Vec<u8> {
        let mut image = vec![0u8; (end - start).max(0) as usize];
        for chunk in &self.chunks {
            for (i, byte) in chunk.bytes.iter().enumerate() {
                let at = chunk.address + i as i64 - start;
                if at >= 0 && (at as usize) < image.len() {
                    image[at as usize] = *byte;
                }
            }
        }
        image
    }

    /// The source line whose code is at an address: the file and line.
    pub fn line_at(&self, address: i64) -> Option<(usize, usize)> {
        self.lines.iter().find(|p| !p.bytes.is_empty() && address >= p.address && address < p.address + p.bytes.len() as i64).map(|p| (p.file, p.line))
    }

    /// Where a source line's code starts, if it made any.
    pub fn address_of(&self, file: usize, line: usize) -> Option<i64> {
        self.lines.iter().find(|p| p.file == file && p.line == line && !p.bytes.is_empty()).map(|p| p.address)
    }

    /// Where it should start: the source's own entry, or else its first byte.
    pub fn start(&self) -> Option<i64> {
        self.entry.or_else(|| self.chunks.iter().find(|c| !c.bytes.is_empty()).map(|c| c.address))
    }
}

/// Assembles a program. `text` is the main file's text (as it is in the editor, saved or not); `read` fetches any
/// other file it includes, by its full path.
pub fn assemble(isa: &dyn Isa, path: &Path, text: &str, read: &dyn Fn(&Path) -> Result<Vec<u8>, String>) -> Assembly {
    assemble_at(isa, path, text, read, 0)
}

/// Assembles a program as if everything were `base` further on: ORG's addresses too. Assembled twice, at two bases,
/// the words that differ by the difference are the addresses that have to be moved when the program is.
pub fn assemble_at(isa: &dyn Isa, path: &Path, text: &str, read: &dyn Fn(&Path) -> Result<Vec<u8>, String>, base: i64) -> Assembly {
    let first = assemble_relocating(isa, path, text, read, base, &[], false);
    if first.relocations.is_empty() {
        return first;
    }
    // sjasmplus's RELOCATE blocks: assembled again with their labels moved, the words that move with them are the
    // ones its RELOCATE_TABLE lists; the table's own size can move the code, so it's gone round until it settles.
    let mut table: Vec<i64> = Vec::new();
    let mut assembly = first;
    for _ in 0..4 {
        let moved = assemble_relocating(isa, path, text, read, base, &table, true);
        let found = relocations_between(&assembly, &moved);
        if found == table {
            break;
        }
        table = found;
        assembly = assemble_relocating(isa, path, text, read, base, &table, false);
    }
    assembly
}

/// How far a RELOCATE block's labels are moved to find what refers to them: words move by all of it, and in a
/// HIGH block, high bytes by its high byte. Chosen so a word's neighbours can't move by it too.
const RELOCATE_BY: i64 = 0x1234;
const RELOCATE_HIGH_BY: i64 = 0x1200;

/// The addresses of the words (or high bytes) in RELOCATE blocks that moved with their labels.
fn relocations_between(normal: &Assembly, moved: &Assembly) -> Vec<i64> {
    let mut out = Vec::new();
    for (start, end, high, shift) in &normal.relocations {
        if end <= start {
            continue;
        }
        let a = normal.image(*start, *end + 1);
        let b = moved.image(*start, *end + 1);
        for at in 0..(end - start) as usize {
            if *high {
                if b[at].wrapping_sub(a[at]) == (RELOCATE_HIGH_BY >> 8) as u8 {
                    out.push(start + at as i64 + shift);
                }
            } else if at + 1 < (end - start) as usize {
                let wa = u16::from_le_bytes([a[at], a[at + 1]]);
                let wb = u16::from_le_bytes([b[at], b[at + 1]]);
                if wb.wrapping_sub(wa) == RELOCATE_BY as u16 {
                    out.push(start + at as i64 + shift);
                }
            }
        }
    }
    out
}

fn assemble_relocating(isa: &dyn Isa, path: &Path, text: &str, read: &dyn Fn(&Path) -> Result<Vec<u8>, String>, base: i64, table: &[i64], moved: bool) -> Assembly {
    let mut previous: HashMap<String, i64> = HashMap::new();
    let mut anonymous: HashMap<String, Vec<i64>> = HashMap::new();
    let mut pages: HashMap<String, i64> = HashMap::new();
    let mut used = std::collections::HashSet::new();
    let mut ever: HashMap<String, i64> = HashMap::new();
    for _ in 0..12 {
        let mut state = Pass::new(isa, read, previous.clone(), false);
        state.base = base;
        state.pc = base;
        state.previous_anonymous = anonymous.clone();
        state.previous_pages = pages.clone();
        state.previous_used = used.clone();
        state.ever = ever.clone();
        state.relocation_table = table.to_vec();
        state.relocate_moved = moved;
        state.run_file(path, text);
        ever.extend(state.symbols.iter().map(|(k, v)| (k.clone(), *v)));
        let now_used = state.all_used.into_inner();
        let settled = state.symbols == previous && state.anonymous == anonymous && state.pages == pages && now_used == used && !state.unknown;
        previous = state.symbols;
        anonymous = state.anonymous;
        pages = state.pages;
        used = now_used;
        if settled {
            break;
        }
    }
    // The last pass, saying what's wrong.
    let mut state = Pass::new(isa, read, previous, true);
    state.base = base;
    state.pc = base;
    state.previous_anonymous = anonymous;
    state.previous_pages = pages;
    state.previous_used = used;
    state.ever = ever;
    state.relocation_table = table.to_vec();
    state.relocate_moved = moved;
    state.run_file(path, text);
    state.finish()
}

/// An ACME block, opened with { and closed with }.
#[derive(Clone, Debug)]
enum Block {
    Plain,
    If,
    Macro,
    For,
    /// A zone: the label local labels belonged to before it.
    Zone(String),
    Pseudo,
}

/// Source lines waiting to be repeated or kept as a macro.
struct Collecting {
    kind: CollectKind,
    depth: usize,
    body: Vec<(usize, usize, String)>,
    at: (usize, usize),
}

enum CollectKind {
    Macro { name: String, params: Vec<String> },
    /// Repeated lines; with a variable, as ACME's !for counts with one.
    Repeat { times: i64, counter: Option<(String, i64)> },
}

#[derive(Clone)]
struct Macro {
    params: Vec<String>,
    body: Vec<(usize, usize, String)>,
}

/// One pass over the source.
struct Pass<'a> {
    isa: &'a dyn Isa,
    read: &'a dyn Fn(&Path) -> Result<Vec<u8>, String>,
    last: bool,
    previous: HashMap<String, i64>,
    symbols: HashMap<String, i64>,
    /// Labels set with DEFL or =, which may change.
    variables: std::collections::HashSet<String>,
    unknown: bool,
    pc: i64,
    /// Inside DISP: how far the addresses the code is assembled for are from where it's put.
    shift: i64,
    /// Anonymous labels (-, --, +, ++), where each was, in order, this pass and the last.
    anonymous: HashMap<String, Vec<i64>>,
    previous_anonymous: HashMap<String, Vec<i64>>,
    /// The 6809's direct page, as SETDP says (none: never assumed).
    direct_page: Option<i64>,
    /// ACME's { } blocks open: what each one is, to close it as it opened.
    blocks: Vec<Block>,
    /// Whether the source has shown itself to be ACME's, by its ! directives.
    acme: bool,
    /// Where it's assembled from: added to every address ORG sets.
    base: i64,
    /// Whether ORG has set an address, so the program can't be moved.
    absolute: bool,
    global: String,
    /// Inside a macro: what its local labels are known by.
    macro_scope: Option<String>,
    macros: HashMap<String, Macro>,
    expansions: usize,
    /// For each IF: whether its lines are assembled, and whether a part of it has been.
    conditions: Vec<(bool, bool)>,
    collecting: Option<Collecting>,
    ended: bool,
    out: Assembly,
    /// Labels used on the last pass; a RefCell, as expressions see the assembler only to read it.
    used: std::cell::RefCell<Vec<Place>>,
    /// Where the line being assembled is: its file and line, or the line a macro was used on.
    at: (usize, usize),
    depth: usize,
    /// sjasmplus's virtual devices used, each keeping its memory when another is chosen, and the one chosen.
    devices: Vec<device::Device>,
    device: Option<usize>,
    /// The page each label was defined in, this pass and the last, for $$label.
    pages: HashMap<String, i64>,
    previous_pages: HashMap<String, i64>,
    /// A .nex being made by SAVENEX, on the last pass.
    nex: Option<device::Nex>,
    /// The file OUTPUT sends assembled bytes to, as an index into the outputs.
    output: Option<usize>,
    /// OPT --zxnext=cspect: CSpect's BREAK and EXIT and the rest.
    cspect: bool,
    /// sjasmplus's DEFINEs, put in place of their names, and DEFARRAYs, put in place of name[index].
    defines: HashMap<String, String>,
    arrays: HashMap<String, Vec<String>>,
    /// The MODULEs open, outermost first.
    modules: Vec<String>,
    /// Every label used, this pass and the last, for IFUSED.
    all_used: std::cell::RefCell<std::collections::HashSet<String>>,
    previous_used: std::collections::HashSet<String>,
    /// STRUCTs defined, and the one being defined.
    structs: HashMap<String, Struct>,
    defining: Option<(String, Struct)>,
    /// The label defined on the line being assembled, for a STRUCT's fields to be named after.
    line_label: Option<String>,
    /// Every label defined on any pass so far, as sjasmplus never forgets one between passes.
    ever: HashMap<String, i64>,
    /// Where labels defined inside DISP were put, for $$$label.
    physical: HashMap<String, i64>,
    /// Whether the source has shown itself to be sjasmplus's (DEVICE, OPT, MODULE and the like), and whether
    /// its OPT --syntax has the a option (lists of operands split by ,, rather than ,).
    sjasm: bool,
    double_comma: bool,
    /// RELOCATE: the table found so far, whether labels in blocks are being moved to find it, and the block open.
    relocation_table: Vec<i64>,
    relocate_moved: bool,
    relocating: Option<(i64, bool, i64)>,
}

/// A STRUCT: its fields' names, offsets, sizes and default values, and its size.
#[derive(Clone, Debug, Default)]
struct Struct {
    fields: Vec<Field>,
    size: i64,
}

#[derive(Clone, Debug)]
struct Field {
    name: String,
    offset: i64,
    /// 1, 2, 3 or 4 for a number; 0 for a block of bytes, or another struct.
    unit: i64,
    size: i64,
    default: Vec<u8>,
}

impl<'a> Pass<'a> {
    fn new(isa: &'a dyn Isa, read: &'a dyn Fn(&Path) -> Result<Vec<u8>, String>, previous: HashMap<String, i64>, last: bool) -> Self {
        Pass {
            isa,
            read,
            last,
            previous,
            symbols: HashMap::new(),
            variables: Default::default(),
            unknown: false,
            pc: 0,
            shift: 0,
            anonymous: HashMap::new(),
            previous_anonymous: HashMap::new(),
            direct_page: Some(0),
            blocks: Vec::new(),
            acme: false,
            base: 0,
            absolute: false,
            global: String::new(),
            macro_scope: None,
            macros: HashMap::new(),
            expansions: 0,
            conditions: Vec::new(),
            collecting: None,
            ended: false,
            out: Assembly::default(),
            used: Default::default(),
            at: (0, 0),
            depth: 0,
            devices: Vec::new(),
            device: None,
            pages: HashMap::new(),
            previous_pages: HashMap::new(),
            nex: None,
            output: None,
            cspect: false,
            defines: HashMap::new(),
            arrays: HashMap::new(),
            modules: Vec::new(),
            all_used: Default::default(),
            previous_used: Default::default(),
            structs: HashMap::new(),
            defining: None,
            line_label: None,
            ever: HashMap::new(),
            physical: HashMap::new(),
            sjasm: false,
            double_comma: false,
            relocation_table: Vec::new(),
            relocate_moved: false,
            relocating: None,
        }
    }

    fn finish(mut self) -> Assembly {
        // A .nex still open at the end is closed, as sjasmplus does.
        if let Some(nex) = self.nex.take() {
            self.close_nex(nex, &[]);
        }
        if let Some(collecting) = &self.collecting {
            let what = if matches!(collecting.kind, CollectKind::Macro { .. }) { "MACRO has no ENDM" } else { "REPT has no ENDR" };
            self.out.problems.push(Problem { file: collecting.at.0, line: collecting.at.1, message: what.into() });
        }
        if !self.conditions.is_empty() {
            let (file, line) = self.at;
            self.out.problems.push(Problem { file, line, message: "an IF has no ENDIF".into() });
        }
        self.out.symbols = self.symbols.into_iter().collect();
        self.out.absolute = self.absolute;
        self.out.uses = self.used.into_inner();
        self.out
    }

    fn warn(&mut self, message: impl Into<String>) {
        if self.last {
            let (file, line) = self.at;
            let message = message.into();
            if !self.out.warnings.iter().any(|p| p.file == file && p.line == line && p.message == message) {
                self.out.warnings.push(Problem { file, line, message });
            }
        }
    }

    fn problem(&mut self, message: impl Into<String>) {
        if self.last {
            let (file, line) = self.at;
            let message = message.into();
            // Once is enough for each line.
            if !self.out.problems.iter().any(|p| p.file == file && p.line == line && p.message == message) {
                self.out.problems.push(Problem { file, line, message });
            }
        }
    }

    fn run_file(&mut self, path: &Path, text: &str) {
        if self.depth > 16 {
            self.problem("files include each other too deeply");
            return;
        }
        let file = match self.out.files.iter().position(|f| f == path) {
            Some(index) => index,
            None => {
                self.out.files.push(path.to_path_buf());
                self.out.files.len() - 1
            }
        };
        self.depth += 1;
        let text = without_block_comments(text);
        let mut joined: Option<(usize, String)> = None;
        for (number, line) in text.lines().enumerate() {
            if self.ended {
                break;
            }
            // A line ending with a backslash goes on into the next, as sjasmplus has it.
            let code = line.trim_end();
            if self.isa.colon_separates() && code.ends_with('\\') && !strip_comment(code).trim_end().is_empty() && strip_comment(code).trim_end().ends_with('\\') {
                let part = &code[..code.len() - 1];
                match &mut joined {
                    Some((_, text)) => {
                        text.push(' ');
                        text.push_str(part);
                    }
                    None => joined = Some((number, part.to_string())),
                }
                continue;
            }
            let (number, line) = match joined.take() {
                Some((first, mut text)) => {
                    text.push(' ');
                    text.push_str(line);
                    (first, std::borrow::Cow::Owned(text))
                }
                None => (number, std::borrow::Cow::Borrowed(line)),
            };
            self.at = (file, number);
            self.source_line(file, number, &line);
        }
        self.depth -= 1;
    }

    fn active(&self) -> bool {
        self.conditions.iter().all(|(on, _)| *on)
    }

    /// ACME's way of writing a line, as the assembler's own: its { } blocks as IF, MACRO, REPT and the rest,
    /// and its other directives' names. None if it isn't one.
    fn translate(&mut self, line: &str) -> Option<String> {
        let code = strip_comment(line).trim();
        if code == "}" {
            return Some(match self.blocks.pop() {
                Some(Block::If) => "endif".into(),
                Some(Block::Macro) => "endm".into(),
                Some(Block::For) => "endr".into(),
                Some(Block::Zone(previous)) => format!("endzone {previous}"),
                Some(Block::Pseudo) => "dephase".into(),
                _ => String::new(),
            });
        }
        if code.starts_with('}') && code.ends_with('{') && code.contains("else") {
            return Some("else".into());
        }
        let (word, rest) = split_word(code);
        let directive = word.strip_prefix('!')?.to_ascii_lowercase();
        self.acme = true;
        let (body, opens) = match rest.trim().strip_suffix('{') {
            Some(body) => (body.trim(), true),
            None => (rest.trim(), false),
        };
        let (block, text) = match directive.as_str() {
            "if" | "ifdef" | "ifndef" => (Block::If, format!("{directive} {body}")),
            "macro" => (Block::Macro, format!("macro {body}")),
            "for" => (Block::For, format!("for {body}")),
            "zone" | "zn" => (Block::Zone(self.global.clone()), format!("zone {}", if body.is_empty() { "zone" } else { body })),
            "pseudopc" => (Block::Pseudo, format!("phase {body}")),
            // !addr marks a label as an address; it makes nothing.
            "addr" | "address" => (Block::Plain, body.to_string()),
            "set" => (Block::Plain, body.to_string()),
            "warn" | "serious" => (Block::Plain, format!("{} {body}", if directive == "warn" { "message" } else { "error" })),
            "8" | "08" => (Block::Plain, format!("byte {body}")),
            "16" => (Block::Plain, format!("word {body}")),
            "24" | "32" => (Block::Plain, format!("dd {body}")),
            "bin" => (Block::Plain, format!("incbin {body}")),
            "h" | "hex" => (Block::Plain, format!("byte {}", body.split_whitespace().flat_map(|w| (0..w.len() / 2).map(move |i| format!("${}", &w[i * 2..i * 2 + 2]))).collect::<Vec<_>>().join(","))),
            _ if opens => (Block::Plain, format!("!{directive} {body}")),
            _ => return None,
        };
        if opens {
            self.blocks.push(block);
        }
        Some(text)
    }

    /// One line of source, as written.
    fn source_line(&mut self, file: usize, number: usize, original: &str) {
        let raw = original;
        // ACME's blocks and directives, when the processor's the 6502.
        let translated = if self.isa.name() == "6502" { self.translate(raw) } else { None };
        let raw = translated.as_deref().map(|t| {
            // Keep the label in front, if there was one.
            let code = raw.trim_start();
            if code.starts_with('!') || code.starts_with('}') { format!(" {t}") } else { t.to_string() }
        });
        let raw: &str = match &raw {
            Some(text) => text,
            None => original,
        };
        let line = strip_comment(raw);
        // Lines kept for a macro or a repeat.
        if self.collecting.is_some() {
            self.collect(file, number, raw);
            return;
        }
        let line = &self.substitute(line);
        let line = line.as_str();
        let trimmed = line.trim();
        // sjasmplus's labels for self-modifying code, name+1: or name+*:, point into the instruction after them.
        if self.isa.colon_separates()
            && self.active()
            && let Some((name, offset, after)) = smc_label(line)
        {
            let statements = self.statements(after);
            let start = self.pc + self.shift;
            let first = statements.first().cloned().unwrap_or_default();
            self.statement(first.trim());
            let length = self.pc + self.shift - start;
            let offset = offset.unwrap_or_else(|| smart_offset(first.trim(), length));
            self.define(&name, start + offset, false);
            for more in statements.iter().skip(1) {
                self.statement(more.trim());
            }
            return;
        }
        if trimmed.is_empty() {
            return;
        }
        // 68000 and 6809 sources use a * in the first column for a comment line.
        if raw.starts_with('*') && !raw[1..].trim_start().starts_with('=') {
            return;
        }
        let at_start = !line.starts_with([' ', '\t']);
        // An anonymous label: - or + (or -- or ++) where a label goes.
        let first = line.split_whitespace().next().unwrap_or("");
        // ca65's is a colon on its own, referred to as :- and :+.
        let anonymous = !first.is_empty() && (first.chars().all(|c| c == '-') || first.chars().all(|c| c == '+') || first == ":") && first.len() <= 8;
        let (label, rest) = if anonymous { (None, &line.trim_start()[first.len()..]) } else { split_label(line, at_start, |w| self.is_keyword(w), |w| self.isa.is_instruction(&w.to_ascii_lowercase())) };
        let rest = rest.trim();
        let word = rest.split(|c: char| c.is_whitespace()).next().unwrap_or("").to_ascii_lowercase();
        let word = word.trim_start_matches(['.', '!']).to_string();
        // IF, ELSE and ENDIF between colons with other statements (ELSE : IFDEF X), each in turn.
        if self.isa.colon_separates() && label.is_none() && rest.contains(':') {
            let parts = self.statements(rest);
            let is_condition = |p: &str| matches!(p.split_whitespace().next().unwrap_or("").to_ascii_lowercase().as_str(), "if" | "ifdef" | "ifndef" | "ifn" | "else" | "elseif" | "endif" | "ifused" | "ifnused");
            if parts.len() > 1 && parts.iter().any(|p| is_condition(p)) {
                for part in parts {
                    let part = part.trim();
                    if is_condition(part) {
                        let (word, operand) = split_word(part);
                        self.condition(&word.to_ascii_lowercase(), operand.trim());
                    } else if self.active() && !part.is_empty() {
                        self.statement(part);
                    }
                }
                return;
            }
        }
        // Conditions are followed even when not assembling, to find their ends.
        if matches!(word.as_str(), "if" | "ifdef" | "ifndef" | "ifn" | "else" | "elseif" | "endif" | "endc" | "ifeq" | "ifne" | "ifused" | "ifnused") {
            self.condition(&word, rest[rest.find(char::is_whitespace).unwrap_or(rest.len())..].trim());
            return;
        }
        if !self.active() {
            return;
        }
        self.line_label = None;
        // Inside STRUCT, lines are its fields.
        if self.defining.is_some() && word != "ends" {
            self.struct_field(label.as_deref(), rest);
            return;
        }
        if anonymous {
            let here = self.pc + self.shift;
            self.anonymous.entry(first.to_string()).or_default().push(here);
        }
        // *= sets the address, as many 6502 assemblers write ORG.
        if let Some(after) = rest.strip_prefix('*')
            && let Some(value) = after.trim_start().strip_prefix('=')
        {
            match self.eval(value.trim()) {
                Ok(v) => {
                    self.pc = v + self.base;
                    self.absolute = true;
                }
                Err(problem) => self.problem(problem),
            }
            return;
        }
        // A label given a value.
        if let Some(label) = &label {
            let (op, value) = split_word(rest);
            match op.to_ascii_lowercase().trim_start_matches(['.', '!']) {
                name @ ("equ" | "=" | "defl" | "set") if name != "set" || !self.isa.is_instruction("set") || op.starts_with(['.', '!']) => {
                    let variable = name != "equ";
                    let (value, more) = self.first_statement(value);
                    match self.eval(&value) {
                        Ok(v) => self.define(label, v, variable),
                        Err(problem) => self.problem(problem),
                    }
                    self.more_statements(&more);
                    return;
                }
                "macro" => {
                    // name MACRO params : body : ENDM, on one line, as sjasmplus allows.
                    let (first, more) = if self.isa.colon_separates() { self.first_statement(value) } else { (value.to_string(), Vec::new()) };
                    let params = split_operands(&first).into_iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
                    self.collecting = Some(Collecting { kind: CollectKind::Macro { name: label.to_ascii_lowercase(), params }, depth: 0, body: Vec::new(), at: (file, number) });
                    for statement in more {
                        if self.collecting.is_some() {
                            self.collect(file, number, &format!(" {}", statement.trim()));
                        } else if !statement.trim().is_empty() {
                            self.statement(statement.trim());
                        }
                    }
                    return;
                }
                _ => {}
            }
            if let Some(value) = rest.strip_prefix(":=").or_else(|| rest.strip_prefix('=')) {
                let (value, more) = self.first_statement(value);
                match self.eval(value.trim()) {
                    Ok(v) => self.define(label, v, true),
                    Err(problem) => self.problem(problem),
                }
                self.more_statements(&more);
                return;
            }
            let here = self.pc + self.shift;
            self.define(label, here, false);
            self.line_label = Some(self.full_name(label));
        }
        if rest.is_empty() {
            return;
        }
        let statements = self.statements(rest);
        for (i, statement) in statements.iter().enumerate() {
            if self.ended {
                break;
            }
            // A MACRO or DUP started on the line takes the rest of it, as sjasmplus has "MACRO name : body : ENDM".
            if self.collecting.is_some() {
                for more in &statements[i..] {
                    if self.collecting.is_none() {
                        self.statement(more.trim());
                        continue;
                    }
                    self.collect(file, number, &format!(" {}", more.trim()));
                }
                break;
            }
            self.statement(statement.trim());
        }
    }

    /// A line with sjasmplus's DEFINEs put in place of their names, and DEFARRAYs' name[index] by the item, outside
    /// quotes; lines that name a define (DEFINE, UNDEFINE, IFDEF and the like) are left alone.
    fn substitute(&mut self, line: &str) -> String {
        if self.defines.is_empty() && self.arrays.is_empty() {
            return line.to_string();
        }
        let words: Vec<String> = line.split_whitespace().take(2).map(|w| w.to_ascii_lowercase()).collect();
        if words.iter().any(|w| matches!(w.trim_start_matches(['.', '!']), "define" | "define+" | "undefine" | "ifdef" | "ifndef" | "defarray" | "defarray+")) {
            return line.to_string();
        }
        let mut text = line.to_string();
        for _ in 0..16 {
            let next = self.substitute_once(&text);
            if next == text {
                break;
            }
            text = next;
        }
        text
    }

    fn substitute_once(&mut self, line: &str) -> String {
        let line = substitute_words(line, &self.defines, false);
        if self.arrays.is_empty() {
            return line;
        }
        // DEFARRAY items: name[index].
        let mut out = String::new();
        let mut rest = line.as_str();
        while let Some(open) = rest.find('[') {
            let before = &rest[..open];
            let name_start = before.rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).map_or(0, |i| i + 1);
            let name = &before[name_start..];
            match (self.arrays.get(name).cloned(), rest[open..].find(']')) {
                (Some(items), Some(close)) if !name.is_empty() => {
                    let index = self.eval(&rest[open + 1..open + close]).unwrap_or(-1);
                    out.push_str(&before[..name_start]);
                    match usize::try_from(index).ok().and_then(|n| items.get(n)) {
                        Some(item) => out.push_str(item),
                        None => {
                            self.problem(format!("{name}[{index}] is outside the array"));
                            out.push('0');
                        }
                    }
                    rest = &rest[open + close + 1..];
                }
                _ => {
                    out.push_str(&rest[..=open]);
                    rest = &rest[open + 1..];
                }
            }
        }
        out.push_str(rest);
        out
    }

    /// A line inside STRUCT: a field, named or not, of a kind and size, with a default value.
    fn struct_field(&mut self, label: Option<&str>, rest: &str) {
        let kinds = |w: &str| matches!(w, "byte" | "db" | "defb" | "word" | "dw" | "defw" | "d24" | "dword" | "dd" | "defd" | "block" | "ds" | "defs" | "align" | "text");
        let (mut name, mut body) = (label.map(str::to_string), rest.to_string());
        if name.is_none() {
            let (first, after) = split_word(rest);
            let lower = first.to_ascii_lowercase();
            if !kinds(lower.trim_start_matches('.')) && !self.structs.contains_key(&lower) && !after.trim().is_empty() {
                name = Some(first.to_string());
                body = after.trim().to_string();
            }
        }
        let name = name.map(|n| n.trim_end_matches(':').trim_start_matches('.').to_string()).unwrap_or_default();
        let (kind, operands) = split_word(&body);
        let kind = kind.to_ascii_lowercase();
        let kind = kind.trim_start_matches('.');
        let Some((_, defining)) = self.defining.as_mut() else {
            return;
        };
        let offset = defining.size;
        let unit = match kind {
            "byte" | "db" | "defb" => 1,
            "word" | "dw" | "defw" => 2,
            "d24" => 3,
            "dword" | "dd" | "defd" => 4,
            _ => 0,
        };
        let field = if unit > 0 {
            let default = if operands.trim().is_empty() { 0 } else { self.eval(operands).unwrap_or(0) };
            Field { name, offset, unit, size: unit, default: default.to_le_bytes()[..unit as usize].to_vec() }
        } else if matches!(kind, "block" | "ds" | "defs") {
            let items = split_operands(operands);
            let count = self.eval(items.first().map(String::as_str).unwrap_or("0")).unwrap_or(0).clamp(0, 0x10000);
            let fill = items.get(1).map(|f| self.eval(f).unwrap_or(0)).unwrap_or(0);
            Field { name, offset, unit: 0, size: count, default: vec![fill as u8; count as usize] }
        } else if kind == "align" {
            let boundary = self.eval(if operands.trim().is_empty() { "4" } else { operands }).unwrap_or(4).max(1);
            let pad = (boundary - offset.rem_euclid(boundary)) % boundary;
            Field { name, offset, unit: 0, size: pad, default: vec![0; pad as usize] }
        } else if kind == "text" {
            let items = split_operands(operands);
            let count = self.eval(items.first().map(String::as_str).unwrap_or("0")).unwrap_or(0).clamp(0, 8192);
            let mut default = items.get(1).and_then(|t| string_value(t)).unwrap_or_default();
            default.resize(count as usize, 0);
            Field { name, offset, unit: 0, size: count, default }
        } else if let Some(inner) = self.structs.get(kind).cloned() {
            // A struct inside a struct: its fields named under this one's.
            let defining = &mut self.defining.as_mut().expect("defining").1;
            for f in &inner.fields {
                let full = if name.is_empty() { f.name.clone() } else if f.name.is_empty() { String::new() } else { format!("{name}.{}", f.name) };
                defining.fields.push(Field { name: full, offset: offset + f.offset, unit: f.unit, size: f.size, default: f.default.clone() });
            }
            if !name.is_empty() {
                defining.fields.push(Field { name, offset, unit: 0, size: 0, default: Vec::new() });
            }
            defining.size += inner.size;
            return;
        } else {
            self.problem(format!("{kind} isn't a kind of STRUCT field (BYTE, WORD, D24, DWORD, BLOCK, ALIGN, TEXT or a STRUCT)"));
            return;
        };
        let defining = &mut self.defining.as_mut().expect("defining").1;
        defining.size += field.size;
        defining.fields.push(field);
    }

    /// A STRUCT's bytes, its defaults or the values given ({a, b} or a, b, in order), and the label before it
    /// given a name for each field.
    fn struct_instance(&mut self, found: &Struct, operands: &str) {
        // label: STRUCT = address puts the label and its fields at the address, making no bytes.
        if let Some(address) = operands.trim().strip_prefix('=') {
            let value = self.eval(address).unwrap_or(0);
            if let Some(label) = self.line_label.clone() {
                self.symbols.insert(label.clone(), value);
                for field in found.fields.iter().filter(|f| !f.name.is_empty()) {
                    self.symbols.insert(format!("{label}.{}", field.name), value + field.offset);
                }
            }
            return;
        }
        let here = self.pc + self.shift;
        if let Some(label) = self.line_label.clone() {
            for field in found.fields.iter().filter(|f| !f.name.is_empty()) {
                let full = format!("{label}.{}", field.name);
                self.symbols.entry(full).or_insert(here + field.offset);
            }
        }
        let inner = operands.trim();
        let inner = inner.strip_prefix('{').and_then(|i| i.strip_suffix('}')).unwrap_or(inner);
        // Only the fields that hold bytes of their own take values, in order.
        let mut values = split_operands(inner).into_iter().map(|v| v.trim().to_string()).filter(|v| !v.is_empty() || !inner.is_empty());
        let mut bytes = vec![0u8; found.size as usize];
        for field in found.fields.iter().filter(|f| f.size > 0) {
            let at = field.offset as usize;
            let mut data = field.default.clone();
            if let Some(value) = values.next().filter(|v| !v.is_empty()) {
                if field.unit > 0 {
                    let v = self.eval(&value).unwrap_or(0);
                    data = v.to_le_bytes()[..field.unit as usize].to_vec();
                } else if let Some(text) = string_value(&value) {
                    data = text;
                    data.resize(field.size as usize, 0);
                }
            }
            let end = (at + data.len()).min(bytes.len());
            bytes[at..end].copy_from_slice(&data[..end - at]);
        }
        self.emit(bytes);
    }

    /// A line kept for a macro or repeat being collected, or the end of it.
    fn collect(&mut self, file: usize, number: usize, raw: &str) {
        let line = strip_comment(raw);
        let Some(collecting) = &mut self.collecting else {
            return;
        };
        let word = first_words(line);
        let opens = word.iter().any(|w| matches!(w.as_str(), "macro" | "rept" | "dup" | "repeat" | "for" | "while"));
        let closes = word.iter().any(|w| matches!(w.as_str(), "endm" | "endr" | "edup" | "endmacro" | "endrepeat" | "endw"));
        if opens {
            collecting.depth += 1;
        }
        if closes {
            if collecting.depth == 0 {
                let done = self.collecting.take().expect("collecting");
                self.collected(done);
                return;
            }
            collecting.depth -= 1;
        }
        collecting.body.push((file, number, raw.to_string()));
    }

    /// A label's value, and any statements after it on the line (after a colon, where colons separate them).
    fn first_statement(&self, text: &str) -> (String, Vec<String>) {
        let mut parts = self.statements(text).into_iter();
        let first = parts.next().unwrap_or_default();
        (first, parts.collect())
    }

    fn more_statements(&mut self, more: &[String]) {
        for statement in more {
            if !statement.trim().is_empty() {
                self.statement(statement.trim());
            }
        }
    }

    fn statements(&self, rest: &str) -> Vec<String> {
        if self.isa.colon_separates() { split_outside_quotes(rest, ':') } else { vec![rest.to_string()] }
    }

    fn is_keyword(&self, word: &str) -> bool {
        let lower = word.to_ascii_lowercase();
        let lower = lower.trim_start_matches(['.', '!']);
        DIRECTIVES.contains(&lower) || self.isa.is_instruction(lower) || self.macros.contains_key(lower)
    }

    fn condition(&mut self, word: &str, operand: &str) {
        let outer = self.active();
        match word {
            "if" | "ifn" | "ifne" | "ifeq" => {
                let value = if outer { self.eval(operand).unwrap_or(0) } else { 0 };
                let on = match word {
                    "ifn" | "ifeq" => value == 0,
                    _ => value != 0,
                };
                self.conditions.push((on, on));
            }
            "ifused" | "ifnused" => {
                // Whether a label is used anywhere (on this pass so far, or the last); with none, the last label.
                let name = if operand.trim().is_empty() { self.global.clone() } else { self.full_name(operand.trim()) };
                let used = self.previous_used.contains(&name) || self.all_used.borrow().contains(&name);
                let on = outer && ((word == "ifused") == used);
                self.conditions.push((on, on));
            }
            "ifdef" | "ifndef" => {
                let name = self.full_name(operand.trim());
                let known = self.defines.contains_key(operand.trim()) || self.symbols.contains_key(&name) || self.macros.contains_key(&operand.trim().to_ascii_lowercase());
                let on = (word == "ifdef") == known;
                self.conditions.push((on, on));
            }
            "elseif" => {
                let value = self.eval(operand).unwrap_or(0);
                match self.conditions.last_mut() {
                    Some((on, taken)) => {
                        *on = !*taken && value != 0;
                        *taken |= *on;
                    }
                    None => self.problem("ELSEIF without IF"),
                }
            }
            "else" => match self.conditions.last_mut() {
                Some((on, taken)) => {
                    *on = !*taken;
                    *taken = true;
                }
                None => self.problem("ELSE without IF"),
            },
            _ => {
                if self.conditions.pop().is_none() {
                    self.problem("ENDIF without IF");
                }
            }
        }
    }

    fn define(&mut self, label: &str, value: i64, variable: bool) {
        let name = self.full_name(label);
        if !label.starts_with(self.isa.local_prefixes()) && !variable && self.macro_scope.is_none() {
            self.global = name.clone();
        }
        if !variable && self.symbols.contains_key(&name) {
            self.problem(format!("{label} is defined twice"));
            return;
        }
        if variable {
            self.variables.insert(name.clone());
        }
        if self.shift != 0 && !variable {
            self.physical.insert(name.clone(), value - self.shift);
        }
        if let Some(page) = self.device.and_then(|d| self.devices[d].page_at(value & 0xFFFF)) {
            self.pages.insert(name.clone(), page as i64);
        }
        if self.last && self.macro_scope.is_none() && !self.out.definitions.iter().any(|d| d.name == name) {
            let (file, line) = self.at;
            self.out.definitions.push(Place { name: name.clone(), written: label.to_string(), file, line });
        }
        self.symbols.insert(name, value);
    }

    fn eval(&mut self, text: &str) -> Result<i64, String> {
        let trimmed = text.trim();
        if !trimmed.is_empty() && trimmed.len() <= 8 && (trimmed.chars().all(|c| c == '-') || trimmed.chars().all(|c| c == '+')) {
            return self.anonymous_label(trimmed);
        }
        if let Some(way) = trimmed.strip_prefix(':').filter(|w| !w.is_empty() && w.len() <= 8 && (w.chars().all(|c| c == '-') || w.chars().all(|c| c == '+'))) {
            return self.colon_label(way);
        }
        match expr::eval(text, self)? {
            Value::Known(v) => Ok(v),
            Value::Unknown => {
                self.unknown = true;
                if self.last {
                    let name = expr::first_unknown(text, self).unwrap_or_else(|| text.to_string());
                    Err(format!("{name} isn't defined"))
                } else {
                    Ok(0)
                }
            }
        }
    }

    /// An anonymous label: - is the last one before, + the next one after (and -- and ++ their own).
    fn anonymous_label(&mut self, name: &str) -> Result<i64, String> {
        let found = if name.starts_with('-') {
            self.anonymous.get(name).and_then(|all| all.last()).copied()
        } else {
            let seen = self.anonymous.get(name).map_or(0, Vec::len);
            self.previous_anonymous.get(name).and_then(|all| all.get(seen)).copied()
        };
        match found {
            Some(address) => Ok(address),
            None if !self.last => {
                self.unknown = true;
                Ok(0)
            }
            None => Err(format!("there's no {name} label {}", if name.starts_with('-') { "before this" } else { "after this" })),
        }
    }

    /// ca65's anonymous labels, all one kind: :- is the last before, :-- the one before that, :+ the next after.
    fn colon_label(&mut self, way: &str) -> Result<i64, String> {
        let count = way.len();
        let found = if way.starts_with('-') {
            let all = self.anonymous.get(":").map(Vec::as_slice).unwrap_or_default();
            all.len().checked_sub(count).and_then(|i| all.get(i)).copied()
        } else {
            let seen = self.anonymous.get(":").map_or(0, Vec::len);
            self.previous_anonymous.get(":").and_then(|all| all.get(seen + count - 1)).copied()
        };
        match found {
            Some(address) => Ok(address),
            None if !self.last => {
                self.unknown = true;
                Ok(0)
            }
            None => Err(format!("there's no anonymous label for :{way}")),
        }
    }

    fn emit(&mut self, bytes: Vec<u8>) {
        if let Some(index) = self.output
            && self.last
        {
            self.out.outputs[index].bytes.extend_from_slice(&bytes);
        }
        if let Some(index) = self.device {
            return self.emit_to_device(index, bytes);
        }
        let address = self.pc;
        self.pc += bytes.len() as i64;
        self.record(address, bytes);
    }

    /// Bytes put in a device's memory, a byte at a time, as MMU's n option can move the address on the way.
    fn emit_to_device(&mut self, index: usize, bytes: Vec<u8>) {
        let mut address = self.pc;
        let mut run_start = address;
        let mut run = Vec::new();
        for byte in bytes {
            let (next, problem) = self.devices[index].emit(address, byte);
            if let Some(problem) = problem {
                self.problem(problem);
            }
            run.push(byte);
            if next != address + 1 {
                // The slot was moved on to its next page: what follows starts at the slot's start again.
                self.record(run_start, std::mem::take(&mut run));
                run_start = next;
            }
            address = next;
        }
        self.record(run_start, run);
        self.pc = address;
    }

    /// Keeps what was assembled where, for listings, debugging and the program's own bytes.
    fn record(&mut self, address: i64, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        if self.last {
            match self.out.chunks.last_mut() {
                Some(chunk) if chunk.address + chunk.bytes.len() as i64 == address => chunk.bytes.extend_from_slice(&bytes),
                _ => self.out.chunks.push(Chunk { address, bytes: bytes.clone() }),
            }
            let (file, line) = self.at;
            match self.out.lines.last_mut() {
                // Several statements on a line, or a macro's lines, all belong to the line they were written on.
                Some(placed) if placed.file == file && placed.line == line && placed.address + placed.bytes.len() as i64 == address + self.shift => placed.bytes.extend(bytes),
                _ => self.out.lines.push(Placed { file, line, address: address + self.shift, bytes }),
            }
        }
    }

    fn word_bytes(&self, value: i64, size: usize) -> Vec<u8> {
        let bytes = value.to_le_bytes();
        let mut out = bytes[..size].to_vec();
        if self.isa.big_endian() {
            out.reverse();
        }
        out
    }

    fn statement(&mut self, text: &str) {
        // A colon at the end of a line leaves an empty statement after it.
        if text.trim().is_empty() {
            return;
        }
        // sjasmplus's repeat: .4 nop, or .(count) ld (hl),a, the statement after it that many times.
        if self.isa.colon_separates()
            && let Some(rest) = text.strip_prefix('.')
            && rest.starts_with(|c: char| c.is_ascii_digit() || c == '(')
        {
            let (count, body) = if rest.starts_with('(') {
                let close = rest.find(')').unwrap_or(rest.len() - 1);
                (&rest[1..close], &rest[close + 1..])
            } else {
                split_word(rest)
            };
            let times = self.eval(count).unwrap_or(0);
            if !(0..=0x10000).contains(&times) {
                self.problem(format!("{times} is too many times to repeat"));
                return;
            }
            for _ in 0..times {
                self.statement(body.trim());
            }
            return;
        }
        let (op, operands) = split_word(text);
        // OUT(C),A: the operand may follow the mnemonic with no space.
        let (op, operands) = match op.find('(') {
            Some(at) if at > 0 && self.isa.is_instruction(&op[..at].to_ascii_lowercase()) => {
                let start = text.len() - text.trim_start().len() + at;
                (&op[..at], &text[start..])
            }
            _ => (op, operands),
        };
        let lower = op.to_ascii_lowercase();
        // CSpect's own instructions, when OPT --zxnext=cspect asks for them.
        if self.cspect && matches!(lower.as_str(), "break" | "exit" | "clrbrk" | "setbrk") {
            let mut bytes = match lower.as_str() {
                "break" => vec![0xFD, 0x00],
                "exit" => vec![0xDD, 0x00],
                "clrbrk" => vec![0xED, 0x02],
                _ => vec![0xED, 0x01],
            };
            if bytes[0] == 0xED {
                let items = split_operands(operands);
                let values: Vec<i64> = items.iter().map(|i| self.eval(i).unwrap_or(0)).collect();
                bytes.push(values.first().copied().unwrap_or(0) as u8);
                for value in values.iter().skip(1).take(2) {
                    bytes.extend((*value as u16).to_le_bytes());
                }
            }
            self.emit(bytes);
            return;
        }
        let directive = lower.trim_start_matches(['.', '!']).to_string();
        // A macro: ACME calls one with a + before its name.
        let called = lower.strip_prefix('+').unwrap_or(&lower).to_string();
        if let Some(found) = self.macros.get(&called).cloned() {
            self.expand(&called, &found, operands);
            return;
        }
        if let Some(found) = self.structs.get(&lower).cloned() {
            self.struct_instance(&found, operands);
            return;
        }
        if DIRECTIVES.contains(&directive.as_str()) && !(self.isa.is_instruction(&lower) && !lower.starts_with(['.', '!'])) {
            self.directive(&directive, operands);
            return;
        }
        if !self.isa.is_instruction(&lower) {
            self.problem(format!("{op} isn't a {} instruction or a directive", self.isa.name()));
            return;
        }
        let isa = self.isa;
        // Motorola's way: the operand ends at a space, and what follows is a comment.
        let operands = if isa.operand_ends_at_space() { cut_at_space(operands.trim()) } else { operands.trim() };
        let mut line = Line { pass: self };
        match isa.encode(&lower, operands, &mut line) {
            Ok(bytes) => self.emit(bytes),
            Err(problem) => self.problem(problem),
        }
    }

    fn directive(&mut self, directive: &str, operands: &str) {
        let operands = operands.trim();
        // Motorola's sizes: DC.B, DC.W, DC.L, DS.B and so on.
        let (directive, size) = match directive.split_once('.') {
            Some((name, size @ ("b" | "w" | "l" | "s"))) if matches!(name, "dc" | "ds" | "dcb" | "rs" | "blk") => (name, Some(size)),
            _ => (directive, None),
        };
        let unit = match size {
            Some("w") => 2,
            Some("l") => 4,
            _ => 1,
        };
        match (directive, size) {
            ("dc", Some("w" | "l")) => return self.directive(if unit == 2 { "dw" } else { "dd" }, operands),
            ("dc", Some(_)) => return self.directive("db", operands),
            ("ds" | "rs" | "blk", Some(_)) => {
                let count = self.eval(operands).unwrap_or(0);
                return self.directive("ds", &format!("{}", count * unit));
            }
            ("dcb", _) => {
                let items = split_operands(operands);
                let count = self.eval(items.first().map(String::as_str).unwrap_or("0")).unwrap_or(0).clamp(0, 0x100_0000);
                let value = items.get(1).map(|v| self.eval(v).unwrap_or(0)).unwrap_or(0);
                let one = self.word_bytes(value, unit as usize);
                return self.emit(one.repeat(count as usize));
            }
            _ => {}
        }
        match directive {
            "fcc" | "fcs" if operands.starts_with(|c: char| c.is_ascii_punctuation() && c != '"' && c != '\'') => {
                // FCC /text/: any mark can start and end the text.
                let mark = operands.chars().next().expect("not empty");
                let inner = &operands[1..];
                let text = inner.split(mark).next().unwrap_or("");
                let mut bytes = text.as_bytes().to_vec();
                if directive == "fcs"
                    && let Some(last) = bytes.last_mut()
                {
                    *last |= 0x80;
                }
                self.emit(bytes);
            }
            "pet" | "scr" => {
                let mut bytes = Vec::new();
                for item in split_operands(operands) {
                    match string_value(&item) {
                        Some(text) => bytes.extend(text.iter().map(|b| if directive == "pet" { petscii(*b) } else { screen_code(*b) })),
                        None => bytes.push(self.eval(&item).unwrap_or(0) as u8),
                    }
                }
                self.emit(bytes);
            }
            "fill" | "res" => {
                let items = split_operands(operands);
                let count = self.eval(items.first().map(String::as_str).unwrap_or("0")).unwrap_or(0).clamp(0, 0x100_0000);
                let value = items.get(1).map(|v| self.eval(v).unwrap_or(0)).unwrap_or(0);
                self.emit(vec![value as u8; count as usize]);
            }
            "cnop" => {
                let items = split_operands(operands);
                let offset = self.eval(items.first().map(String::as_str).unwrap_or("0")).unwrap_or(0);
                let boundary = items.get(1).map(|v| self.eval(v).unwrap_or(1)).unwrap_or(1).max(1);
                let pad = (boundary - (self.pc - offset).rem_euclid(boundary)) % boundary;
                self.emit(vec![0; pad as usize]);
            }
            _ => self.plain_directive(directive, operands),
        }
    }

    fn plain_directive(&mut self, directive: &str, operands: &str) {
        match directive {
            "org" => {
                let items = split_operands(operands);
                match self.eval(items.first().map(String::as_str).unwrap_or("")) {
                    Ok(v) => {
                        // Under a device, addresses are the Z80's 64K.
                        self.pc = if self.device.is_some() { (v & 0xFFFF) + self.base } else { v + self.base };
                        self.absolute = true;
                    }
                    Err(problem) => self.problem(problem),
                }
                if let Some(index) = self.device {
                    // ORG address, page: the page goes in the slot PAGE works on.
                    if let Some(page) = items.get(1) {
                        let page = self.eval(page).unwrap_or(0);
                        if let Err(problem) = self.devices[index].set_page(page) {
                            self.problem(format!("ORG's {problem}"));
                        }
                    }
                    let pc = self.pc;
                    self.devices[index].reset_tracking(Some(pc));
                }
            }
            "db" | "defb" | "byte" | "by" | "dm" | "defm" | "fcb" | "fcc" | "text" | "tx" | "ascii" | "asc" | "dz" | "asciz" | "dc" | "abyt" | "dfb" | "raw" => {
                let mut bytes = Vec::new();
                for item in split_operands(operands) {
                    match string_value(&item) {
                        Some(text) => bytes.extend(text),
                        None => match self.eval(&item) {
                            Ok(v) => {
                                if self.last && !(-256..=255).contains(&v) {
                                    self.warn(format!("{item} is {v}, too big for a byte; its low byte is used"));
                                }
                                bytes.push(v as u8);
                            }
                            Err(problem) => self.problem(problem),
                        },
                    }
                }
                if matches!(directive, "dz" | "asciz") {
                    bytes.push(0);
                }
                if directive == "dc"
                    && let Some(last) = bytes.last_mut()
                {
                    *last |= 0x80;
                }
                self.emit(bytes);
            }
            "dw" | "defw" | "word" | "wo" | "fdb" | "addr" | "dd" | "dword" | "long" | "defd" => {
                let size = if matches!(directive, "dd" | "dword" | "long" | "defd") { 4 } else { 2 };
                let mut bytes = Vec::new();
                for item in split_operands(operands) {
                    let value = self.eval(&item).unwrap_or_else(|problem| {
                        self.problem(problem);
                        0
                    });
                    bytes.extend(self.word_bytes(value, size));
                }
                self.emit(bytes);
            }
            "ds" | "defs" | "block" | "rmb" | "dsb" => {
                let items = split_operands(operands);
                let count = self.eval(items.first().map(String::as_str).unwrap_or("0")).unwrap_or(0);
                let fill = items.get(1).map(|f| self.eval(f).unwrap_or(0)).unwrap_or(0);
                if !(0..=0x100_0000).contains(&count) {
                    self.problem(format!("{count} bytes is too many to leave"));
                    return;
                }
                self.emit(vec![fill as u8; count as usize]);
            }
            "align" | "even" => {
                let items = split_operands(operands);
                let boundary = if directive == "even" { 2 } else { self.eval(items.first().map(String::as_str).unwrap_or("2")).unwrap_or(2).max(1) };
                let fill = items.get(1).map(|f| self.eval(f).unwrap_or(0)).unwrap_or(0);
                let pad = (boundary - self.pc.rem_euclid(boundary)) % boundary;
                self.emit(vec![fill as u8; pad as usize]);
            }
            "include" | "incsrc" | "use" | "get" | "source" | "src" => {
                let name = unquote(operands);
                let path = self.beside(&name);
                match (self.read)(&path) {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes).into_owned();
                        let at = self.at;
                        self.run_file(&path, &text);
                        self.at = at;
                    }
                    Err(problem) => self.problem(format!("{name} couldn't be read: {problem}")),
                }
            }
            "incbin" | "binary" | "insert" | "incbin_" => {
                let items = split_operands(operands);
                let name = unquote(items.first().map(String::as_str).unwrap_or(""));
                let path = self.beside(&name);
                match (self.read)(&path) {
                    Ok(bytes) => {
                        let skip = items.get(1).map(|v| self.eval(v).unwrap_or(0)).unwrap_or(0).max(0) as usize;
                        let length = items.get(2).map(|v| self.eval(v).unwrap_or(0) as usize);
                        let from = skip.min(bytes.len());
                        let to = length.map_or(bytes.len(), |l| (from + l).min(bytes.len()));
                        self.emit(bytes[from..to].to_vec());
                    }
                    Err(problem) => self.problem(format!("{name} couldn't be read: {problem}")),
                }
            }
            "end" => {
                if !operands.is_empty() {
                    match self.eval(operands) {
                        Ok(v) => self.out.entry = Some(v),
                        Err(problem) => self.problem(problem),
                    }
                }
                self.ended = true;
            }
            "disp" | "phase" => match self.eval(operands) {
                Ok(v) => self.shift = v - self.pc,
                Err(problem) => self.problem(problem),
            },
            "dephase" | "unphase" => self.shift = 0,
            "ent" if self.shift != 0 => self.shift = 0,
            "ent" | "entry" | "start" => {
                let value = if operands.is_empty() { Ok(self.pc) } else { self.eval(operands) };
                match value {
                    Ok(v) => self.out.entry = Some(v),
                    Err(problem) => self.problem(problem),
                }
            }
            "macro" => {
                let mut items = split_operands(operands).into_iter();
                let first = items.next().unwrap_or_default();
                // MACRO name a, b, or MACRO name a b.
                let mut words = first.split_whitespace();
                let name = words.next().unwrap_or("").to_ascii_lowercase();
                let mut params: Vec<String> = words.map(str::to_string).collect();
                params.extend(items.map(|p| p.trim().to_string()));
                if name.is_empty() {
                    self.problem("a MACRO needs a name");
                }
                self.collecting = Some(Collecting { kind: CollectKind::Macro { name, params }, depth: 0, body: Vec::new(), at: self.at });
            }
            "rept" | "dup" | "repeat" => {
                let times = self.eval(operands).unwrap_or(0);
                self.collecting = Some(Collecting { kind: CollectKind::Repeat { times, counter: None }, depth: 0, body: Vec::new(), at: self.at });
            }
            "for" => {
                // !for name, last (from 1) or !for name, first, last.
                let items = split_operands(operands);
                let name = items.first().cloned().unwrap_or_default();
                let (first, last) = match items.len() {
                    2 => (1, self.eval(&items[1]).unwrap_or(0)),
                    _ => (items.get(1).map(|v| self.eval(v).unwrap_or(0)).unwrap_or(0), items.get(2).map(|v| self.eval(v).unwrap_or(0)).unwrap_or(0)),
                };
                let times = (last - first + 1).max(0);
                self.collecting = Some(Collecting { kind: CollectKind::Repeat { times, counter: Some((name, first)) }, depth: 0, body: Vec::new(), at: self.at });
            }
            "zone" => {
                self.global = operands.trim().to_string();
            }
            // ca65's .proc: a label, and the scope its @ labels belong to.
            "proc" => {
                let name = operands.split_whitespace().next().unwrap_or("").to_string();
                let here = self.pc + self.shift;
                self.define(&name, here, false);
            }
            "endzone" => {
                self.global = operands.trim().to_string();
            }
            "endm" | "endr" | "edup" | "endmacro" | "endrepeat" => self.problem(format!("{} without its start", directive.to_ascii_uppercase())),
            "device" => {
                self.sjasm = true;
                self.choose_device(operands);
            }
            "relocate_start" => {
                let high = operands.trim().eq_ignore_ascii_case("high");
                if self.relocating.is_some() {
                    self.problem("RELOCATE_START inside another RELOCATE block");
                } else {
                    self.relocating = Some((self.pc, high, self.shift));
                    if self.relocate_moved {
                        self.shift += if high { RELOCATE_HIGH_BY } else { RELOCATE_BY };
                    }
                }
            }
            "relocate_end" => match self.relocating.take() {
                Some((start, high, shift)) => {
                    if self.relocate_moved {
                        self.shift -= if high { RELOCATE_HIGH_BY } else { RELOCATE_BY };
                    }
                    if self.last {
                        self.out.relocations.push((start, self.pc, high, shift));
                    }
                }
                None => self.problem("RELOCATE_END without RELOCATE_START"),
            },
            "relocate_table" => {
                let subtract = if operands.trim().is_empty() { 0 } else { self.eval(operands).unwrap_or(0) };
                let words: Vec<u8> = self.relocation_table.iter().flat_map(|w| ((*w - subtract) as u16).to_le_bytes()).collect();
                self.emit(words);
            }
            "opt" => {
                self.sjasm = true;
                for option in operands.split_whitespace() {
                    let lower = option.to_ascii_lowercase();
                    if lower == "reset" {
                        self.double_comma = false;
                    } else if lower == "--zxnext=cspect" {
                        self.cspect = true;
                    } else if let Some(letters) = lower.strip_prefix("--syntax=") {
                        self.double_comma = letters.contains('a');
                    }
                }
            }
            "define" | "define+" => {
                let (name, value) = split_word(operands);
                if !name.is_empty() {
                    if directive == "define" && self.defines.contains_key(name) && self.last {
                        self.problem(format!("{name} is DEFINEd twice"));
                    }
                    self.defines.insert(name.to_string(), value.trim().to_string());
                }
            }
            "undefine" => {
                let name = operands.trim();
                if name == "*" {
                    self.defines.clear();
                } else {
                    self.defines.remove(name);
                }
            }
            "defarray" | "defarray+" => {
                let (name, values) = split_word(operands);
                let items: Vec<String> = split_operands(values).into_iter().map(|v| v.trim().to_string()).collect();
                if directive == "defarray+" {
                    self.arrays.entry(name.to_string()).or_default().extend(items);
                } else {
                    self.arrays.insert(name.to_string(), items);
                }
            }
            "module" => {
                self.sjasm = true;
                let name = operands.trim();
                if name.is_empty() || name.contains('.') {
                    self.problem("MODULE needs a name with no dots in it");
                } else {
                    self.modules.push(name.to_string());
                    self.global = format!("{}._", self.modules.join("."));
                }
            }
            "endmodule" => {
                if self.modules.pop().is_none() {
                    self.problem("ENDMODULE without MODULE");
                }
                self.global = if self.modules.is_empty() { String::new() } else { format!("{}._", self.modules.join(".")) };
            }
            "struct" => {
                let items = split_operands(operands);
                let name = items.first().map(|n| n.trim().to_string()).unwrap_or_default();
                let start = items.get(1).map(|o| self.eval(o).unwrap_or(0)).unwrap_or(0);
                if name.is_empty() {
                    self.problem("STRUCT needs a name");
                } else {
                    self.defining = Some((name, Struct { fields: Vec::new(), size: start }));
                }
            }
            "ends" => match self.defining.take() {
                Some((name, found)) => {
                    let full = self.full_name(&name);
                    // The structure's name is its size, and each field's full name its offset.
                    self.define(&name, found.size, true);
                    for field in &found.fields {
                        if !field.name.is_empty() {
                            self.define(&format!("{name}.{}", field.name), field.offset, true);
                        }
                    }
                    let _ = full;
                    self.structs.insert(name.to_ascii_lowercase(), found);
                }
                None => self.problem("ENDS without STRUCT"),
            },
            "dg" | "defg" | "dh" | "defh" | "hex" => {
                // DG: a picture of bits, # (or anything but - . _ and space) for 1; DH: hex digits.
                let text: String = unquote(operands).chars().filter(|c| !c.is_whitespace()).collect();
                let bytes: Vec<u8> = if matches!(directive, "dg" | "defg") {
                    let bits: Vec<u8> = text.chars().map(|c| u8::from(!matches!(c, '-' | '.' | '_' | '0'))).collect();
                    if !bits.len().is_multiple_of(8) {
                        self.problem("DG needs whole bytes: a multiple of 8 bits");
                    }
                    bits.chunks(8).map(|byte| byte.iter().fold(0u8, |v, b| (v << 1) | b)).collect()
                } else {
                    (0..text.len() / 2).filter_map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()).collect()
                };
                self.emit(bytes);
            }
            "d24" => {
                let mut bytes = Vec::new();
                for item in split_operands(operands) {
                    let value = self.eval(&item).unwrap_or(0);
                    bytes.extend(&value.to_le_bytes()[..3]);
                }
                self.emit(bytes);
            }
            "inchob" => {
                // A Hobeta file's data: its 17-byte header gives the length.
                let items = split_operands(operands);
                let name = unquote(items.first().map(String::as_str).unwrap_or(""));
                let path = self.beside(&name);
                match (self.read)(&path) {
                    Ok(bytes) if bytes.len() >= 17 => {
                        let length = usize::from(u16::from_le_bytes([bytes[11], bytes[12]])).min(bytes.len() - 17);
                        let data = &bytes[17..17 + length];
                        let skip = items.get(1).map(|v| self.eval(v).unwrap_or(0)).unwrap_or(0).max(0) as usize;
                        let take = items.get(2).map(|v| self.eval(v).unwrap_or(0) as usize);
                        let from = skip.min(data.len());
                        let to = take.map_or(data.len(), |t| (from + t).min(data.len()));
                        self.emit(data[from..to].to_vec());
                    }
                    Ok(_) => self.problem(format!("{name} is too short to be a Hobeta file")),
                    Err(problem) => self.problem(format!("{name} couldn't be read: {problem}")),
                }
            }
            "slot" | "page" | "mmu" | "savebin" | "save3dos" | "savesna" | "savetap" | "emptytap" | "savenex" | "savedev" | "output" | "outend" if self.device_directive(directive, operands) => {}
            "savetrd" => {
                let items = split_operands(operands);
                // SAVENEX takes words first: OPEN file, start; the rest set up the file and are left to it.
                let (file, start) = if directive == "savenex" {
                    let mut words = items.first().map(|i| i.split_whitespace().map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
                    if words.first().is_some_and(|w| w.eq_ignore_ascii_case("open")) {
                        words.remove(0);
                        (unquote(&words.join(" ")), items.get(1).cloned())
                    } else {
                        return;
                    }
                } else {
                    (unquote(items.first().map(String::as_str).unwrap_or("")), items.get(1).cloned())
                };
                let start = start.and_then(|s| self.eval(&s).ok());
                if matches!(directive, "savesna" | "savenex") && start.is_some() && self.out.entry.is_none() {
                    self.out.entry = start;
                }
                self.out.saves.push(Save { kind: directive.to_string(), file, start });
            }
            "display" | "message" | "echo" | "print" | "warning" => {
                if self.last {
                    let mut words = String::new();
                    // sjasmplus's /D, /H, /A and /C say how the next value is shown.
                    let mut decimal = false;
                    for item in split_operands(operands) {
                        let flag = item.trim().to_ascii_lowercase();
                        if matches!(flag.as_str(), "/d" | "/h" | "/a" | "/c" | "/b" | "/t") {
                            decimal = flag == "/d";
                            continue;
                        }
                        match string_value(&item) {
                            Some(text) => words.push_str(&String::from_utf8_lossy(&text)),
                            None => match self.eval(&item) {
                                Ok(v) if decimal => words.push_str(&v.to_string()),
                                Ok(v) => words.push_str(&format!("{v:#X}")),
                                Err(problem) => self.problem(problem),
                            },
                        }
                    }
                    self.out.notes.push(words);
                }
            }
            "assert" => {
                if self.last && self.eval(operands).is_ok_and(|v| v == 0) {
                    self.problem(format!("ASSERT {operands} failed"));
                }
            }
            "error" | "fail" => self.problem(unquote(operands)),
            "setdp" => {
                self.direct_page = if operands.is_empty() { None } else { self.eval(operands).ok().map(|v| if v > 0xFF { v >> 8 } else { v }) };
            }
            "zmb" => {
                let count = self.eval(operands).unwrap_or(0).clamp(0, 0x100_0000);
                self.emit(vec![0; count as usize]);
            }
            // Directives that change only listings and the like.
            _ => {}
        }
    }

    /// DEVICE name[, RAMTOP]: the virtual device code is put into, made the first time it's chosen and kept after.
    fn choose_device(&mut self, operands: &str) {
        let items = split_operands(operands);
        let name = items.first().map(|n| n.trim().to_ascii_uppercase()).unwrap_or_default();
        self.out.device = Some(name.clone());
        if name.is_empty() || name == "NONE" {
            self.device = None;
            return;
        }
        if let Some(index) = self.devices.iter().position(|d| d.name == name) {
            self.device = Some(index);
            return;
        }
        let ramtop = items.get(1).and_then(|r| self.eval(r).ok());
        match device::Device::new(&name, ramtop) {
            Some(made) => {
                self.devices.push(made);
                self.device = Some(self.devices.len() - 1);
                let pc = self.pc;
                self.devices.last_mut().expect("just made").reset_tracking(Some(pc));
            }
            None => self.problem(format!("{name} isn't a DEVICE Limoniter knows (ZXSPECTRUM48, ZXSPECTRUM128 and the larger ones, ZXSPECTRUMNEXT, NOSLOT64K, AMSTRADCPC464, AMSTRADCPC6128)")),
        }
    }

    /// The values of a directive's operands; an empty one is None, and a problem says which couldn't be worked out.
    fn values(&mut self, items: &[String]) -> Vec<Option<i64>> {
        items
            .iter()
            .map(|item| {
                if item.trim().is_empty() {
                    return None;
                }
                match self.eval(item) {
                    Ok(v) => Some(v),
                    Err(problem) => {
                        self.problem(problem);
                        None
                    }
                }
            })
            .collect()
    }

    /// A file name as a directive gives it: in quotes, or as written.
    fn file_name(&self, item: &str) -> String {
        unquote(item.trim())
    }

    /// Adds a file the source asks for, or replaces one of the same name made earlier.
    fn write_output(&mut self, name: String, bytes: Vec<u8>) {
        match self.out.outputs.iter_mut().find(|o| o.name == name) {
            Some(output) => output.bytes = bytes,
            None => self.out.outputs.push(device::Output { name, bytes }),
        }
    }

    fn append_output(&mut self, name: String, bytes: Vec<u8>) {
        match self.out.outputs.iter_mut().find(|o| o.name == name) {
            Some(output) => output.bytes.extend(bytes),
            None => self.out.outputs.push(device::Output { name, bytes }),
        }
    }

    /// The paging directives, and the files sjasmplus writes from a device's memory. False if it isn't one.
    fn device_directive(&mut self, directive: &str, operands: &str) -> bool {
        let items = split_operands(operands);
        match directive {
            "output" => {
                // OUTPUT file[, t|r|a]: assembled bytes are written to the file from here on.
                if self.last {
                    let name = self.file_name(items.first().map(String::as_str).unwrap_or(""));
                    let mode = items.get(1).map(|m| m.trim().to_ascii_lowercase()).unwrap_or_default();
                    // Rewound (r) or appended to (a), an earlier OUTPUT's file is kept; otherwise it starts empty.
                    if !matches!(mode.as_str(), "a" | "r") || !self.out.outputs.iter().any(|o| o.name == name) {
                        self.write_output(name.clone(), Vec::new());
                    }
                    self.output = self.out.outputs.iter().position(|o| o.name == name);
                }
                return true;
            }
            "outend" => {
                self.output = None;
                return true;
            }
            "emptytap" => {
                if self.last {
                    let name = self.file_name(items.first().map(String::as_str).unwrap_or(""));
                    self.write_output(name, Vec::new());
                }
                return true;
            }
            _ => {}
        }
        let Some(index) = self.device else {
            // SAVESNA and SAVENEX's start still says where the program starts, for running it.
            if matches!(directive, "savesna") {
                let start = items.get(1).and_then(|s| self.eval(s).ok());
                if start.is_some() && self.out.entry.is_none() {
                    self.out.entry = start;
                }
                let file = self.file_name(items.first().map(String::as_str).unwrap_or(""));
                self.out.saves.push(Save { kind: directive.to_string(), file, start });
            }
            self.problem(format!("{} needs a DEVICE (DEVICE ZXSPECTRUM48, ZXSPECTRUM128 or ZXSPECTRUMNEXT)", directive.to_ascii_uppercase()));
            return true;
        };
        match directive {
            "slot" => {
                let value = self.eval(operands).unwrap_or(-1);
                let slot = self.devices[index].slot_number(value);
                if (0..self.devices[index].slots.len() as i64).contains(&slot) {
                    self.devices[index].current = slot as usize;
                } else {
                    self.problem(format!("SLOT is a slot's number, 0 to {}, or the address it starts at", self.devices[index].slots.len() - 1));
                }
            }
            "page" => {
                let page = self.eval(operands).unwrap_or(-1);
                if let Err(problem) = self.devices[index].set_page(page) {
                    self.problem(format!("PAGE: {problem}"));
                }
                let pc = self.pc;
                self.devices[index].reset_tracking(Some(pc));
            }
            "mmu" => self.mmu(index, operands),
            "savebin" => {
                let values = self.values(&items[1.min(items.len())..]);
                if self.last {
                    let name = self.file_name(items.first().map(String::as_str).unwrap_or(""));
                    let start = values.first().copied().flatten().unwrap_or(0);
                    let length = values.get(1).copied().flatten().unwrap_or(0);
                    match device::save_bin(&self.devices[index], start, length) {
                        Ok(bytes) => self.write_output(name, bytes),
                        Err(problem) => self.problem(problem),
                    }
                }
            }
            "savedev" => {
                let values = self.values(&items[1.min(items.len())..]);
                if self.last {
                    let name = self.file_name(items.first().map(String::as_str).unwrap_or(""));
                    let (page, offset, length) = (values.first().copied().flatten().unwrap_or(-1), values.get(1).copied().flatten().unwrap_or(0), values.get(2).copied().flatten().unwrap_or(0));
                    match (page >= 0 && length > 0).then(|| self.devices[index].page_bytes(page as usize, offset as usize, length as usize)).flatten() {
                        Some(bytes) => {
                            let bytes = bytes.to_vec();
                            self.write_output(name, bytes);
                        }
                        None => self.problem("SAVEDEV's page, offset and length go outside the device's memory"),
                    }
                }
            }
            "save3dos" => {
                let values = self.values(&items[1.min(items.len())..]);
                if self.last {
                    let name = self.file_name(items.first().map(String::as_str).unwrap_or(""));
                    let get = |i: usize| values.get(i).copied().flatten();
                    match device::save_3dos(&self.devices[index], get(0).unwrap_or(-1), get(1).unwrap_or(0), get(2).unwrap_or(3), get(3), get(4)) {
                        Ok(bytes) => self.write_output(name, bytes),
                        Err(problem) => self.problem(problem),
                    }
                }
            }
            "savesna" => {
                let start = match items.get(1) {
                    Some(item) => self.eval(item).ok(),
                    None => self.out.entry,
                };
                if self.out.entry.is_none() {
                    self.out.entry = start;
                }
                if self.last {
                    let name = self.file_name(items.first().map(String::as_str).unwrap_or(""));
                    let Some(start) = start else {
                        self.problem("SAVESNA needs a start address (or END with one)");
                        return true;
                    };
                    // The snapshot is made from a copy, as saving one puts the start address on its stack.
                    let mut copy = self.devices[index].clone();
                    match device::save_sna(&mut copy, start) {
                        Ok(bytes) => {
                            self.devices[index].memory = copy.memory;
                            self.write_output(name, bytes)
                        }
                        Err(problem) => self.problem(problem),
                    }
                }
            }
            "savetap" => {
                if !self.last {
                    return true;
                }
                let name = self.file_name(items.first().map(String::as_str).unwrap_or(""));
                let kind = items.get(1).map(|k| k.trim().to_ascii_lowercase()).unwrap_or_default();
                if matches!(kind.as_str(), "basic" | "code" | "numbers" | "chars" | "headless") {
                    let (title, rest) = if kind == "headless" { (None, &items[2.min(items.len())..]) } else { (items.get(2).map(|t| self.file_name(t)), &items[3.min(items.len())..]) };
                    let values = self.values(rest);
                    let get = |i: usize| values.get(i).copied().flatten();
                    let Some(start) = get(0) else {
                        self.problem("SAVETAP needs where the block starts");
                        return true;
                    };
                    match device::tap_block(&self.devices[index], &kind, title.as_deref(), start, get(1), get(2), get(3)) {
                        Ok(bytes) => self.append_output(name, bytes),
                        Err(problem) => self.problem(problem),
                    }
                } else {
                    self.problem("SAVETAP of a whole snapshot, with sjasmplus's loader, isn't made by Limoniter's assembler yet: use SAVETAP name, CODE, ... or SAVESNA");
                }
            }
            "savenex" => self.savenex(index, operands),
            _ => return false,
        }
        true
    }

    /// MMU slot[, last slot | option e/w/n], page[, address]: pages mapped into slots, in a row.
    fn mmu(&mut self, index: usize, operands: &str) {
        let items = split_operands(operands);
        let (slots_part, rest) = match items.len() {
            0 | 1 => {
                self.problem("MMU needs slot, page");
                return;
            }
            _ => (items[0].trim().to_string(), &items[1..]),
        };
        // "MMU 6 n, 20" or "MMU 0 7, 14" or "MMU 6, 20".
        let mut words = slots_part.split_whitespace();
        let first = words.next().unwrap_or("").to_string();
        let second = words.next().map(str::to_string);
        let mut option = device::SlotOption::None;
        let mut last_text = None;
        if let Some(second) = &second {
            match second.to_ascii_lowercase().as_str() {
                "e" => option = device::SlotOption::Error,
                "w" => option = device::SlotOption::Warning,
                "n" => option = device::SlotOption::Next,
                _ => last_text = Some(second.clone()),
            }
        }
        // A range given as "MMU 0,1,page"? sjasmplus writes the range with a space, but a comma before the page.
        let slot1 = self.eval(&first).unwrap_or(-1);
        let slot2 = match &last_text {
            Some(text) => self.eval(text).unwrap_or(-1),
            None => slot1,
        };
        let page = self.eval(rest.first().map(String::as_str).unwrap_or("")).unwrap_or(-1);
        let address = rest.get(1).and_then(|a| self.eval(a).ok()).map(|a| a & 0xFFFF);
        let device = &mut self.devices[index];
        let (slot1, slot2) = (device.slot_number(slot1), device.slot_number(slot2));
        if slot1 < 0 || slot2 < slot1 || slot2 >= device.slots.len() as i64 {
            let most = device.slots.len() - 1;
            self.problem(format!("MMU's slots are 0 to {most} (or the address a slot starts at), first to last"));
            return;
        }
        if page < 0 || page + (slot2 - slot1) >= device.pages as i64 {
            let most = device.pages - 1;
            self.problem(format!("MMU's pages are 0 to {most}"));
            return;
        }
        for (i, slot) in (slot1..=slot2).enumerate() {
            device.slots[slot as usize] = (page + i as i64) as usize;
            device.options[slot as usize] = option;
        }
        self.pc &= 0xFFFF;
        if let Some(address) = address {
            self.pc = address + self.base;
            self.absolute = true;
        }
        let pc = self.pc;
        self.devices[index].reset_tracking(Some(pc));
    }

    /// SAVENEX and its commands, which make a .nex in stages, on the last pass.
    fn savenex(&mut self, index: usize, operands: &str) {
        let (command, rest) = split_word(operands.trim());
        let command = command.to_ascii_lowercase();
        let items = split_operands(rest);
        if command == "open" {
            let values = self.values(&items[1.min(items.len())..]);
            let start = values.first().copied().flatten();
            if self.out.entry.is_none() {
                self.out.entry = start;
            }
        }
        if !self.last {
            return;
        }
        if self.devices[index].name != "ZXSPECTRUMNEXT" {
            self.problem("SAVENEX needs DEVICE ZXSPECTRUMNEXT");
            return;
        }
        if command != "open" && self.nex.is_none() {
            self.problem("SAVENEX OPEN has to come first");
            return;
        }
        let values = |pass: &mut Self, from: usize| pass.values(&items[from.min(items.len())..]);
        let result: Result<(), String> = match command.as_str() {
            "open" => {
                if self.nex.is_some() {
                    Err("a NEX file is already open".to_string())
                } else {
                    let name = self.file_name(items.first().map(String::as_str).unwrap_or(""));
                    let v = values(self, 1);
                    let get = |i: usize| v.get(i).copied().flatten();
                    let start = get(0).or(self.out.entry).unwrap_or(0);
                    device::Nex::open(name, start, get(1).unwrap_or(0xFFFE), get(2).unwrap_or(0), get(3).unwrap_or(0)).map(|nex| self.nex = Some(nex))
                }
            }
            "core" => {
                let v = values(self, 0);
                let get = |i: usize| v.get(i).copied().flatten().unwrap_or(0);
                self.nex.as_mut().expect("open").core(get(0), get(1), get(2));
                Ok(())
            }
            "cfg" => {
                let v = values(self, 0);
                let get = |i: usize| v.get(i).copied().flatten();
                self.nex.as_mut().expect("open").cfg(get(0).unwrap_or(0), get(1), get(2), get(3));
                Ok(())
            }
            "cfg3" => {
                let v = values(self, 0);
                let get = |i: usize| v.get(i).copied().flatten();
                self.nex.as_mut().expect("open").cfg3(get(0).unwrap_or(1), get(1), get(2), get(3))
            }
            "bar" => {
                let v = values(self, 0);
                let get = |i: usize| v.get(i).copied().flatten();
                self.nex.as_mut().expect("open").bar(get(0).unwrap_or(0), get(1).unwrap_or(0), get(2), get(3), get(4));
                Ok(())
            }
            "palette" => {
                let (kind, more) = split_word(rest.trim());
                let more_items = split_operands(more);
                let v = self.values(&more_items);
                let device = &self.devices[index];
                let nex = self.nex.as_mut().expect("open");
                match kind.to_ascii_lowercase().as_str() {
                    "none" => nex.palette_none(),
                    "default" => nex.palette_default(),
                    "mem" => nex.palette_mem(device, v.first().copied().flatten().unwrap_or(0), v.get(1).copied().flatten().unwrap_or(0)),
                    "bmp" => Err("SAVENEX PALETTE BMP isn't read by Limoniter's assembler yet; use PALETTE MEM".to_string()),
                    other => Err(format!("{other} isn't a SAVENEX PALETTE kind (NONE, DEFAULT, MEM, BMP)")),
                }
            }
            "screen" => {
                let (kind, more) = split_word(rest.trim());
                let kind = kind.to_ascii_lowercase();
                let more_items = split_operands(more);
                let v = self.values(&more_items);
                let get = |i: usize| v.get(i).copied().flatten();
                let device = &self.devices[index];
                let nex = self.nex.as_mut().expect("open");
                match kind.as_str() {
                    "l2" | "lr" | "l2_320" | "l2_640" => nex.screen_bitmap(device, &kind, get(0).filter(|p| *p >= 0), get(1).unwrap_or(0), get(2).filter(|p| *p >= 0).map(|p| (p, get(3).unwrap_or(0)))),
                    "scr" | "shc" | "shr" => nex.screen_ula(device, &kind, get(0)),
                    "tile" => nex.screen_tile(device, [get(0).unwrap_or(0), get(1).unwrap_or(0), get(2).unwrap_or(0), get(3).unwrap_or(0)], get(4).unwrap_or(1) != 0),
                    "bmp" => Err("SAVENEX SCREEN BMP isn't read by Limoniter's assembler yet; put the picture in memory and use SCREEN L2".to_string()),
                    other => Err(format!("{other} isn't a SAVENEX SCREEN kind (L2, L2_320, L2_640, LR, SCR, SHC, SHR, TILE, BMP)")),
                }
            }
            "copper" => {
                let v = values(self, 0);
                let get = |i: usize| v.get(i).copied().flatten().unwrap_or(0);
                let device = &self.devices[index];
                self.nex.as_mut().expect("open").copper(device, get(0), get(1))
            }
            "bank" => {
                let v = values(self, 0);
                let device = &self.devices[index];
                let nex = self.nex.as_mut().expect("open");
                v.into_iter().try_for_each(|bank| nex.save_bank(device, bank.unwrap_or(-1), false))
            }
            "auto" => {
                let v = values(self, 0);
                let get = |i: usize| v.get(i).copied().flatten();
                let device = &self.devices[index];
                self.nex.as_mut().expect("open").auto(device, get(0), get(1))
            }
            "close" => {
                let mut appended = Vec::new();
                for item in items.iter().filter(|i| !i.trim().is_empty()) {
                    let name = self.file_name(item);
                    let path = self.beside(&name);
                    match (self.read)(&path) {
                        Ok(bytes) => appended.push(bytes),
                        Err(problem) => self.problem(format!("{name} couldn't be read: {problem}")),
                    }
                }
                let nex = self.nex.take().expect("open");
                self.close_nex(nex, &appended);
                Ok(())
            }
            other => Err(format!("{other} isn't a SAVENEX command (OPEN, CORE, CFG, CFG3, BAR, PALETTE, SCREEN, COPPER, BANK, AUTO, CLOSE)")),
        };
        if let Err(problem) = result {
            self.problem(format!("SAVENEX: {problem}"));
        }
    }

    fn close_nex(&mut self, nex: device::Nex, appended: &[Vec<u8>]) {
        let name = nex.name.clone();
        match nex.close(appended) {
            Ok(bytes) => self.write_output(name, bytes),
            Err(problem) => self.problem(format!("SAVENEX: {problem}")),
        }
    }

    /// A file named in the source, beside the file naming it.
    fn beside(&self, name: &str) -> PathBuf {
        let path = Path::new(name);
        if path.is_absolute() {
            return path.to_path_buf();
        }
        let from = self.out.files.get(self.at.0).and_then(|f| f.parent()).unwrap_or(Path::new("."));
        let beside = from.join(path);
        // Else beside the file being assembled, as sjasmplus looks from where it's run.
        if !beside.exists()
            && let Some(main) = self.out.files.first().and_then(|f| f.parent())
            && main.join(path).exists()
        {
            return main.join(path);
        }
        beside
    }

    fn collected(&mut self, done: Collecting) {
        match done.kind {
            CollectKind::Macro { name, params } => {
                if self.last {
                    let (file, line) = done.at;
                    self.out.macros.push(Place { name: name.clone(), written: name.clone(), file, line });
                }
                self.macros.insert(name, Macro { params, body: done.body });
            }
            CollectKind::Repeat { times, counter } => {
                if times > 65536 {
                    self.problem("a REPT can't repeat so many times");
                    return;
                }
                let at = self.at;
                for round in 0..times.max(0) {
                    if let Some((name, first)) = &counter {
                        self.variables.insert(self.full_name(name));
                        let full = self.full_name(name);
                        self.symbols.insert(full, first + round);
                    }
                    for (file, line, text) in &done.body {
                        if self.ended {
                            break;
                        }
                        // Repeated lines count as the line REPT is on in the listing, so the code stays together.
                        let _ = (file, line);
                        self.at = at;
                        self.source_line(at.0, at.1, text);
                    }
                }
                self.at = at;
            }
        }
    }

    fn expand(&mut self, name: &str, found: &Macro, operands: &str) {
        if self.expansions > 100_000 || self.depth > 64 {
            self.problem("macros use each other too deeply");
            return;
        }
        self.expansions += 1;
        let values = split_operands(operands);
        let at = self.at;
        let scope = self.macro_scope.replace(format!("{name}_{}", self.expansions));
        self.depth += 1;
        for (_, _, text) in &found.body {
            if self.ended {
                break;
            }
            let mut line = text.clone();
            if self.isa.colon_separates() {
                // sjasmplus: the arguments are substituted as defines are, parts of identifiers too.
                let map: HashMap<String, String> = found.params.iter().enumerate().map(|(i, p)| (p.to_ascii_lowercase(), values.get(i).map(|v| v.trim().to_string()).unwrap_or_default())).collect();
                line = substitute_words(&line, &map, true);
                for (i, value) in values.iter().enumerate() {
                    line = line.replace(&format!("\\{}", i + 1), value.trim());
                }
            } else {
                for (i, param) in found.params.iter().enumerate() {
                    let value = values.get(i).map(|v| v.trim()).unwrap_or("");
                    line = replace_word(&line, param, value);
                    line = line.replace(&format!("\\{}", i + 1), value);
                }
            }
            self.at = at;
            self.source_line(at.0, at.1, &line);
        }
        self.depth -= 1;
        self.macro_scope = scope;
        self.at = at;
    }
}

impl Scope for Pass<'_> {
    fn value(&self, name: &str) -> Option<i64> {
        // sjasmplus's own: the size of RELOCATE_TABLE.
        match name {
            "relocate_count" => return Some(self.relocation_table.len() as i64),
            "relocate_size" => return Some(2 * self.relocation_table.len() as i64),
            _ => {}
        }
        let found = |name: &str| self.symbols.get(name).or_else(|| self.previous.get(name)).copied();
        found(name)
            .or_else(|| {
                // Inside a MODULE, a name not found in it is looked for outside it.
                let prefix = format!("{}.", self.modules.join("."));
                name.strip_prefix(prefix.as_str()).filter(|_| !self.modules.is_empty()).and_then(found)
            })
            .or_else(|| {
                // A local label used in a macro (passed in, say) that the macro doesn't define is the one outside it.
                let scope = self.macro_scope.as_deref()?;
                let local = name.strip_prefix(scope)?;
                found(&format!("{}{local}", self.global))
            })
            // sjasmplus keeps labels from pass to pass: one defined on an earlier pass is still known.
            .or_else(|| self.ever.get(name).copied())
    }

    fn page_of(&self, name: &str) -> Option<i64> {
        self.pages.get(name).or_else(|| self.previous_pages.get(name)).copied()
    }

    fn physical_here(&self) -> i64 {
        self.pc
    }

    fn physical_of(&self, name: &str) -> Option<i64> {
        self.physical.get(name).copied().or_else(|| self.value(name))
    }

    fn page_here(&self) -> Option<i64> {
        self.device.and_then(|d| self.devices[d].page_at(self.pc & 0xFFFF)).map(|p| p as i64)
    }

    fn peek(&self, address: i64, size: usize) -> Option<i64> {
        let device = &self.devices[self.device?];
        Some((0..size).fold(0i64, |value, i| value | (i64::from(device.peek(address + i as i64)) << (8 * i))))
    }

    fn here(&self) -> i64 {
        self.pc + self.shift
    }

    fn caret_is_power(&self) -> bool {
        self.acme
    }

    fn used(&self, written: &str, full: &str) {
        self.all_used.borrow_mut().insert(full.to_string());
        if !self.last || self.macro_scope.is_some() {
            return;
        }
        let (file, line) = self.at;
        let mut used = self.used.borrow_mut();
        if !used.iter().rev().take(8).any(|u| u.file == file && u.line == line && u.name == full && u.written == written) {
            used.push(Place { name: full.to_string(), written: written.to_string(), file, line });
        }
    }

    fn full_name(&self, name: &str) -> String {
        let locals = self.isa.local_prefixes();
        if !locals.contains(&'@')
            && let Some(name) = name.strip_prefix('@')
        {
            // @.label in a macro is local to the label before the macro, not to the macro.
            if name.starts_with(locals) {
                return format!("{}{name}", self.global);
            }
            return name.to_string();
        }
        if name.starts_with(locals) {
            return match &self.macro_scope {
                Some(scope) => format!("{scope}{name}"),
                None => format!("{}{name}", self.global),
            };
        }
        if !self.modules.is_empty() {
            return format!("{}.{name}", self.modules.join("."));
        }
        name.to_string()
    }
}

/// What an instruction sees of the assembler: values, and where it is.
pub struct Line<'p, 'a> {
    pass: &'p mut Pass<'a>,
}

impl Line<'_, '_> {
    /// An expression's value. On an early pass a label not known yet is 0.
    pub fn eval(&mut self, text: &str) -> Result<i64, String> {
        self.pass.eval(text)
    }

    /// An expression's value if it's known now, without counting it as missing.
    pub fn known(&mut self, text: &str) -> Option<i64> {
        match expr::eval(text, self.pass) {
            Ok(Value::Known(v)) => Some(v),
            _ => None,
        }
    }

    pub fn here(&self) -> i64 {
        self.pass.pc + self.pass.shift
    }

    /// The 6809's direct page, as SETDP set it.
    pub fn direct_page(&self) -> Option<i64> {
        self.pass.direct_page
    }

    /// Whether this is the pass that reports problems (and so values must be in range).
    pub fn checking(&self) -> bool {
        self.pass.last
    }

    /// Says something about the line that doesn't stop the program being made.
    pub fn warn(&mut self, message: impl Into<String>) {
        self.pass.warn(message);
    }

    /// How sjasmplus takes lists of operands: None when the source isn't sjasmplus's, else whether they're split
    /// by ,, (its --syntax=a) rather than ,.
    pub fn sjasm_lists(&self) -> Option<bool> {
        self.pass.sjasm.then_some(self.pass.double_comma)
    }
}

/// Words the assembler itself understands.
pub const DIRECTIVES: &[&str] = &[
    "org", "equ", "defl", "db", "defb", "byte", "dm", "defm", "fcb", "fcc", "text", "ascii", "asc", "dz", "asciz", "dc", "abyt", "dfb", "dw", "defw", "word", "fdb", "addr", "dd", "dword", "long", "defd", "ds", "defs", "block", "rmb", "dsb", "align", "even", "include", "incsrc", "use", "get", "incbin", "binary", "insert", "end", "ent", "entry", "macro", "rept", "dup", "repeat", "endm", "endr", "edup", "endmacro", "endrepeat", "device", "savesna", "savetap", "savenex", "savebin", "savetrd", "save3dos", "emptytap", "output", "display", "message", "echo", "print", "warning", "assert", "error", "fail", "opt", "list", "nolist", "title", "page", "section", "cpu", "setdp", "module", "endmodule", "slot", "mmu", "labelslist", "nam", "ttl", "spc", "disp", "phase", "dephase", "unphase", "by", "wo", "tx", "raw", "pet", "scr", "fill", "res", "source", "src", "to", "segment", "zeropage", "code", "data", "bss", "rodata", "import", "export", "importzp", "exportzp", "proc", "endproc", "scope", "endscope", "setcpu", "p02", "pc02", "incdir", "fcs", "dc.b", "dc.w", "dc.l", "ds.b", "ds.w", "ds.l", "dcb.b", "dcb.w", "dcb.l", "rs.b", "rs.w", "rs.l", "cnop", "xdef", "xref", "set", "zmb", "for", "zone", "endzone", "define", "define+", "undefine", "defarray", "defarray+", "ifused", "ifnused", "struct", "ends", "dg", "dh", "inchob", "sldopt", "cspectmap", "d24", "outend", "savedev", "defg", "defh", "hex", "relocate_start", "relocate_end", "relocate_table",
];

/// The line without its comment: from a ; or // that isn't in quotes. AF' isn't a quote.
pub fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match quote {
            // A backslash in double quotes escapes what follows it, as sjasmplus has \" in strings.
            Some(b'"') if c == b'\\' => i += 1,
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                b';' => return &line[..i],
                b'/' if bytes.get(i + 1) == Some(&b'/') => return &line[..i],
                b'"' | b'\'' if is_quote(bytes, i) => quote = Some(c),
                _ => {}
            },
        }
        i += 1;
    }
    line
}

/// A character as PETSCII, the Commodores' character set: letters swap case.
fn petscii(byte: u8) -> u8 {
    match byte {
        b'a'..=b'z' => byte - 32,
        b'A'..=b'Z' => byte + 128,
        other => other,
    }
}

/// A character as a Commodore screen code.
fn screen_code(byte: u8) -> u8 {
    match byte {
        b'@' => 0,
        b'a'..=b'z' => byte - 96,
        b'A'..=b'Z' => byte - 64 + 64,
        b'[' => 27,
        b']' => 29,
        other => other,
    }
}

/// An operand up to the first space outside quotes and brackets.
fn cut_at_space(text: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut depth = 0i32;
    for (i, c) in text.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '"' => quote = Some(c),
                '\'' if text[i + 1..].contains('\'') => quote = Some(c),
                '(' | '[' => depth += 1,
                ')' | ']' => depth -= 1,
                c if c.is_whitespace() && depth <= 0 => return &text[..i],
                _ => {}
            },
        }
    }
    text
}

/// The text with /* block comments */ taken out, keeping its lines where they were.
fn without_block_comments(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains("/*") {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    // They nest, as sjasmplus has them.
    let mut inside = 0usize;
    for line in text.split_inclusive('\n') {
        let mut rest = line;
        loop {
            if inside > 0 {
                let close = rest.find("*/");
                let open = rest.find("/*");
                match (open, close) {
                    (Some(o), Some(c)) if o < c => {
                        inside += 1;
                        rest = &rest[o + 2..];
                    }
                    (Some(o), None) => {
                        inside += 1;
                        rest = &rest[o + 2..];
                    }
                    (_, Some(at)) => {
                        inside -= 1;
                        rest = &rest[at + 2..];
                    }
                    (None, None) => {
                        if line.ends_with('\n') {
                            out.push('\n');
                        }
                        break;
                    }
                }
            } else {
                // Only outside a line comment and quotes.
                let code = strip_comment(rest);
                match code.find("/*") {
                    Some(at) if !code[..at].ends_with('/') => {
                        out.push_str(&rest[..at]);
                        inside = 1;
                        rest = &rest[at + 2..];
                    }
                    _ => {
                        out.push_str(rest);
                        break;
                    }
                }
            }
        }
    }
    std::borrow::Cow::Owned(out)
}

/// The first two words of a line, in lower case and without dots, to see if it opens or closes a block.
fn first_words(line: &str) -> Vec<String> {
    line.split_whitespace().take(2).map(|w| w.trim_end_matches(':').trim_start_matches('.').to_ascii_lowercase()).collect()
}

/// A label at the start of a line, and what follows it.
fn split_label(line: &str, at_start: bool, is_keyword: impl Fn(&str) -> bool, is_instruction: impl Fn(&str) -> bool) -> (Option<String>, &str) {
    let trimmed = line.trim_start();
    let length = trimmed.bytes().take_while(|c| expr::is_name_char(*c)).count();
    if length == 0 {
        return (None, line);
    }
    let name = &trimmed[..length];
    let after = &trimmed[length..];
    if let Some(rest) = after.strip_prefix(':') {
        // name: or name:: (a global label in some assemblers).
        return (Some(name.to_string()), rest.strip_prefix(':').unwrap_or(rest));
    }
    if !expr::is_name_start(name.as_bytes()[0]) {
        return (None, line);
    }
    // name EQU 5 or name = 5, wherever it starts.
    let (next, _) = split_word(after);
    let next = next.to_ascii_lowercase();
    if matches!(next.trim_start_matches('.'), "equ" | "defl" | "macro") || after.trim_start().starts_with(":=") || after.trim_start().starts_with('=') && !after.trim_start().starts_with("==") {
        return (Some(name.to_string()), after);
    }
    if at_start && !is_keyword(name) {
        return (Some(name.to_string()), after);
    }
    // A directive's name alone at the start of a line is a label (PRINT, as a routine is often called); an
    // instruction alone there (RET, NOP) is still the instruction.
    let next_is_keyword = split_word(after).0.split(':').next().is_some_and(|w| !w.is_empty() && is_keyword(w));
    // So is any word there with an instruction or directive after it (SL1 LD SP,0: SL1 is a label).
    if at_start && (next_is_keyword || after.trim().is_empty() && !is_instruction(name)) {
        return (Some(name.to_string()), after);
    }
    (None, line)
}

/// The first word, and the rest.
fn split_word(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    match text.find(char::is_whitespace) {
        Some(at) => (&text[..at], &text[at..]),
        None => (text, ""),
    }
}

/// Splits on a character outside quotes and brackets.
fn split_outside_quotes(text: &str, separator: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut depth = 0i32;
    let chars: Vec<char> = text.chars().collect();
    let ascii: Vec<u8> = chars.iter().map(|c| if c.is_ascii() { *c as u8 } else { b'x' }).collect();
    let mut escaped = false;
    for (i, &c) in chars.iter().enumerate() {
        match quote {
            _ if escaped => escaped = false,
            Some('"') if c == '\\' => escaped = true,
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '"' | '\'' if is_quote(&ascii, i) => quote = Some(c),
                '(' | '[' => depth += 1,
                ')' | ']' => depth -= 1,
                // ca65's :- and :+ aren't separators.
                c if c == separator && depth == 0 && !(c == ':' && matches!(chars.get(i + 1), Some('-' | '+'))) => {
                    parts.push(std::mem::take(&mut current));
                    continue;
                }
                _ => {}
            },
        }
        current.push(c);
    }
    parts.push(current);
    parts
}

/// Operands split at commas outside quotes and brackets, each trimmed; none for an empty line.
pub fn split_operands(text: &str) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }
    split_outside_quotes(text, ',').into_iter().map(|p| p.trim().to_string()).collect()
}

/// A quoted string's bytes, if the item is one (longer than one character, or in double quotes).
fn string_value(item: &str) -> Option<Vec<u8>> {
    let item = item.trim();
    let quote = item.chars().next()?;
    if !(quote == '"' || quote == '\'') || item.len() < 2 || !item.ends_with(quote) {
        return None;
    }
    let inner = &item[1..item.len() - 1];
    // A quote inside ends the string early, unless it's escaped in double quotes.
    let mut escaped = false;
    for c in inner.chars() {
        if escaped {
            escaped = false;
        } else if c == '\\' && quote == '"' {
            escaped = true;
        } else if c == quote {
            return None;
        }
    }
    if escaped {
        return None;
    }
    if quote == '\'' && inner.len() == 1 {
        return None;
    }
    // Escapes as sjasmplus has them in double quotes.
    let mut bytes = Vec::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' && quote == '"' {
            match chars.next() {
                Some('n') => bytes.push(10),
                Some('r') => bytes.push(13),
                Some('t') => bytes.push(9),
                Some('0') => bytes.push(0),
                Some('\\') => bytes.push(b'\\'),
                Some('"') => bytes.push(b'"'),
                Some('\'') => bytes.push(b'\''),
                Some('a') => bytes.push(7),
                Some('b') => bytes.push(8),
                Some('e') => bytes.push(27),
                Some('f') => bytes.push(12),
                Some('v') => bytes.push(11),
                Some('?') => bytes.push(b'?'),
                Some(other) => {
                    bytes.push(b'\\');
                    bytes.push(other as u8);
                }
                None => bytes.push(b'\\'),
            }
        } else {
            let mut buffer = [0u8; 4];
            bytes.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
        }
    }
    Some(bytes)
}

fn unquote(text: &str) -> String {
    let text = text.trim();
    for (open, close) in [('"', '"'), ('\'', '\''), ('<', '>')] {
        if text.len() >= 2 && text.starts_with(open) && text.ends_with(close) {
            return text[1..text.len() - 1].to_string();
        }
    }
    text.to_string()
}

/// Replaces a whole word, not part of a longer name.
/// sjasmplus's substitution of DEFINEs and macro arguments into a line, outside quotes: each identifier is matched
/// whole, or, failing that, by its parts between underscores (an argument starting with _ only at an identifier's
/// start), as sjasmplus's sub-word matching does. Keys are looked up lowercase when `fold` says so.
fn substitute_words(line: &str, map: &HashMap<String, String>, fold: bool) -> String {
    if map.is_empty() {
        return line.to_string();
    }
    let key = |text: &str| if fold { text.to_ascii_lowercase() } else { text.to_string() };
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    let mut quote: Option<u8> = None;
    let is_word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'?';
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            out.push(c as char);
            if c == b'\\' && q == b'"' && i + 1 < bytes.len() {
                out.push(bytes[i + 1] as char);
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if is_quote(bytes, i) {
            quote = Some(c);
            out.push(c as char);
            i += 1;
            continue;
        }
        let starts = (c.is_ascii_alphabetic() || c == b'_') && (i == 0 || !(is_word(bytes[i - 1]) || matches!(bytes[i - 1], b'$' | b'#' | b'%')));
        if !starts {
            // Keep any other character whole, multi-byte ones too.
            let ch = line[i..].chars().next().expect("not at the end");
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let start = i;
        while i < bytes.len() && is_word(bytes[i]) {
            i += 1;
        }
        let word = &line[start..i];
        if let Some(value) = map.get(&key(word)) {
            out.push_str(value);
            continue;
        }
        // Parts between underscores, tried longest first from each.
        let parts: Vec<&str> = word.split('_').collect();
        let mut at = 0;
        let mut pieces: Vec<String> = Vec::new();
        while at < parts.len() {
            let mut matched = None;
            for end in (at..parts.len()).rev() {
                let candidate = parts[at..=end].join("_");
                if candidate.is_empty() || (at > 0 && candidate.starts_with('_')) {
                    continue;
                }
                if let Some(value) = map.get(&key(&candidate)) {
                    matched = Some((end, value.clone()));
                    break;
                }
            }
            match matched {
                Some((end, value)) => {
                    pieces.push(value);
                    at = end + 1;
                }
                None => {
                    pieces.push(parts[at].to_string());
                    at += 1;
                }
            }
        }
        out.push_str(&pieces.join("_"));
    }
    out
}

/// A self-modifying-code label at the start of a line: name+N: or name+*:, its offset (None for *) and the rest.
fn smc_label(line: &str) -> Option<(String, Option<i64>, &str)> {
    let trimmed = line.trim_start();
    let name_length = trimmed.bytes().take_while(|c| expr::is_name_char(*c)).count();
    let name = &trimmed[..name_length];
    let rest = trimmed[name_length..].strip_prefix('+')?;
    let (offset, after) = match rest.as_bytes() {
        [b'*', b':', ..] => (None, &rest[2..]),
        [d @ b'0'..=b'9', b':', ..] => (Some(i64::from(d - b'0')), &rest[2..]),
        _ => return None,
    };
    (!name.is_empty() && expr::is_name_start(name.as_bytes()[0])).then(|| (name.to_string(), offset, after))
}

/// Where in an instruction sjasmplus's smart SMC label points: at its last immediate value, a word or a byte.
fn smart_offset(statement: &str, length: i64) -> i64 {
    let (op, operands) = split_word(statement);
    let op = op.to_ascii_lowercase();
    let items: Vec<String> = split_operands(operands).iter().map(|i| i.trim().to_ascii_lowercase()).collect();
    let pair = |t: &str| matches!(t, "bc" | "de" | "hl" | "sp" | "ix" | "iy");
    let memory = |t: &str| t.starts_with('(') && !matches!(t, "(hl)" | "(bc)" | "(de)" | "(sp)" | "(c)") && !t.starts_with("(ix") && !t.starts_with("(iy");
    let word = match op.as_str() {
        "jp" | "call" => items.last().is_some_and(|t| !t.starts_with('(')),
        "ld" => items.iter().any(|t| memory(t)) || (items.first().is_some_and(|t| pair(t)) && items.len() == 2),
        // The Next's ADD HL,nn (and DE, BC) take a word.
        "add" => items.len() == 2 && items.first().is_some_and(|t| pair(t)) && !pair(&items[1]) && items[1] != "a",
        "push" => true,
        _ => false,
    };
    length - if word { 2 } else { 1 }
}

/// Whether a quote at this place starts a string or character: not AF', and not a ' between digits, which
/// sjasmplus allows to group them (%0'001'0000).
fn is_quote(bytes: &[u8], i: usize) -> bool {
    match bytes[i] {
        b'"' => true,
        b'\'' => {
            let after_af = i >= 2 && bytes[i - 2..i].eq_ignore_ascii_case(b"af");
            let separator = i >= 1 && bytes[i - 1].is_ascii_alphanumeric() && bytes.get(i + 1).is_some_and(u8::is_ascii_alphanumeric) && number_before(bytes, i);
            !after_af && !separator
        }
        _ => false,
    }
}

/// Whether what comes just before a place is a number (so a ' after it separates its digits).
fn number_before(bytes: &[u8], i: usize) -> bool {
    let mut start = i;
    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'\'') {
        start -= 1;
    }
    let first = bytes[start];
    first.is_ascii_digit() || (start > 0 && matches!(bytes[start - 1], b'$' | b'%' | b'#'))
}

fn replace_word(line: &str, word: &str, with: &str) -> String {
    if word.is_empty() {
        return line.to_string();
    }
    let mut out = String::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < line.len() {
        let rest = &line[i..];
        let before_ok = i == 0 || !expr::is_name_char(bytes[i - 1]);
        if before_ok && rest.len() >= word.len() && rest.is_char_boundary(word.len()) && rest[..word.len()].eq_ignore_ascii_case(word) && !rest.as_bytes().get(word.len()).is_some_and(|c| expr::is_name_char(*c)) {
            out.push_str(with);
            i += word.len();
            continue;
        }
        let c = rest.chars().next().expect("not at the end");
        out.push(c);
        i += c.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests;
