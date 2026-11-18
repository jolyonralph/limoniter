//! Words that reach `tr` through a variable rather than written inside it: a label and tip taken from a table
//! of tools, `(label, hint)` chosen by an `if`, a block's name from a list of them, or a function's parameter
//! given words wherever it's called. Each `tr(name)` is followed back to where `name` comes from.

use super::{literals, wordy};
use regex::Regex;
use std::collections::BTreeSet;

/// How far back a variable is looked for.
const REACH: usize = 8000;

/// From `at`, the expression up to the `;` (or, for a loop, the `{` of its body) that ends it at its own depth.
fn statement(text: &str, at: usize, loop_body: bool) -> &str {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    for (i, c) in text[at..].char_indices() {
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
            '{' if depth == 0 && loop_body => return &text[at..at + i],
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth < 0 {
                    return &text[at..at + i];
                }
            }
            ';' if depth == 0 => return &text[at..at + i],
            _ => {}
        }
    }
    &text[at..]
}

/// A call's arguments, split at its own commas, from just after its `(`.
fn arguments(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start, mut in_string, mut escaped) = (0i32, 0, false, false);
    for (i, c) in text.char_indices() {
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
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth == 0 => {
                out.push(text[start..i].trim());
                return out;
            }
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(text[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out
}

/// Whether a literal is an identifier rather than words: `markdown-view`, an id for egui.
fn an_id(value: &str) -> bool {
    !value.contains(' ') && value.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') && value.contains(['-', '_'])
}

/// The words among some code's literals: not byte strings (b"PICT"), and not identifiers.
fn words(code: &str, out: &mut BTreeSet<String>) {
    for (value, before) in literals(code) {
        let byte_string = before.ends_with('b') && !before[..before.len() - 1].ends_with(|c: char| c.is_alphanumeric() || c == '_');
        if wordy(&value) && !an_id(&value) && !byte_string {
            out.insert(value);
        }
    }
}

/// The words in an expression, and in the constants and statics of the same file that it names.
fn words_in(expression: &str, file: &str, out: &mut BTreeSet<String>) {
    words(expression, out);
    let constant = Regex::new(r"\b([A-Z][A-Z0-9_]{2,})\b").unwrap();
    for name in constant.captures_iter(expression).map(|c| c[1].to_string()).collect::<BTreeSet<_>>() {
        let definition = Regex::new(&format!(r"(?:const|static)\s+{name}\s*:[^=]*=")).unwrap();
        if let Some(found) = definition.find(file) {
            words(statement(file, found.end(), false), out);
        }
    }
}

/// Every piece of English given to `tr` through a variable, in all the files.
pub fn through_variables(files: &[(String, String)], out: &mut BTreeSet<String>) {
    let site = Regex::new(r"i18n::tr\(([a-z_][a-z_0-9]*)\)").unwrap();
    for (_, text) in files {
        for found in site.captures_iter(text) {
            let whole = found.get(0).unwrap();
            let name = &found[1];
            let from = whole.start().saturating_sub(REACH);
            let from = (0..=from).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
            let back = &text[from..whole.start()];
            // Where it's bound: the nearest of a let, a for, a closure's parameters and the function's own.
            let binding = [
                (Regex::new(&format!(r"\blet\s+(?:mut\s+)?[(\[]?[^=;]*\b{name}\b[^=;]*?[^=!<>]=[^=>]")).unwrap(), 0),
                (Regex::new(&format!(r"\bfor\s+[^;{{]*\b{name}\b[^;{{]*?\bin\b")).unwrap(), 1),
                (Regex::new(&format!(r"\|[^|\n]*\b{name}\b[^|\n]*\|")).unwrap(), 2),
                (Regex::new(&format!(r"\bfn\s+([a-z_][a-z_0-9]*)\s*(?:<[^>]*>)?\(([^)]*\b{name}\s*:[^)]*)\)")).unwrap(), 3),
            ];
            let Some((m, kind, caps)) = binding.iter().filter_map(|(re, kind)| re.captures_iter(back).last().map(|c| (c.get(0).unwrap(), *kind, c))).max_by_key(|(m, _, _)| m.start()) else {
                continue;
            };
            match kind {
                0 => words_in(statement(text, from + m.end() - 1, false), text, out),
                1 => words_in(statement(text, from + m.end(), true), text, out),
                2 => {
                    // The chain the closure's run over: back to the statement's start.
                    let start = back[..m.start()].rfind([';', '{']).map_or(0, |s| s + 1);
                    words_in(&back[start..m.start()], text, out);
                }
                _ => {
                    // A parameter: the words given in its place wherever the function is called.
                    let function = caps[1].to_string();
                    let parameters: Vec<&str> = caps[2].split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
                    let Some(mut index) = parameters.iter().position(|p| p.starts_with(&format!("{name}:")) || p.starts_with(&format!("{name} :"))) else { continue };
                    let method = parameters.first().is_some_and(|p| p.ends_with("self"));
                    if method {
                        index -= 1;
                    }
                    let call = Regex::new(&format!(r"\b{function}\(")).unwrap();
                    for (_, other) in files {
                        for c in call.find_iter(other) {
                            if other[..c.start()].ends_with("fn ") {
                                continue;
                            }
                            // Words written in its place: a literal, or one of two chosen by an if.
                            if let Some(argument) = arguments(&other[c.end()..]).get(index)
                                && (argument.starts_with('"') || argument.starts_with("if "))
                            {
                                words(argument, out);
                            }
                        }
                    }
                }
            }
        }
    }
}
