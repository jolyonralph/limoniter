//! Amiga locale catalogs: a program's strings described in a catalog description (.cd), translated in catalog
//! translations (.ct), checked against each other, the strings' C header made from the description, and the IFF
//! catalogs locale.library reads (.catalog) written from each translation.
//!
//! The formats are as CatComp's documentation (Tools/Catcomp/CatComp.doc in the NDK for AmigaOS 3.2) and FlexCat's
//! (FlexCat.guide, "Catalog description files" and "Catalog translation files") describe them:
//! - A description is `#` commands, `;` comments, and entries of two lines: `NAME (id/min/max)` and the string.
//!   Each number may be left out: the id is then the last one plus one (0 for the first), the shortest 0, and the
//!   longest unlimited. A number may be hexadecimal after `$`, and an id may be `+n`, that many past the next.
//! - A translation is `## version`, `## language`, `## codeset`, `## rcsid` and `## name` commands, `;` comments,
//!   and entries of two lines: the name and the translated string.
//! - In both, a string line ending with a backslash carries on on the next, and the strings take C's escapes plus
//!   `\c` (155, the control sequence introducer) and `\e` (27).
//! - A catalog is an IFF FORM of type CTLG: FVER (the version string), LANG (the language), CSET (the code set and
//!   seven reserved longwords) and STRS, each string as its id, its length, and the string, padded with zeros to a
//!   longword. Strings the same as the description's are left out, as CatComp and FlexCat leave them out (their
//!   NOOPTIM puts them in), since GetCatalogStr() gives the program's own string for any it doesn't find.
//!
//! Catalogs are in ISO-8859-1 (Latin 1), code set 0, the Amiga's own: a character outside it can't be in one.

use std::fmt::Write as _;

/// A problem with a description or a translation: its line (from 1), what's wrong, and whether it only matters a
/// little.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub line: usize,
    pub message: String,
    pub warning: bool,
}

fn problem(line: usize, message: String) -> Problem {
    Problem { line, message, warning: false }
}

fn warning(line: usize, message: String) -> Problem {
    Problem { line, message, warning: true }
}

/// One string a description gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub id: u32,
    pub min: usize,
    pub max: Option<usize>,
    /// The string as it's written, escapes and all, its carried-on lines joined.
    pub text: String,
    /// The line its name is on, from 1.
    pub line: usize,
}

/// A catalog description: its strings, its commands (`#header`, `#language` and the rest, by lower-case name), and
/// what's wrong with it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Description {
    pub entries: Vec<Entry>,
    pub commands: Vec<(String, String)>,
    pub problems: Vec<Problem>,
}

impl Description {
    pub fn command(&self, name: &str) -> Option<&str> {
        self.commands.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    /// The bytes of length CatComp's `#lengthbytes` puts before each string.
    pub fn length_bytes(&self) -> usize {
        self.command("lengthbytes").and_then(|v| v.trim().parse().ok()).unwrap_or(0).min(4)
    }

    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }
}

/// One translated string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Translated {
    pub name: String,
    pub text: String,
    pub line: usize,
}

/// A catalog translation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Translation {
    pub version: Option<String>,
    pub language: Option<String>,
    pub codeset: u32,
    pub rcsid: Option<String>,
    pub name: Option<String>,
    pub entries: Vec<Translated>,
    pub problems: Vec<Problem>,
}

impl Translation {
    pub fn entry(&self, name: &str) -> Option<&Translated> {
        self.entries.iter().find(|e| e.name == name)
    }
}

/// The lines of a file, each with its number from 1, with a string's carried-on lines joined to it.
struct Lines<'a> {
    lines: Vec<&'a str>,
    at: usize,
}

impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Self {
        Self { lines: text.lines().map(|l| l.strip_suffix('\r').unwrap_or(l)).collect(), at: 0 }
    }

    fn next(&mut self) -> Option<(usize, &'a str)> {
        let line = *self.lines.get(self.at)?;
        self.at += 1;
        Some((self.at, line))
    }

    /// A string: this line, and the next for as long as each ends with a backslash that isn't itself escaped.
    fn string(&mut self) -> Option<(usize, String)> {
        let (number, first) = self.next()?;
        let mut text = first.to_string();
        while ends_carried_on(&text) {
            text.pop();
            match self.next() {
                Some((_, more)) => text.push_str(more),
                None => break,
            }
        }
        Some((number, text))
    }
}

/// Whether a string line ends with a backslash of its own: an odd number of them, since `\\` is one backslash.
fn ends_carried_on(line: &str) -> bool {
    line.bytes().rev().take_while(|&b| b == b'\\').count() % 2 == 1
}

