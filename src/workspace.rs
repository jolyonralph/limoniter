//! A folder opened as a project, as VS Code opens one: what's in it, read from the disk as it is now. Git's own
//! rules for what to leave out (.gitignore in each folder, and .git/info/exclude) are followed when every file in
//! it is listed, for Go to file and searching; folders such as .git that are never anyone's work are left out of
//! everything. And Go to file's way of finding a file from a few of its letters.

use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// What's never shown or searched, as VS Code leaves them out.
pub const HIDDEN: [&str; 6] = [".git", ".svn", ".hg", "CVS", ".DS_Store", "Thumbs.db"];

/// The most files listed in a folder's index.
pub const MOST_FILES: usize = 250_000;

/// One line of a .gitignore.
#[derive(Debug, Clone)]
struct Rule {
    regex: Regex,
    negated: bool,
    folders_only: bool,
}

/// A .gitignore pattern as a regular expression over a path relative to its folder, written with /.
fn rule(line: &str) -> Option<Rule> {
    let mut text = line.trim_end_matches(['\r', '\n']);
    // Trailing spaces don't count, unless escaped.
    while text.ends_with(' ') && !text.ends_with("\\ ") {
        text = &text[..text.len() - 1];
    }
    if text.is_empty() || text.starts_with('#') {
        return None;
    }
    let (negated, text) = match text.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix("\\!").map_or(text, |_| &text[1..])),
    };
    let (folders_only, text) = match text.strip_suffix('/') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    // A slash anywhere but the end ties the pattern to this folder; without one, it matches at any depth.
    let anchored = text.contains('/');
    let text = text.strip_prefix('/').unwrap_or(text);
    let mut out = String::from(if anchored { "^" } else { "^(?:.*/)?" });
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '*' if chars.get(i + 1) == Some(&'*') => {
                // **/ is any folders (or none); /** at the end is everything inside; ** elsewhere, anything.
                if chars.get(i + 2) == Some(&'/') {
                    out.push_str("(?:.*/)?");
                    i += 3;
                    continue;
                }
                out.push_str(".*");
                i += 2;
                continue;
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            '[' => {
                let close = chars[i + 1..].iter().position(|&c| c == ']').map(|p| p + i + 1);
                match close {
                    Some(close) if close > i + 1 => {
                        out.push('[');
                        let mut inner: String = chars[i + 1..close].iter().collect();
                        if let Some(rest) = inner.strip_prefix('!') {
                            inner = format!("^{rest}");
                        }
                        out.push_str(&inner.replace('\\', "\\\\"));
                        out.push(']');
                        i = close + 1;
                        continue;
                    }
                    _ => out.push_str("\\["),
                }
            }
            '\\' if i + 1 < chars.len() => {
                i += 1;
                out.push_str(&regex::escape(&chars[i].to_string()));
            }
            c => out.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    out.push('$');
    Some(Rule { regex: Regex::new(&out).ok()?, negated, folders_only })
}

fn rules_in(text: &str) -> Vec<Rule> {
    text.lines().filter_map(rule).collect()
}

/// The rules of every folder under a root, read as they're needed.
#[derive(Default)]
pub struct Ignores {
    root: PathBuf,
    folders: HashMap<PathBuf, Vec<Rule>>,
}

fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

impl Ignores {
    pub fn new(root: &Path) -> Ignores {
        let mut ignores = Ignores { root: root.to_path_buf(), folders: HashMap::new() };
        // .git/info/exclude is the root's too.
        let mut rules = std::fs::read_to_string(root.join(".git").join("info").join("exclude")).map(|t| rules_in(&t)).unwrap_or_default();
        rules.extend(std::fs::read_to_string(root.join(".gitignore")).map(|t| rules_in(&t)).unwrap_or_default());
        ignores.folders.insert(root.to_path_buf(), rules);
        ignores
    }

    fn rules(&mut self, folder: &Path) -> &[Rule] {
        self.folders.entry(folder.to_path_buf()).or_insert_with(|| std::fs::read_to_string(folder.join(".gitignore")).map(|t| rules_in(&t)).unwrap_or_default())
    }

    /// Whether Git would leave a file or folder out, by the rules of the folders from the root down to it (the
    /// nearest saying last). That its folder is left out isn't asked here: a walk doesn't go into one that is.
    pub fn ignored(&mut self, path: &Path, is_folder: bool) -> bool {
        let Ok(relative) = path.strip_prefix(&self.root) else { return false };
        let mut ignored = false;
        let mut folder = self.root.clone();
        let parts: Vec<_> = relative.components().collect();
        for depth in 0..parts.len() {
            let below: PathBuf = parts[depth..].iter().collect();
            let below = slashed(&below);
            for rule in self.rules(&folder.clone()) {
                if rule.folders_only && !is_folder {
                    continue;
                }
                if rule.regex.is_match(&below) {
                    ignored = !rule.negated;
                }
            }
            folder.push(parts[depth]);
        }
        ignored
    }
}

