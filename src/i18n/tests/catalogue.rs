//! The catalogue: every piece of English Limoniter translates, found in its own source. The words given to `tr`
//! and `tf!`; the menus' and settings' labels; window titles kept in constants; and the comments in the source it
//! writes (the starters, players and programs), which `comments` translates line by line. Each language's table
//! must have them all.

use super::super::*;

mod through;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// A Rust string literal's value, from just after its opening quote; and where it ends.
fn literal_at(text: &str, start: usize) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    let mut out = String::new();
    let mut i = start;
    let chars: Vec<(usize, char)> = text[start..].char_indices().map(|(n, c)| (n + start, c)).collect();
    let mut k = 0;
    while k < chars.len() {
        let (at, c) = chars[k];
        match c {
            '"' => return Some((out, at + 1)),
            '\\' => {
                let (_, next) = *chars.get(k + 1)?;
                k += 2;
                match next {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    '0' => out.push('\0'),
                    '\\' | '"' | '\'' => out.push(next),
                    '\n' | '\r' => {
                        while chars.get(k).is_some_and(|(_, c)| c.is_whitespace()) {
                            k += 1;
                        }
                    }
                    'u' => {
                        let mut hex = String::new();
                        k += 1;
                        while let Some(&(_, h)) = chars.get(k) {
                            k += 1;
                            if h == '}' {
                                break;
                            }
                            hex.push(h);
                        }
                        out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                    }
                    'x' => {
                        let hex: String = [chars.get(k)?.1, chars.get(k + 1)?.1].iter().collect();
                        k += 2;
                        out.push(char::from(u8::from_str_radix(&hex, 16).ok()?));
                    }
                    _ => return None,
                }
                continue;
            }
            _ => out.push(c),
        }
        k += 1;
        i = at;
    }
    let _ = (bytes, i);
    None
}

/// Whether some English is worth a translation: a word of two letters or more, outside its placeholders.
fn wordy(text: &str) -> bool {
    let mut outside = String::new();
    let mut depth = 0;
    for c in text.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = (depth - 1).max(0),
            _ if depth == 0 => outside.push(c),
            _ => {}
        }
    }
    outside.split(|c: char| !c.is_alphabetic()).any(|w| w.chars().count() >= 2)
}

/// The string literals in Rust source, outside its comments: each one's value, and the code just before it.
fn literals(text: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut code = String::new();
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && next == Some('*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        // A raw string: r"..." or r#"..."#.
        if c == 'r' && (next == Some('"') || next == Some('#')) && !chars.get(i.wrapping_sub(1)).is_some_and(|p| p.is_alphanumeric() || *p == '_') {
            let mut j = i + 1;
            let mut hashes = 0;
            while chars.get(j) == Some(&'#') {
                hashes += 1;
                j += 1;
            }
            if chars.get(j) == Some(&'"') {
                let start = j + 1;
                let mut k = start;
                loop {
                    if k >= chars.len() {
                        break;
                    }
                    if chars[k] == '"' && (0..hashes).all(|h| chars.get(k + 1 + h) == Some(&'#')) {
                        break;
                    }
                    k += 1;
                }
                let value: String = chars[start..k.min(chars.len())].iter().collect();
                out.push((value, code.clone()));
                code.clear();
                i = k + 1 + hashes;
                continue;
            }
        }
        // A character: 'x', '\n', '\u{..}'; not a lifetime.
        if c == '\'' {
            if next == Some('\\') {
                let mut k = i + 2;
                while k < chars.len() && chars[k] != '\'' {
                    k += 1;
                }
                i = k + 1;
                continue;
            }
            if chars.get(i + 2) == Some(&'\'') {
                i += 3;
                continue;
            }
        }
        if c == '"' {
            let byte_start: usize = chars[..i + 1].iter().map(|c| c.len_utf8()).sum();
            if let Some((value, end)) = literal_at(text, byte_start) {
                out.push((value, code.clone()));
                code.clear();
                // Back to characters from bytes.
                i = text[..end].chars().count();
                continue;
            }
        }
        code.push(c);
        if code.len() > 40 {
            code.drain(..code.len() - 40);
        }
        i += 1;
    }
    out
}

