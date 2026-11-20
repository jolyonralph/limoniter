//! The words Limoniter shows, in the language chosen: English as it's written in the code, or French, German,
//! Italian or Spanish from the tables in `data/lang`, each line of which is the English, a tab, and the
//! translation. Fixed words go through `tr`, words with values in them through `tf!` (`format!` translated), and
//! the comments in the source Limoniter writes (starters, players, programs to run) through `comments`. Anything
//! a table doesn't have stays in English.
//!
//! Languages are listed under their own names (Deutsch, not German), so whichever is chosen, the way back can
//! always be read.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

pub use limoniter_macros::tf;

/// A language: its code (as the settings keep it), its own name for itself, and its table.
pub struct Language {
    pub code: &'static str,
    pub name: &'static str,
    table: &'static str,
}

/// English first; then the others by their own names.
pub const LANGUAGES: [Language; 5] = [
    Language { code: "en", name: "English", table: "" },
    Language { code: "de", name: "Deutsch", table: include_str!("../data/lang/de.tsv") },
    Language { code: "es", name: "Espa\u{F1}ol", table: include_str!("../data/lang/es.tsv") },
    Language { code: "fr", name: "Fran\u{E7}ais", table: include_str!("../data/lang/fr.tsv") },
    Language { code: "it", name: "Italiano", table: include_str!("../data/lang/it.tsv") },
];

#[cfg(test)]
thread_local! {
    /// A table for this thread's test alone, in place of the language chosen.
    pub static OVERRIDE: std::cell::RefCell<Option<HashMap<String, String>>> = const { std::cell::RefCell::new(None) };
}

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static TABLES: [OnceLock<HashMap<&'static str, &'static str>>; 5] = [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()];

/// Chooses a language by its code; one not known is English.
pub fn set(code: &str) {
    CURRENT.store(LANGUAGES.iter().position(|l| l.code == code).unwrap_or(0), Ordering::Relaxed);
}

/// A table's escapes undone: `\n` a new line, `\t` a tab, `\\` a backslash.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A table read: each line's English and its translation. Lines starting # with no tab in them are notes.
pub fn read_table(table: &str) -> Vec<(String, String)> {
    table
        .lines()
        .filter(|l| !(l.starts_with('#') && !l.contains('\t')) && !l.trim().is_empty())
        .filter_map(|l| l.split_once('\t'))
        .map(|(english, translated)| (unescape(english), unescape(translated)))
        .filter(|(_, t)| !t.is_empty())
        .collect()
}

fn table(index: usize) -> &'static HashMap<&'static str, &'static str> {
    TABLES[index].get_or_init(|| {
        // Read once and kept for good: the words are needed for as long as the window is open.
        read_table(LANGUAGES[index].table).into_iter().map(|(e, t)| (&*Box::leak(e.into_boxed_str()), &*Box::leak(t.into_boxed_str()))).collect()
    })
}

/// The translation of some English, in the language chosen, if there is one.
pub fn translation(english: &str) -> Option<&'static str> {
    #[cfg(test)]
    if let Some(t) = OVERRIDE.with(|o| o.borrow().as_ref().map(|table| table.get(english).map(|t| &*Box::leak(t.clone().into_boxed_str())))) {
        return t;
    }
    let index = CURRENT.load(Ordering::Relaxed);
    if index == 0 {
        return None;
    }
    table(index).get(english).copied()
}

/// Some English in the language chosen, or as it is.
pub fn tr(english: &str) -> &str {
    translation(english).unwrap_or(english)
}

/// A translated format string filled in: each placeholder (as the English wrote it) given its value; unnamed
/// ones (`{}`) in turn. `{{` and `}}` are braces.
pub fn fill(template: &str, pieces: &[(&str, String)]) -> String {
    let mut used = vec![false; pieces.len()];
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }
        if tail.starts_with('{')
            && let Some(end) = tail.find('}')
        {
            let key = &tail[..=end];
            match pieces.iter().enumerate().find(|(n, (k, _))| *k == key && !used[*n]) {
                Some((n, (_, value))) => {
                    used[n] = true;
                    out.push_str(value);
                }
                None => match pieces.iter().find(|(k, _)| *k == key) {
                    // A named value used twice in the translation.
                    Some((_, value)) => out.push_str(value),
                    None => out.push_str(key),
                },
            }
            rest = &tail[end + 1..];
            continue;
        }
        out.push_str(&tail[..1]);
        rest = &tail[1..];
    }
    out.push_str(rest);
    out
}

/// Where a line of source's comment starts, if it has one: after `;` (not in quotes) for assembler, `//` for C and
/// Rust, or a `*` starting the line, as 68000 source writes whole-line comments. The index of its text.
pub fn comment_start(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    // BASIC's ' at the start of a line (Boriel's ZX Basic writes comments that way), and 68000 source's *.
    if trimmed.starts_with("* ") || trimmed.starts_with("' ") {
        return Some(line.len() - trimmed.len() + 2);
    }
    let mut quote: Option<char> = None;
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    for (k, &(i, c)) in chars.iter().enumerate() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '"' => quote = Some('"'),
                '\'' if chars.get(k + 2).is_some_and(|&(_, end)| end == '\'') => quote = Some('\''),
                ';' => return Some(i + 1),
                '/' if chars.get(k + 1).is_some_and(|&(_, next)| next == '/') => return Some(i + 2),
                _ => {}
            },
        }
    }
    None
}

/// Source Limoniter writes, with its comments in the language chosen: each comment that's in the table, line by
/// line. The code is never touched.
pub fn comments(source: &str) -> String {
    #[cfg(test)]
    let english = CURRENT.load(Ordering::Relaxed) == 0 && OVERRIDE.with(|o| o.borrow().is_none());
    #[cfg(not(test))]
    let english = CURRENT.load(Ordering::Relaxed) == 0;
    if english {
        return source.to_string();
    }
    let mut out = String::with_capacity(source.len() + source.len() / 8);
    for (n, line) in source.split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        let Some(at) = comment_start(line) else {
            out.push_str(line);
            continue;
        };
        let (code, comment) = line.split_at(at);
        let text = comment.trim();
        let spaces = &comment[..comment.len() - comment.trim_start().len()];
        let ending = if comment.ends_with('\r') { "\r" } else { "" };
        match translation(text) {
            Some(t) => {
                out.push_str(code);
                out.push_str(spaces);
                out.push_str(t);
                out.push_str(ending);
            }
            None => out.push_str(line),
        }
    }
    out
}

#[cfg(test)]
mod tests;
