//! PAL, the PDP-8's assembly language, assembled by Limoniter for the built-in PDP-8 (see `pdp8`).
//!
//! It reads the language as DEC's PAL-III and PAL8 manuals describe it:
//!
//! - a / starts a comment, and ; separates statements on a line;
//! - `TAG,` labels a word; `NAME=expression` gives a name a value; `*expression` sets where the next word goes;
//! - a memory reference instruction (AND, TAD, ISZ, DCA, JMS, JMP) is followed by I for an indirect reference,
//!   then its address, which must be on page zero or on the page the instruction is on (Z says page zero
//!   outright). An address in brackets is a literal: (value) is put at the top of the current page and [value] at
//!   the top of page zero, and the instruction refers to it there;
//! - symbols written one after another are combined, as microinstructions are (CLA CLL); + and - add and
//!   subtract, ! ORs and & ANDs; `.` is where this word goes, and `"A` is a character's code with its top bit set,
//!   as the Teletype sends it;
//! - numbers are octal, until DECIMAL (and again after OCTAL); PAGE goes on to the next page (or page n);
//!   TEXT /words/ packs six-bit characters two to a word, ending with a zero; ZBLOCK n leaves n words of zero; $
//!   ends the program. FIELD 0 is taken, as this PDP-8 has one field of 4K words.
//!
//! Microinstructions from group 1 and group 2 can't be combined in one word, and saying so is kinder than the
//! wrong word the real assembler would make.

use std::collections::BTreeMap;

/// What a symbol in the permanent table is, so that operate microinstructions from different groups can be caught.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Memory,
    Group1,
    Group2,
    Both,
    Other,
}

/// The permanent symbols: the instructions, the microinstructions, the Teletype's and reader's IOTs, and I and Z.
const PERMANENT: [(&str, u16, Kind); 47] = [
    ("AND", 0o0000, Kind::Memory),
    ("TAD", 0o1000, Kind::Memory),
    ("ISZ", 0o2000, Kind::Memory),
    ("DCA", 0o3000, Kind::Memory),
    ("JMS", 0o4000, Kind::Memory),
    ("JMP", 0o5000, Kind::Memory),
    ("ION", 0o6001, Kind::Other),
    ("IOF", 0o6002, Kind::Other),
    ("RSF", 0o6011, Kind::Other),
    ("RRB", 0o6012, Kind::Other),
    ("RFC", 0o6014, Kind::Other),
    ("PSF", 0o6021, Kind::Other),
    ("PCF", 0o6022, Kind::Other),
    ("PPC", 0o6024, Kind::Other),
    ("PLS", 0o6026, Kind::Other),
    ("KSF", 0o6031, Kind::Other),
    ("KCC", 0o6032, Kind::Other),
    ("KRS", 0o6034, Kind::Other),
    ("KRB", 0o6036, Kind::Other),
    ("TSF", 0o6041, Kind::Other),
    ("TCF", 0o6042, Kind::Other),
    ("TPC", 0o6044, Kind::Other),
    ("TLS", 0o6046, Kind::Other),
    ("NOP", 0o7000, Kind::Group1),
    ("IAC", 0o7001, Kind::Group1),
    ("RAL", 0o7004, Kind::Group1),
    ("RTL", 0o7006, Kind::Group1),
    ("RAR", 0o7010, Kind::Group1),
    ("RTR", 0o7012, Kind::Group1),
    ("CML", 0o7020, Kind::Group1),
    ("CMA", 0o7040, Kind::Group1),
    ("CIA", 0o7041, Kind::Group1),
    ("CLL", 0o7100, Kind::Group1),
    ("STL", 0o7120, Kind::Group1),
    ("CLA", 0o7200, Kind::Both),
    ("GLK", 0o7204, Kind::Group1),
    ("STA", 0o7240, Kind::Group1),
    ("HLT", 0o7402, Kind::Group2),
    ("OSR", 0o7404, Kind::Group2),
    ("SKP", 0o7410, Kind::Group2),
    ("SNL", 0o7420, Kind::Group2),
    ("SZL", 0o7430, Kind::Group2),
    ("SZA", 0o7440, Kind::Group2),
    ("SNA", 0o7450, Kind::Group2),
    ("SMA", 0o7500, Kind::Group2),
    ("SPA", 0o7510, Kind::Group2),
    ("LAS", 0o7604, Kind::Group2),
];

