//! What 68000 source for the Amiga does with the custom chips, for hover and the copper list view: which address
//! registers are pointed at the chips ($DFF000) or a CIA, where the copper lists are (runs of `dc.w` word pairs,
//! or CMOVE, CWAIT, CSKIP and CEND), and what a line writes to which register.
//!
//! It's read from the text, with the values of labels and EQUs from Limoniter's own assembler when it has them.

use crate::amiga_hw::{self, CIAA, CIAB, CUSTOM, Copper, CopperState, Register};
use crate::asm::Assembly;
use std::collections::HashMap;

/// One copper instruction in the source: the line it's on and its two words (the second unknown when it's an
/// expression only the running program knows).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub line: usize,
    pub first: u16,
    pub second: Option<u16>,
}

impl Entry {
    pub fn op(&self) -> Copper {
        Copper::decode(self.first, self.second.unwrap_or(0))
    }
}

/// A copper list: a run of lines of copper instructions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopperList {
    /// The label it starts at, if it has one.
    pub label: Option<String>,
    pub first_line: usize,
    pub last_line: usize,
    pub entries: Vec<Entry>,
}

/// One instruction of a list as the view shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub line: usize,
    pub words: String,
    pub said: String,
    /// For a colour MOVE: the colour's number and how it looks.
    pub colour: Option<(u16, [u8; 3])>,
}

impl CopperList {
    pub fn name(&self) -> String {
        match &self.label {
            Some(label) => format!("{label} (line {})", self.first_line + 1),
            None => format!("Line {}", self.first_line + 1),
        }
    }

    /// Each instruction decoded in order, BPLCON3 and waits past line 255 carried along.
    pub fn rows(&self) -> Vec<Row> {
        let mut state = CopperState::default();
        self.entries
            .iter()
            .map(|e| {
                let op = e.op();
                let colour = state.colour_of(op).filter(|_| e.second.is_some());
                let mut said = state.step(op);
                if e.second.is_none() {
                    said = unknown_value(op, said);
                }
                let second = e.second.map(|v| format!("${v:04X}")).unwrap_or_else(|| "?".into());
                Row { line: e.line, words: format!("${:04X},{second}", e.first), said, colour }
            })
            .collect()
    }
}

/// A MOVE whose value isn't known until the program runs.
fn unknown_value(op: Copper, said: String) -> String {
    match op {
        Copper::Move { register, .. } => {
            let name = amiga_hw::custom(register as u32).map(|r| r.name.clone()).unwrap_or_else(|| format!("${register:03X}"));
            format!("MOVE (set when the program runs) to {name}")
        }
        _ => said,
    }
}

/// What a source does with the hardware.
#[derive(Clone, Debug, Default)]
pub struct Source {
    /// Whether it looks like Amiga code at all, so register names aren't explained in someone else's.
    pub amiga: bool,
    /// Each time an address register is loaded: the line, the register (0-7), and the address put there if known.
    loads: Vec<(usize, u8, Option<u32>)>,
    pub lists: Vec<CopperList>,
    symbols: HashMap<String, i64>,
    lower_symbols: HashMap<String, i64>,
}

/// A line split into its parts, as character spans.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Parts {
    pub label: Option<String>,
    pub mnemonic: Option<(usize, usize)>,
    pub operands: Vec<(usize, usize)>,
}

pub(crate) fn split(line: &str) -> Parts {
    let chars: Vec<char> = line.chars().collect();
    let mut parts = Parts::default();
    let mut i = 0;
    if chars.first().is_some_and(|c| *c == '*' || *c == ';') {
        return parts;
    }
    if chars.first().is_some_and(|c| !c.is_whitespace()) {
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() && chars[i] != ':' && chars[i] != ';' {
            i += 1;
        }
        parts.label = Some(chars[start..i].iter().collect());
        if chars.get(i) == Some(&':') {
            i += 1;
        }
    }
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if i >= chars.len() || chars[i] == ';' {
        return parts;
    }
    let start = i;
    while i < chars.len() && !chars[i].is_whitespace() && chars[i] != ';' {
        i += 1;
    }
    parts.mnemonic = Some((start, i));
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    // Operands, split at commas outside brackets and quotes; a space ends them (what follows is a comment) unless
    // it's beside a comma.
    let mut start = i;
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    while i < chars.len() {
        let c = chars[i];
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '"' | '\'' => quote = Some(c),
                '(' => depth += 1,
                ')' => depth -= 1,
                ';' => break,
                ',' if depth == 0 => {
                    parts.operands.push((start, i));
                    start = i + 1;
                }
                c if c.is_whitespace() => {
                    let before = chars[..i].iter().rev().find(|c| !c.is_whitespace());
                    let after = chars[i..].iter().find(|c| !c.is_whitespace());
                    if before != Some(&',') && after != Some(&',') && depth == 0 {
                        break;
                    }
                }
                _ => {}
            },
        }
        i += 1;
    }
    if start < i {
        parts.operands.push((start, i));
    }
    // Trimmed.
    for span in &mut parts.operands {
        while span.0 < span.1 && chars[span.0].is_whitespace() {
            span.0 += 1;
        }
        while span.1 > span.0 && chars[span.1 - 1].is_whitespace() {
            span.1 -= 1;
        }
    }
    parts
}

