//! Running Claude Code or Codex that is installed in WSL from Windows Limoniter, for the AI agents panel.
//!
//! Three small jobs: finding where each tool is (on Windows first, then in each WSL distribution), the command that
//! runs it there, and translating paths. The tool in WSL writes `/mnt/c/Users/a/project/x.rs` where Windows writes
//! `C:\Users\a\project\x.rs`, so every line sent to it has its paths turned into WSL's form, and every line it sends
//! back has them turned into Windows' form. That is done on the JSON lines themselves, so that nothing else in the
//! panel needs to know where a session is running.
//!
//! The translating works on text and doesn't care which computer it runs on, so it is tested everywhere.

use crate::claude_ide::{from_wsl, to_wsl};
use serde_json::Value;
use std::path::Path;

// ---------- Where a tool is ----------

/// Where a tool can be run from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    /// The WSL distribution it is in, or None for this computer's own.
    pub distribution: Option<String>,
    /// What to run: its name for this computer's own, the full path where `command -v` found it in WSL.
    pub program: String,
}

impl Place {
    /// How the person is told where it is: "Codex, in WSL: Ubuntu".
    pub fn describe(&self, tool_name: &str) -> String {
        match &self.distribution {
            Some(name) => crate::i18n::tf!("{tool_name}, in WSL: {name}"),
            None => crate::i18n::tf!("{tool_name}, on Windows"),
        }
    }
}

/// What a distribution printed when asked where a tool is: the path, if it is a path.
pub fn found_path(output: &str) -> Option<String> {
    output.lines().map(str::trim).find(|line| line.starts_with('/')).map(str::to_string)
}

/// Asks one distribution where a tool is. A login shell is used so that the paths nvm and npm add are looked in.
/// `wsl` is the program that runs WSL and what goes before its own arguments.
pub fn find_in(wsl: &(String, Vec<String>), distribution: &str, tool: &str) -> Option<String> {
    let output = crate::platform::command(&wsl.0).args(&wsl.1).args(["-d", distribution, "--exec", "sh", "-lc", &format!("command -v {tool}")]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    found_path(&String::from_utf8_lossy(&output.stdout))
}

/// Where a tool is in WSL: each distribution that has it.
pub fn find_places(wsl: &(String, Vec<String>), distributions: &[String], tool: &str) -> Vec<Place> {
    distributions.iter().filter_map(|distribution| Some(Place { distribution: Some(distribution.clone()), program: find_in(wsl, distribution, tool)? })).collect()
}

/// The place to use: the one remembered if it's still there, otherwise the first.
pub fn choose<'a>(places: &'a [Place], remembered: Option<&str>) -> Option<&'a Place> {
    places.iter().find(|place| place.distribution.as_deref() == remembered.filter(|name| !name.is_empty())).or_else(|| places.first())
}

/// What is run to start a tool in a distribution, in the folder given in WSL's form: `wsl.exe` with the program
/// and what goes before the tool's own arguments. The tool is started through a login shell, which is what makes
/// node (and so a tool installed by npm or nvm) findable. `exec "$@"` hands the arguments over as they are.
pub fn runner(wsl: &(String, Vec<String>), distribution: &str, linux_folder: &str, tool: &str) -> (String, Vec<String>) {
    let mut prefix = wsl.1.clone();
    prefix.extend(["-d", distribution, "--cd", linux_folder, "--exec", "sh", "-lc", "exec \"$@\"", "sh", tool].map(String::from));
    (wsl.0.clone(), prefix)
}

// ---------- Earlier sessions ----------

