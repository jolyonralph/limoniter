//! Setting Git up to use Limoniter for `git difftool` and `git mergetool`, and back again. Only the person's own
//! Git settings (`git config --global`) are changed, never a repository's, and what was there before is kept in
//! a file so that stopping puts back exactly what was set, and unsets what wasn't.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The tool's name in Git's settings.
pub const NAME: &str = "limoniter";

/// Every setting this changes, in the order they're written.
pub const KEYS: [&str; 5] = ["diff.tool", "merge.tool", "difftool.limoniter.cmd", "mergetool.limoniter.cmd", "mergetool.limoniter.trustExitCode"];

/// The file, in Limoniter's own settings folder, holding what the settings were before.
pub const BACKUP_FILE: &str = "git-tools-before.json";

/// The person's own Git settings: the real ones, or, for trying things out, a file standing in for them.
#[derive(Clone, Debug, Default)]
pub struct Global {
    pub file: Option<PathBuf>,
}

impl Global {
    fn git(&self) -> Command {
        let mut command = crate::platform::command(crate::git::program().unwrap_or_else(|| PathBuf::from("git")));
        command.arg("config").arg("--global").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(file) = &self.file {
            command.env("GIT_CONFIG_GLOBAL", file);
        }
        command
    }

    fn run(&self, args: &[&str]) -> Result<std::process::Output, String> {
        self.git().args(args).output().map_err(|e| crate::i18n::tf!("Git couldn't be started: {e}"))
    }

    /// A setting's value, or None if it isn't set.
    pub fn get(&self, key: &str) -> Result<Option<String>, String> {
        let output = self.run(&["--get", key])?;
        match output.status.code() {
            Some(0) => Ok(Some(String::from_utf8_lossy(&output.stdout).trim_end_matches(['\n', '\r']).to_string())),
            // Git says 1 for a setting that isn't there.
            Some(1) => Ok(None),
            _ => Err(said(&output, "read")),
        }
    }

    pub fn set(&self, key: &str, value: &str) -> Result<(), String> {
        let output = self.run(&[key, value])?;
        if output.status.success() { Ok(()) } else { Err(said(&output, "change")) }
    }

    pub fn unset(&self, key: &str) -> Result<(), String> {
        let output = self.run(&["--unset-all", key])?;
        // 5 is what Git says when there was nothing to unset.
        if output.status.success() || output.status.code() == Some(5) { Ok(()) } else { Err(said(&output, "change")) }
    }
}

fn said(output: &std::process::Output, doing: &str) -> String {
    let text = String::from_utf8_lossy(&output.stderr).trim().to_string();
    match (text.is_empty(), doing) {
        (false, _) => text,
        (true, "read") => crate::i18n::tr("Git couldn't read its settings.").to_string(),
        (true, _) => crate::i18n::tr("Git couldn't change its settings.").to_string(),
    }
}