pub(crate) fn text_of(line: &str, span: (usize, usize)) -> String {
    line.chars().skip(span.0).take(span.1 - span.0).collect()
}

// ---------- Expressions ----------

struct Eval<'a> {
    chars: Vec<char>,
    at: usize,
    lookup: &'a dyn Fn(&str) -> Option<i64>,
}

impl Eval<'_> {
    fn space(&mut self) {
        while self.chars.get(self.at).is_some_and(|c| c.is_whitespace()) {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.space();
        self.chars.get(self.at).copied()
    }

    fn eat(&mut self, s: &str) -> bool {
        self.space();
        let want: Vec<char> = s.chars().collect();
        if self.chars[self.at..].starts_with(&want) {
            self.at += want.len();
            true
        } else {
            false
        }
    }

    fn or(&mut self) -> Option<i64> {
        let mut v = self.and()?;
        loop {
            if self.eat("|") || self.eat("!") {
                v |= self.and()?;
            } else if self.eat("^") {
                v ^= self.and()?;
            } else {
                return Some(v);
            }
        }
    }

    fn and(&mut self) -> Option<i64> {
        let mut v = self.shift()?;
        while self.eat("&") {
            v &= self.shift()?;
        }
        Some(v)
    }

    fn shift(&mut self) -> Option<i64> {
        let mut v = self.sum()?;
        loop {
            if self.eat("<<") {
                v = v.checked_shl(self.sum()? as u32)?;
            } else if self.eat(">>") {
                v = v.checked_shr(self.sum()? as u32)?;
            } else {
                return Some(v);
            }
        }
    }

    fn sum(&mut self) -> Option<i64> {
        let mut v = self.product()?;
        loop {
            if self.eat("+") {
                v = v.wrapping_add(self.product()?);
            } else if self.eat("-") {
                v = v.wrapping_sub(self.product()?);
            } else {
                return Some(v);
            }
        }
    }

    fn product(&mut self) -> Option<i64> {
        let mut v = self.unary()?;
        loop {
            if self.eat("*") {
                v = v.wrapping_mul(self.unary()?);
            } else if self.eat("/") {
                v = v.checked_div(self.unary()?)?;
            } else {
                return Some(v);
            }
        }
    }

    fn unary(&mut self) -> Option<i64> {
        if self.eat("-") {
            return Some(self.unary()?.wrapping_neg());
        }
        if self.eat("~") {
            return Some(!self.unary()?);
        }
        if self.eat("+") {
            return self.unary();
        }
        if self.eat("(") {
            let v = self.or()?;
            return self.eat(")").then_some(v);
        }
        let c = self.peek()?;
        let digits = |e: &mut Self, radix: u32| -> Option<i64> {
            let start = e.at;
            while e.chars.get(e.at).is_some_and(|c| c.is_digit(radix) || *c == '_') {
                e.at += 1;
            }
            let s: String = e.chars[start..e.at].iter().filter(|c| **c != '_').collect();
            i64::from_str_radix(&s, radix).ok()
        };
        match c {
            '$' => {
                self.at += 1;
                digits(self, 16)
            }
            '%' => {
                self.at += 1;
                digits(self, 2)
            }
            '@' => {
                self.at += 1;
                digits(self, 8)
            }
            '0' if self.chars.get(self.at + 1).is_some_and(|c| *c == 'x' || *c == 'X') => {
                self.at += 2;
                digits(self, 16)
            }
            c if c.is_ascii_digit() => digits(self, 10),
            '\'' | '"' => {
                self.at += 1;
                let mut v = 0i64;
                while let Some(&ch) = self.chars.get(self.at) {
                    self.at += 1;
                    if ch == c {
                        return Some(v);
                    }
                    v = (v << 8) | (ch as i64 & 0xFF);
                }
                None
            }
            c if c.is_alphabetic() || c == '_' || c == '.' => {
                let start = self.at;
                while self.chars.get(self.at).is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '@' | '\\')) {
                    self.at += 1;
                }
                let name: String = self.chars[start..self.at].iter().collect();
                (self.lookup)(&name)
            }
            _ => None,
        }
    }
}

