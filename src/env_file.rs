//! `.env` files: the lines a project keeps its settings and secrets in. Their values are hidden in the editor until
//! they're hovered or their line is being edited, a warning shows when git isn't ignoring the file (a secret
//! committed by mistake is hard to take back), and a project's `.env` is handed to what its tasks and F9 run.

use std::path::{Path, PathBuf};

/// Whether a file is one: `.env`, `.env.local`, `.env.production`, `prod.env`.
pub fn is_env_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { return false };
    let name = name.to_lowercase();
    name == ".env" || name.starts_with(".env.") && !name.ends_with(".example") && !name.ends_with(".sample") && !name.ends_with(".template") || name.ends_with(".env") && name.len() > 4
}

/// Where a line's value is, as characters (from, to): after the `=`, and before a trailing comment. None for a
/// comment, a blank line, a line with no `=`, or an empty value.
pub fn value_span(line: &str) -> Option<(usize, usize)> {
    let chars: Vec<char> = line.chars().collect();
    let first = chars.iter().position(|c| !c.is_whitespace())?;
    if chars[first] == '#' {
        return None;
    }
    let equals = chars.iter().position(|&c| c == '=')?;
    let mut start = equals + 1;
    while start < chars.len() && chars[start].is_whitespace() {
        start += 1;
    }
    if start >= chars.len() {
        return None;
    }
    let end = match chars[start] {
        quote @ ('"' | '\'') => {
            let mut i = start + 1;
            while i < chars.len() && chars[i] != quote {
                if chars[i] == '\\' && quote == '"' {
                    i += 1;
                }
                i += 1;
            }
            (i + 1).min(chars.len())
        }
        _ => {
            // An unquoted value stops at a comment, which is a # after a space.
            let comment = (start..chars.len()).find(|&i| chars[i] == '#' && i > start && chars[i - 1].is_whitespace());
            let mut end = comment.unwrap_or(chars.len());
            while end > start && chars[end - 1].is_whitespace() {
                end -= 1;
            }
            end
        }
    };
    (end > start).then_some((start, end))
}

/// The line as shown with its value hidden: every character of the value becomes a dot.
pub fn hidden(line: &str) -> String {
    let Some((from, to)) = value_span(line) else { return line.to_string() };
    line.chars().enumerate().map(|(i, c)| if i >= from && i < to { '\u{2022}' } else { c }).collect()
}

/// The names and values a file gives, as the usual dotenv libraries read it: `export` allowed in front, quotes
/// taken off (with \n and \" understood in double quotes only), and comments left out.
pub fn parse(text: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let trimmed = trimmed.strip_prefix("export ").map_or(trimmed, str::trim_start);
        let Some((name, _)) = trimmed.split_once('=') else { continue };
        let name = name.trim();
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '-')) {
            continue;
        }
        let value = match value_span(trimmed) {
            None => String::new(),
            Some((from, to)) => {
                let raw: String = trimmed.chars().skip(from).take(to - from).collect();
                unquote(&raw)
            }
        };
        found.push((name.to_string(), value));
    }
    found
}

fn unquote(raw: &str) -> String {
    if let Some(inner) = raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some(other) => out.push(other),
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        out
    } else if let Some(inner) = raw.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
        inner.to_string()
    } else {
        raw.to_string()
    }
}

/// A line in the file asking for its values to win over what the system already has: `# limoniter: override`.
pub fn overrides(text: &str) -> bool {
    text.lines().take_while(|l| l.trim().is_empty() || l.trim_start().starts_with('#')).any(|l| l.trim_start_matches(['#', ' ']).trim().eq_ignore_ascii_case("limoniter: override"))
}

/// What a project's `.env` adds to the environment of what runs in its folder: its keys, except those the system
/// already has, unless the file asks for its own to win.
pub fn environment_for(folder: &Path) -> Vec<(String, String)> {
    environment_from(&folder.join(".env"))
}

/// The same from a file chosen by name, as a project's settings can choose one.
pub fn environment_from(file: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(file) else { return Vec::new() };
    let win = overrides(&text);
    parse(&text).into_iter().filter(|(name, _)| win || std::env::var_os(name).is_none()).collect()
}

/// Whether git ignores a file: Some(true) or Some(false), or None where git isn't there or the file isn't in a
/// repository.
pub fn ignored_by_git(file: &Path) -> Option<bool> {
    let folder = file.parent()?;
    let name = file.file_name()?;
    let status = crate::platform::command("git").args(["check-ignore", "-q", "--"]).arg(name).current_dir(folder).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().ok()?;
    match status.code()? {
        0 => Some(true),
        1 => Some(false),
        _ => None,
    }
}

