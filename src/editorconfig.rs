//! EditorConfig (editorconfig.org): the .editorconfig files in a file's folder and those above it, up to one that
//! says `root = true`, say how the file is indented, how its lines end and what character set it's in. Nearer
//! files win over farther ones, and later sections over earlier ones. What Limoniter takes from them: indentation
//! (indent_style, indent_size, tab_width) for as long as the file's in front; insert_final_newline and
//! trim_trailing_whitespace when it's saved; and end_of_line for a file that's empty, whose lines have no ending
//! of their own yet.

use regex::Regex;
use std::path::{Path, PathBuf};

/// What the .editorconfig files say of one file. None is "not said", and Limoniter's own settings stand.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// Spaces (true) or tabs (false).
    pub spaces: Option<bool>,
    pub indent_size: Option<usize>,
    pub tab_width: Option<usize>,
    pub end_of_line: Option<crate::codec::Eol>,
    /// The character set, by Limoniter's id; and whether UTF-8 has its byte order mark.
    pub charset: Option<(&'static str, bool)>,
    pub trim_trailing_whitespace: Option<bool>,
    pub insert_final_newline: Option<bool>,
    /// The files that said anything, nearest last.
    pub from: Vec<PathBuf>,
}

impl Settings {
    /// The tab's width: tab_width, or else indent_size.
    pub fn tab_size(&self) -> Option<usize> {
        self.tab_width.or(self.indent_size).filter(|n| (1..=32).contains(n))
    }
}

/// A section's glob as a regular expression over a path written with /, as the specification has it: a glob
/// with no / matches a file's name in any folder; one with a / is from the .editorconfig's folder.
fn glob(pattern: &str) -> Option<Regex> {
    let (anchored, pattern) = match pattern.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (pattern.contains('/'), pattern),
    };
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::from(if anchored { "^" } else { "^(?:.*/)?" });
    let mut i = 0;
    let mut braces = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '*' if chars.get(i + 1) == Some(&'*') => {
                out.push_str(".*");
                i += 1;
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            '[' => match chars[i + 1..].iter().position(|&c| c == ']') {
                Some(n) => {
                    let inner: String = chars[i + 1..i + 1 + n].iter().collect();
                    let inner = inner.strip_prefix('!').map_or_else(|| inner.clone(), |r| format!("^{r}"));
                    out.push('[');
                    out.push_str(&inner.replace('\\', "\\\\"));
                    out.push(']');
                    i += n + 1;
                }
                None => out.push_str("\\["),
            },
            '{' => {
                // {1..10}: a range of numbers; {a,b}: either.
                let close = chars[i + 1..].iter().position(|&c| c == '}').map(|n| n + i + 1);
                let inner: String = close.map(|close| chars[i + 1..close].iter().collect()).unwrap_or_default();
                if let Some((a, b)) = inner.split_once("..")
                    && let (Ok(a), Ok(b)) = (a.parse::<i64>(), b.parse::<i64>())
                {
                    let (low, high) = (a.min(b), a.max(b));
                    if high - low > 5000 {
                        return None;
                    }
                    out.push_str("(?:");
                    out.push_str(&(low..=high).map(|n| n.to_string()).collect::<Vec<_>>().join("|"));
                    out.push(')');
                    i = close.unwrap_or(i);
                } else if close.is_some() && inner.contains(',') {
                    out.push_str("(?:");
                    braces += 1;
                } else {
                    out.push_str("\\{");
                }
            }
            ',' if braces > 0 => out.push('|'),
            '}' if braces > 0 => {
                out.push(')');
                braces -= 1;
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
    Regex::new(&out).ok()
}

/// The sections of an .editorconfig: each glob and its properties (names and values in lower case, as the
/// specification says they're compared); and whether it says it's the root.
fn parse(text: &str) -> (bool, Vec<(String, Vec<(String, String)>)>) {
    let mut root = false;
    let mut sections: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((name.to_string(), Vec::new()));
            continue;
        }
        let Some((key, value)) = line.split_once(['=', ':']) else { continue };
        let (key, value) = (key.trim().to_lowercase(), value.trim().to_lowercase());
        match sections.last_mut() {
            Some((_, properties)) => properties.push((key, value)),
            None if key == "root" => root = value == "true",
            None => {}
        }
    }
    (root, sections)
}