/// The value of an expression, its names found with `lookup`.
pub fn evaluate(expr: &str, lookup: &dyn Fn(&str) -> Option<i64>) -> Option<i64> {
    let mut e = Eval { chars: expr.chars().collect(), at: 0, lookup };
    let v = e.or()?;
    e.space();
    (e.at == e.chars.len()).then_some(v)
}

/// The names in an expression.
fn names_in(expr: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = String::new();
    let mut in_number = false;
    for c in expr.chars().chain([' ']) {
        if c.is_alphanumeric() || c == '_' || c == '.' {
            if current.is_empty() && (c.is_ascii_digit() || in_number) {
                in_number = true;
                continue;
            }
            current.push(c);
        } else {
            in_number = c == '$' || c == '%';
            if !current.is_empty() {
                names.push(std::mem::take(&mut current));
            }
        }
    }
    names
}

/// Where an operand is: a custom register (its offset, which may be a byte's), a CIA register, or a CIA register
/// whose CIA isn't known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Custom(u32),
    Cia(u32),
    AnyCia(u32),
}

fn address_register(s: &str) -> Option<u8> {
    let s = s.trim().to_ascii_lowercase();
    match s.as_str() {
        "sp" => Some(7),
        _ => s.strip_prefix('a').and_then(|n| n.parse::<u8>().ok()).filter(|n| *n < 8),
    }
}

/// An operand that's a 68000 address register, ignoring case.
fn strip_size(s: &str) -> &str {
    let lower = s.to_ascii_lowercase();
    if lower.ends_with(".w") || lower.ends_with(".l") || lower.ends_with(".b") { &s[..s.len() - 2] } else { s }
}

impl Source {
    /// Reads a document's text, with what the assembler made of it if there is that.
    pub fn scan(text: &str, assembly: Option<&Assembly>) -> Source {
        let mut source = Source::default();
        if let Some(assembly) = assembly {
            source.symbols = assembly.symbols.iter().map(|(k, v)| (k.clone(), *v)).collect();
            source.lower_symbols = assembly.symbols.iter().map(|(k, v)| (k.to_ascii_lowercase(), *v)).collect();
        }
        let lower = text.to_ascii_lowercase();
        let hints = ["$dff", "$bfe", "$bfd", "hardware/", "_custom", "dmacon", "bplcon0", "cop1lc", "intena", "_lvo", "copjmp1", "intreq"];
        source.amiga = hints.iter().any(|h| lower.contains(h));
        // The bytes each line of the document became, unless the assembler had trouble with it.
        let mut bytes: HashMap<usize, &[u8]> = HashMap::new();
        if let Some(assembly) = assembly {
            for placed in assembly.lines.iter().filter(|p| p.file == 0) {
                bytes.insert(placed.line, &placed.bytes);
            }
            for problem in assembly.problems.iter().filter(|p| p.file == 0) {
                bytes.remove(&problem.line);
            }
        }
        let mut run: Option<CopperList> = None;
        let mut pending_label: Option<String> = None;
        for (n, line) in text.lines().enumerate() {
            let parts = split(line);
            if let Some(load) = source.load(line, &parts) {
                source.loads.push((n, load.0, load.1));
            }
            let words = source.copper_words(line, &parts, bytes.get(&n).copied());
            match words {
                Some(words) => {
                    let list = run.get_or_insert_with(|| CopperList { label: parts.label.clone().or(pending_label.take()), first_line: n, last_line: n, entries: Vec::new() });
                    list.last_line = n;
                    for [first, second] in words.as_chunks::<2>().0 {
                        list.entries.push(Entry { line: n, first: first.unwrap_or(0), second: *second });
                    }
                    // The end of a list ends the run: what follows is something else.
                    if list.entries.last().is_some_and(|e| e.op() == Copper::End)
                        && let Some(list) = run.take()
                    {
                        source.keep(list);
                    }
                }
                None if parts.mnemonic.is_none() => {
                    if let Some(label) = parts.label
                        && run.is_none()
                    {
                        pending_label = Some(label);
                    }
                }
                None => {
                    if let Some(list) = run.take() {
                        source.keep(list);
                    }
                    pending_label = None;
                }
            }
        }
        if let Some(list) = run.take() {
            source.keep(list);
        }
        source.amiga |= !source.lists.is_empty();
        source
    }