/// Adds the file to a `.gitignore` in its own folder, made if there isn't one.
pub fn add_to_gitignore(file: &Path) -> Result<PathBuf, String> {
    let folder = file.parent().ok_or_else(|| crate::i18n::tr("The file has no folder.").to_string())?;
    let name = file.file_name().ok_or_else(|| crate::i18n::tr("The file has no name.").to_string())?.to_string_lossy().into_owned();
    let ignore = folder.join(".gitignore");
    let mut text = std::fs::read_to_string(&ignore).unwrap_or_default();
    let line = format!("/{name}");
    if !text.lines().any(|l| l.trim() == line || l.trim() == name) {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&line);
        text.push('\n');
        std::fs::write(&ignore, text).map_err(|e| crate::i18n::tf!("The .gitignore couldn't be written: {e}"))?;
    }
    Ok(ignore)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_files_are_known_by_name() {
        for yes in [".env", ".env.local", ".env.production", "/a/b/.env", "prod.env"] {
            assert!(is_env_file(Path::new(yes)), "{yes}");
        }
        for no in ["env.rs", "main.env.rs", ".environment", ".env.example", ".env.sample", "readme.md", "env"] {
            assert!(!is_env_file(Path::new(no)), "{no}");
        }
    }

    #[test]
    fn values_are_found_and_hidden() {
        assert_eq!(value_span("KEY=secret"), Some((4, 10)));
        assert_eq!(value_span("export KEY = \"a b\" # note"), Some((13, 18)), "quotes and what's after them");
        assert_eq!(value_span("KEY=abc # why"), Some((4, 7)));
        assert_eq!(value_span("KEY=a#b"), Some((4, 7)), "a # with no space before it is part of the value");
        assert_eq!(value_span("# KEY=nope"), None);
        assert_eq!(value_span("KEY="), None);
        assert_eq!(value_span("no equals"), None);
        assert_eq!(hidden("TOKEN=abc123 # mine"), "TOKEN=\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022} # mine");
        assert_eq!(hidden("# comment=1"), "# comment=1");
        assert_eq!(hidden("K=\"añb\""), "K=\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}");
    }

    #[test]
    fn files_are_read_as_dotenv_libraries_do() {
        let text = "# a comment\nA=1\nexport B=\"two words\"\nC='single $x'\nD=\"line\\nbreak\"\nE=plain # trailing\nF=\n\nbad line\n1X=ok\n";
        let found = parse(text);
        let get = |k: &str| found.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        assert_eq!(get("A"), Some("1"));
        assert_eq!(get("B"), Some("two words"));
        assert_eq!(get("C"), Some("single $x"));
        assert_eq!(get("D"), Some("line\nbreak"));
        assert_eq!(get("E"), Some("plain"));
        assert_eq!(get("F"), Some(""));
        assert_eq!(found.len(), 7, "{found:?}");
    }

    #[test]
    fn the_system_keeps_its_own_unless_asked() {
        let folder = std::env::temp_dir().join(format!("limoniter-env-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        // PATH is always set on the system.
        std::fs::write(folder.join(".env"), "LIMONITER_TEST_ENV_ONLY=1\nPATH=mine\n").unwrap();
        let given = environment_for(&folder);
        assert_eq!(given, [("LIMONITER_TEST_ENV_ONLY".to_string(), "1".to_string())]);
        std::fs::write(folder.join(".env"), "# limoniter: override\nLIMONITER_TEST_ENV_ONLY=1\nPATH=mine\n").unwrap();
        assert_eq!(environment_for(&folder).len(), 2);
        assert!(environment_for(&folder.join("nowhere")).is_empty());
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn git_is_asked_and_the_file_added_to_gitignore() {
        let folder = std::env::temp_dir().join(format!("limoniter-env-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join(".env");
        std::fs::write(&file, "A=1\n").unwrap();
        assert_eq!(ignored_by_git(&file), None, "not in a repository (or git isn't there)");
        if !crate::lsp::on_path("git") {
            eprintln!("git isn't installed, so the rest wasn't checked.");
            return std::fs::remove_dir_all(&folder).unwrap();
        }
        let init = crate::platform::command("git").args(["init", "-q"]).current_dir(&folder).status().unwrap();
        assert!(init.success());
        assert_eq!(ignored_by_git(&file), Some(false));
        std::fs::write(folder.join(".gitignore"), "target").unwrap();
        add_to_gitignore(&file).unwrap();
        assert_eq!(std::fs::read_to_string(folder.join(".gitignore")).unwrap(), "target\n/.env\n");
        assert_eq!(ignored_by_git(&file), Some(true));
        add_to_gitignore(&file).unwrap();
        assert_eq!(std::fs::read_to_string(folder.join(".gitignore")).unwrap().matches(".env").count(), 1, "not added twice");
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