/// A program's path as the shell Git runs tools with reads it, in double quotes. On Windows that shell is Git's
/// own, which is happiest with forward slashes.
pub fn quote(path: &Path, windows: bool) -> String {
    let text = path.to_string_lossy();
    let text = if windows { text.replace('\\', "/") } else { text.into_owned() };
    let mut out = String::from("\"");
    for c in text.chars() {
        if matches!(c, '"' | '\\' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// The settings that make Git use this program.
pub fn wanted(program: &Path, windows: bool) -> Vec<(&'static str, String)> {
    let program = quote(program, windows);
    vec![
        ("diff.tool", NAME.to_string()),
        ("merge.tool", NAME.to_string()),
        ("difftool.limoniter.cmd", format!("{program} --diff \"$LOCAL\" \"$REMOTE\"")),
        ("mergetool.limoniter.cmd", format!("{program} --merge \"$BASE\" \"$LOCAL\" \"$REMOTE\" \"$MERGED\"")),
        ("mergetool.limoniter.trustExitCode", "true".to_string()),
    ]
}

/// Another tool already chosen for diffs or merges, which turning this on would replace: (setting, value).
pub fn others(global: &Global) -> Result<Vec<(&'static str, String)>, String> {
    let mut found = Vec::new();
    for key in ["diff.tool", "merge.tool"] {
        if let Some(value) = global.get(key)?
            && !value.eq_ignore_ascii_case(NAME)
        {
            found.push((key, value));
        }
    }
    Ok(found)
}

/// Whether Git is set to use Limoniter for both.
pub fn is_on(global: &Global) -> bool {
    ["diff.tool", "merge.tool"].iter().all(|key| global.get(key).ok().flatten().is_some_and(|value| value.eq_ignore_ascii_case(NAME)))
}

/// What each setting was before, as kept in the file: a value, or null for not set.
fn read_backup(file: &Path) -> Option<Vec<(String, Option<String>)>> {
    let text = std::fs::read_to_string(file).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let object = value.as_object()?;
    Some(object.iter().map(|(key, value)| (key.clone(), value.as_str().map(str::to_string))).collect())
}

/// Makes Git use this program, keeping what was set before (unless that's been kept already, from an earlier time
/// it was turned on and not yet off, so the person's own settings are what's kept, not this program's).
pub fn turn_on(global: &Global, program: &Path, backup: &Path) -> Result<(), String> {
    if read_backup(backup).is_none() {
        let mut before = serde_json::Map::new();
        for key in KEYS {
            before.insert(key.to_string(), global.get(key)?.map_or(serde_json::Value::Null, serde_json::Value::String));
        }
        if let Some(folder) = backup.parent() {
            std::fs::create_dir_all(folder).map_err(|e| crate::i18n::tf!("Couldn't keep Git's settings as they were: {e}"))?;
        }
        let text = serde_json::to_string_pretty(&serde_json::Value::Object(before)).map_err(|e| e.to_string())?;
        std::fs::write(backup, text).map_err(|e| crate::i18n::tf!("Couldn't keep Git's settings as they were: {e}"))?;
    }
    for (key, value) in wanted(program, cfg!(windows)) {
        global.set(key, &value)?;
    }
    Ok(())
}

/// Puts back what Git's settings were before, exactly, and forgets them. With nothing kept, just takes this
/// program's own settings away, if Git is using it.
pub fn turn_off(global: &Global, backup: &Path) -> Result<(), String> {
    match read_backup(backup) {
        Some(before) => {
            for (key, value) in &before {
                match value {
                    Some(value) => global.set(key, value)?,
                    None => global.unset(key)?,
                }
            }
            std::fs::remove_file(backup).map_err(|e| crate::i18n::tf!("Git's settings are back as they were, but the note of them couldn't be removed: {e}"))?;
        }
        None => {
            for key in ["diff.tool", "merge.tool"] {
                if global.get(key)?.is_some_and(|value| value.eq_ignore_ascii_case(NAME)) {
                    global.unset(key)?;
                }
            }
            for key in &KEYS[2..] {
                global.unset(key)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A folder of its own for each test, with a Git settings file standing in for the person's.
    fn scratch(tag: &str) -> (PathBuf, Global) {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let folder = std::env::temp_dir().join(format!("limoniter-git-setup-{tag}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("gitconfig");
        (folder, Global { file: Some(file) })
    }

    fn have_git() -> bool {
        crate::git::program().is_some()
    }

    #[test]
    fn paths_are_quoted_for_the_shell_git_runs_tools_with() {
        assert_eq!(quote(Path::new("/usr/bin/limoniter"), false), "\"/usr/bin/limoniter\"");
        assert_eq!(quote(Path::new("/Applications/Limoniter.app/Contents/MacOS/limoniter"), false), "\"/Applications/Limoniter.app/Contents/MacOS/limoniter\"");
        assert_eq!(quote(Path::new("/home/a b/$x/`y`/\"q\"/back\\slash"), false), "\"/home/a b/\\$x/\\`y\\`/\\\"q\\\"/back\\\\slash\"");
        assert_eq!(quote(Path::new(r"C:\Program Files\Limoniter\limoniter.exe"), true), "\"C:/Program Files/Limoniter/limoniter.exe\"");
        assert_eq!(quote(Path::new(r"C:\Users\Ann $\Limoniter.exe"), true), "\"C:/Users/Ann \\$/Limoniter.exe\"");
    }

    #[test]
    fn the_commands_hand_git_s_files_on() {
        let settings = wanted(Path::new(r"C:\Program Files\Limoniter\limoniter.exe"), true);
        let get = |key: &str| settings.iter().find(|(k, _)| *k == key).unwrap().1.clone();
        assert_eq!(get("diff.tool"), "limoniter");
        assert_eq!(get("merge.tool"), "limoniter");
        assert_eq!(get("difftool.limoniter.cmd"), "\"C:/Program Files/Limoniter/limoniter.exe\" --diff \"$LOCAL\" \"$REMOTE\"");
        assert_eq!(get("mergetool.limoniter.cmd"), "\"C:/Program Files/Limoniter/limoniter.exe\" --merge \"$BASE\" \"$LOCAL\" \"$REMOTE\" \"$MERGED\"");
        assert_eq!(get("mergetool.limoniter.trustExitCode"), "true");
        assert_eq!(settings.len(), KEYS.len());
    }

    #[test]
    fn turning_on_and_off_with_nothing_set_before_leaves_nothing_set() {
        if !have_git() {
            return;
        }
        let (folder, global) = scratch("empty");
        let backup = folder.join("data").join(BACKUP_FILE);
        assert!(others(&global).unwrap().is_empty());
        assert!(!is_on(&global));
        turn_on(&global, Path::new("/opt/lim/limoniter"), &backup).unwrap();
        assert!(is_on(&global));
        assert_eq!(global.get("mergetool.limoniter.cmd").unwrap().unwrap(), "\"/opt/lim/limoniter\" --merge \"$BASE\" \"$LOCAL\" \"$REMOTE\" \"$MERGED\"");
        assert_eq!(global.get("mergetool.limoniter.trustexitcode").unwrap().as_deref(), Some("true"));
        assert!(backup.is_file());
        turn_off(&global, &backup).unwrap();
        for key in KEYS {
            assert_eq!(global.get(key).unwrap(), None, "{key}");
        }
        assert!(!backup.exists());
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn turning_off_puts_back_exactly_what_was_there() {
        if !have_git() {
            return;
        }
        let (folder, global) = scratch("before");
        let backup = folder.join(BACKUP_FILE);
        global.set("diff.tool", "meld").unwrap();
        global.set("mergetool.limoniter.cmd", "old command \"$MERGED\"").unwrap();
        global.set("user.name", "Someone").unwrap();
        assert_eq!(others(&global).unwrap(), [("diff.tool", "meld".to_string())]);
        turn_on(&global, Path::new("/x/limoniter"), &backup).unwrap();
        assert_eq!(global.get("diff.tool").unwrap().as_deref(), Some("limoniter"));
        // Turned on a second time, what's kept is still the person's own, not this program's.
        turn_on(&global, Path::new("/y/limoniter"), &backup).unwrap();
        assert!(global.get("difftool.limoniter.cmd").unwrap().unwrap().starts_with("\"/y/limoniter\""));
        turn_off(&global, &backup).unwrap();
        assert_eq!(global.get("diff.tool").unwrap().as_deref(), Some("meld"));
        assert_eq!(global.get("merge.tool").unwrap(), None);
        assert_eq!(global.get("mergetool.limoniter.cmd").unwrap().as_deref(), Some("old command \"$MERGED\""));
        assert_eq!(global.get("difftool.limoniter.cmd").unwrap(), None);
        assert_eq!(global.get("mergetool.limoniter.trustExitCode").unwrap(), None);
        assert_eq!(global.get("user.name").unwrap().as_deref(), Some("Someone"), "nothing else is touched");
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn turning_off_with_nothing_kept_only_takes_away_its_own() {
        if !have_git() {
            return;
        }
        let (folder, global) = scratch("nobackup");
        let backup = folder.join(BACKUP_FILE);
        global.set("diff.tool", "limoniter").unwrap();
        global.set("merge.tool", "kdiff3").unwrap();
        global.set("difftool.limoniter.cmd", "x").unwrap();
        turn_off(&global, &backup).unwrap();
        assert_eq!(global.get("diff.tool").unwrap(), None);
        assert_eq!(global.get("merge.tool").unwrap().as_deref(), Some("kdiff3"));
        assert_eq!(global.get("difftool.limoniter.cmd").unwrap(), None);
        std::fs::remove_dir_all(folder).unwrap();
    }
}