    /// Keeps a run of copper-like lines if it looks like a copper list: it ends one, it waits as a copper list
    /// does, its label says copper, or it writes at least two registers the copper usually writes.
    fn keep(&mut self, list: CopperList) {
        let ops: Vec<Copper> = list.entries.iter().map(|e| e.op()).collect();
        let end = ops.contains(&Copper::End);
        let waits = list.entries.iter().any(|e| e.first & 1 == 1 && e.second.is_some_and(|s| s & 0xFF00 == 0xFF00 || s & 0xFF00 == 0x7F00));
        let named = list.label.as_ref().is_some_and(|l| l.to_ascii_lowercase().contains("cop"));
        let moves = ops.iter().filter(|op| matches!(op, Copper::Move { register, .. } if *register >= 0x80)).count();
        if end || waits || named || moves >= 2 && self.amiga {
            self.lists.push(list);
        }
    }

    fn lookup(&self, name: &str) -> Option<i64> {
        self.symbols.get(name).or_else(|| self.lower_symbols.get(&name.to_ascii_lowercase())).copied().or_else(|| amiga_hw::builtin_value(name))
    }

    fn eval(&self, expr: &str) -> Option<i64> {
        evaluate(expr, &|name| self.lookup(name))
    }

    /// An address register being loaded on a line: which, and with what if it's known.
    fn load(&self, line: &str, parts: &Parts) -> Option<(u8, Option<u32>)> {
        let mnemonic = text_of(line, parts.mnemonic?).to_ascii_lowercase();
        let [from, to] = parts.operands[..] else {
            return None;
        };
        let register = address_register(&text_of(line, to))?;
        let from = text_of(line, from);
        let value = match mnemonic.as_str() {
            "lea" | "lea.l" => {
                let lower = from.to_ascii_lowercase();
                let expr = lower.strip_suffix("(pc)").map(|e| &from[..e.len()]).unwrap_or(&from);
                self.eval(strip_size(expr))
            }
            "move.l" | "movea.l" | "movea" => from.strip_prefix('#').and_then(|e| self.eval(e)),
            _ => return None,
        };
        Some((register, value.map(|v| v as u32 & 0xFF_FFFF)))
    }

    /// What an address register holds on a line, as far as the loads above it say.
    fn base_at(&self, line: usize, register: u8) -> Option<u32> {
        self.loads.iter().rfind(|(l, r, _)| *l <= line && *r == register).and_then(|(_, _, v)| *v)
    }

    /// The copper words on a line, if it's a line of them: `dc.w` pairs, or CMOVE, CWAIT, CSKIP or CEND. Each pair
    /// must be a plausible instruction: a MOVE to a register the copper can write, or a WAIT or SKIP.
    fn copper_words(&self, line: &str, parts: &Parts, bytes: Option<&[u8]>) -> Option<Vec<Option<u16>>> {
        let mnemonic = text_of(line, parts.mnemonic?).to_ascii_lowercase();
        let operands: Vec<String> = parts.operands.iter().map(|s| text_of(line, *s)).collect();
        let value = |s: &str| self.eval(s).map(|v| v as u16);
        let words: Vec<Option<u16>> = match mnemonic.as_str() {
            "dc.w" => {
                let mut words: Vec<Option<u16>> = operands.iter().map(|o| value(o)).collect();
                if words.iter().any(Option::is_none)
                    && let Some(bytes) = bytes.filter(|b| b.len() == words.len() * 2)
                {
                    words = bytes.as_chunks::<2>().0.iter().map(|w| Some(u16::from_be_bytes(*w))).collect();
                }
                words
            }
            "cmove" if operands.len() == 2 => vec![value(&operands[0]), value(&operands[1])],
            "cwait" | "cskip" if operands.len() == 2 => {
                let (v, h) = (value(&operands[0])?, value(&operands[1])?);
                vec![Some((v << 8) | (h & 0xFE) | 1), Some(if mnemonic == "cskip" { 0xFFFF } else { 0xFFFE })]
            }
            "cend" => vec![Some(0xFFFF), Some(0xFFFE)],
            _ => return None,
        };
        if words.is_empty() || !words.len().is_multiple_of(2) {
            return None;
        }
        for [first, _] in words.as_chunks::<2>().0 {
            let first = (*first)?;
            if first & 1 == 0 {
                let writable = amiga_hw::custom(first as u32).is_some_and(|r| r.access.writable());
                if !(0x40..=0x1FE).contains(&first) || !writable {
                    return None;
                }
            }
        }
        Some(words)
    }