/// The values of the literals right after each of the marks.
fn after(text: &str, mark: &str, out: &mut BTreeSet<String>) {
    for (at, _) in text.match_indices(mark) {
        let start = at + mark.len();
        if let Some((value, _)) = literal_at(text, start)
            && wordy(&value)
        {
            out.insert(value);
        }
    }
}

/// The comments a piece of generated source has that `comments` would look up: whole sentences, not ones
/// with values put in them.
fn source_comments(text: &str, out: &mut BTreeSet<String>) {
    for line in text.lines() {
        let trimmed = line.trim_start();
        // Assembler, as Limoniter writes it: a comment on its own, or after code that starts with a label or an
        // indented instruction.
        if trimmed.is_empty() {
            continue;
        }
        let Some(at) = comment_start(line) else { continue };
        let comment = line[at..].trim();
        let code = &line[..at.saturating_sub(1)];
        let looks_like_code = code.trim().is_empty() || code.starts_with(' ') || code.starts_with('\t') || code.split_whitespace().next().is_some_and(|w| w.chars().all(|c| c.is_alphanumeric() || "_.:@!*=".contains(c)));
        if looks_like_code && wordy(comment) && !comment.contains(['{', '}']) && comment.split_whitespace().count() >= 2 {
            out.insert(comment.to_string());
        }
    }
}