/// What the .editorconfig files above a file say of it.
pub fn for_file(path: &Path) -> Settings {
    // The files from the nearest up to the root, then taken farthest first so the nearest win.
    let mut files = Vec::new();
    let mut folder = path.parent();
    while let Some(here) = folder {
        let candidate = here.join(".editorconfig");
        if let Ok(text) = std::fs::read_to_string(&candidate) {
            let (root, sections) = parse(&text);
            files.push((here.to_path_buf(), candidate, sections));
            if root {
                break;
            }
        }
        folder = here.parent();
    }
    let mut settings = Settings::default();
    for (folder, file, sections) in files.into_iter().rev() {
        let Ok(relative) = path.strip_prefix(&folder) else { continue };
        let relative = relative.to_string_lossy().replace('\\', "/");
        let mut said = false;
        for (pattern, properties) in sections {
            if !glob(&pattern).is_some_and(|g| g.is_match(&relative)) {
                continue;
            }
            for (key, value) in properties {
                said = true;
                apply(&mut settings, &key, &value);
            }
        }
        if said {
            settings.from.push(file);
        }
    }
    // indent_size = tab means the tab's width; a tab width not given is the indent size.
    settings
}

fn apply(settings: &mut Settings, key: &str, value: &str) {
    // "unset" takes back what a farther file said.
    let unset = value == "unset";
    match key {
        "indent_style" => {
            settings.spaces = match value {
                "space" => Some(true),
                "tab" => Some(false),
                "unset" => None,
                _ => settings.spaces,
            }
        }
        "indent_size" => {
            settings.indent_size = if unset {
                None
            } else if value == "tab" {
                settings.tab_width
            } else {
                value.parse().ok()
            }
        }
        "tab_width" => settings.tab_width = if unset { None } else { value.parse().ok() },
        "end_of_line" => {
            settings.end_of_line = match value {
                "lf" => Some(crate::codec::Eol::Lf),
                "crlf" => Some(crate::codec::Eol::CrLf),
                "cr" => Some(crate::codec::Eol::Cr),
                _ => None,
            }
        }
        "charset" => {
            settings.charset = match value {
                "utf-8" => Some(("utf-8", false)),
                "utf-8-bom" => Some(("utf-8", true)),
                "latin1" => Some(("iso-8859-1", false)),
                "utf-16be" => Some(("utf-16be", true)),
                "utf-16le" => Some(("utf-16le", true)),
                _ => None,
            }
        }
        "trim_trailing_whitespace" => settings.trim_trailing_whitespace = if unset { None } else { Some(value == "true") },
        "insert_final_newline" => settings.insert_final_newline = if unset { None } else { Some(value == "true") },
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_match_as_the_specification_says() {
        let m = |pattern: &str, path: &str| glob(pattern).unwrap().is_match(path);
        assert!(m("*", "a.txt") && m("*", "src/deep/a.txt"));
        assert!(m("*.py", "a.py") && m("*.py", "lib/a.py") && !m("*.py", "a.pyc"));
        assert!(m("{*.js,*.json}", "x/a.json") && m("*.{js,json}", "a.js") && !m("*.{js,json}", "a.ts"));
        assert!(m("lib/**.js", "lib/a/b/c.js") && !m("lib/**.js", "src/lib/c.js"));
        assert!(m("/Makefile", "Makefile") && !m("/Makefile", "src/Makefile"));
        assert!(m("Makefile", "src/Makefile"));
        assert!(m("file{1..3}.c", "file2.c") && !m("file{1..3}.c", "file4.c"));
        assert!(m("[ab].c", "a.c") && m("[!ab].c", "c.c") && !m("[!ab].c", "b.c"));
    }

    #[test]
    fn nearer_files_and_later_sections_win_and_root_stops_the_search() {
        let base = std::env::temp_dir().join(format!("limoniter-editorconfig-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("project/src")).unwrap();
        // Above the root: never read.
        std::fs::write(base.join(".editorconfig"), "[*]\nindent_size = 8\ncharset = latin1\n").unwrap();
        std::fs::write(base.join("project/.editorconfig"), "root = true\n\n[*]\nindent_style = space\nindent_size = 4\nend_of_line = lf\ninsert_final_newline = true\n\n[Makefile]\nindent_style = tab\n\n[*.md]\ntrim_trailing_whitespace = false\n").unwrap();
        std::fs::write(base.join("project/src/.editorconfig"), "[*.s]\nindent_size = 8\ntab_width = 8\nindent_style = tab\ntrim_trailing_whitespace = true\n").unwrap();
        let source = for_file(&base.join("project/src/main.s"));
        assert_eq!((source.spaces, source.tab_size(), source.end_of_line, source.trim_trailing_whitespace, source.insert_final_newline), (Some(false), Some(8), Some(crate::codec::Eol::Lf), Some(true), Some(true)));
        assert_eq!(source.charset, None, "the file above the root isn't read");
        assert_eq!(source.from.len(), 2);
        let rust = for_file(&base.join("project/src/lib.rs"));
        assert_eq!((rust.spaces, rust.tab_size()), (Some(true), Some(4)));
        let make = for_file(&base.join("project/Makefile"));
        assert_eq!(make.spaces, Some(false));
        let other = for_file(&std::env::temp_dir().join("nothing-here-at-all.txt"));
        assert_eq!(other.tab_size(), None);
        let _ = std::fs::remove_dir_all(&base);
    }
}