    /// The copper list a line is in.
    pub fn list_at(&self, line: usize) -> Option<&CopperList> {
        self.lists.iter().find(|l| l.first_line <= line && line <= l.last_line)
    }

    /// Where an operand points, on a line.
    fn target(&self, operand: &str, line: usize) -> Option<Target> {
        let operand = operand.trim();
        if operand.starts_with('#') || operand.starts_with('-') && operand.ends_with(')') || operand.ends_with(")+") {
            return None;
        }
        let names = names_in(operand);
        let mentions_custom = names.iter().any(|n| !self.symbols.contains_key(n.as_str()) && amiga_hw::builtin_value(n).is_some() && !amiga_hw::is_cia_offset_name(n) || amiga_hw::named(n).is_some_and(|r| !r.is_cia()));
        let mentions_cia = names.iter().any(|n| amiga_hw::is_cia_offset_name(n));
        if let Some(open) = operand.find('(')
            && operand.ends_with(')')
        {
            let inside = &operand[open + 1..operand.len() - 1];
            let register = address_register(inside)?;
            let expr = operand[..open].trim();
            let offset = if expr.is_empty() { 0 } else { self.eval(expr)? };
            return match self.base_at(line, register) {
                Some(CUSTOM) if (0..0x200).contains(&offset) => Some(Target::Custom(offset as u32)),
                Some(base @ (CIAA | CIAB)) => amiga_hw::at(base.wrapping_add(offset as u32)).map(|r| Target::Cia(r.address)),
                Some(_) => None,
                None if mentions_cia && (0..0x1000).contains(&offset) => Some(Target::AnyCia(offset as u32 / 0x100)),
                None if mentions_custom && (0..0x200).contains(&offset) => Some(Target::Custom(offset as u32)),
                None => None,
            };
        }
        let value = self.eval(strip_size(operand))? as u32 & 0xFF_FFFF;
        if (CUSTOM..CUSTOM + 0x200).contains(&value) {
            return Some(Target::Custom(value - CUSTOM));
        }
        if let Some(found) = amiga_hw::at(value).filter(|r| r.is_cia()) {
            return Some(Target::Cia(found.address));
        }
        if mentions_cia && value < 0x1000 {
            return Some(Target::AnyCia(value / 0x100));
        }
        (mentions_custom && value < 0x200).then_some(Target::Custom(value))
    }

    /// What the pointer resting on a line is over, said: a copper instruction, a register being written (and the
    /// value written, read into its bits), a bit being tested, or a register's name.
    pub fn describe(&self, line: usize, text: &str, col: usize) -> Option<String> {
        let parts = split(text);
        let operand = parts.operands.iter().position(|(s, e)| *s <= col && col <= *e);
        let on_mnemonic = parts.mnemonic.is_some_and(|(s, e)| s <= col && col <= e);
        if let Some(list) = self.list_at(line)
            && (operand.is_some() || on_mnemonic)
            && list.entries.iter().any(|e| e.line == line)
        {
            return Some(self.describe_copper(list, line, operand));
        }
        if !self.amiga {
            return None;
        }
        if let Some(index) = operand {
            let mnemonic = parts.mnemonic.map(|m| text_of(text, m).to_ascii_lowercase()).unwrap_or_default();
            let operands: Vec<String> = parts.operands.iter().map(|s| text_of(text, *s)).collect();
            if let Some(said) = self.describe_instruction(line, &mnemonic, &operands, index) {
                return Some(said);
            }
        }
        // A register's name anywhere else, such as where it's defined.
        let word = word_at(text, col)?;
        if amiga_hw::is_cia_offset_name(&word) {
            let n = amiga_hw::builtin_value(&word)? as u32;
            let a = amiga_hw::at(CIAA + n)?;
            let b = amiga_hw::at(CIAB + n)?;
            return Some(format!("{}\n\n{}", amiga_hw::describe(a), amiga_hw::describe(b)));
        }
        let found = amiga_hw::named(&word)?;
        Some(amiga_hw::describe(found))
    }