/// Every piece of English to translate.
pub fn catalogue() -> BTreeSet<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&root.join("src"), &mut files);
    let mut out = BTreeSet::new();
    let mut texts: Vec<(String, String)> = Vec::new();
    for path in &files {
        // With forward slashes on Windows too.
        let name = path.to_string_lossy().replace('\\', "/");
        // Tests' own words are never shown.
        if name.contains("/tests") || name.ends_with("_tests.rs") || name.contains("/i18n") {
            continue;
        }
        // As Git leaves it on Windows too, with \r\n line ends, it's read with \n.
        let text = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
        // Up to the tests at the end of a file.
        let text = text.split("\n#[cfg(test)]\nmod tests {").next().unwrap_or(&text);
        texts.push((name.clone(), text.to_string()));
        after(text, "i18n::tr(\"", &mut out);
        after(text, "i18n::tf!(\"", &mut out);
        // The words given to tf! as values, which it translates too.
        for (at, _) in text.match_indices("i18n::tf!(") {
            let start = at + "i18n::tf!(".len();
            let mut depth = 1;
            let mut end = start;
            let mut in_string = false;
            let mut escaped = false;
            for (i, c) in text[start..].char_indices() {
                if in_string {
                    match (escaped, c) {
                        (true, _) => escaped = false,
                        (false, '\\') => escaped = true,
                        (false, '"') => in_string = false,
                        _ => {}
                    }
                    continue;
                }
                match c {
                    '"' => in_string = true,
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = start + i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            for (value, _) in literals(&text[start..end]).into_iter().skip(1) {
                if wordy(&value) {
                    out.insert(value);
                }
            }
        }
        after(text, "message(\"", &mut out);
        if name.ends_with("menus.rs") {
            for mark in ["Item(\"", "Keyed(\"", "Check(\"", "Sub(\"", "Live(\"", "(\""] {
                after(text, mark, &mut out);
            }
        }
        if name.ends_with("settings.rs") {
            for mark in ["Row::Flag(\"", "Row::Special(\"", "Row::Heading(\"", "        \""] {
                after(text, mark, &mut out);
            }
            // A flag's description, its second string.
            for (at, _) in text.match_indices("Row::Flag(\"") {
                if let Some((_, end)) = literal_at(text, at + 11)
                    && let Some(open) = text[end..].find('"')
                    && let Some((tip, _)) = literal_at(text, end + open + 1)
                {
                    out.insert(tip);
                }
            }
        }
        // Names that types give themselves for showing (a machine's, a layout's), as `name()` and its kin
        // return them: the literals in those functions' bodies.
        for sig in ["fn name(self) -> &'static str {", "fn name(&self) -> &'static str {", "fn label(self) -> &'static str {", "fn title(self) -> &'static str {", "fn words(self) -> &'static str {"] {
            for (at, _) in text.match_indices(sig) {
                let body_start = at + sig.len();
                let mut depth = 1;
                let mut end = body_start;
                for (i, c) in text[body_start..].char_indices() {
                    match c {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = body_start + i;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                for (value, _) in literals(&text[body_start..end]) {
                    if wordy(&value) {
                        out.insert(value);
                    }
                }
            }
        }
        // Labels kept beside the values they choose, (Kind::This, "Its label"), for lists that show them.
        for (value, before) in literals(text) {
            let before = before.trim_end();
            if let Some(head) = before.strip_suffix(',')
                && let Some(open) = head.rfind('(')
                && head[open + 1..].trim().contains("::")
                && head[open + 1..].trim().chars().all(|c| c.is_alphanumeric() || c == ':' || c == '_')
                && wordy(&value)
            {
                out.insert(value);
            }
        }
        // Tables of labels with what they choose, ("Open…", "What it does", Asked::Open): every literal of a
        // bracketed list that also names a value of some type.
        let mut from = 0;
        while let Some(open) = text[from..].find('(') {
            let start = from + open + 1;
            from = start;
            let Some(close) = text[start..].find([')', '(']) else { break };
            if !text[start..].starts_with('"') || text.as_bytes()[start + close] != b')' {
                continue;
            }
            let inside = &text[start..start + close];
            if inside.contains("::") && !inside.contains('\n') {
                for (value, _) in literals(inside) {
                    if wordy(&value) {
                        out.insert(value);
                    }
                }
            }
        }
        // Constants that are translated where they're used, as window titles are.
        for (at, _) in text.match_indices("const ") {
            let rest = &text[at + 6..];
            let Some(colon) = rest.find(": &str = \"") else { continue };
            let name = &rest[..colon];
            if name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                && (text.contains(&format!("tr({name})")) || text.contains(&format!("message({name}")) || text.contains("const TITLE"))
                && let Some((value, _)) = literal_at(text, at + 6 + colon + 10)
                && wordy(&value)
                && (name == "TITLE" || text.contains(&format!("tr({name})")))
            {
                out.insert(value);
            }
        }
        // Generated source in string literals: its comments.
        for (value, before) in literals(text) {
            let translated_call = before.ends_with("tr(") || before.ends_with("tf!(");
            if !translated_call && (value.contains('\n') || value.starts_with(' ') || value.starts_with(';')) {
                source_comments(&value, &mut out);
            }
        }
    }
    through::through_variables(&texts, &mut out);
    // The starters File, New opens.
    for entry in std::fs::read_dir(root.join("src/starters")).unwrap().flatten() {
        if let Ok(text) = std::fs::read_to_string(entry.path()) {
            source_comments(&text.replace("\r\n", "\n"), &mut out);
        }
    }
    out.retain(|s| wordy(s) && s.len() < 2000);
    out
}

/// Writes the catalogue to `data/lang/catalogue.txt`, one piece a line (escaped as the tables are), for
/// translating: `cargo test -- --ignored write_the_catalogue`.
#[test]
#[ignore]
fn write_the_catalogue() {
    let escape = |s: &str| s.replace('\\', "\\\\").replace('\n', "\\n").replace('\t', "\\t");
    let lines: Vec<String> = catalogue().iter().map(|s| escape(s)).collect();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/lang/catalogue.txt");
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    eprintln!("{} pieces in {}", lines.len(), path.display());
}

#[test]
fn every_piece_of_english_is_in_every_language() {
    let wanted = catalogue();
    assert!(wanted.len() > 2000, "the catalogue found {}", wanted.len());
    assert!(wanted.contains("Open\u{2026}") || wanted.contains("Settings"), "it finds the menus' words");
    for language in &LANGUAGES[1..] {
        let have: std::collections::HashSet<String> = read_table(language.table).into_iter().map(|(e, _)| e).collect();
        let missing: Vec<&String> = wanted.iter().filter(|w| !have.contains(*w)).collect();
        assert!(missing.is_empty(), "{} is missing {} of {}, such as {:?}", language.name, missing.len(), wanted.len(), missing.iter().take(8).collect::<Vec<_>>());
    }
}