/// A program assembled.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Assembly {
    /// The words, in the order they were made.
    pub words: Vec<(u16, u16)>,
    /// Which source line (from 0) made the word at each address.
    pub lines: BTreeMap<u16, usize>,
    pub symbols: BTreeMap<String, u16>,
    /// Where it starts: START if there's a label of that name, else its first word.
    pub start: u16,
}

impl Assembly {
    pub fn address_of(&self, line: usize) -> Option<u16> {
        self.lines.iter().find(|(_, l)| **l == line).map(|(a, _)| *a)
    }

    pub fn line_at(&self, address: u16) -> Option<usize> {
        self.lines.get(&address).copied()
    }
}

pub type Problem = (usize, String);

struct Pass {
    last: bool,
    symbols: BTreeMap<String, (i64, Kind)>,
    labels: BTreeMap<String, usize>,
    location: u16,
    decimal: bool,
    /// For each page, the lowest word its literals take, as they're put at its top going down.
    literals: BTreeMap<u16, u16>,
    literal_words: std::collections::BTreeSet<u16>,
    words: Vec<(u16, u16)>,
    lines: BTreeMap<u16, usize>,
    problems: Vec<Problem>,
    line: usize,
    ended: bool,
}

/// Assembles PAL source.
pub fn assemble(source: &str) -> Result<Assembly, Vec<Problem>> {
    let mut symbols: BTreeMap<String, (i64, Kind)> = PERMANENT.iter().map(|(n, v, k)| (n.to_string(), (i64::from(*v), *k))).collect();
    symbols.insert("I".into(), (0o400, Kind::Other));
    symbols.insert("Z".into(), (0, Kind::Other));
    let mut pass = Pass { last: false, symbols, labels: BTreeMap::new(), location: 0o200, decimal: false, literals: BTreeMap::new(), literal_words: Default::default(), words: Vec::new(), lines: BTreeMap::new(), problems: Vec::new(), line: 0, ended: false };
    for last in [false, true] {
        pass.last = last;
        pass.location = 0o200;
        pass.decimal = false;
        pass.literals.clear();
        pass.literal_words.clear();
        pass.words.clear();
        pass.lines.clear();
        pass.problems.clear();
        pass.ended = false;
        for (number, text) in source.lines().enumerate() {
            if pass.ended {
                break;
            }
            pass.line = number;
            for statement in statements(text) {
                pass.statement(&statement);
            }
        }
    }
    if !pass.problems.is_empty() {
        return Err(pass.problems);
    }
    // Two words in the same place is a mistake the person should hear of.
    let mut seen: BTreeMap<u16, usize> = BTreeMap::new();
    let mut problems = Vec::new();
    for (address, _) in &pass.words {
        if let Some(line) = pass.lines.get(address)
            && seen.insert(*address, *line).is_some()
        {
            problems.push((*line, crate::i18n::tf!("Two words were put at {}.", crate::pdp8::octal(*address))));
        }
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let symbols: BTreeMap<String, u16> = pass.labels.keys().filter_map(|name| pass.symbols.get(name).map(|(v, _)| (name.clone(), (*v as u16) & 0o7777))).collect();
    let start = symbols.get("START").copied().or_else(|| pass.words.iter().find(|(a, _)| !pass.literal_words.contains(a)).map(|(a, _)| *a)).unwrap_or(0o200);
    Ok(Assembly { words: pass.words, lines: pass.lines, symbols, start })
}

/// A line's statements: its comment taken off, split at semicolons, with TEXT's delimited words and character
/// constants kept whole.
fn statements(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = vec![String::new()];
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '/' => break,
            ';' => out.push(String::new()),
            '"' => {
                out.last_mut().expect("a statement").push(c);
                if let Some(next) = chars.get(i + 1) {
                    out.last_mut().expect("a statement").push(*next);
                    i += 1;
                }
            }
            _ if c.is_ascii_alphabetic() => {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_alphanumeric() {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                out.last_mut().expect("a statement").push_str(&word);
                if word.eq_ignore_ascii_case("TEXT") {
                    // The delimiter, then everything up to the next one, which may hold / or ;.
                    while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
                        out.last_mut().expect("a statement").push(chars[i]);
                        i += 1;
                    }
                    if let Some(delimiter) = chars.get(i).copied() {
                        let end = chars[i + 1..].iter().position(|c| *c == delimiter).map_or(chars.len(), |p| i + 1 + p);
                        let text: String = chars[i..chars.len().min(end + 1)].iter().collect();
                        out.last_mut().expect("a statement").push_str(&text);
                        i = end + 1;
                    }
                }
                continue;
            }
            _ => out.last_mut().expect("a statement").push(c),
        }
        i += 1;
    }
    out.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// The terms of an expression and the operators before each: '+', '-', '!', '&', or ' ' for symbols combined.