    fn describe_copper(&self, list: &CopperList, line: usize, operand: Option<usize>) -> String {
        let mut state = CopperState::default();
        let mut said = Vec::new();
        let mut detail = None;
        let mut index = 0;
        for e in &list.entries {
            if e.line > line {
                break;
            }
            let op = e.op();
            let text = state.step(op);
            if e.line == line {
                said.push(if e.second.is_none() { unknown_value(op, text) } else { text });
                if operand.is_some_and(|o| o / 2 == index)
                    && let Copper::Move { register, value } = op
                    && let Some(found) = amiga_hw::custom(register as u32)
                    && found.colour().is_none()
                {
                    detail = Some(if e.second.is_some() && !found.fields.is_empty() { amiga_hw::describe_value(found, value) } else { amiga_hw::describe(found) });
                }
                index += 1;
            }
        }
        let mut out = format!("Copper list {}\n{}", list.name(), said.join("\n"));
        if let Some(detail) = detail {
            out.push_str("\n\n");
            out.push_str(&detail);
        }
        out
    }

    fn describe_instruction(&self, line: usize, mnemonic: &str, operands: &[String], index: usize) -> Option<String> {
        let (base, size) = match mnemonic.rsplit_once('.') {
            Some((b, s)) => (b, s),
            None => (mnemonic, "w"),
        };
        let targets: Vec<Option<Target>> = operands.iter().map(|o| self.target(o, line)).collect();
        let immediate = |o: &String| o.strip_prefix('#').and_then(|e| self.eval(e));
        // A bit tested or changed: on memory, the 68000 works on a byte, so the bit is in the high byte of a
        // custom register's word at its even address.
        if matches!(base, "btst" | "bset" | "bclr" | "bchg")
            && operands.len() == 2
            && let Some(n) = immediate(&operands[0])
            && let Some(target) = targets[1]
        {
            let n = (n & 7) as u8;
            return match target {
                Target::Custom(offset) => {
                    let found = amiga_hw::custom(offset)?;
                    Some(amiga_hw::describe_bit(found, if offset & 1 == 0 { n + 8 } else { n }))
                }
                Target::Cia(address) => Some(amiga_hw::describe_bit(amiga_hw::at(address)?, n)),
                Target::AnyCia(n2) => Some(format!("{}\n\n{}", amiga_hw::describe_bit(amiga_hw::at(CIAA + n2 * 0x100)?, n), amiga_hw::describe_bit(amiga_hw::at(CIAB + n2 * 0x100)?, n))),
            };
        }
        // Which operand is a register: the one under the pointer, or, over an immediate value, the other.
        let at = match targets.get(index).copied().flatten() {
            Some(_) => index,
            None if operands[index].starts_with('#') => targets.iter().rposition(Option::is_some)?,
            None => return None,
        };
        let target = targets[at]?;
        let value = operands.iter().enumerate().filter(|(i, _)| *i != at).find_map(|(_, o)| immediate(o));
        let registers: Vec<&Register> = match target {
            Target::Custom(offset) => vec![amiga_hw::custom(offset)?],
            Target::Cia(address) => vec![amiga_hw::at(address)?],
            Target::AnyCia(n) => vec![amiga_hw::at(CIAA + n * 0x100)?, amiga_hw::at(CIAB + n * 0x100)?],
        };
        let first = registers[0];
        // A long word to a custom register is two registers: a pointer's two halves, or two side by side.
        if size == "l"
            && let Target::Custom(offset) = target
        {
            if first.name.ends_with('H') && first.fields.iter().any(|f| f.name == "A20-16") {
                let pair = &first.name[..first.name.len() - 1];
                let about = first.about.split(" (high word").next().unwrap_or(&first.about);
                let shown = match value {
                    Some(v) => format!("${:08X}", v as u32),
                    None => operands.iter().find_map(|o| o.strip_prefix('#')).unwrap_or("the long word").to_string(),
                };
                return Some(format!("{pair} ({pair}H and {pair}L) = {shown}\n{about}, written as one long word"));
            }
            let second = amiga_hw::custom(offset + 2)?;
            return Some(match value {
                Some(v) => format!("{}\n\n{}", amiga_hw::describe_value(first, (v >> 16) as u16), amiga_hw::describe_value(second, v as u16)),
                None => format!("{}\n\n{}", amiga_hw::describe(first), amiga_hw::describe(second)),
            });
        }
        let said: Vec<String> = registers
            .iter()
            .map(|r| match value {
                Some(v) if size != "b" || r.is_cia() => amiga_hw::describe_value(r, v as u16),
                _ => amiga_hw::describe(r),
            })
            .collect();
        Some(said.join("\n\n"))
    }
}

