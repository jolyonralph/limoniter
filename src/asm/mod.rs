//! Limoniter's own assemblers. The part every processor shares is here: reading the source a line at a time
//! (labels, local labels, comments, several statements to a line), includes, macros, repeats and conditions, the
//! data directives, and passes until every label has settled. Each processor (`Isa`) only turns one instruction
//! into its bytes. The syntax follows what's usual for each machine, sjasmplus's for the Z80 above all.

pub mod expr;
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
    let mut previous: HashMap<String, i64> = HashMap::new();
    for _ in 0..12 {
        let mut state = Pass::new(isa, read, previous.clone(), false);
        state.run_file(path, text);
        let settled = state.symbols == previous && !state.unknown;
        previous = state.symbols;
        if settled {
            break;
        }
    }
    // The last pass, saying what's wrong.
    let mut state = Pass::new(isa, read, previous, true);
    state.run_file(path, text);
    state.finish()
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
    Repeat { times: i64 },
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
    /// Where the line being assembled is: its file and line, or the line a macro was used on.
    at: (usize, usize),
    depth: usize,
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
            global: String::new(),
            macro_scope: None,
            macros: HashMap::new(),
            expansions: 0,
            conditions: Vec::new(),
            collecting: None,
            ended: false,
            out: Assembly::default(),
            at: (0, 0),
            depth: 0,
        }
    }

    fn finish(mut self) -> Assembly {
        if let Some(collecting) = &self.collecting {
            let what = if matches!(collecting.kind, CollectKind::Macro { .. }) { "MACRO has no ENDM" } else { "REPT has no ENDR" };
            self.out.problems.push(Problem { file: collecting.at.0, line: collecting.at.1, message: what.into() });
        }
        if !self.conditions.is_empty() {
            let (file, line) = self.at;
            self.out.problems.push(Problem { file, line, message: "an IF has no ENDIF".into() });
        }
        self.out.symbols = self.symbols.into_iter().collect();
        self.out
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
        for (number, line) in text.lines().enumerate() {
            if self.ended {
                break;
            }
            self.at = (file, number);
            self.source_line(file, number, line);
        }
        self.depth -= 1;
    }

    fn active(&self) -> bool {
        self.conditions.iter().all(|(on, _)| *on)
    }

    /// One line of source, as written.
    fn source_line(&mut self, file: usize, number: usize, raw: &str) {
        let line = strip_comment(raw);
        // Lines kept for a macro or a repeat.
        if let Some(collecting) = &mut self.collecting {
            let word = first_words(line);
            let opens = word.iter().any(|w| matches!(w.as_str(), "macro" | "rept" | "dup" | "repeat"));
            let closes = word.iter().any(|w| matches!(w.as_str(), "endm" | "endr" | "edup" | "endmacro" | "endrepeat"));
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
            return;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }
        // 68000 and 6809 sources use a * in the first column for a comment line.
        if raw.starts_with('*') {
            return;
        }
        let at_start = !line.starts_with([' ', '\t']);
        let (label, rest) = split_label(line, at_start, |w| self.is_keyword(w), |w| self.isa.is_instruction(&w.to_ascii_lowercase()));
        let rest = rest.trim();
        let word = rest.split(|c: char| c.is_whitespace()).next().unwrap_or("").to_ascii_lowercase();
        let word = word.strip_prefix('.').unwrap_or(&word).to_string();
        // Conditions are followed even when not assembling, to find their ends.
        if matches!(word.as_str(), "if" | "ifdef" | "ifndef" | "ifn" | "else" | "elseif" | "endif" | "endc" | "ifeq" | "ifne") {
            self.condition(&word, rest[rest.find(char::is_whitespace).unwrap_or(rest.len())..].trim());
            return;
        }
        if !self.active() {
            return;
        }
        // A label given a value.
        if let Some(label) = &label {
            let (op, value) = split_word(rest);
            match op.to_ascii_lowercase().trim_start_matches('.') {
                "equ" | "=" | "defl" => {
                    let variable = !op.eq_ignore_ascii_case("equ");
                    match self.eval(value) {
                        Ok(v) => self.define(label, v, variable),
                        Err(problem) => self.problem(problem),
                    }
                    return;
                }
                "macro" => {
                    let params = split_operands(value).into_iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
                    self.collecting = Some(Collecting { kind: CollectKind::Macro { name: label.to_ascii_lowercase(), params }, depth: 0, body: Vec::new(), at: (file, number) });
                    return;
                }
                _ => {}
            }
            if let Some(value) = rest.strip_prefix('=') {
                match self.eval(value.trim()) {
                    Ok(v) => self.define(label, v, true),
                    Err(problem) => self.problem(problem),
                }
                return;
            }
            let here = self.pc + self.shift;
            self.define(label, here, false);
        }
        if rest.is_empty() {
            return;
        }
        for statement in self.statements(rest) {
            if self.ended || self.collecting.is_some() {
                break;
            }
            self.statement(statement.trim());
        }
    }

    fn statements(&self, rest: &str) -> Vec<String> {
        if self.isa.colon_separates() { split_outside_quotes(rest, ':') } else { vec![rest.to_string()] }
    }

    fn is_keyword(&self, word: &str) -> bool {
        let lower = word.to_ascii_lowercase();
        let lower = lower.strip_prefix('.').unwrap_or(&lower);
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
            "ifdef" | "ifndef" => {
                let name = self.full_name(operand.trim());
                let known = self.symbols.contains_key(&name) || self.macros.contains_key(&operand.trim().to_ascii_lowercase());
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
        if !label.starts_with('.') && !variable && self.macro_scope.is_none() {
            self.global = name.clone();
        }
        if !variable && self.symbols.contains_key(&name) {
            self.problem(format!("{label} is defined twice"));
            return;
        }
        if variable {
            self.variables.insert(name.clone());
        }
        self.symbols.insert(name, value);
    }

    fn eval(&mut self, text: &str) -> Result<i64, String> {
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

    fn emit(&mut self, bytes: Vec<u8>) {
        let address = self.pc;
        self.pc += bytes.len() as i64;
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
        let (op, operands) = split_word(text);
        let lower = op.to_ascii_lowercase();
        let directive = lower.strip_prefix('.').unwrap_or(&lower).to_string();
        if let Some(found) = self.macros.get(&lower).cloned() {
            self.expand(&lower, &found, operands);
            return;
        }
        if DIRECTIVES.contains(&directive.as_str()) && !(self.isa.is_instruction(&lower) && !lower.starts_with('.')) {
            self.directive(&directive, operands);
            return;
        }
        if !self.isa.is_instruction(&lower) {
            self.problem(format!("{op} isn't a {} instruction or a directive", self.isa.name()));
            return;
        }
        let isa = self.isa;
        let mut line = Line { pass: self };
        match isa.encode(&lower, operands.trim(), &mut line) {
            Ok(bytes) => self.emit(bytes),
            Err(problem) => self.problem(problem),
        }
    }

    fn directive(&mut self, directive: &str, operands: &str) {
        let operands = operands.trim();
        match directive {
            "org" => match self.eval(split_operands(operands).first().map(String::as_str).unwrap_or("")) {
                Ok(v) => self.pc = v,
                Err(problem) => self.problem(problem),
            },
            "db" | "defb" | "byte" | "dm" | "defm" | "fcb" | "fcc" | "text" | "ascii" | "asc" | "dz" | "asciz" | "dc" | "abyt" | "dfb" => {
                let mut bytes = Vec::new();
                for item in split_operands(operands) {
                    match string_value(&item) {
                        Some(text) => bytes.extend(text),
                        None => match self.eval(&item) {
                            Ok(v) => {
                                if self.last && !(-128..=255).contains(&v) {
                                    self.problem(format!("{item} is {v}, too big for a byte"));
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
            "dw" | "defw" | "word" | "fdb" | "addr" | "dd" | "dword" | "long" | "defd" => {
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
            "include" | "incsrc" | "use" | "get" => {
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
                self.collecting = Some(Collecting { kind: CollectKind::Repeat { times }, depth: 0, body: Vec::new(), at: self.at });
            }
            "endm" | "endr" | "edup" | "endmacro" | "endrepeat" => self.problem(format!("{} without its start", directive.to_ascii_uppercase())),
            "device" => self.out.device = Some(operands.split(',').next().unwrap_or("").trim().to_ascii_uppercase()),
            "savesna" | "savetap" | "savenex" | "savebin" | "savetrd" | "save3dos" | "emptytap" | "output" => {
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
                    for item in split_operands(operands) {
                        match string_value(&item) {
                            Some(text) => words.push_str(&String::from_utf8_lossy(&text)),
                            None => match self.eval(&item) {
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
            // Directives that change only listings and the like.
            _ => {}
        }
    }

    /// A file named in the source, beside the file naming it.
    fn beside(&self, name: &str) -> PathBuf {
        let path = Path::new(name);
        if path.is_absolute() {
            return path.to_path_buf();
        }
        let from = self.out.files.get(self.at.0).and_then(|f| f.parent()).unwrap_or(Path::new("."));
        from.join(path)
    }

    fn collected(&mut self, done: Collecting) {
        match done.kind {
            CollectKind::Macro { name, params } => {
                self.macros.insert(name, Macro { params, body: done.body });
            }
            CollectKind::Repeat { times } => {
                if times > 65536 {
                    self.problem("a REPT can't repeat so many times");
                    return;
                }
                let at = self.at;
                for _ in 0..times.max(0) {
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
            for (i, param) in found.params.iter().enumerate() {
                let value = values.get(i).map(|v| v.trim()).unwrap_or("");
                line = replace_word(&line, param, value);
                line = line.replace(&format!("\\{}", i + 1), value);
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
        self.symbols.get(name).or_else(|| self.previous.get(name)).copied()
    }

    fn here(&self) -> i64 {
        self.pc + self.shift
    }

    fn full_name(&self, name: &str) -> String {
        if let Some(name) = name.strip_prefix('@') {
            return name.to_string();
        }
        if name.starts_with('.') {
            return match &self.macro_scope {
                Some(scope) => format!("{scope}{name}"),
                None => format!("{}{name}", self.global),
            };
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
    #[allow(dead_code)]
    pub fn known(&mut self, text: &str) -> Option<i64> {
        match expr::eval(text, self.pass) {
            Ok(Value::Known(v)) => Some(v),
            _ => None,
        }
    }

    pub fn here(&self) -> i64 {
        self.pass.pc + self.pass.shift
    }

    /// Whether this is the pass that reports problems (and so values must be in range).
    pub fn checking(&self) -> bool {
        self.pass.last
    }
}

/// Words the assembler itself understands.
const DIRECTIVES: [&str; 90] = [
    "org", "equ", "defl", "db", "defb", "byte", "dm", "defm", "fcb", "fcc", "text", "ascii", "asc", "dz", "asciz", "dc", "abyt", "dfb", "dw", "defw", "word", "fdb", "addr", "dd", "dword", "long", "defd", "ds", "defs", "block", "rmb", "dsb", "align", "even", "include", "incsrc", "use", "get", "incbin", "binary", "insert", "end", "ent", "entry", "macro", "rept", "dup", "repeat", "endm", "endr", "edup", "endmacro", "endrepeat", "device", "savesna", "savetap", "savenex", "savebin", "savetrd", "save3dos", "emptytap", "output", "display", "message", "echo", "print", "warning", "assert", "error", "fail", "opt", "list", "nolist", "title", "page", "section", "cpu", "setdp", "module", "endmodule", "slot", "mmu", "labelslist", "nam", "ttl", "spc", "disp", "phase", "dephase", "unphase",
];

/// The line without its comment: from a ; or // that isn't in quotes. AF' isn't a quote.
pub fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                b';' => return &line[..i],
                b'/' if bytes.get(i + 1) == Some(&b'/') => return &line[..i],
                b'\'' if i >= 2 && bytes[i - 2..i].eq_ignore_ascii_case(b"af") => {}
                b'"' | b'\'' => quote = Some(c),
                _ => {}
            },
        }
        i += 1;
    }
    line
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
    if matches!(next.trim_start_matches('.'), "equ" | "defl" | "macro") || after.trim_start().starts_with('=') && !after.trim_start().starts_with("==") {
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
    for (i, &c) in chars.iter().enumerate() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '\'' if i >= 2 && chars[i - 2].eq_ignore_ascii_case(&'a') && chars[i - 1].eq_ignore_ascii_case(&'f') => {}
                '"' | '\'' => quote = Some(c),
                '(' | '[' => depth += 1,
                ')' | ']' => depth -= 1,
                c if c == separator && depth == 0 => {
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
    if inner.contains(quote) {
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