fn terms(text: &str) -> Vec<(char, String)> {
    let mut out: Vec<(char, String)> = Vec::new();
    let mut op = '+';
    let mut term = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '+' | '-' | '!' | '&' => {
                if !term.is_empty() {
                    out.push((op, std::mem::take(&mut term)));
                }
                op = c;
            }
            ' ' | '\t' => {
                if !term.is_empty() {
                    out.push((op, std::mem::take(&mut term)));
                    op = ' ';
                }
            }
            '"' => {
                term.push('"');
                if let Some(next) = chars.next() {
                    term.push(next);
                }
            }
            _ => term.push(c),
        }
    }
    if !term.is_empty() {
        out.push((op, term));
    }
    out
}

impl Pass {
    fn problem(&mut self, message: String) {
        self.problems.push((self.line, message));
    }

    fn emit(&mut self, word: i64) {
        let at = self.location & 0o7777;
        if let Some(low) = self.literals.get(&(at / 128))
            && at >= *low
        {
            self.problem(crate::i18n::tf!("Page {} is full: its literals and its code have met.", crate::pdp8::octal(at / 128 * 128)));
        }
        self.words.push((at, (word as u16) & 0o7777));
        self.lines.entry(at).or_insert(self.line);
        self.location = (at + 1) & 0o7777;
    }