/// A number in a description's brackets: decimal, or hexadecimal after a $.
fn number(text: &str) -> Option<u32> {
    let text = text.trim();
    match text.strip_prefix('$') {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

fn is_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Reads a catalog description.
pub fn read_description(text: &str) -> Description {
    let mut cd = Description::default();
    let mut lines = Lines::new(text);
    let mut next_id: u32 = 0;
    while let Some((number_at, line)) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with("//") {
            continue;
        }
        if let Some(command) = trimmed.strip_prefix('#') {
            let command = command.trim_start_matches('#').trim_start();
            let (name, value) = command.split_once(char::is_whitespace).unwrap_or((command, ""));
            cd.commands.push((name.to_ascii_lowercase(), value.trim().to_string()));
            continue;
        }
        let (name, rest) = match trimmed.find(|c: char| c == '(' || c.is_whitespace()) {
            Some(at) => (trimmed[..at].trim(), trimmed[at..].trim()),
            None => (trimmed, ""),
        };
        if !is_name(name) {
            cd.problems.push(problem(number_at, crate::i18n::tf!("\u{201c}{name}\u{201d} isn't a string's name: a name is letters, digits and underscores, starting with a letter.")));
            // The line after is its string all the same, so it isn't taken for a name.
            let _ = lines.string();
            continue;
        }
        let Some(inside) = rest.strip_prefix('(').and_then(|r| r.strip_suffix(')')) else {
            cd.problems.push(problem(number_at, crate::i18n::tf!("{name} needs its id and lengths in brackets after it, such as (//) or (0/1/40).")));
            // The line after is its string all the same.
            let _ = lines.string();
            continue;
        };
        let parts: Vec<&str> = inside.split('/').collect();
        if parts.len() != 3 {
            cd.problems.push(problem(number_at, crate::i18n::tf!("{name}'s brackets need three parts, kept apart by two slashes: (id/shortest/longest), any of them left out.")));
        }
        let part = |n: usize| parts.get(n).map(|p| p.trim()).filter(|p| !p.is_empty());
        let id = match part(0) {
            None => next_id,
            Some(relative) if relative.starts_with('+') => match number(&relative[1..]) {
                Some(n) => next_id.saturating_add(n),
                None => {
                    cd.problems.push(problem(number_at, crate::i18n::tf!("{name}'s id, {relative}, isn't a number.")));
                    next_id
                }
            },
            Some(given) => number(given).unwrap_or_else(|| {
                cd.problems.push(problem(number_at, crate::i18n::tf!("{name}'s id, {given}, isn't a number.")));
                next_id
            }),
        };
        let mut length = |n: usize, what: &str| -> Option<usize> {
            let given = part(n)?;
            match number(given) {
                Some(value) => Some(value as usize),
                None => {
                    cd.problems.push(problem(number_at, crate::i18n::tf!("{name}'s {what} length, {given}, isn't a number.")));
                    None
                }
            }
        };
        let min = length(1, "shortest").unwrap_or(0);
        let max = length(2, "longest");
        next_id = id.saturating_add(1);
        let Some((_, string)) = lines.string() else {
            cd.problems.push(problem(number_at, crate::i18n::tf!("{name} has no string: the line after its name should be the string.")));
            break;
        };
        if let Some(earlier) = cd.entries.iter().find(|e| e.name == name) {
            cd.problems.push(problem(number_at, crate::i18n::tf!("{name} is already on line {}.", earlier.line)));
        } else if let Some(earlier) = cd.entries.iter().find(|e| e.id == id) {
            cd.problems.push(problem(number_at, crate::i18n::tf!("{name} has the id {id}, which {} already has.", earlier.name)));
        }
        cd.entries.push(Entry { name: name.to_string(), id, min, max, text: string, line: number_at });
    }
    for entry in &cd.entries {
        check_own_string(entry, &entry.text, entry.line, &mut cd.problems);
    }
    cd
}

/// Reads a catalog translation.
pub fn read_translation(text: &str) -> Translation {
    let mut ct = Translation::default();
    let mut lines = Lines::new(text);
    while let Some((number_at, line)) = lines.next() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with("//") {
            continue;
        }
        if let Some(command) = trimmed.strip_prefix('#') {
            let command = command.trim_start_matches('#').trim_start();
            let (name, value) = command.split_once(char::is_whitespace).unwrap_or((command, ""));
            let value = value.trim().to_string();
            match name.to_ascii_lowercase().as_str() {
                "version" => ct.version = Some(value),
                "language" => ct.language = Some(value),
                "codeset" => match value.parse() {
                    Ok(set) => ct.codeset = set,
                    Err(_) => ct.problems.push(problem(number_at, crate::i18n::tf!("The code set, {value}, isn't a number."))),
                },
                "rcsid" => ct.rcsid = Some(value),
                "name" => ct.name = Some(value),
                // FlexCat's own (## chunk, ## lengthbytes and others) are let through.
                _ => {}
            }
            continue;
        }
        // CatComp's own example writes a translation's name with empty brackets after it, so they're allowed.
        let name = trimmed.split_once('(').map_or(trimmed, |(n, _)| n).trim();
        if !is_name(name) {
            ct.problems.push(problem(number_at, crate::i18n::tf!("\u{201c}{name}\u{201d} isn't a string's name: a name is letters, digits and underscores, starting with a letter.")));
            continue;
        }
        let Some((_, string)) = lines.string() else {
            ct.problems.push(problem(number_at, crate::i18n::tf!("{name} has no string: the line after its name should be the translation.")));
            break;
        };
        if let Some(earlier) = ct.entries.iter().find(|e| e.name == name) {
            ct.problems.push(problem(number_at, crate::i18n::tf!("{name} is already on line {}.", earlier.line)));
            continue;
        }
        ct.entries.push(Translated { name: name.to_string(), text: string, line: number_at });
    }
    ct
}

/// A string's escapes worked out, to its bytes in Latin 1: CatComp's table, with C's `\\`, `\"` and `\'`.
pub fn unescape(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let byte = |c: char| -> Result<u8, String> { u8::try_from(u32::from(c)).map_err(|_| crate::i18n::tf!("\u{201c}{c}\u{201d} isn't in Latin 1 (ISO-8859-1), so a catalog can't hold it.")) };
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(byte(c)?);
            continue;
        }
        let Some(next) = chars.next() else {
            out.push(b'\\');
            break;
        };
        match next {
            'a' => out.push(7),
            'b' => out.push(8),
            'c' => out.push(155),
            'e' => out.push(27),
            'f' => out.push(12),
            'n' => out.push(10),
            'r' => out.push(13),
            't' => out.push(9),
            'v' => out.push(11),
            'x' | 'X' => {
                let mut value = 0u32;
                let mut digits = 0;
                while digits < 2
                    && let Some(d) = chars.peek().and_then(|c| c.to_digit(16))
                {
                    value = value * 16 + d;
                    chars.next();
                    digits += 1;
                }
                if digits == 0 {
                    return Err(crate::i18n::tr("\\x needs a hexadecimal number after it, such as \\x9B.").into());
                }
                out.push(value as u8);
            }
            '0'..='7' => {
                let mut value = next.to_digit(8).unwrap_or(0);
                let mut digits = 1;
                while digits < 3
                    && let Some(d) = chars.peek().and_then(|c| c.to_digit(8))
                {
                    value = value * 8 + d;
                    chars.next();
                    digits += 1;
                }
                if value > 255 {
                    return Err(crate::i18n::tf!("\\{value:o} is more than a byte can hold."));
                }
                out.push(value as u8);
            }
            other => out.push(byte(other)?),
        }
    }
    Ok(out)
}