/// The earlier sessions of a project that the tools kept in a WSL distribution, whose files are under `root` as
/// Windows sees them (`\\wsl.localhost\Ubuntu`). Each is marked with the distribution, so that taking it up again
/// runs the tool there.
pub fn past_in(root: &Path, distribution: &str, paths: &Paths) -> Vec<crate::agent_logs::Past> {
    let folder = std::path::PathBuf::from(paths.linux_folder());
    let mut found = Vec::new();
    for home in crate::usage_logs::wsl_homes(root) {
        found.extend(crate::agent_logs::list(&home.join(".claude"), &folder));
        found.extend(crate::agent_codex_logs::list(&home.join(".codex"), &folder));
    }
    for past in &mut found {
        past.wsl = Some(distribution.to_string());
    }
    found
}

// ---------- What is remembered ----------

/// Where each tool was last run from, kept in a file beside the settings: `claude=Ubuntu` and `codex=` (this
/// computer's own).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Remembered {
    pub claude: Option<String>,
    pub codex: Option<String>,
}

impl Remembered {
    pub fn from_text(text: &str) -> Self {
        let mut remembered = Remembered::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = Some(value.trim().to_string());
            match key.trim() {
                "claude" => remembered.claude = value,
                "codex" => remembered.codex = value,
                _ => {}
            }
        }
        remembered
    }

    pub fn to_text(&self) -> String {
        format!("claude={}\ncodex={}\n", self.claude.as_deref().unwrap_or(""), self.codex.as_deref().unwrap_or(""))
    }

    fn file() -> Option<std::path::PathBuf> {
        crate::settings::data_folder().map(|dir| dir.join("agent-places.txt"))
    }

    /// Read from the file (never in tests, which mustn't see the person's own).
    pub fn load() -> Self {
        if cfg!(test) {
            return Remembered::default();
        }
        Self::file().and_then(|file| std::fs::read_to_string(file).ok()).map(|text| Self::from_text(&text)).unwrap_or_default()
    }

    pub fn save(&self) {
        if cfg!(test) {
            return;
        }
        if let Some(file) = Self::file() {
            let _ = std::fs::create_dir_all(file.parent().expect("in a folder"));
            let _ = std::fs::write(file, self.to_text());
        }
    }
}

// ---------- Paths ----------

/// Keys whose value is one whole path, so it is translated however many spaces it has.
const PATH_KEYS: [&str; 9] = ["file_path", "filePath", "path", "cwd", "move_path", "notebook_path", "grantRoot", "directory", "root"];

/// What a path in free text may not hold: a path ends at a space or a quotation mark.
fn ends_path(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '<' | '>' | '|' | '*' | '?')
}

/// Whether a character could come before a path without being part of a longer word or path.
fn starts_path_after(previous: Option<char>) -> bool {
    previous.is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '_' | '.' | '-' | '/' | '\\' | ':' | '$' | '~')))
}

/// The end of the path that begins at `from` in `text`: it goes on to a space or quotation mark, less any
/// punctuation that is more likely to belong to the sentence.
fn path_end(text: &str, from: usize) -> usize {
    let mut end = text[from..].find(ends_path).map_or(text.len(), |at| from + at);
    while end > from && text[..end].ends_with(['.', ',', ';', ':', ')', ']', '}', '!']) {
        end -= 1;
    }
    end
}

/// The paths of one session: its project's folder in each of the two forms, and the distribution it is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paths {
    pub distribution: String,
    /// The project's folder as Windows writes it, and as WSL does.
    windows: String,
    linux: String,
}

impl Paths {
    /// For a project folder on Windows (a drive or a `\\wsl.localhost\...` one).
    pub fn new(distribution: &str, folder: &Path) -> Self {
        let windows = folder.display().to_string();
        let linux = to_wsl(&windows, Some(distribution));
        Paths { distribution: distribution.to_string(), windows, linux }
    }

    /// With both forms of the folder given (what a test uses, to stand for a Windows folder on any computer).
    #[cfg(test)]
    pub fn with_folders(distribution: &str, windows: &str, linux: &str) -> Self {
        Paths { distribution: distribution.to_string(), windows: windows.to_string(), linux: linux.to_string() }
    }

    /// The project's folder, as WSL writes it.
    pub fn linux_folder(&self) -> &str {
        &self.linux
    }

