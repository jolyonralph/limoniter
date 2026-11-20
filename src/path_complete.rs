//! File paths completed as they're typed in strings, includes and imports: the files and folders in the folder
//! written so far, from the folder the file is in (as Path Intellisense does).

use std::path::{Path, PathBuf};

/// The most names offered.
const MOST: usize = 200;

/// Words before a path that isn't quoted: assembler includes and the like.
const BARE_BEFORE: [&str; 6] = ["include ", "incbin ", "INCLUDE ", "INCBIN ", ".include ", ".incbin "];

/// If the cursor is in a path being typed, where its last part starts (by character) and the names that could
/// finish it: folders first, each with a / after it.
pub fn complete(line: &str, column: usize, folder: &Path) -> Option<(usize, Vec<String>)> {
    let chars: Vec<char> = line.chars().collect();
    let at = column.min(chars.len());
    let before: String = chars[..at].iter().collect();
    // The path starts after the last open quote (or < of a C include), or after an include word.
    let mut open = None;
    let mut quote: Option<char> = None;
    for (i, c) in chars[..at].iter().enumerate() {
        match (quote, *c) {
            (None, '"' | '\'' | '`') => {
                quote = Some(*c);
                open = Some(i + 1);
            }
            (None, '<') if before.trim_start().starts_with("#include") => {
                quote = Some('>');
                open = Some(i + 1);
            }
            (Some(q), c) if c == q => {
                quote = None;
                open = None;
            }
            _ => {}
        }
    }
    let from = match open {
        Some(from) => from,
        None => {
            let trimmed = before.trim_start();
            let word = BARE_BEFORE.iter().find(|w| trimmed.starts_with(*w))?;
            before.len() - trimmed.len() + word.len()
        }
    };
    let typed: String = chars[from..at].iter().collect();
    // Only what looks like a path: with a slash in it, or starting with a dot, or after an include.
    let looks_like_path = typed.contains(['/', '\\']) || typed.starts_with('.') || open.is_none() || before.contains("include") || before.contains("import") || before.contains("require");
    if !looks_like_path || typed.contains(['\n', '*', '?']) {
        return None;
    }
    let split = typed.rfind(['/', '\\']).map_or(0, |i| i + 1);
    let (dir, prefix) = typed.split_at(split);
    let base: PathBuf = if Path::new(dir).is_absolute() { PathBuf::from(dir) } else { folder.join(dir) };
    let entries = std::fs::read_dir(&base).ok()?;
    let lower = prefix.to_lowercase();
    let mut found: Vec<(bool, String)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (name.to_lowercase().starts_with(&lower) && (prefix.starts_with('.') || !name.starts_with('.'))).then(|| (e.path().is_dir(), name))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    let names: Vec<String> = found.into_iter().take(MOST).map(|(dir, name)| if dir { format!("{name}/") } else { name }).collect();
    (!names.is_empty()).then(|| (from + dir.chars().count(), names))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_finished_from_the_files_folder() {
        let folder = std::env::temp_dir().join(format!("limoniter-paths-{}", std::process::id()));
        std::fs::create_dir_all(folder.join("gfx")).unwrap();
        std::fs::write(folder.join("gfx/ship.bin"), "").unwrap();
        std::fs::write(folder.join("gfx/shot.bin"), "").unwrap();
        std::fs::write(folder.join("main.s"), "").unwrap();
        let line = r#"  img = load("gfx/sh"#;
        assert_eq!(complete(line, line.len(), &folder), Some((line.len() - 2, vec!["ship.bin".into(), "shot.bin".into()])));
        let include = "        incbin gf";
        assert_eq!(complete(include, include.len(), &folder), Some((include.len() - 2, vec!["gfx/".into()])));
        assert_eq!(complete("#include <gf", 12, &folder).map(|c| c.1), Some(vec!["gfx/".into()]));
        assert_eq!(complete(r#"print("hello wor"#, 16, &folder), None, "a string that isn't a path");
        assert_eq!(complete("x = 1", 5, &folder), None);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