/// One of a string's % formatting commands: which argument it takes (from 1), and what kind of thing that is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Format {
    pub argument: usize,
    /// Its kind, so that two can be compared: the conversion's family and whether it's a longword (`l`). RawDoFmt()
    /// takes a word for %d unless it's %ld, so %d and %ld don't take the same thing.
    pub kind: String,
    /// As it's written, to say.
    pub written: String,
}

/// The conversions CatComp knows (its documentation's list), with RawDoFmt()'s %D and %U.
const CONVERSIONS: &str = "bcdeEfgGiopsuxXNTDU";

/// A string's % formatting commands, and any it doesn't know.
pub fn formats(text: &str) -> (Vec<Format>, Vec<String>) {
    let mut found = Vec::new();
    let mut unknown = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut next_argument = 1;
    while i < chars.len() {
        if chars[i] != '%' {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        if chars.get(i) == Some(&'%') {
            i += 1;
            continue;
        }
        // An argument's place, as in %2$s.
        let mut place = None;
        let digits: String = chars[i..].iter().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() && chars.get(i + digits.len()) == Some(&'$') {
            place = digits.parse::<usize>().ok();
            i += digits.len() + 1;
        }
        while i < chars.len() && "-+ #0'".contains(chars[i]) {
            i += 1;
        }
        while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == '*') {
            i += 1;
        }
        let mut long = false;
        while i < chars.len() && "lhL".contains(chars[i]) {
            long |= chars[i] != 'h';
            i += 1;
        }
        let Some(&conversion) = chars.get(i) else {
            unknown.push(chars[start..].iter().collect());
            break;
        };
        i += 1;
        let written: String = chars[start..i].iter().collect();
        if !CONVERSIONS.contains(conversion) {
            unknown.push(written);
            continue;
        }
        let family = match conversion {
            'd' | 'i' | 'D' => "signed",
            'u' | 'U' | 'x' | 'X' | 'o' | 'c' => "unsigned",
            'e' | 'E' | 'f' | 'g' | 'G' => "floating",
            's' | 'N' | 'T' => "string",
            'b' => "bcpl string",
            _ => "pointer",
        };
        let argument = place.unwrap_or(next_argument);
        next_argument = argument + 1;
        let kind = if long && !matches!(family, "string" | "bcpl string" | "pointer") { format!("long {family}") } else { family.to_string() };
        found.push(Format { argument, kind, written });
    }
    (found, unknown)
}

/// The argument each place takes, by place.
fn arguments(found: &[Format]) -> std::collections::BTreeMap<usize, &Format> {
    found.iter().map(|f| (f.argument, f)).collect()
}

/// What's wrong with a string on its own: its escapes, its length, % commands nobody knows.
fn check_own_string(entry: &Entry, text: &str, line: usize, problems: &mut Vec<Problem>) {
    match unescape(text) {
        Ok(bytes) => {
            let length = bytes.len();
            if length < entry.min {
                problems.push(problem(line, crate::i18n::tf!("{} is {length} characters long, but it should be at least {}.", entry.name, entry.min)));
            }
            if let Some(max) = entry.max
                && length > max
            {
                problems.push(problem(line, crate::i18n::tf!("{} is {length} characters long, but it can be no more than {max}.", entry.name)));
            }
        }
        Err(why) => problems.push(problem(line, crate::i18n::tf!("{}: {why}", entry.name))),
    }
    for written in formats(text).1 {
        problems.push(warning(line, crate::i18n::tf!("{}: {written} isn't a formatting command CatComp knows.", entry.name)));
    }
}

/// What's wrong with a translation, taken with its description: strings missing or unknown, lengths outside their
/// limits, % commands that don't take the same arguments, and a "..." the original ends with and it doesn't. The
/// problems are the translation's lines.
pub fn check(cd: &Description, ct: &Translation) -> Vec<Problem> {
    let mut problems = ct.problems.clone();
    if ct.language.as_deref().is_none_or(str::is_empty) {
        problems.push(warning(1, crate::i18n::tr("There's no ## language line, so the catalog's language is taken from the file's name.").into()));
    }
    if ct.version.is_none() && ct.rcsid.is_none() {
        problems.push(warning(1, crate::i18n::tr("There's no ## version line, such as ## version $VER: program.catalog 1.0 (20.11.2026), so the catalog has no version.").into()));
    }
    for translated in &ct.entries {
        let Some(entry) = cd.entry(&translated.name) else {
            problems.push(problem(translated.line, crate::i18n::tf!("{} isn't in the description.", translated.name)));
            continue;
        };
        if translated.text.is_empty() {
            problems.push(warning(translated.line, crate::i18n::tf!("{} isn't translated yet.", entry.name)));
            continue;
        }
        check_own_string(entry, &translated.text, translated.line, &mut problems);
        let (original, _) = formats(&entry.text);
        let (theirs, _) = formats(&translated.text);
        let (mine, yours) = (arguments(&original), arguments(&theirs));
        if mine.len() != yours.len() {
            problems.push(problem(translated.line, crate::i18n::tf!("{} has {} formatting commands, but the original has {}.", entry.name, yours.len(), mine.len())));
        } else {
            for (place, format) in &mine {
                match yours.get(place) {
                    Some(other) if other.kind == format.kind => {}
                    Some(other) => problems.push(problem(translated.line, crate::i18n::tf!("{}: {} takes something different from the original's {}.", entry.name, other.written, format.written))),
                    None => problems.push(problem(translated.line, crate::i18n::tf!("{}: nothing takes argument {place}, which the original's {} takes.", entry.name, format.written))),
                }
            }
        }
        // A new line or spaces after the dots don't hide them.
        let dots = |t: &str| t.trim_end().trim_end_matches("\\n").trim_end().ends_with("...");
        if dots(&entry.text) && !dots(&translated.text) {
            problems.push(warning(translated.line, crate::i18n::tf!("{}: the original ends with \u{201c}...\u{201d}, but the translation doesn't.", entry.name)));
        }
    }
    // Strings missing altogether are said at the top, where the translation's commands are.
    for entry in &cd.entries {
        if ct.entry(&entry.name).is_none() {
            problems.push(warning(1, crate::i18n::tf!("{} isn't in the translation.", entry.name)));
        }
    }
    problems.sort_by_key(|p| p.line);
    problems
}