    /// Whether the two forms are the same, so that nothing needs translating.
    fn same(&self) -> bool {
        self.windows == self.linux
    }

    /// What separates the parts of a path in a Windows folder like this one.
    fn separator(&self) -> char {
        if self.windows.contains('\\') || self.windows.contains(':') { '\\' } else { '/' }
    }

    /// One path, as WSL writes it, from the form Windows does. Whatever isn't a Windows path is left alone.
    pub fn path_to_linux(&self, path: &str) -> String {
        if self.same() {
            return path.to_string();
        }
        if let Some(rest) = strip_folder(path, &self.windows) {
            return format!("{}{}", self.linux.trim_end_matches('/'), rest.replace('\\', "/"));
        }
        to_wsl(path, Some(&self.distribution))
    }

    /// One path, as Windows writes it, from the form WSL does.
    pub fn path_to_windows(&self, path: &str) -> String {
        if self.same() {
            return path.to_string();
        }
        if let Some(rest) = strip_folder_linux(path, &self.linux) {
            let separator = self.separator();
            let rest = if separator == '\\' { rest.replace('/', "\\") } else { rest.to_string() };
            return format!("{}{}", self.windows.trim_end_matches(['\\', '/']), rest);
        }
        from_wsl(path, &self.distribution)
    }

    /// Text the person or Limoniter wrote, with each Windows path in it turned into WSL's form.
    pub fn text_to_linux(&self, text: &str) -> String {
        if self.same() {
            return text.to_string();
        }
        let mut out = String::with_capacity(text.len());
        let mut previous = None;
        let mut at = 0;
        while at < text.len() {
            let rest = &text[at..];
            let c = rest.chars().next().unwrap_or(' ');
            let mut taken = 0;
            if starts_path_after(previous) {
                if let Some(tail) = strip_folder(rest, &self.windows) {
                    // The project's folder is known exactly, so spaces in its name are no trouble.
                    let end = path_end(rest, rest.len() - tail.len());
                    taken = end;
                } else if is_drive_path(rest) || is_wsl_unc(rest) {
                    taken = path_end(rest, 0);
                }
            }
            if taken > 0 {
                out.push_str(&self.path_to_linux(&rest[..taken]));
                previous = rest[..taken].chars().last();
                at += taken;
            } else {
                out.push(c);
                previous = Some(c);
                at += c.len_utf8();
            }
        }
        out
    }

    /// Text a tool in WSL wrote, with each path in it that Windows can name turned into Windows' form.
    pub fn text_to_windows(&self, text: &str) -> String {
        if self.same() {
            return text.to_string();
        }
        let mut out = String::with_capacity(text.len());
        let mut previous = None;
        let mut at = 0;
        while at < text.len() {
            let rest = &text[at..];
            let c = rest.chars().next().unwrap_or(' ');
            let mut taken = 0;
            if c == '/' && starts_path_after(previous) {
                if let Some(tail) = strip_folder_linux(rest, &self.linux) {
                    taken = path_end(rest, rest.len() - tail.len());
                } else if is_mnt_path(rest) {
                    taken = path_end(rest, 0);
                }
            }
            if taken > 0 {
                out.push_str(&self.path_to_windows(&rest[..taken]));
                previous = rest[..taken].chars().last();
                at += taken;
            } else {
                out.push(c);
                previous = Some(c);
                at += c.len_utf8();
            }
        }
        out
    }

    /// A JSON value going to the tool, translated, with `key` the name it has in its object.
    fn value_to_linux(&self, value: &mut Value, key: &str) {
        match value {
            Value::String(text) => {
                let whole = text.trim();
                *text = if PATH_KEYS.contains(&key) && !whole.contains('\n') && (is_drive_path(whole) || is_wsl_unc(whole) || strip_folder(whole, &self.windows).is_some()) { self.path_to_linux(whole) } else { self.text_to_linux(text) };
            }
            Value::Array(items) => items.iter_mut().for_each(|item| self.value_to_linux(item, key)),
            Value::Object(map) => {
                let entries = std::mem::take(map);
                for (name, mut inner) in entries {
                    self.value_to_linux(&mut inner, &name);
                    map.insert(self.text_to_linux(&name), inner);
                }
            }
            _ => {}
        }
    }