    /// A term's value: a number, a symbol, `.`, or a character.
    fn term(&mut self, term: &str) -> Option<(i64, Kind)> {
        if term == "." {
            return Some((i64::from(self.location), Kind::Other));
        }
        if let Some(c) = term.strip_prefix('"') {
            return Some((c.chars().next().map_or(0, |c| i64::from(c as u32 & 0o177 | 0o200)), Kind::Other));
        }
        if term.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            let radix = if self.decimal { 10 } else { 8 };
            return match i64::from_str_radix(term, radix) {
                Ok(v) => Some((v, Kind::Other)),
                Err(_) => {
                    let base = if self.decimal { crate::i18n::tr("decimal") } else { crate::i18n::tr("octal") };
                    self.problem(crate::i18n::tf!("{term} isn't a {base} number."));
                    None
                }
            };
        }
        let name = term.to_ascii_uppercase();
        match self.symbols.get(&name) {
            Some(found) => Some(*found),
            None if self.last => {
                self.problem(crate::i18n::tf!("{term} isn't defined."));
                None
            }
            None => Some((0, Kind::Other)),
        }
    }

    /// An expression's value, and whether its microinstructions mix groups.
    fn value(&mut self, text: &str) -> Option<i64> {
        let mut total = 0i64;
        let mut kinds = Vec::new();
        for (op, term) in terms(text) {
            let (v, kind) = self.term(&term)?;
            kinds.push(kind);
            total = match op {
                '+' => total + v,
                '-' => total - v,
                '&' => total & v,
                _ => total | v,
            };
        }
        if kinds.contains(&Kind::Group1) && kinds.contains(&Kind::Group2) {
            self.problem(crate::i18n::tf!("{text} mixes microinstructions from group 1 and group 2, which can't be done in one word."));
        }
        Some(total)
    }

    /// A literal's address, in page zero or the current page.
    fn literal(&mut self, value: i64, zero: bool) -> u16 {
        let page = if zero { 0 } else { self.location / 128 };
        let low = self.literals.entry(page).or_insert(page * 128 + 128);
        *low -= 1;
        let at = *low;
        let taken = self.words.iter().any(|(a, _)| *a == at);
        self.words.push((at, (value as u16) & 0o7777));
        self.literal_words.insert(at);
        self.lines.insert(at, self.line);
        if taken || (page == self.location / 128 && at < self.location) {
            self.problem(crate::i18n::tf!("Page {} is full: its literals and its code have met.", crate::pdp8::octal(page * 128)));
        }
        at
    }

    fn statement(&mut self, text: &str) {
        let mut text = text.trim();
        // Labels.
        while let Some((head, rest)) = text.split_once(',') {
            let name = head.trim();
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric()) || !name.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
                break;
            }
            self.define(name, i64::from(self.location), true);
            text = rest.trim();
        }
        if text.is_empty() {
            return;
        }
        if text == "$" || text.starts_with("$ ") {
            self.ended = true;
            return;
        }
        if let Some(rest) = text.strip_prefix('*') {
            if let Some(v) = self.value(rest.trim()) {
                self.location = (v as u16) & 0o7777;
            }
            return;
        }
        if let Some((name, rest)) = text.split_once('=')
            && name.trim().chars().all(|c| c.is_ascii_alphanumeric())
            && !name.trim().is_empty()
        {
            if let Some(v) = self.value(rest.trim()) {
                self.define(name.trim(), v, false);
            }
            return;
        }
        let first = text.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
        let rest = text[text.find(char::is_whitespace).unwrap_or(text.len())..].trim();
        match first.as_str() {
            "DECIMAL" => {
                self.decimal = true;
                return;
            }
            "OCTAL" => {
                self.decimal = false;
                return;
            }
            "EJECT" | "XLIST" | "NOPUNCH" | "ENPUNCH" | "PAUSE" => return,
            "FIELD" => {
                if self.value(rest).is_some_and(|v| v != 0) {
                    self.problem(crate::i18n::tr("This PDP-8 has one field of 4K words: only FIELD 0 can be used.").into());
                }
                return;
            }
            "PAGE" => {
                let page = if rest.is_empty() { Some(i64::from(self.location.div_ceil(128))) } else { self.value(rest) };
                if let Some(page) = page {
                    self.location = ((page as u16) * 128) & 0o7777;
                }
                return;
            }
            "ZBLOCK" => {
                if let Some(n) = self.value(rest) {
                    for _ in 0..n.clamp(0, 4096) {
                        self.emit(0);
                    }
                }
                return;
            }
            "TEXT" => return self.text(rest),
            _ => {}
        }
        if let Some((value, Kind::Memory)) = self.symbols.get(&first).copied() {
            return self.memory_reference(value, rest);
        }
        if let Some(v) = self.value(text) {
            self.emit(v);
        }
    }

    fn define(&mut self, name: &str, value: i64, label: bool) {
        let key = name.to_ascii_uppercase();
        if label {
            // The first definition is the one kept; a second, on another line, is a mistake.
            let first = *self.labels.entry(key.clone()).or_insert(self.line);
            if first != self.line {
                if self.last {
                    self.problem(crate::i18n::tf!("{name} is defined twice."));
                }
                return;
            }
        }
        // A permanent symbol may be given a new value, as PAL allows; it's then an ordinary one.
        self.symbols.insert(key, (value, Kind::Other));
    }

    fn memory_reference(&mut self, opcode: i64, rest: &str) {
        let mut word = opcode;
        let mut rest = rest.trim();
        let mut page_zero = false;
        loop {
            let (head, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            match head.to_ascii_uppercase().as_str() {
                "I" if !tail.trim().is_empty() => word |= 0o400,
                "Z" if !tail.trim().is_empty() => page_zero = true,
                _ => break,
            }
            rest = tail.trim();
        }
        if rest.is_empty() {
            self.problem(crate::i18n::tr("This instruction needs an address.").into());
            return self.emit(word);
        }
        let address = if let Some(inner) = rest.strip_prefix('(') {
            let Some(v) = self.value(inner.trim_end_matches(')')) else { return self.emit(word) };
            i64::from(self.literal(v, false))
        } else if let Some(inner) = rest.strip_prefix('[') {
            let Some(v) = self.value(inner.trim_end_matches(']')) else { return self.emit(word) };
            i64::from(self.literal(v, true))
        } else {
            match self.value(rest) {
                Some(v) => v,
                None => return self.emit(word),
            }
        };
        let address = (address as u16) & 0o7777;
        let here = self.location & 0o7600;
        if address & 0o7600 == 0 {
            word |= i64::from(address);
        } else if address & 0o7600 == here && !page_zero {
            word |= 0o200 | i64::from(address & 0o177);
        } else if self.last {
            self.problem(crate::i18n::tf!("{} is neither on page zero nor on this page ({}), so it can't be reached directly. Put its address in a word on this page and refer to it with I, or use a literal: (address).", crate::pdp8::octal(address), crate::pdp8::octal(here)));
        }
        self.emit(word);
    }

    /// TEXT: six-bit characters two to a word, then a zero character.
    fn text(&mut self, rest: &str) {
        let mut chars = rest.chars();
        let Some(delimiter) = chars.next() else {
            return self.problem(crate::i18n::tr("TEXT needs its words between two of the same character, as TEXT /HELLO/.").into());
        };
        let body: String = chars.take_while(|c| *c != delimiter).collect();
        let mut codes: Vec<i64> = body.chars().map(|c| i64::from(c.to_ascii_uppercase() as u32 & 0o77)).collect();
        codes.push(0);
        if codes.len() % 2 == 1 {
            codes.push(0);
        }
        for pair in codes.chunks(2) {
            self.emit(pair[0] << 6 | pair[1]);
        }
    }
}