/// A translation to fill in, made from a description as CatComp's CTFILE makes one: each string's name, an empty
/// line for its translation, and the original as a comment.
pub fn blank_translation(cd: &Description, program: &str, language: &str, date: &str) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "## version $VER: {program}.catalog 1.0 ({date})");
    let _ = writeln!(out, "## language {language}");
    out.push_str("## codeset 0\n;\n");
    for entry in &cd.entries {
        let _ = writeln!(out, "{}\n\n; {}\n;", entry.name, entry.text);
    }
    out
}

/// Strings missing from a translation added to its end, each as a blank translation has it, so the translation
/// can be filled in as the program grows.
pub fn add_missing(cd: &Description, ct_text: &str) -> (String, usize) {
    let ct = read_translation(ct_text);
    let mut out = ct_text.to_string();
    let mut added = 0;
    for entry in cd.entries.iter().filter(|e| ct.entry(&e.name).is_none()) {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        let _ = writeln!(out, "{}\n\n; {}\n;", entry.name, entry.text);
        added += 1;
    }
    (out, added)
}

/// A string as C source writes it: printable ASCII as it is, and anything else (Latin 1's accented letters among
/// it) as a three-figure octal escape, which can't run on into the next character as \x can.
fn c_string(bytes: &[u8]) -> String {
    let mut out = String::from("\"");
    for &b in bytes {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'?' => out.push_str("\\?"),
            0x20..=0x7E => out.push(b as char),
            _ => {
                let _ = write!(out, "\\{b:03o}");
            }
        }
    }
    out.push('"');
    out
}

/// The length bytes CatComp's `#lengthbytes` puts before a string: its length, high byte first, in that many bytes.
fn length_prefix(length: usize, bytes: usize) -> Vec<u8> {
    let length = length as u64;
    (0..bytes).rev().map(|n| (length >> (8 * n)) as u8).collect()
}

/// The strings' C header, made from a description in the shape CatComp's CFILE has: the sections a program picks
/// by defining CATCOMP_NUMBERS (each name's id), CATCOMP_STRINGS (each name's string, as NAME_STR), CATCOMP_ARRAY
/// (a table of both) and CATCOMP_CODE (GetString(), which asks locale.library for the translation and gives the
/// program's own string when there's none). `stem` names it: hello.cd's header is hello_strings.h, guarded by
/// HELLO_STRINGS_H, unless #header names it.
pub fn c_header(cd: &Description, stem: &str) -> String {
    let guard = cd.command("header").map(str::to_string).unwrap_or_else(|| format!("{}_STRINGS", stem.to_ascii_uppercase().replace(|c: char| !c.is_ascii_alphanumeric(), "_")));
    let array = cd.command("array").unwrap_or("CatCompArray");
    let options = cd.command("arrayopts").unwrap_or("static const");
    let function = cd.command("function").unwrap_or("GetString");
    let length_bytes = cd.length_bytes();
    let mut out = String::new();
    let _ = writeln!(out, "#ifndef {guard}_H\n#define {guard}_H\n");
    let _ = writeln!(out, "/* {} */\n", crate::i18n::tf!("Made by Limoniter from {stem}.cd, as CatComp makes its C header. Change {stem}.cd rather than this, which is made again from it."));
    out.push_str("#ifndef EXEC_TYPES_H\n#include <exec/types.h>\n#endif\n\n");
    out.push_str("#ifdef CATCOMP_CODE\n#ifndef CATCOMP_ARRAY\n#define CATCOMP_ARRAY\n#endif\n#endif\n\n");
    out.push_str("#ifdef CATCOMP_ARRAY\n#ifndef CATCOMP_NUMBERS\n#define CATCOMP_NUMBERS\n#endif\n#ifndef CATCOMP_STRINGS\n#define CATCOMP_STRINGS\n#endif\n#endif\n\n");
    out.push_str("#ifdef CATCOMP_NUMBERS\n\n");
    for entry in &cd.entries {
        let _ = writeln!(out, "#define {} {}", entry.name, entry.id);
    }
    out.push_str("\n#endif /* CATCOMP_NUMBERS */\n\n#ifdef CATCOMP_STRINGS\n\n");
    for entry in &cd.entries {
        let mut bytes = unescape(&entry.text).unwrap_or_else(|_| entry.text.bytes().collect());
        if length_bytes > 0 {
            let mut prefixed = length_prefix(bytes.len(), length_bytes);
            prefixed.append(&mut bytes);
            bytes = prefixed;
        }
        let _ = writeln!(out, "#define {}_STR {}", entry.name, c_string(&bytes));
    }
    out.push_str("\n#endif /* CATCOMP_STRINGS */\n\n#ifdef CATCOMP_ARRAY\n\n");
    out.push_str("struct CatCompArrayType\n{\n    LONG   cca_ID;\n    STRPTR cca_Str;\n};\n\n");
    let _ = writeln!(out, "{options} struct CatCompArrayType {array}[] =\n{{");
    for entry in &cd.entries {
        let _ = writeln!(out, "    {{{}, (STRPTR){}_STR}},", entry.name, entry.name);
    }
    out.push_str("};\n\n#endif /* CATCOMP_ARRAY */\n\n");
    // GetString() as CatComp writes it in spirit: LocaleBase is, for the call, the one in the LocaleInfo, so that
    // the same code works with each compiler's way of calling a library (SAS/C's pragmas, gcc's and vbcc's inlines).
    out.push_str("#ifdef CATCOMP_CODE\n\n");
    out.push_str("#ifndef PROTO_LOCALE_H\n#ifndef __NOLIBBASE__\n#define __NOLIBBASE__\n#define LIMONITER_NOLIBBASE\n#endif\n#include <proto/locale.h>\n#ifdef LIMONITER_NOLIBBASE\n#undef __NOLIBBASE__\n#undef LIMONITER_NOLIBBASE\n#endif\n#endif\n\n");
    out.push_str("struct LocaleInfo\n{\n    APTR li_LocaleBase;\n    APTR li_Catalog;\n};\n\n");
    let _ = writeln!(out, "STRPTR {function}(struct LocaleInfo *li, LONG stringNum)\n{{");
    out.push_str("    STRPTR builtIn = NULL;\n    ULONG i;\n\n");
    let _ = writeln!(out, "    for (i = 0; i < sizeof({array}) / sizeof({array}[0]); i++)\n    {{\n        if ({array}[i].cca_ID == stringNum)\n        {{\n            builtIn = {array}[i].cca_Str;\n            break;\n        }}\n    }}");
    out.push_str("    if (li == NULL || li->li_LocaleBase == NULL)\n        return builtIn;\n");
    out.push_str("#define XLocaleBase LocaleBase\n#define LocaleBase li->li_LocaleBase\n");
    out.push_str("    return (STRPTR)GetCatalogStr(li->li_Catalog, stringNum, builtIn);\n");
    out.push_str("#undef LocaleBase\n#define LocaleBase XLocaleBase\n#undef XLocaleBase\n}\n\n");
    out.push_str("#endif /* CATCOMP_CODE */\n\n");
    let _ = writeln!(out, "#endif /* {guard}_H */");
    out
}