/// The name under a column: letters, digits and underscores.
fn word_at(line: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let at = col.min(chars.len());
    let mut start = at;
    while start > 0 && is_word(chars[start - 1]) {
        start -= 1;
    }
    let mut end = at;
    while end < chars.len() && is_word(chars[end]) {
        end += 1;
    }
    (start < end && !chars[start].is_ascii_digit()).then(|| chars[start..end].iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEMO: &str = "\tinclude \"hardware/custom.i\"\n\
start:\tlea\t$dff000,a6\n\
\tmove.w\t#$7fff,dmacon(a6)\n\
\tmove.w\t#$5200,$100(a6)\n\
\tmove.l\t#copper,cop1lc(a6)\n\
\tmove.w\t#$0fff,$dff180\n\
\tbtst\t#6,$bfe001\n\
\tbtst\t#6,dmaconr(a6)\n\
\tmove.w\t#$83c0,DMACON\n\
\trts\n\
\n\
copper:\n\
\tdc.w\t$0180,$0000\t; black\n\
\tdc.w\t$2c07,$fffe\n\
\tdc.w\tbplcon0,$5200,$0182,$0f00\n\
\tdc.w\t$ffff,$fffe\n\
table:\tdc.w\t1,2,3,4\n";

    fn hover(source: &Source, line: usize, text: &str, find: &str) -> String {
        let col = text.find(find).unwrap_or_else(|| panic!("{find} in {text}")) + 1;
        source.describe(line, text, col).unwrap_or_else(|| panic!("nothing over {find} in {text}"))
    }

    #[test]
    fn lines_split_into_their_parts() {
        let parts = split("loop:\tmove.w  #$5200, bplcon0(a6) a comment");
        assert_eq!(parts.label.as_deref(), Some("loop"));
        let line = "loop:\tmove.w  #$5200, bplcon0(a6) a comment";
        assert_eq!(parts.operands.iter().map(|s| text_of(line, *s)).collect::<Vec<_>>(), ["#$5200", "bplcon0(a6)"]);
        assert_eq!(split("* all comment"), Parts::default());
        assert_eq!(evaluate("$dff000+$100", &|_| None), Some(0xDFF100));
        assert_eq!(evaluate("(1<<15)!%101", &|_| None), Some(0x8005));
        assert_eq!(evaluate("custom+bplcon0", &amiga_hw::builtin_value), Some(0xDFF100));
        assert_eq!(evaluate("bplpt+4", &amiga_hw::builtin_value), Some(0xE4));
    }

    #[test]
    fn copper_lists_are_found_and_decoded() {
        let source = Source::scan(DEMO, None);
        assert_eq!(source.lists.len(), 1, "the table of 1,2,3,4 isn't one: {:?}", source.lists);
        let list = &source.lists[0];
        assert_eq!(list.label.as_deref(), Some("copper"));
        assert_eq!((list.first_line, list.last_line), (12, 15));
        let rows = list.rows();
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].colour, Some((0, [0, 0, 0])));
        assert_eq!(rows[1].said, "WAIT for line $2C (44), x $06");
        assert_eq!(rows[2].said, "MOVE $5200 to BPLCON0: 5 bitplanes, lowres, colour on");
        assert_eq!(rows[3].colour, Some((1, [255, 0, 0])));
        assert!(rows[4].said.starts_with("END"));
        let text = DEMO.lines().nth(14).unwrap();
        let said = hover(&source, 14, text, "bplcon0");
        assert!(said.contains("MOVE $5200 to BPLCON0") && said.contains("MOVE $0F00 to COLOR01"), "{said}");
        assert!(said.contains("ECSENA"), "over the register, its bits too: {said}");
        let said = hover(&source, 13, DEMO.lines().nth(13).unwrap(), "2c07");
        assert!(said.contains("WAIT for line $2C"), "{said}");
    }

    #[test]
    fn lists_by_macro_and_without_a_label() {
        let text = "\tlea\tcustom,a5\n\tCMOVE\tcolor00,$0f0\n\tCWAIT\t$80,$07\n\tCEND\n";
        let source = Source::scan(text, None);
        let rows = source.lists[0].rows();
        assert_eq!(rows[0].said, "MOVE $00F0 to COLOR00: colour 0, red 0, green 15, blue 0");
        assert_eq!(rows[1].said, "WAIT for line $80 (128), x $06");
        assert!(rows[2].said.starts_with("END"));
    }

    #[test]
    fn writes_to_registers_are_explained() {
        let source = Source::scan(DEMO, None);
        let line = |n: usize| DEMO.lines().nth(n).unwrap();
        let said = hover(&source, 3, line(3), "$5200");
        assert!(said.starts_with("BPLCON0") && said.contains("5 bitplanes, lowres, colour on"), "{said}");
        let said = hover(&source, 3, line(3), "$100");
        assert!(said.contains("5 bitplanes"), "the register under the pointer, with the value: {said}");
        assert!(hover(&source, 2, line(2), "dmacon").contains("turns off everything"));
        let said = hover(&source, 4, line(4), "cop1lc");
        assert!(said.starts_with("COP1LC (COP1LCH and COP1LCL) = copper"), "{said}");
        assert!(hover(&source, 5, line(5), "dff180").contains("colour 0: red 15, green 15, blue 15"));
        assert!(hover(&source, 6, line(6), "bfe001").contains("/FIR0"), "the left mouse button");
        assert!(hover(&source, 7, line(7), "dmaconr").contains("BBUSY"), "bit 6 of the high byte is bit 14");
        assert!(hover(&source, 8, line(8), "DMACON").contains("turns on DMAEN, BPLEN, COPEN, BLTEN"));
        assert!(source.describe(9, line(9), 2).is_none());
        assert!(hover(&source, 0, "BPLCON3 equ $106", "BPLCON3").contains("LOCT"));
    }

    #[test]
    fn a_base_register_counts_only_while_it_holds_the_chips() {
        let text = "\tlea\t$dff000,a6\n\tmove.w\t#$0fff,$180(a6)\n\tmove.l\t4.w,a6\n\tmove.l\t$114(a6),a1\n";
        let source = Source::scan(text, None);
        assert!(source.describe(1, "\tmove.w\t#$0fff,$180(a6)", 10).is_some_and(|s| s.starts_with("COLOR00")));
        assert!(source.describe(3, "\tmove.l\t$114(a6),a1", 10).is_none(), "a6 is ExecBase by then");
        let plain = Source::scan("\tmove.w\t#1,d0\n\tdc.w\t1,2\n", None);
        assert!(!plain.amiga && plain.lists.is_empty());
        assert!(plain.describe(0, "\tmove.w\t#1,d0", 9).is_none());
    }

    #[test]
    fn cia_registers_are_found_through_their_base() {
        let text = "\tlea\t$bfd000,a5\n\tbset\t#7,ciaprb(a5)\n\tbtst\t#6,ciapra\n";
        let source = Source::scan(text, None);
        assert!(source.describe(1, "\tbset\t#7,ciaprb(a5)", 12).is_some_and(|s| s.contains("CIAB PRB") && s.contains("/MTR")));
        let both = source.describe(2, "\tbtst\t#6,ciapra", 11).unwrap();
        assert!(both.contains("/FIR0") && both.contains("/RTS"), "without a base, both CIAs: {both}");
    }

    #[test]
    fn hover_comes_through_code_intel_with_the_assembler_s_labels() {
        let text = "custom\tequ\t$dff000\nbackground\tequ\t$180\nstart:\tmove.w\t#$0f00,custom+background\n\tmove.l\t#mylist,custom+cop1lc\n\trts\nmylist:\tdc.w\tbackground,$0fff\n\tdc.w\t$ffff,$fffe\n";
        let assembly = crate::assemblers::assemble_own(crate::assemblers::Cpu::M68k, std::path::Path::new("t.s"), text).assembly;
        let source = Source::scan(text, Some(&assembly));
        let intel = crate::code_intel::CodeIntel { hardware: Some(std::sync::Arc::new(source)), ..crate::code_intel::CodeIntel::for_cpu(crate::assemblers::Cpu::M68k) };
        let line = |n: usize| text.lines().nth(n).unwrap();
        let said = intel.describe(2, line(2), line(2).find("background").unwrap() + 2).unwrap();
        assert!(said.starts_with("COLOR00") && said.contains("red 15, green 0, blue 0"), "{said}");
        let said = intel.describe(3, line(3), line(3).find("cop1lc").unwrap() + 2).unwrap();
        assert!(said.contains("COP1LC (COP1LCH and COP1LCL) = $"), "the label's address, from the assembler: {said}");
        let said = intel.describe(5, line(5), line(5).find("background").unwrap() + 2).unwrap();
        assert!(said.contains("MOVE $0FFF to COLOR00"), "{said}");
        assert!(intel.describe(2, "start:\tmove.w\t#$0f00,custom+background ; COLOR00", 45).is_none(), "nothing in a comment");
    }
}