/// A first PDP-8 program, written for Limoniter: it prints a greeting on the Teletype, a character at a time,
/// waiting for each to be printed, as every PDP-8 program did.
pub const HELLO: &str = "\
/ A first PDP-8 program, written for Limoniter.
/ It prints HELLO, WORLD on the Teletype, one character at a time, then halts.
/ Numbers are octal, as on the PDP-8. Press F9 to run it, or step through it
/ and watch the lights on the front panel.

*200                    / programs usually start at 0200
START,  CLA CLL         / clear the accumulator and the link
        TAD (MESSAGE-1) / where the message starts, less one...
        DCA 10          / ...into auto-index register 10
NEXT,   TAD I 10        / 10 goes up by one, then the character it points at
        SNA             / a zero ends the message
        JMP DONE
        JMS TYPE        / print it
        JMP NEXT
DONE,   HLT             / stop: press Continue to print it again
        JMP START

/ Prints the character in the accumulator, and returns with it cleared.
TYPE,   0               / JMS puts the return address here
        TLS             / start printing the character
        TSF             / is the Teletype ready again?
        JMP .-1         / not yet: ask again
        CLA
        JMP I TYPE      / go back to where it was called from

MESSAGE, \"H; \"E; \"L; \"L; \"O; \",; 240; \"W; \"O; \"R; \"L; \"D
        215; 212; 0     / carriage return, line feed, and the end
$
";

/// A second program, written for Limoniter: whatever is typed on the Teletype's keyboard is printed back.
pub const ECHO: &str = "\
/ Echo: what's typed on the Teletype's keyboard is printed back, written for Limoniter.
/ Click the Teletype's paper so it has the keyboard, then type.

*200
START,  KCC             / clear the keyboard's flag
LOOP,   KSF             / has a key been pressed?
        JMP .-1         / not yet
        KRB             / read it (and clear the flag)
        TLS             / print it
        TSF             / wait for the printer
        JMP .-1
        CLA
        JMP LOOP
$
";

#[cfg(test)]
mod tests {
    use super::*;

    fn words(source: &str) -> Vec<(u16, u16)> {
        assemble(source).unwrap_or_else(|p| panic!("{p:?}")).words
    }

    #[test]
    fn memory_references_are_put_together_as_dec_does() {
        // TAD 0210 on page 0200 is 1210; DCA 50 on page zero 3050; JMP I 0211 is 5611; Z forces page zero.
        let w = words("*200\nTAD 210\nDCA 50\nJMP I 211\nTAD Z 20");
        assert_eq!(w, [(0o200, 0o1210), (0o201, 0o3050), (0o202, 0o5611), (0o203, 0o1020)]);
        // An address on another page can't be reached.
        assert!(assemble("*200\nTAD 400").is_err());
        // Labels, . and expressions.
        let w = words("*300\nA, JMP .+2\nB, TAD A\nJMP B-1");
        assert_eq!(w, [(0o300, 0o5302), (0o301, 0o1300), (0o302, 0o5300)]);
    }