/// A catalog as it's read back from its IFF form.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    pub version: Option<String>,
    pub language: Option<String>,
    pub codeset: u32,
    /// Each string's id and bytes, as they are in the file.
    pub strings: Vec<(u32, Vec<u8>)>,
}

/// A day's date as AmigaDOS version strings write it: 20.11.2026.
pub fn amiga_date(year: i32, month: u32, day: u32) -> String {
    format!("{day}.{month}.{year}")
}

/// The version string a translation gives: its ## version, or one made from ## rcsid (`$Date: 2001-07-08 ... $
/// $Revision: 1.1 $`) and ## name, as CatComp makes it.
pub fn version_string(ct: &Translation, file_name: &str) -> Option<String> {
    if let Some(version) = &ct.version {
        return Some(version.clone());
    }
    let rcs = ct.rcsid.as_deref()?;
    let field = |name: &str| {
        let at = rcs.find(&format!("${name}:"))? + name.len() + 2;
        let rest = &rcs[at..];
        Some(rest[..rest.find('$')?].trim().to_string())
    };
    let revision = field("Revision")?;
    let date = field("Date")?;
    let day: Vec<&str> = date.split_whitespace().next()?.split(['-', '/']).collect();
    let [year, month, dayn] = day[..] else { return None };
    let shown = amiga_date(year.parse().ok()?, month.parse().ok()?, dayn.parse().ok()?);
    let name = ct.name.clone().unwrap_or_else(|| file_name.to_string());
    Some(format!("$VER: {name} {revision} ({shown})"))
}

fn chunk(out: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(id);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
    if body.len() % 2 == 1 {
        out.push(0);
    }
}

/// A string as an IFF chunk holds one: its bytes and a zero, padded to an even length.
fn c_text(text: &str) -> Result<Vec<u8>, String> {
    let mut bytes = text.chars().map(|c| u8::try_from(u32::from(c)).map_err(|_| crate::i18n::tf!("\u{201c}{c}\u{201d} isn't in Latin 1 (ISO-8859-1), so a catalog can't hold it."))).collect::<Result<Vec<u8>, String>>()?;
    bytes.push(0);
    Ok(bytes)
}

/// Writes a translation's catalog. `file_name` is the catalog's own name (hello.catalog), for a version made from
/// ## rcsid; `language` is used when the translation has no ## language line; `optimise` leaves out strings the
/// same as the description's. Strings not translated are left out, so the program's own are used for them.
pub fn write_catalog(cd: &Description, ct: &Translation, file_name: &str, language: &str, optimise: bool) -> Result<Vec<u8>, String> {
    let mut body = b"CTLG".to_vec();
    if let Some(version) = version_string(ct, file_name) {
        chunk(&mut body, b"FVER", &c_text(&version)?);
    }
    let language = ct.language.clone().filter(|l| !l.is_empty()).unwrap_or_else(|| language.to_string());
    chunk(&mut body, b"LANG", &c_text(&language.to_lowercase())?);
    let mut set = ct.codeset.to_be_bytes().to_vec();
    set.resize(32, 0);
    chunk(&mut body, b"CSET", &set);
    let length_bytes = cd.length_bytes();
    let mut strings: Vec<(u32, Vec<u8>)> = Vec::new();
    for entry in &cd.entries {
        let Some(translated) = ct.entry(&entry.name) else { continue };
        if translated.text.is_empty() || optimise && translated.text == entry.text {
            continue;
        }
        let mut bytes = unescape(&translated.text).map_err(|why| crate::i18n::tf!("{} (line {}): {why}", entry.name, translated.line))?;
        if length_bytes > 0 {
            let mut prefixed = length_prefix(bytes.len(), length_bytes);
            prefixed.append(&mut bytes);
            bytes = prefixed;
        }
        strings.push((entry.id, bytes));
    }
    strings.sort_by_key(|(id, _)| *id);
    let mut strs = Vec::new();
    for (id, bytes) in &strings {
        // As CatComp 40.7 writes them (read from its code, in the 3.2 NDK): the length is the string's, and the
        // zeros after it bring the next to a longword, so a reader rounding the length up to a longword finds a
        // zero at the string's end. A string a whole number of longwords long has 1 added to its length for the
        // zero that's then needed.
        let length = if bytes.len() % 4 == 0 { bytes.len() + 1 } else { bytes.len() };
        strs.extend_from_slice(&id.to_be_bytes());
        strs.extend_from_slice(&(length as u32).to_be_bytes());
        strs.extend_from_slice(bytes);
        strs.resize(strs.len() + length.div_ceil(4) * 4 - bytes.len(), 0);
    }
    chunk(&mut body, b"STRS", &strs);
    let mut out = b"FORM".to_vec();
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    bytes.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// The text of a chunk holding a zero-ended string.
fn chunk_text(body: &[u8]) -> String {
    let end = body.iter().position(|&b| b == 0).unwrap_or(body.len());
    body[..end].iter().map(|&b| b as char).collect()
}

/// Reads a catalog back: an IFF FORM of type CTLG.
pub fn read_catalog(bytes: &[u8]) -> Result<Catalog, String> {
    if bytes.get(0..4) != Some(b"FORM") || bytes.get(8..12) != Some(b"CTLG") {
        return Err(crate::i18n::tr("That isn't a catalog: a catalog starts FORM, then CTLG.").into());
    }
    let size = be32(bytes, 4).unwrap_or(0) as usize;
    let end = (8 + size).min(bytes.len());
    let mut catalog = Catalog::default();
    let mut at = 12;
    while at + 8 <= end {
        let id = &bytes[at..at + 4];
        let length = be32(bytes, at + 4).unwrap_or(0) as usize;
        let body = bytes.get(at + 8..at + 8 + length).ok_or_else(|| crate::i18n::tr("The catalog ends partway through a chunk.").to_string())?;
        match id {
            b"FVER" => catalog.version = Some(chunk_text(body)),
            b"LANG" => catalog.language = Some(chunk_text(body)),
            b"CSET" => catalog.codeset = be32(body, 0).unwrap_or(0),
            b"STRS" => {
                let mut p = 0;
                while p + 8 <= body.len() {
                    let id = be32(body, p).unwrap_or(0);
                    let length = be32(body, p + 4).unwrap_or(0) as usize;
                    let text = body.get(p + 8..p + 8 + length).ok_or_else(|| crate::i18n::tr("A string runs past the end of the catalog's strings.").to_string())?;
                    let text = text.strip_suffix(&[0]).unwrap_or(text);
                    catalog.strings.push((id, text.to_vec()));
                    p += 8 + length.div_ceil(4) * 4;
                }
            }
            _ => {}
        }
        at += 8 + length + length % 2;
    }
    Ok(catalog)
}

/// Text read from a file that may be in Latin 1, as the Amiga's are, or in UTF-8.
pub fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// Text as the Amiga's files keep it, in Latin 1, with anything outside it as a question mark.
pub fn latin1(text: &str) -> Vec<u8> {
    text.chars().map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')).collect()
}

/// FlexCat, where it's installed: on the PATH, or where it's usually unpacked.
pub fn flexcat() -> Option<std::path::PathBuf> {
    crate::emulators::find_program(&["flexcat", "FlexCat"], &["flexcat"])
}

/// FlexCat's command line for a catalog, as its documentation gives it: `FlexCat hello.cd deutsch.ct CATALOG
/// Catalogs/deutsch/hello.catalog`.
pub fn flexcat_arguments(cd: &std::path::Path, ct: &std::path::Path, catalog: &std::path::Path) -> Vec<String> {
    vec![cd.display().to_string(), ct.display().to_string(), "CATALOG".into(), catalog.display().to_string()]
}

/// The language a translation is for: its ## language line, or else its file's name.
pub fn language_of(ct: &Translation, path: &std::path::Path) -> String {
    ct.language.clone().filter(|l| !l.is_empty()).unwrap_or_else(|| path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned())).to_lowercase()
}