/// Whether a name is one that's never shown.
pub fn hidden(name: &str) -> bool {
    HIDDEN.contains(&name)
}

/// Every file under a folder that Git wouldn't leave out, nor the project's Explorer hides (so Go to file and
/// searching don't find it), sorted; and whether there were more than `MOST_FILES`. Links aren't followed.
pub fn index(root: &Path, rules: &[crate::explorer_kept::Hide]) -> (Vec<PathBuf>, bool) {
    let mut ignores = Ignores::new(root);
    let hides = |path: &Path, is_folder: bool| crate::explorer_kept::hidden_by(rules, root, path, is_folder);
    let mut files = Vec::new();
    let mut waiting = vec![root.to_path_buf()];
    while let Some(folder) = waiting.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if hidden(&name) {
                continue;
            }
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() {
                if !ignores.ignored(&path, true) && !hides(&path, true) {
                    waiting.push(path);
                }
            } else if kind.is_file() && !ignores.ignored(&path, false) && !hides(&path, false) {
                if files.len() >= MOST_FILES {
                    files.sort_by_key(|p: &PathBuf| p.to_string_lossy().to_lowercase());
                    return (files, true);
                }
                files.push(path);
            }
        }
    }
    files.sort_by_key(|p| p.to_string_lossy().to_lowercase());
    (files, false)
}

/// How well a few typed letters find a path: None if they aren't all in it in order. Higher is better: letters
/// together, at the start of a word or a folder, and in the file's own name count most, and a shorter path wins a
/// tie. The places matched come back, to show them.
pub fn score(query: &str, path: &str) -> Option<(i64, Vec<usize>)> {
    let query: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let chars: Vec<char> = path.chars().collect();
    let lower: Vec<char> = chars.iter().map(|c| c.to_lowercase().next().unwrap_or(*c)).collect();
    let (m, n) = (query.len(), chars.len());
    if m > n {
        return None;
    }
    let name_start = chars.iter().rposition(|&c| c == '/' || c == '\\').map_or(0, |p| p + 1);
    let bonus = |at: usize| -> i64 {
        let start_of_word = at == 0 || matches!(chars[at - 1], '/' | '\\' | '_' | '-' | '.' | ' ') || (chars[at].is_uppercase() && chars[at - 1].is_lowercase());
        1 + if start_of_word { 8 } else { 0 } + if at >= name_start { 4 } else { 0 }
    };
    const NONE: i64 = i64::MIN / 4;
    // best[q][at]: the best score with the query's letter q matched at `at`; back says where letter q-1 was.
    let mut best = vec![vec![NONE; n]; m];
    let mut back = vec![vec![usize::MAX; n]; m];
    for at in 0..n {
        if lower[at] == query[0] {
            best[0][at] = bonus(at);
        }
    }
    for q in 1..m {
        let (mut before, mut before_at) = (NONE, usize::MAX);
        for at in 0..n {
            // The best place for the letter before, two or more back.
            if at >= 2 && best[q - 1][at - 2] > before {
                before = best[q - 1][at - 2];
                before_at = at - 2;
            }
            if lower[at] != query[q] {
                continue;
            }
            let mut from = (before, before_at);
            // Right after the letter before is better still.
            if at >= 1 && best[q - 1][at - 1] > NONE && best[q - 1][at - 1] + 10 > from.0 {
                from = (best[q - 1][at - 1] + 10, at - 1);
            }
            if from.0 > NONE {
                best[q][at] = from.0 + bonus(at);
                back[q][at] = from.1;
            }
        }
    }
    let (end, &top) = best[m - 1].iter().enumerate().max_by_key(|(_, s)| **s)?;
    if top <= NONE {
        return None;
    }
    let mut places = vec![end];
    let mut at = end;
    for q in (1..m).rev() {
        at = back[q][at];
        places.push(at);
    }
    places.reverse();
    Some((top * 1000 - n as i64, places))
}

/// The paths a few letters find best, best first, at most `most` of them.
pub fn find<'a>(query: &str, paths: impl Iterator<Item = (&'a Path, String)>, most: usize) -> Vec<(&'a Path, String, Vec<usize>)> {
    let mut found: Vec<(i64, &Path, String, Vec<usize>)> = paths.filter_map(|(path, shown)| score(query, &shown).map(|(s, m)| (s, path, shown, m))).collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.cmp(&b.2)));
    found.truncate(most);
    found.into_iter().map(|(_, p, shown, m)| (p, shown, m)).collect()
}

#[cfg(test)]
mod tests;