    #[test]
    fn microinstructions_combine_and_groups_dont_mix() {
        assert_eq!(words("CLA CLL")[0].1, 0o7300);
        assert_eq!(words("CLA CLL IAC RAL")[0].1, 0o7305);
        assert_eq!(words("SZA CLA")[0].1, 0o7640);
        assert_eq!(words("SPA SNA")[0].1, 0o7550);
        assert_eq!(words("KRB")[0].1, 0o6036);
        assert!(assemble("RAL SZA").is_err());
    }

    #[test]
    fn numbers_characters_text_and_pseudo_ops() {
        assert_eq!(words("DECIMAL\n100\nOCTAL\n100").iter().map(|w| w.1).collect::<Vec<_>>(), [100, 0o100]);
        assert_eq!(words("\"A")[0].1, 0o301);
        // TEXT /AB/ is 0102 and a zero word; TEXT /ABC/ 0102, 0300.
        assert_eq!(words("TEXT /AB/").iter().map(|w| w.1).collect::<Vec<_>>(), [0o0102, 0]);
        assert_eq!(words("TEXT \"ABC\"").iter().map(|w| w.1).collect::<Vec<_>>(), [0o0102, 0o0300]);
        assert_eq!(words("*200\n1\nPAGE\n2")[1].0, 0o400);
        assert_eq!(words("*200\nPAGE\n1")[0].0, 0o200, "already at the start of a page");
        assert_eq!(words("ZBLOCK 3\n7").len(), 4);
        assert_eq!(words("X=5\nX+1")[0].1, 6);
        assert!(assemble("9").is_err(), "9 isn't octal");
        // Comments and statements on one line.
        assert_eq!(words("1; 2 / a comment; 3").len(), 2);
        assert_eq!(words("1\n$\n2").len(), 1);
    }

    #[test]
    fn literals_go_at_the_top_of_the_page() {
        let w = words("*200\nTAD (123)\nTAD [7]");
        assert!(w.contains(&(0o377, 0o123)));
        assert!(w.contains(&(0o177, 7)));
        assert!(w.contains(&(0o200, 0o1377)));
        assert!(w.contains(&(0o201, 0o1177)));
    }

    #[test]
    fn mistakes_are_found_with_their_lines() {
        let problems = assemble("*200\nTAD NOWHERE\nA, 1\nA, 2").unwrap_err();
        assert_eq!(problems.iter().map(|p| p.0).collect::<Vec<_>>(), [1, 3]);
    }

    #[test]
    fn hello_prints_hello_world() {
        let assembly = assemble(HELLO).unwrap();
        assert_eq!(assembly.start, 0o200);
        let mut pdp = crate::pdp8::Pdp8::default();
        pdp.load(&assembly.words);
        pdp.pc = assembly.start;
        pdp.start();
        pdp.run_for(u64::MAX / 2, 50_000_000, &[]);
        assert!(pdp.halted);
        assert_eq!(pdp.teletype_text(), "HELLO, WORLD\n");
        // Fourteen characters at ten a second.
        assert!(pdp.time / 100_000_000 >= 14);
        // And its lines are known by address.
        let line = HELLO.lines().position(|l| l.starts_with("DONE,")).unwrap();
        assert_eq!(assembly.line_at(assembly.symbols["DONE"]), Some(line));
    }

    #[test]
    fn echo_prints_what_is_typed() {
        let assembly = assemble(ECHO).unwrap();
        let mut pdp = crate::pdp8::Pdp8::default();
        pdp.load(&assembly.words);
        pdp.pc = assembly.start;
        pdp.start();
        // Keys pressed before KCC has run are cleared by it, as on the real machine.
        pdp.run_for(1_000_000, 1000, &[]);
        for c in b"HI" {
            pdp.type_key(*c);
        }
        pdp.run_for(1_000_000_000, 10_000_000, &[]);
        assert_eq!(pdp.teletype_text(), "HI");
    }
}