    fn value_to_windows(&self, value: &mut Value, key: &str) {
        match value {
            Value::String(text) => {
                *text = if PATH_KEYS.contains(&key) && text.starts_with('/') && !text.contains('\n') { self.path_to_windows(text) } else { self.text_to_windows(text) };
            }
            Value::Array(items) => items.iter_mut().for_each(|item| self.value_to_windows(item, key)),
            Value::Object(map) => {
                let entries = std::mem::take(map);
                for (name, mut inner) in entries {
                    self.value_to_windows(&mut inner, &name);
                    map.insert(self.text_to_windows(&name), inner);
                }
            }
            _ => {}
        }
    }

    /// A line of JSON to send to the tool, with its paths in WSL's form. A line that isn't JSON is sent as it is.
    pub fn outgoing(&self, line: &str) -> String {
        if self.same() {
            return line.to_string();
        }
        match serde_json::from_str::<Value>(line) {
            Ok(mut value) => {
                self.value_to_linux(&mut value, "");
                value.to_string()
            }
            Err(_) => line.to_string(),
        }
    }

    /// A line of JSON from the tool, with its paths in Windows' form.
    pub fn incoming(&self, line: &str) -> String {
        if self.same() || !line.contains('/') {
            return line.to_string();
        }
        match serde_json::from_str::<Value>(line) {
            Ok(mut value) => {
                self.value_to_windows(&mut value, "");
                value.to_string()
            }
            Err(_) => line.to_string(),
        }
    }
}

/// `C:\` or `c:/` and what follows.
fn is_drive_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/')
}

/// `\\wsl.localhost\Ubuntu\...` (or the old `\\wsl$\`).
fn is_wsl_unc(text: &str) -> bool {
    text.starts_with("\\\\wsl.localhost\\") || text.starts_with("\\\\wsl$\\")
}

/// `/mnt/c` alone, or with more after it.
fn is_mnt_path(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("/mnt/") else { return false };
    let mut chars = rest.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && chars.next().is_none_or(|c| c == '/' || ends_path(c) || matches!(c, '.' | ',' | ';' | ':' | ')'))
}

/// What follows a Windows folder at the start of `text`, if the text starts with it as a whole name (no matter the
/// case, as Windows has it, and with either slash).
fn strip_folder<'a>(text: &'a str, folder: &str) -> Option<&'a str> {
    let folder = folder.trim_end_matches(['\\', '/']);
    if folder.is_empty() || text.len() < folder.len() || !text.is_char_boundary(folder.len()) {
        return None;
    }
    let (head, rest) = text.split_at(folder.len());
    let alike = head.chars().zip(folder.chars()).all(|(a, b)| a == b || (a.eq_ignore_ascii_case(&b)) || (matches!(a, '\\' | '/') && matches!(b, '\\' | '/')));
    (alike && head.chars().count() == folder.chars().count() && (rest.is_empty() || rest.starts_with(['\\', '/']) || !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == '-' || c == '.'))).then_some(rest)
}

/// The same for WSL's folder, whose names have their case.
fn strip_folder_linux<'a>(text: &'a str, folder: &str) -> Option<&'a str> {
    let folder = folder.trim_end_matches('/');
    if folder.is_empty() {
        return None;
    }
    let rest = text.strip_prefix(folder)?;
    (rest.is_empty() || rest.starts_with('/') || !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')).then_some(rest)
}

#[cfg(test)]
#[path = "agent_wsl_tests.rs"]
pub(crate) mod agent_wsl_tests;