/// Where a translation's catalog goes, as programs look for it: Catalogs/<language>/<program>.catalog beside the
/// description (PROGDIR:Catalogs, which locale.library looks in before LOCALE:Catalogs).
pub fn catalog_path(cd_path: &std::path::Path, language: &str) -> std::path::PathBuf {
    let folder = cd_path.parent().map(std::path::Path::to_path_buf).unwrap_or_default();
    let stem = cd_path.file_stem().map_or_else(|| "program".to_string(), |s| s.to_string_lossy().into_owned());
    folder.join("Catalogs").join(language).join(format!("{stem}.catalog"))
}

/// The translations that go with a description: the .ct files beside it and in the folders below it (a catalogs
/// or locale folder, say), in name order.
pub fn translations_for(cd_path: &std::path::Path) -> Vec<std::path::PathBuf> {
    let Some(folder) = cd_path.parent() else { return Vec::new() };
    let mut found = Vec::new();
    let mut waiting = vec![(folder.to_path_buf(), 0)];
    while let Some((next, depth)) = waiting.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && depth < 2 {
                waiting.push((path, depth + 1));
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ct")) {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The description a translation goes with: a .cd beside it, or in the folder above it (or the one above that). If
/// there are several, the one named in the translation's version string.
pub fn description_for(ct_path: &std::path::Path, ct: &Translation) -> Option<std::path::PathBuf> {
    let wanted = ct.version.as_deref().and_then(|v| v.split_whitespace().nth(1)).map(|name| name.trim_end_matches(".catalog").to_ascii_lowercase());
    let mut folder = ct_path.parent();
    for _ in 0..3 {
        let here = folder?;
        let mut found: Vec<std::path::PathBuf> = std::fs::read_dir(here).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("cd"))).collect();
        found.sort();
        if let Some(named) = found.iter().find(|p| wanted.as_ref().is_some_and(|w| p.file_stem().is_some_and(|s| s.to_string_lossy().to_ascii_lowercase() == *w))) {
            return Some(named.clone());
        }
        if let Some(first) = found.first() {
            return Some(first.clone());
        }
        folder = here.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const CD: &str = "; The strings of a test program.
#language english
#version 0
MSG_HELLO (0/4/50)
Hello, world
;
MSG_BYE (+1/1/12)
Goodbye\\n
;
MSG_FREE (//)
%ld bytes free on %s
MSG_LONG (//)
A string carried \\
on to the next line
MSG_MORE (//)
More...
MSG_HEX ($20//)
\\e[1mBold\\e[0m \\c \\x41\\101
";

    #[test]
    fn a_description_is_read_as_catcomp_documents_it() {
        let cd = read_description(CD);
        assert!(cd.problems.is_empty(), "{:?}", cd.problems);
        let ids: Vec<(&str, u32)> = cd.entries.iter().map(|e| (e.name.as_str(), e.id)).collect();
        // +1 adds one to the id it would have had, as the documentation's example has it.
        assert_eq!(ids, [("MSG_HELLO", 0), ("MSG_BYE", 2), ("MSG_FREE", 3), ("MSG_LONG", 4), ("MSG_MORE", 5), ("MSG_HEX", 32)]);
        let hello = cd.entry("MSG_HELLO").unwrap();
        assert_eq!((hello.min, hello.max, hello.line), (4, Some(50), 4));
        assert_eq!(cd.entry("MSG_FREE").unwrap().max, None);
        assert_eq!(cd.entry("MSG_LONG").unwrap().text, "A string carried on to the next line");
        assert_eq!(cd.command("language"), Some("english"));
        assert_eq!(unescape(&cd.entry("MSG_HEX").unwrap().text).unwrap(), b"\x1b[1mBold\x1b[0m \x9b AA");
        assert_eq!(unescape("Gr\u{fc}\u{df}e").unwrap(), [b'G', b'r', 0xFC, 0xDF, b'e']);
        assert!(unescape("\u{20ac}").is_err(), "the euro sign isn't in Latin 1");
    }

    #[test]
    fn mistakes_in_a_description_are_found() {
        let cd = read_description("MSG_A (0//)\nOne\nMSG_B (0//)\nTwo\nMSG_A (//)\nThree\nMSG_C\nFour\nMSG_D (//3)\nToo long\n9BAD (//)\nx\nMSG_E (//)\n%q\n");
        let said: Vec<(usize, bool)> = cd.problems.iter().map(|p| (p.line, p.warning)).collect();
        assert!(said.contains(&(3, false)), "the same id twice: {:?}", cd.problems);
        assert!(said.contains(&(5, false)), "the same name twice");
        assert!(said.contains(&(7, false)), "no brackets");
        assert!(said.contains(&(9, false)), "longer than its longest");
        assert!(said.contains(&(11, false)), "not a name");
        assert!(said.contains(&(13, true)), "%q isn't a formatting command");
    }

    #[test]
    fn formatting_commands_are_compared_by_the_arguments_they_take() {
        let (found, unknown) = formats("FAST: %lU, GRAPHIC: %lU, %d%% %-10s");
        assert!(unknown.is_empty());
        assert_eq!(found.iter().map(|f| (f.argument, f.kind.as_str())).collect::<Vec<_>>(), [(1, "long unsigned"), (2, "long unsigned"), (3, "signed"), (4, "string")]);
        // The documentation's own example: the order changed with 2$ and 1$.
        let (moved, _) = formats("GRAPHIQUE: %2$lU, AUTRE %1$lU");
        assert_eq!(moved.iter().map(|f| f.argument).collect::<Vec<_>>(), [2, 1]);
        let cd = read_description("MSG_FREE (//)\n%ld bytes free on %s\nMSG_TWO (//)\n%s and %s\n");
        let ct = read_translation("## version $VER: t.catalog 1.0 (1.1.2026)\n## language deutsch\nMSG_FREE\n%s: %ld Bytes frei\nMSG_TWO\n%2$s und %1$s\n");
        let problems = check(&cd, &ct);
        // Problems are given at the string's name.
        assert!(problems.iter().any(|p| p.line == 3 && !p.warning), "%s where %ld was: {problems:?}");
        assert!(!problems.iter().any(|p| p.line == 5), "reordered with $ is fine: {problems:?}");
        let ct = read_translation("## version x\n## language deutsch\nMSG_FREE\n%2$s: %1$ld Bytes frei\nMSG_TWO\nnur %s\n");
        let problems = check(&cd, &ct);
        assert!(!problems.iter().any(|p| p.line == 3), "{problems:?}");
        assert!(problems.iter().any(|p| p.line == 5 && !p.warning), "one argument short: {problems:?}");
        // %d is a word to RawDoFmt and %ld a longword, so they don't match.
        let cd = read_description("MSG_N (//)\n%ld files\n");
        let ct = read_translation("## version x\n## language deutsch\nMSG_N\n%d Dateien\n");
        assert!(check(&cd, &ct).iter().any(|p| !p.warning));
    }

    #[test]
    fn a_translation_is_checked_against_its_description() {
        let cd = read_description(CD);
        let ct = read_translation("## version $VER: test.catalog 1.0 (20.11.2026)\n## language deutsch\n## codeset 0\n;\nMSG_HELLO\nHallo, Welt\n; Hello, world\nMSG_BYE\nAuf Wiedersehen und tsch\u{fc}ss\\n\nMSG_FREE\n\nMSG_MORE\nMehr\nMSG_UNKNOWN\nWas?\nMSG_HELLO\nNochmal\n");
        assert_eq!(ct.language.as_deref(), Some("deutsch"));
        assert_eq!(ct.entry("MSG_HELLO").unwrap().line, 5);
        let problems = check(&cd, &ct);
        let at = |line: usize| problems.iter().filter(|p| p.line == line).map(|p| p.warning).collect::<Vec<_>>();
        assert_eq!(at(8), [false], "longer than 12: {problems:?}");
        assert_eq!(at(10), [true], "not translated yet");
        assert_eq!(at(12), [true], "lost its ...");
        let cd2 = read_description("MSG_WAIT (//)\nWait...\\n\n");
        assert!(check(&cd2, &read_translation("## version x\n## language deutsch\nMSG_WAIT\nWarte\\n\n")).iter().any(|p| p.warning && p.line == 3), "dots before a new line");
        assert!(check(&cd2, &read_translation("## version x\n## language deutsch\nMSG_WAIT\nWarte...\\n\n")).is_empty());
        assert_eq!(at(14), [false], "not in the description");
        assert_eq!(at(16), [false], "twice");
        assert!(problems.iter().any(|p| p.warning && p.message.contains("MSG_LONG")), "missing altogether: {problems:?}");
        assert!(at(6).is_empty(), "MSG_HELLO is fine");
    }

    #[test]
    fn a_catalog_is_written_as_locale_library_reads_it_and_reads_back() {
        let cd = read_description(CD);
        let ct = read_translation("## version $VER: test.catalog 1.2 (20.11.2026)\n## language Deutsch\n## codeset 0\nMSG_HELLO\nHallo, Welt\nMSG_BYE\nTsch\u{fc}ss\\n\nMSG_FREE\n%ld Bytes frei auf %s\nMSG_LONG\nA string carried \\\non to the next line\nMSG_MORE\n\nMSG_HEX\n\\e[1mFett\\e[0m\n");
        let bytes = write_catalog(&cd, &ct, "test.catalog", "deutsch", true).unwrap();
        // The IFF form's own size, and each chunk's, as the IFF standard has them.
        assert_eq!(&bytes[0..4], b"FORM");
        assert_eq!(be32(&bytes, 4).unwrap() as usize, bytes.len() - 8);
        assert_eq!(&bytes[8..16], b"CTLGFVER");
        let catalog = read_catalog(&bytes).unwrap();
        assert_eq!(catalog.version.as_deref(), Some("$VER: test.catalog 1.2 (20.11.2026)"));
        assert_eq!(catalog.language.as_deref(), Some("deutsch"), "language names are lower case, as LOCALE:Catalogs' folders are");
        assert_eq!(catalog.codeset, 0);
        // MSG_LONG is the same as the original, so it's left out; MSG_MORE isn't translated, so it's left out too.
        let ids: Vec<u32> = catalog.strings.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, [0, 2, 3, 32]);
        assert_eq!(catalog.strings[1].1, b"Tsch\xfcss\n");
        assert_eq!(catalog.strings[3].1, b"\x1b[1mFett\x1b[0m");
        // Each string's length is its own, and zeros bring the next to a longword, as CatComp writes them.
        let strs = bytes.windows(4).position(|w| w == b"STRS").unwrap() + 8;
        assert_eq!(be32(&bytes, strs), Some(0));
        assert_eq!(be32(&bytes, strs + 4), Some(11), "\"Hallo, Welt\"");
        assert_eq!(&bytes[strs + 8..strs + 20], b"Hallo, Welt\0");
        assert_eq!(be32(&bytes, strs + 20), Some(2), "the next string straight after, on a longword");
        // "Tsch\xfcss\n" is 8 long, two whole longwords, so its length counts the zero after it, and three more
        // zeros follow.
        assert_eq!(be32(&bytes, strs + 24), Some(9));
        assert_eq!(&bytes[strs + 28..strs + 40], b"Tsch\xfcss\n\0\0\0\0");
        assert_eq!(be32(&bytes, strs + 40), Some(3));
        let all = write_catalog(&cd, &ct, "test.catalog", "deutsch", false).unwrap();
        assert_eq!(read_catalog(&all).unwrap().strings.len(), 5, "without leaving out the same ones");
        // CSET is the code set and seven reserved longwords.
        let cset = bytes.windows(4).position(|w| w == b"CSET").unwrap();
        assert_eq!(be32(&bytes, cset + 4), Some(32));
        assert!(write_catalog(&cd, &read_translation("MSG_HELLO\n\u{20ac}uro\n"), "t.catalog", "x", true).is_err());
    }

    #[test]
    fn a_version_is_made_from_rcsid_as_catcomp_makes_it() {
        let ct = read_translation("## rcsid $Date: 2001-07-08 10:18:45 $ $Revision: 1.1 $\n## name myprogram.catalog\n");
        assert_eq!(version_string(&ct, "x.catalog").as_deref(), Some("$VER: myprogram.catalog 1.1 (8.7.2001)"));
    }

    #[test]
    fn the_c_header_has_catcomps_sections() {
        let cd = read_description("#header HELLO\nMSG_HELLO (5//)\nGr\u{fc}\u{df} \"dich\"\\n\nMSG_NEXT (//)\nNext?\n");
        let header = c_header(&cd, "hello");
        assert!(header.starts_with("#ifndef HELLO_H\n#define HELLO_H\n"));
        assert!(header.contains("#define MSG_HELLO 5\n#define MSG_NEXT 6\n"));
        assert!(header.contains("#define MSG_HELLO_STR \"Gr\\374\\337 \\\"dich\\\"\\012\"\n"), "{header}");
        assert!(header.contains("#define MSG_NEXT_STR \"Next\\?\""), "no trigraphs");
        assert!(header.contains("static const struct CatCompArrayType CatCompArray[] =\n{\n    {MSG_HELLO, (STRPTR)MSG_HELLO_STR},"));
        assert!(header.contains("STRPTR GetString(struct LocaleInfo *li, LONG stringNum)"));
        assert_eq!(header.matches("#if").count(), header.matches("#endif").count(), "balanced");
        let cd = read_description("MSG_A (//)\nx\n");
        assert!(c_header(&cd, "my-prog").starts_with("#ifndef MY_PROG_STRINGS_H"));
    }

    #[test]
    fn a_blank_translation_has_every_string_to_fill_in() {
        let cd = read_description(CD);
        let blank = blank_translation(&cd, "test", "deutsch", "20.11.2026");
        let ct = read_translation(&blank);
        assert_eq!(ct.entries.len(), cd.entries.len());
        assert!(ct.entries.iter().all(|e| e.text.is_empty()));
        assert_eq!(ct.language.as_deref(), Some("deutsch"));
        let (grown, added) = add_missing(&cd, "## language deutsch\nMSG_HELLO\nHallo, Welt\n");
        assert_eq!(added, 5);
        assert_eq!(read_translation(&grown).entry("MSG_HELLO").unwrap().text, "Hallo, Welt");
        assert_eq!(read_translation(&grown).entries.len(), 6);
    }

    #[test]
    fn translations_and_descriptions_find_each_other() {
        let dir = std::env::temp_dir().join(format!("limoniter-catalog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("catalogs")).unwrap();
        std::fs::write(dir.join("hello.cd"), "MSG_A (//)\nx\n").unwrap();
        std::fs::write(dir.join("catalogs").join("deutsch.ct"), "## language deutsch\nMSG_A\ny\n").unwrap();
        std::fs::write(dir.join("italiano.ct"), "MSG_A\nz\n").unwrap();
        let found = translations_for(&dir.join("hello.cd"));
        assert_eq!(found.len(), 2);
        let ct_path = dir.join("catalogs").join("deutsch.ct");
        assert_eq!(description_for(&ct_path, &read_translation("")), Some(dir.join("hello.cd")));
        assert_eq!(catalog_path(&dir.join("hello.cd"), "deutsch"), dir.join("Catalogs").join("deutsch").join("hello.catalog"));
        assert_eq!(language_of(&read_translation("MSG_A\nz\n"), &dir.join("Italiano.ct")), "italiano");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
