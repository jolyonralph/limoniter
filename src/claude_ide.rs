//! Limoniter as an IDE for Claude Code, the part that has no window: a small WebSocket server on this computer
//! that a `claude` started in Limoniter's terminal (or picked with its /ide command) connects to, and the lock
//! file that tells it the server is here. It speaks MCP (JSON-RPC) as Claude Code's own VS Code extension does:
//! the tools `openDiff`, `getDiagnostics`, `close_tab` and `closeAllDiffTabs` are asked of Limoniter, and
//! Limoniter tells it what's selected (`selection_changed`) and what to mention (`at_mentioned`).
//!
//! The server runs on threads of its own. It asks the window things through a channel of [`Event`]s, each with a
//! channel to answer on, so nothing here touches the window's data.

use serde_json::{Value, json};
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tungstenite::Message;
use tungstenite::handshake::server::{ErrorResponse, Request, Response};

/// What the client matches on: Claude Code looks for "Claude Code <name> MCP".
pub const SERVER_NAME: &str = "Claude Code Limoniter MCP";
/// How the lock file names this program.
pub const IDE_NAME: &str = "Limoniter";
/// The header the client proves itself with.
const AUTH_HEADER: &str = "x-claude-code-ide-authorization";
/// The header a session started by Limoniter sends, naming itself ("Codex 1") to the task board.
pub const SESSION_HEADER: &str = "x-limoniter-session";
/// The most clients at once; a few terminals is usual.
const MOST_CLIENTS: usize = 16;
/// How long the connection threads wait for a message before looking at what the window has to say.
const TICK: Duration = Duration::from_millis(20);
/// MCP versions this server knows. A client asking for another is answered with the newest.
const VERSIONS: [&str; 4] = ["2025-06-18", "2025-03-26", "2024-11-05", "2024-10-07"];

// ---------- Paths, as the client wants them ----------

/// How a client writes paths. With Limoniter on Windows, a `claude` in WSL writes Linux paths, and one run in
/// PowerShell writes Windows ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathStyle {
    /// The paths this computer's own programs use.
    Native,
    /// Linux paths, from a program in WSL: `/mnt/c/Users/…` and `/home/…`.
    Wsl,
}

impl PathStyle {
    fn to_byte(self) -> u8 {
        u8::from(self == PathStyle::Wsl)
    }

    fn from_byte(byte: u8) -> Self {
        if byte == 1 { PathStyle::Wsl } else { PathStyle::Native }
    }
}

/// A Windows path as a program in WSL writes it: `C:\Users\a` is `/mnt/c/Users/a`, and a file in the distribution
/// itself, `\\wsl.localhost\Ubuntu\home\a`, is `/home/a`. A path in another distribution can't be written that
/// way, and stays as it was. Anything else (a path that is already Linux's) is left alone.
pub fn to_wsl(path: &str, distro: Option<&str>) -> String {
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && (bytes.len() == 2 || bytes[2] == b'\\' || bytes[2] == b'/') {
        let rest = path[2..].replace('\\', "/");
        return format!("/mnt/{}{}", (bytes[0] as char).to_ascii_lowercase(), rest);
    }
    let unc = path.strip_prefix("\\\\wsl.localhost\\").or_else(|| path.strip_prefix("\\\\wsl$\\"));
    if let Some(unc) = unc {
        let (name, rest) = unc.split_once('\\').unwrap_or((unc, ""));
        if distro.is_none_or(|distro| distro.eq_ignore_ascii_case(name)) {
            return format!("/{}", rest.replace('\\', "/"));
        }
    }
    path.to_string()
}

/// A path from a program in WSL as Windows writes it: `/mnt/c/Users/a` is `C:\Users\a`, and any other,
/// `/home/a`, is a file in the distribution, `\\wsl.localhost\Ubuntu\home\a`. A path that is already Windows's is
/// left alone.
pub fn from_wsl(path: &str, distro: &str) -> String {
    if let Some(rest) = path.strip_prefix("/mnt/") {
        let mut chars = rest.chars();
        if let Some(letter) = chars.next().filter(char::is_ascii_alphabetic)
            && matches!(chars.next(), None | Some('/'))
        {
            let tail = &rest[1..];
            return format!("{}:{}", letter.to_ascii_uppercase(), if tail.is_empty() { "\\".to_string() } else { tail.replace('/', "\\") });
        }
    }
    if path.starts_with('/') {
        return format!("\\\\wsl.localhost\\{distro}{}", path.replace('/', "\\"));
    }
    path.to_string()
}

/// A file's address as the client will compare it with its own: `file://` and the path, with only the characters
/// that would be misread changed (as VS Code writes them), so that the client's own spelling comes back.
pub fn file_uri(path: &str) -> String {
    let text = path.replace('\\', "/");
    let encoded: String = text
        .chars()
        .map(|c| match c {
            '%' => "%25".to_string(),
            '#' => "%23".to_string(),
            '?' => "%3F".to_string(),
            c => c.to_string(),
        })
        .collect();
    if encoded.starts_with('/') { format!("file://{encoded}") } else { format!("file:///{encoded}") }
}

/// The path in a `file://` address, in the form it was written (Linux's `/home/a`, or Windows' `C:\a`). The client
/// doesn't encode what it sends (`file://${path}`), so a `%` is read as the start of a code only when two
/// hexadecimal digits follow it.
pub fn path_of_uri(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(code) = rest.get(i + 1..i + 3).and_then(|digits| u8::from_str_radix(digits, 16).ok())
        {
            out.push(code);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let text = String::from_utf8(out).ok()?;
    // file:///C:/a is /C:/a; the drive's letter comes first on Windows.
    let b = text.as_bytes();
    if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
        return Some(text[1..].replace('/', "\\"));
    }
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return Some(text.replace('/', "\\"));
    }
    Some(text)
}

/// What a path says about the program that wrote it: a Linux path on a Windows computer, or a Windows share
/// in a WSL distribution, is from one in WSL; a drive letter is from one run in Windows.
pub fn style_of_path(path: &str) -> Option<PathStyle> {
    let b = path.as_bytes();
    if path.starts_with("\\\\wsl") || path.starts_with("//wsl") {
        return Some(PathStyle::Wsl);
    }
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return Some(PathStyle::Native);
    }
    path.starts_with('/').then_some(PathStyle::Wsl)
}

// ---------- The lock file ----------

/// The folder Claude Code keeps its settings in: CLAUDE_CONFIG_DIR, or .claude in the home folder.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).filter(|home| !home.is_empty())?;
    Some(PathBuf::from(home).join(".claude"))
}

/// Where lock files go: an `ide` folder in the config folder.
pub fn lock_dir() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("ide"))
}

/// What the lock file says, as Claude Code reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct LockContents {
    pub pid: u32,
    pub workspace_folders: Vec<String>,
    pub ide_name: String,
    pub running_in_windows: bool,
    pub auth_token: String,
}

impl LockContents {
    pub fn to_json(&self) -> Value {
        json!({
            "pid": self.pid,
            "workspaceFolders": self.workspace_folders,
            "ideName": self.ide_name,
            "transport": "ws",
            "runningInWindows": self.running_in_windows,
            "authToken": self.auth_token,
        })
    }

    pub fn from_json(value: &Value) -> Option<Self> {
        Some(Self {
            pid: u32::try_from(value.get("pid")?.as_u64()?).ok()?,
            workspace_folders: value.get("workspaceFolders")?.as_array()?.iter().filter_map(|f| f.as_str().map(str::to_string)).collect(),
            ide_name: value.get("ideName")?.as_str()?.to_string(),
            running_in_windows: value.get("runningInWindows").and_then(Value::as_bool).unwrap_or(false),
            auth_token: value.get("authToken")?.as_str()?.to_string(),
        })
    }
}

/// A lock file written for a running server, and removed when this is dropped.
pub struct Lock {
    path: PathBuf,
}

impl Lock {
    /// Writes `<dir>/<port>.lock`, readable by the person alone (it holds the token that lets a program in).
    pub fn write(dir: &Path, port: u16, contents: &LockContents) -> std::io::Result<Lock> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
        }
        #[cfg(not(unix))]
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{port}.lock"));
        let lock = Lock { path };
        lock.rewrite(contents)?;
        Ok(lock)
    }

    /// Writes it again, with other folders say.
    pub fn rewrite(&self, contents: &LockContents) -> std::io::Result<()> {
        let text = serde_json::to_string(&contents.to_json()).unwrap_or_default();
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&self.path)?;
            file.write_all(text.as_bytes())
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&self.path, text)
        }
    }

    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Reads a lock file.
pub fn read_lock(path: &Path) -> Option<LockContents> {
    LockContents::from_json(&serde_json::from_slice(&std::fs::read(path).ok()?).ok()?)
}

/// Removes lock files this program left when it ended badly: ones that name it and whose port nobody answers on.
/// Another Limoniter's, still running, answers, and is left. Returns how many went.
pub fn remove_stale(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let port = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".lock")).and_then(|n| n.parse::<u16>().ok());
        let (Some(port), Some(lock)) = (port, read_lock(&path)) else {
            continue;
        };
        if lock.ide_name != IDE_NAME {
            continue;
        }
        let answers = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), Duration::from_millis(300)).is_ok();
        if !answers && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// A random identifier in the form of a UUID, to be the token.
fn new_token() -> String {
    use ring::rand::SecureRandom;
    let mut bytes = [0u8; 16];
    if ring::rand::SystemRandom::new().fill(&mut bytes).is_err() {
        // Without a source of randomness there's no safe token; the time and process are the best left.
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        bytes = (now ^ (u128::from(std::process::id()) << 64)).to_le_bytes();
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

/// The address Windows gives WSL's network, read from `ipconfig`'s output, for a `claude` in WSL to reach this
/// server by (it asks its default gateway, which is that). None when there's no such adapter, as with mirrored
/// networking, where 127.0.0.1 does.
pub fn wsl_host_address(ipconfig: &str) -> Option<Ipv4Addr> {
    let mut in_wsl = false;
    for line in ipconfig.lines() {
        if !line.starts_with(' ') && !line.trim().is_empty() {
            in_wsl = line.contains("vEthernet (WSL");
            continue;
        }
        if in_wsl && line.contains(':') {
            let value = line.rsplit(':').next().unwrap_or("").trim();
            let value = value.split('(').next().unwrap_or("").trim();
            if let Ok(address) = value.parse::<Ipv4Addr>() {
                return Some(address);
            }
        }
    }
    None
}

// ---------- What the window is asked, and what it says ----------

/// The proposed change a client asks to be shown.
#[derive(Clone, Debug, PartialEq)]
pub struct DiffRequest {
    pub old_file_path: String,
    pub new_file_path: String,
    pub new_file_contents: String,
    /// The label the client calls it by, to ask for it to be closed.
    pub tab_name: String,
}

/// What the window is asked by a client. Each is answered, if it's to be, on its `reply`, with the texts of the
/// answer (a tool's result is a list of texts).
pub enum Event {
    Connected,
    Disconnected { client: u64 },
    OpenDiff { client: u64, request: DiffRequest, reply: Sender<Vec<String>> },
    CloseTab { client: u64, tab_name: String, reply: Sender<Vec<String>> },
    CloseAllDiffTabs { client: u64, reply: Sender<Vec<String>> },
    OpenFile { client: u64, request: OpenFileRequest, reply: Sender<Vec<String>> },
    /// `uri` as the client wrote it, or None for every file.
    Diagnostics { client: u64, uri: Option<String>, reply: Sender<Vec<String>> },
    /// What's selected in the file in front (Codex asks; Claude Code is told as it changes).
    CurrentSelection { client: u64, reply: Sender<Vec<String>> },
    /// The open files.
    OpenEditors { client: u64, reply: Sender<Vec<String>> },
}

/// What `openFile` asks for: a file as a tab, and perhaps part of it selected, found by the text it starts and ends with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenFileRequest {
    pub file_path: String,
    pub start_text: Option<String>,
    pub end_text: Option<String>,
    pub select_to_end_of_line: bool,
    /// False when the window shouldn't come to the front.
    pub make_frontmost: bool,
}

/// Where `openFile`'s selection goes in a file's lines: from where the start text first appears to the end of the
/// end text found after it (or of the start text itself, with no end text), as a line and a character along it
/// for each end. None if there's no start text, or it isn't in the file.
pub fn selection_for(lines: &[String], start_text: Option<&str>, end_text: Option<&str>, to_end_of_line: bool) -> Option<((usize, usize), (usize, usize))> {
    let start_text = start_text.filter(|text| !text.is_empty())?;
    let whole = lines.join("\n");
    let place = |byte: usize| -> (usize, usize) {
        let before = &whole[..byte];
        let line = before.matches('\n').count();
        let start = before.rfind('\n').map_or(0, |at| at + 1);
        (line, before[start..].chars().count())
    };
    let from = whole.find(start_text)?;
    let mut to = from + start_text.len();
    if let Some(end) = end_text.filter(|text| !text.is_empty())
        && let Some(at) = whole[from..].find(end)
    {
        to = from + at + end.len();
    }
    let mut end = place(to);
    if to_end_of_line {
        end.1 = lines.get(end.0).map_or(end.1, |line| line.chars().count());
    }
    Some((place(from), end))
}

/// What a person's decision on a diff is told to the client as.
pub fn diff_answer(decision: &crate::review::Decision, tab_name: &str) -> Vec<String> {
    use crate::review::Decision;
    match decision {
        Decision::Accepted(text) => vec!["FILE_SAVED".into(), text.clone()],
        Decision::Rejected => vec!["DIFF_REJECTED".into(), tab_name.into()],
        Decision::Closed => vec!["TAB_CLOSED".into(), tab_name.into()],
    }
}

/// The tools offered, as `tools/list` gives them.
pub fn tools() -> Value {
    json!([
        {
            "name": "openDiff",
            "description": "Open a diff view in the editor showing a proposed change, and wait for it to be accepted or rejected.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "old_file_path": { "type": "string" },
                    "new_file_path": { "type": "string" },
                    "new_file_contents": { "type": "string" },
                    "tab_name": { "type": "string" }
                },
                "required": ["old_file_path", "new_file_path", "new_file_contents", "tab_name"],
                "additionalProperties": false
            }
        },
        {
            "name": "getDiagnostics",
            "description": "Get language diagnostics (errors and warnings) from the editor.",
            "inputSchema": {
                "type": "object",
                "properties": { "uri": { "type": "string", "description": "File URI to get diagnostics for. If not provided, gets diagnostics for all files." } },
                "additionalProperties": false
            }
        },
        {
            "name": "close_tab",
            "description": "Close a tab in the editor by its name.",
            "inputSchema": {
                "type": "object",
                "properties": { "tab_name": { "type": "string" } },
                "required": ["tab_name"],
                "additionalProperties": false
            }
        },
        {
            "name": "openFile",
            "description": "Open a file in the editor and optionally select a range of text.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "filePath": { "type": "string", "description": "Path to the file to open." },
                    "preview": { "type": "boolean", "description": "Whether to open the file in preview mode.", "default": false },
                    "startText": { "type": "string", "description": "Text pattern to find the start of the selection range." },
                    "endText": { "type": "string", "description": "Text pattern to find the end of the selection range." },
                    "selectToEndOfLine": { "type": "boolean", "description": "Extend the selection to the end of the line containing the end text.", "default": false },
                    "makeFrontmost": { "type": "boolean", "description": "Whether to make the file the active editor tab.", "default": true }
                },
                "required": ["filePath"],
                "additionalProperties": false
            }
        },
        {
            "name": "closeAllDiffTabs",
            "description": "Close all diff tabs in the editor.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }
    ])
}

/// The tools offered to a client that asks over HTTP (Codex): the same ones for problems and opening files, and
/// two to ask what the person has in front of them, since it isn't told as Claude Code is. No diffs: Codex's
/// changes are reviewed through its own approvals.
pub fn tools_for_http() -> Value {
    let mut tools: Vec<Value> = tools().as_array().cloned().unwrap_or_default().into_iter().filter(|t| matches!(t.get("name").and_then(Value::as_str), Some("getDiagnostics" | "openFile"))).collect();
    tools.push(json!({
        "name": "getCurrentSelection",
        "description": "Get the text selected in the editor's active file, and where it is.",
        "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
    }));
    tools.push(json!({
        "name": "getOpenEditors",
        "description": "List the files open in the editor, and which one is active.",
        "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
    }));
    // Codex asks the person before running a tool that might change something. These only look, or (openFile) show a
    // file, so they say so, and run without asking.
    for tool in &mut tools {
        let looks_only = tool.get("name").and_then(Value::as_str) != Some("openFile");
        tool["annotations"] = json!({ "readOnlyHint": looks_only, "destructiveHint": false, "openWorldHint": false });
    }
    Value::Array(tools)
}

/// The language servers' problems as `getDiagnostics` answers with them: one entry per file, in VS Code's form.
/// Each file is its address, how many lines it has, and its problems as the servers gave them (severity named,
/// not numbered).
pub fn diagnostics_json(files: &[(String, Option<usize>, Vec<Value>)]) -> Value {
    Value::Array(
        files
            .iter()
            .map(|(uri, lines, problems)| {
                let mut entry = json!({ "uri": uri, "diagnostics": problems.iter().map(diagnostic_json).collect::<Vec<_>>() });
                if let Some(lines) = lines {
                    entry["linesInFile"] = json!(lines);
                }
                entry
            })
            .collect(),
    )
}

/// One problem from a language server, in VS Code's form: the message, the severity by name, the range, where it
/// came from, and its code.
pub fn diagnostic_json(problem: &Value) -> Value {
    let severity = match problem.get("severity").and_then(Value::as_u64) {
        Some(2) => "Warning",
        Some(3) => "Information",
        Some(4) => "Hint",
        _ => "Error",
    };
    let position = |at: Option<&Value>| json!({ "line": at.and_then(|p| p.get("line")).and_then(Value::as_u64).unwrap_or(0), "character": at.and_then(|p| p.get("character")).and_then(Value::as_u64).unwrap_or(0) });
    let range = problem.get("range");
    let mut out = json!({
        "message": problem.get("message").and_then(Value::as_str).unwrap_or(""),
        "severity": severity,
        "range": { "start": position(range.and_then(|r| r.get("start"))), "end": position(range.and_then(|r| r.get("end"))) },
    });
    if let Some(source) = problem.get("source").and_then(Value::as_str) {
        out["source"] = json!(source);
    }
    // A code can be a number, a string, or an object with the code in `value`.
    match problem.get("code") {
        Some(Value::Object(code)) => {
            if let Some(value) = code.get("value") {
                out["code"] = value.clone();
            }
        }
        Some(code @ (Value::String(_) | Value::Number(_))) => out["code"] = code.clone(),
        _ => {}
    }
    out
}

/// The selection `selection_changed` tells the client of: lines and characters from 0.
#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    /// The file, in the form this computer writes it.
    pub path: String,
    pub text: String,
    pub start: (usize, usize),
    pub end: (usize, usize),
}

impl Selection {
    /// The notification's parameters, with the path written the way the client does.
    pub fn params(&self, style: PathStyle, distro: Option<&str>) -> Value {
        let path = if style == PathStyle::Wsl { to_wsl(&self.path, distro) } else { self.path.clone() };
        json!({
            "text": self.text,
            "filePath": path,
            "fileUrl": file_uri(&path),
            "selection": {
                "start": { "line": self.start.0, "character": self.start.1 },
                "end": { "line": self.end.0, "character": self.end.1 },
                "isEmpty": self.start == self.end,
            }
        })
    }
}

/// Waits for a value to stop changing before it's let through, and lets each through once.
pub struct Debounce<T> {
    delay: Duration,
    latest: Option<T>,
    changed: Option<Instant>,
    sent: Option<T>,
}

impl<T: Clone + PartialEq> Debounce<T> {
    pub fn new(delay: Duration) -> Self {
        Self { delay, latest: None, changed: None, sent: None }
    }

    /// What there is now. A change starts the wait again.
    pub fn offer(&mut self, value: Option<T>, now: Instant) {
        if value != self.latest {
            self.latest = value;
            self.changed = Some(now);
        }
    }

    /// The value, once it has stayed the same for the delay and hasn't been let through already.
    pub fn ready(&mut self, now: Instant) -> Option<T> {
        let since = self.changed?;
        if now.duration_since(since) < self.delay {
            return None;
        }
        self.changed = None;
        let value = self.latest.clone()?;
        if self.sent.as_ref() == Some(&value) {
            return None;
        }
        self.sent = Some(value.clone());
        Some(value)
    }

    /// How long until `ready` could say something, to wake the window by.
    pub fn wait(&self, now: Instant) -> Option<Duration> {
        self.changed.map(|since| self.delay.saturating_sub(now.duration_since(since)))
    }

    /// Forgets what was let through, so the current value goes out again after `after` (for a new client).
    pub fn again(&mut self, now: Instant, after: Duration) {
        self.sent = None;
        if self.latest.is_some() {
            // Shifted back so that `ready` finds the delay over once `after` has passed.
            self.changed = Some(now + after.saturating_sub(self.delay));
        }
    }
}

// ---------- The server ----------

/// How the server is started.
pub struct Options {
    /// The folder for the lock file, or None for no lock file.
    pub lock_dir: Option<PathBuf>,
    /// The project's folders, as the client's own computer writes them.
    pub folders: Vec<String>,
    /// Whether Limoniter is on Windows, which the client needs to know to translate paths.
    pub running_in_windows: bool,
    /// Other addresses to listen on too, at the same port (Windows' side of WSL's network).
    pub also_on: Vec<IpAddr>,
    /// Called when something has happened that the window should look at.
    pub wake: Arc<dyn Fn() + Send + Sync>,
}

struct Client {
    id: u64,
    to: Sender<Out>,
    /// When it last asked for something, to tell which one a mention goes to.
    last_seen: Arc<AtomicU64>,
    style: Arc<AtomicU8>,
}

enum Out {
    Notify(Value),
    Close,
}

struct Shared {
    clients: Mutex<Vec<Client>>,
    stop: AtomicBool,
    next_id: AtomicU64,
    /// A counter that goes up with each request, for `last_seen`.
    clock: AtomicU64,
    default_style: AtomicU8,
    token: String,
    events: Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

pub struct Server {
    pub port: u16,
    #[cfg_attr(not(test), allow(dead_code))] // Tests connect with it.
    pub token: String,
    events: Receiver<Event>,
    shared: Arc<Shared>,
    lock: Option<Lock>,
    contents: Mutex<LockContents>,
}

impl Server {
    /// Starts listening, at a port the system chooses, and writes the lock file.
    pub fn start(options: Options) -> std::io::Result<Server> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let token = new_token();
        let (events_tx, events) = channel();
        let shared = Arc::new(Shared {
            clients: Mutex::new(Vec::new()),
            stop: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            clock: AtomicU64::new(1),
            default_style: AtomicU8::new(PathStyle::Native.to_byte()),
            token: token.clone(),
            events: events_tx,
            wake: options.wake,
        });
        let mut listeners = vec![listener];
        for address in options.also_on {
            // Another address is a help for programs in WSL; if it can't be had, they use 127.0.0.1 instead.
            if let Ok(extra) = TcpListener::bind((address, port)) {
                listeners.push(extra);
            }
        }
        for listener in listeners {
            listener.set_nonblocking(true)?;
            let shared = shared.clone();
            std::thread::Builder::new().name("claude-ide-listen".into()).spawn(move || accept_loop(&listener, &shared))?;
        }
        let contents = LockContents { pid: std::process::id(), workspace_folders: options.folders, ide_name: IDE_NAME.into(), running_in_windows: options.running_in_windows, auth_token: token.clone() };
        let lock = match &options.lock_dir {
            Some(dir) => {
                remove_stale(dir);
                Some(Lock::write(dir, port, &contents)?)
            }
            None => None,
        };
        Ok(Server { port, token, events, shared, lock, contents: Mutex::new(contents) })
    }

    /// What the clients have asked since last asked.
    pub fn events(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    /// How many clients are connected.
    pub fn clients(&self) -> usize {
        self.shared.clients.lock().map_or(0, |clients| clients.len())
    }

    /// How a client writes paths, as far as can be told.
    pub fn style(&self, client: u64) -> PathStyle {
        self.shared.clients.lock().ok().and_then(|clients| clients.iter().find(|c| c.id == client).map(|c| PathStyle::from_byte(c.style.load(Ordering::Relaxed)))).unwrap_or(PathStyle::Native)
    }

    /// How a client is taken to write paths until its first request tells.
    pub fn set_default_style(&self, style: PathStyle) {
        self.shared.default_style.store(style.to_byte(), Ordering::Relaxed);
    }

    /// Tells every client something, written for each in its own way of writing paths.
    pub fn notify_all(&self, method: &str, make: impl Fn(PathStyle) -> Value) {
        if let Ok(clients) = self.shared.clients.lock() {
            for client in clients.iter() {
                let params = make(PathStyle::from_byte(client.style.load(Ordering::Relaxed)));
                let _ = client.to.send(Out::Notify(json!({ "jsonrpc": "2.0", "method": method, "params": params })));
            }
        }
    }

    /// Tells the client that asked for something most recently (the one the person is at).
    pub fn notify_latest(&self, method: &str, make: impl Fn(PathStyle) -> Value) {
        if let Ok(clients) = self.shared.clients.lock()
            && let Some(client) = clients.iter().max_by_key(|c| c.last_seen.load(Ordering::Relaxed))
        {
            let params = make(PathStyle::from_byte(client.style.load(Ordering::Relaxed)));
            let _ = client.to.send(Out::Notify(json!({ "jsonrpc": "2.0", "method": method, "params": params })));
        }
    }

    /// Changes the folders the lock file names.
    pub fn set_folders(&self, folders: Vec<String>) {
        if let (Some(lock), Ok(mut contents)) = (&self.lock, self.contents.lock())
            && contents.workspace_folders != folders
        {
            contents.workspace_folders = folders;
            let _ = lock.rewrite(&contents);
        }
    }

    /// Where the lock file is.
    #[cfg(test)]
    pub fn lock_path(&self) -> Option<&Path> {
        self.lock.as_ref().map(Lock::path)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Ok(clients) = self.shared.clients.lock() {
            for client in clients.iter() {
                let _ = client.to.send(Out::Close);
            }
        }
        // The lock goes with it.
    }
}

fn accept_loop(listener: &TcpListener, shared: &Arc<Shared>) {
    while !shared.stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                let shared = shared.clone();
                let full = shared.clients.lock().map_or(true, |clients| clients.len() >= MOST_CLIENTS);
                if full {
                    continue;
                }
                let _ = std::thread::Builder::new().name("claude-ide-client".into()).spawn(move || serve(stream, &shared));
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// Checks the handshake: the right token, and the subprotocol `mcp` agreed. (The refusal's type is the library's.)
#[allow(clippy::result_large_err)]
fn check_handshake(token: &str, request: &Request, mut response: Response) -> Result<Response, ErrorResponse> {
    let given = request.headers().get(AUTH_HEADER).and_then(|v| v.to_str().ok());
    if given != Some(token) {
        let mut refusal = ErrorResponse::new(Some("Unauthorized".to_string()));
        *refusal.status_mut() = tungstenite::http::StatusCode::UNAUTHORIZED;
        return Err(refusal);
    }
    let offers_mcp = request.headers().get_all("sec-websocket-protocol").iter().filter_map(|v| v.to_str().ok()).any(|v| v.split(',').any(|p| p.trim() == "mcp"));
    if offers_mcp {
        response.headers_mut().insert("sec-websocket-protocol", tungstenite::http::HeaderValue::from_static("mcp"));
    }
    Ok(response)
}

/// One answer being waited for from the window.
struct Pending {
    id: Value,
    reply: Receiver<Vec<String>>,
}

fn result_of(texts: &[String]) -> Value {
    json!({ "content": texts.iter().map(|t| json!({ "type": "text", "text": t })).collect::<Vec<_>>() })
}

/// Looks after one client from its handshake to its end.
#[allow(clippy::result_large_err)] // The handshake's callback is the library's.
fn serve(stream: TcpStream, shared: &Arc<Shared>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_nodelay(true);
    if is_plain_http(&stream) {
        serve_http(stream, shared);
        return;
    }
    let token = shared.token.clone();
    let Ok(mut socket) = tungstenite::accept_hdr(stream, move |request: &Request, response: Response| check_handshake(&token, request, response)) else {
        return;
    };
    let _ = socket.get_ref().set_read_timeout(Some(TICK));
    let id = shared.next_id.fetch_add(1, Ordering::Relaxed);
    let (to, from) = channel();
    let last_seen = Arc::new(AtomicU64::new(shared.clock.fetch_add(1, Ordering::Relaxed)));
    let style = Arc::new(AtomicU8::new(shared.default_style.load(Ordering::Relaxed)));
    if let Ok(mut clients) = shared.clients.lock() {
        clients.push(Client { id, to, last_seen: last_seen.clone(), style: style.clone() });
    }
    let _ = shared.events.send(Event::Connected);
    (shared.wake)();
    let mut pending: Vec<Pending> = Vec::new();
    'talk: while !shared.stop.load(Ordering::Relaxed) {
        // What the window has to say: notifications, or an end.
        while let Ok(out) = from.try_recv() {
            match out {
                Out::Notify(message) => {
                    if socket.send(Message::text(message.to_string())).is_err() {
                        break 'talk;
                    }
                }
                Out::Close => break 'talk,
            }
        }
        // Answers the window has made.
        let mut index = 0;
        while index < pending.len() {
            match pending[index].reply.try_recv() {
                Ok(texts) => {
                    let done = pending.remove(index);
                    if socket.send(Message::text(json!({ "jsonrpc": "2.0", "id": done.id, "result": result_of(&texts) }).to_string())).is_err() {
                        break 'talk;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    let done = pending.remove(index);
                    let message = json!({ "jsonrpc": "2.0", "id": done.id, "error": { "code": -32603, "message": "Limoniter couldn't do that" } });
                    if socket.send(Message::text(message.to_string())).is_err() {
                        break 'talk;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => index += 1,
            }
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                last_seen.store(shared.clock.fetch_add(1, Ordering::Relaxed), Ordering::Relaxed);
                let Ok(message) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                match handle(&message, id, shared, &style, false, None) {
                    Handled::Reply(reply) => {
                        if socket.send(Message::text(reply.to_string())).is_err() {
                            break 'talk;
                        }
                    }
                    Handled::Wait(pend) => pending.push(pend),
                    Handled::Nothing => {}
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
    }
    let _ = socket.close(None);
    if let Ok(mut clients) = shared.clients.lock() {
        clients.retain(|c| c.id != id);
    }
    let _ = shared.events.send(Event::Disconnected { client: id });
    (shared.wake)();
}

/// Whether what's arriving is a plain HTTP request (Codex's MCP client) and not a WebSocket's opening: a POST, or
/// a GET that doesn't ask to be upgraded.
fn is_plain_http(stream: &TcpStream) -> bool {
    let mut seen = [0u8; 2048];
    let began = Instant::now();
    loop {
        let Ok(count) = stream.peek(&mut seen) else { return false };
        if count >= 5 || began.elapsed() > Duration::from_secs(3) {
            let head = String::from_utf8_lossy(&seen[..count]).to_lowercase();
            return head.starts_with("post ") || head.starts_with("delete ") || (head.starts_with("get ") && !head.contains("upgrade: websocket"));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The most of a request body taken.
const MOST_BODY: usize = 4 * 1024 * 1024;
/// How long a tool call is waited for: the window answers within a moment, but a person may take longer on a diff.
const WAIT_FOR_WINDOW: Duration = Duration::from_secs(600);

fn http_reply(stream: &mut TcpStream, status: &str, body: &str) {
    use std::io::Write;
    let head = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    let _ = stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(body.as_bytes())).and_then(|()| stream.flush());
}

/// Looks after one HTTP request, in MCP's streamable form: a JSON-RPC message in the POST's body, its answer in the
/// reply's. The key is `Authorization: Bearer <key>`, or the header Claude Code uses.
fn serve_http(mut stream: TcpStream, shared: &Arc<Shared>) {
    use std::io::Read;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut bytes: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break at;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
        }
        if bytes.len() > 64 * 1024 {
            return;
        }
    };
    let head = String::from_utf8_lossy(&bytes[..head_end]).into_owned();
    let mut lines = head.lines();
    let method = lines.next().unwrap_or("").split(' ').next().unwrap_or("").to_string();
    let header = |name: &str| lines.clone().find_map(|l| l.split_once(':').filter(|(k, _)| k.trim().eq_ignore_ascii_case(name)).map(|(_, v)| v.trim().to_string()));
    let given = header("authorization").map(|v| v.strip_prefix("Bearer ").unwrap_or(&v).to_string()).or_else(|| header(AUTH_HEADER));
    if given.as_deref() != Some(shared.token.as_str()) {
        return http_reply(&mut stream, "401 Unauthorized", "{}");
    }
    if method != "POST" {
        // No stream of messages from the server is offered; the client does without.
        return http_reply(&mut stream, "405 Method Not Allowed", "{}");
    }
    let length: usize = header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0).min(MOST_BODY);
    let mut body = bytes[head_end + 4..].to_vec();
    while body.len() < length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    body.truncate(length);
    let Ok(message) = serde_json::from_slice::<Value>(&body) else {
        return http_reply(&mut stream, "400 Bad Request", "{}");
    };
    let id = shared.next_id.fetch_add(1, Ordering::Relaxed);
    let style = AtomicU8::new(shared.default_style.load(Ordering::Relaxed));
    // A session Limoniter started says which it is, so the board knows who is asking.
    let session = header(SESSION_HEADER);
    match handle(&message, id, shared, &style, true, session.as_deref()) {
        Handled::Reply(reply) => http_reply(&mut stream, "200 OK", &reply.to_string()),
        Handled::Wait(pend) => {
            let answer = match pend.reply.recv_timeout(WAIT_FOR_WINDOW) {
                Ok(texts) => json!({ "jsonrpc": "2.0", "id": pend.id, "result": result_of(&texts) }),
                Err(_) => json!({ "jsonrpc": "2.0", "id": pend.id, "error": { "code": -32603, "message": "Limoniter couldn't do that" } }),
            };
            http_reply(&mut stream, "200 OK", &answer.to_string());
        }
        Handled::Nothing => http_reply(&mut stream, "202 Accepted", ""),
    }
}

enum Handled {
    Reply(Value),
    Wait(Pending),
    Nothing,
}

fn error(id: &Value, code: i64, message: &str) -> Handled {
    Handled::Reply(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }))
}

/// Deals with one message from a client: answers it, or asks the window and says what to wait for.
fn handle(message: &Value, client: u64, shared: &Shared, style: &AtomicU8, http: bool, who: Option<&str>) -> Handled {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Handled::Nothing;
    };
    // A notification (`notifications/initialized`, `ide_connected`) has no id and wants no answer.
    let Some(id) = message.get("id").cloned() else {
        return Handled::Nothing;
    };
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let ok = |result: Value| Handled::Reply(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
            let version = VERSIONS.iter().find(|v| **v == asked).unwrap_or(&VERSIONS[0]);
            ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
            }))
        }
        "ping" => ok(json!({})),
        "tools/list" => {
            let mut listed = if http { tools_for_http() } else { tools() };
            if let (Some(list), Some(team)) = (listed.as_array_mut(), crate::agent_team::tools().as_array()) {
                list.extend(team.iter().cloned());
            }
            ok(json!({ "tools": listed }))
        }
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(Value::Null);
            let text = |key: &str| args.get(key).and_then(Value::as_str).map(str::to_string);
            // The task board, claims and notes between the agents are Limoniter's own to answer, at once.
            if crate::agent_team::is_team_tool(name) {
                let (text, failed) = crate::agent_team::call(who, name, &args);
                (shared.wake)();
                return ok(json!({ "content": [{ "type": "text", "text": text }], "isError": failed }));
            }
            let (reply, wait) = channel();
            let asked = match name {
                "openDiff" => {
                    let (Some(old), Some(new), Some(contents), Some(tab_name)) = (text("old_file_path"), text("new_file_path"), text("new_file_contents"), text("tab_name")) else {
                        return error(&id, -32602, "openDiff needs old_file_path, new_file_path, new_file_contents and tab_name");
                    };
                    // How this client writes paths shows in the ones it sends.
                    if let Some(found) = style_of_path(&new) {
                        style.store(found.to_byte(), Ordering::Relaxed);
                    }
                    Event::OpenDiff { client, request: DiffRequest { old_file_path: old, new_file_path: new, new_file_contents: contents, tab_name }, reply }
                }
                "getDiagnostics" => {
                    let uri = text("uri");
                    if let Some(found) = uri.as_deref().and_then(path_of_uri).as_deref().and_then(style_of_path) {
                        style.store(found.to_byte(), Ordering::Relaxed);
                    }
                    Event::Diagnostics { client, uri, reply }
                }
                "close_tab" => {
                    let Some(tab_name) = text("tab_name") else {
                        return error(&id, -32602, "close_tab needs tab_name");
                    };
                    Event::CloseTab { client, tab_name, reply }
                }
                "closeAllDiffTabs" => Event::CloseAllDiffTabs { client, reply },
                "getCurrentSelection" => Event::CurrentSelection { client, reply },
                "getOpenEditors" => Event::OpenEditors { client, reply },
                "openFile" => {
                    let Some(file_path) = text("filePath") else {
                        return error(&id, -32602, "openFile needs filePath");
                    };
                    if let Some(found) = style_of_path(&file_path) {
                        style.store(found.to_byte(), Ordering::Relaxed);
                    }
                    let flag = |key: &str, otherwise: bool| args.get(key).and_then(Value::as_bool).unwrap_or(otherwise);
                    let request = OpenFileRequest { file_path, start_text: text("startText"), end_text: text("endText"), select_to_end_of_line: flag("selectToEndOfLine", false), make_frontmost: flag("makeFrontmost", true) };
                    Event::OpenFile { client, request, reply }
                }
                _ => return ok(json!({ "content": [{ "type": "text", "text": format!("Limoniter doesn't have a tool called {name}") }], "isError": true })),
            };
            if shared.events.send(asked).is_err() {
                return error(&id, -32603, "Limoniter isn't listening");
            }
            (shared.wake)();
            Handled::Wait(Pending { id, reply: wait })
        }
        _ => error(&id, -32601, "Method not found"),
    }
}

// ---------- What the rest of the program asks ----------

/// The port the server is on, 0 for none: terminals started after it's up are told it.
static PORT: AtomicU64 = AtomicU64::new(0);
/// Whether a client is connected, for the places that only offer something then (a right-click menu).
static CONNECTED: AtomicBool = AtomicBool::new(false);
/// The WSL distribution the last terminal was run in, to translate paths for.
static WSL_TERMINAL: Mutex<Option<String>> = Mutex::new(None);

pub fn publish_port(port: Option<u16>) {
    PORT.store(port.map_or(0, u64::from), Ordering::Relaxed);
}

pub fn publish_connected(connected: bool) {
    CONNECTED.store(connected, Ordering::Relaxed);
}

/// Whether a `claude` is connected now.
pub fn connected() -> bool {
    CONNECTED.load(Ordering::Relaxed)
}

/// A terminal's shell has been chosen: a WSL distribution's name, or None for any other. A `claude` that
/// connects before it has asked for anything is taken to write paths as the last shell did.
pub fn note_terminal(wsl_distro: Option<&str>) {
    if let Ok(mut last) = WSL_TERMINAL.lock() {
        *last = wsl_distro.map(str::to_string);
    }
}

/// The distribution of the last WSL terminal, or the only one there is.
pub fn wsl_distro() -> Option<String> {
    WSL_TERMINAL.lock().ok().and_then(|last| last.clone())
}

/// How a client that hasn't shown it is taken to write paths: Linux's, if the last terminal was a WSL one.
pub fn default_style() -> PathStyle {
    if cfg!(windows) && wsl_distro().is_some() { PathStyle::Wsl } else { PathStyle::Native }
}

/// What a terminal's shell is given, so that a `claude` in it finds Limoniter without being asked to: its port,
/// and, for a shell run through WSL, the same named in WSLENV so that WSL passes it on.
pub fn terminal_env(wsl: bool, wslenv: Option<&str>) -> Vec<(String, String)> {
    let port = PORT.load(Ordering::Relaxed);
    if port == 0 {
        return Vec::new();
    }
    let mut env = vec![("CLAUDE_CODE_SSE_PORT".to_string(), port.to_string())];
    if wsl {
        let existing = wslenv.unwrap_or("");
        let named = existing.split(':').any(|name| name.split('/').next() == Some("CLAUDE_CODE_SSE_PORT"));
        let all = if named { existing.to_string() } else if existing.is_empty() { "CLAUDE_CODE_SSE_PORT".to_string() } else { format!("{existing}:CLAUDE_CODE_SSE_PORT") };
        env.push(("WSLENV".to_string(), all));
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use tungstenite::client::IntoClientRequest;
    use tungstenite::stream::MaybeTlsStream;

    type Client = tungstenite::WebSocket<MaybeTlsStream<TcpStream>>;

    fn folder(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-claude-ide-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn start(tag: &str) -> (Server, PathBuf) {
        let dir = folder(tag);
        let options = Options { lock_dir: Some(dir.clone()), folders: vec!["/work/project".into()], running_in_windows: false, also_on: Vec::new(), wake: Arc::new(|| {}) };
        (Server::start(options).unwrap(), dir)
    }

    fn connect(port: u16, token: Option<&str>) -> Result<(Client, tungstenite::handshake::client::Response), tungstenite::Error> {
        let mut request = format!("ws://127.0.0.1:{port}").into_client_request().unwrap();
        request.headers_mut().insert("sec-websocket-protocol", "mcp".parse().unwrap());
        if let Some(token) = token {
            request.headers_mut().insert(AUTH_HEADER, token.parse().unwrap());
        }
        let (socket, response) = tungstenite::connect(request)?;
        if let MaybeTlsStream::Plain(stream) = socket.get_ref() {
            stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        }
        Ok((socket, response))
    }

    fn ask(socket: &mut Client, id: u64, method: &str, params: Value) {
        socket.send(Message::text(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string())).unwrap();
    }

    fn next_message(socket: &mut Client) -> Value {
        loop {
            match socket.read().unwrap() {
                Message::Text(text) => return serde_json::from_str(&text).unwrap(),
                _ => continue,
            }
        }
    }

    /// Waits for the server to have something to say to the window.
    fn next_event(server: &Server) -> Event {
        let began = Instant::now();
        loop {
            if let Ok(event) = server.events.try_recv() {
                return event;
            }
            assert!(began.elapsed() < Duration::from_secs(10), "the window was never asked");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Connects and says hello, as `claude` does; the connection event is taken.
    fn hello(server: &Server) -> Client {
        let (mut socket, _) = connect(server.port, Some(&server.token)).unwrap();
        ask(&mut socket, 1, "initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "claude-code", "version": "2.1.332" } }));
        assert_eq!(next_message(&mut socket)["id"], 1);
        assert!(matches!(next_event(server), Event::Connected));
        socket
    }

    #[test]
    fn a_connection_without_the_right_token_is_refused() {
        let (server, dir) = start("token");
        for token in [None, Some("not-the-token")] {
            match connect(server.port, token) {
                Err(tungstenite::Error::Http(response)) => assert_eq!(response.status(), 401),
                Err(other) => panic!("expected a refusal, got {other}"),
                Ok(_) => panic!("a connection with {token:?} was let in"),
            }
        }
        assert_eq!(server.clients(), 0);
        assert!(server.events().is_empty(), "the window isn't troubled by them");
        // And the right one is let in, and agrees on the subprotocol.
        let (_socket, response) = connect(server.port, Some(&server.token)).unwrap();
        assert_eq!(response.headers().get("sec-websocket-protocol").and_then(|v| v.to_str().ok()), Some("mcp"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn it_says_hello_and_lists_its_tools() {
        let (server, dir) = start("hello");
        let (mut socket, _) = connect(server.port, Some(&server.token)).unwrap();
        ask(&mut socket, 7, "initialize", json!({ "protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": { "name": "claude-code", "version": "1" } }));
        let reply = next_message(&mut socket);
        assert_eq!(reply["id"], 7);
        assert_eq!(reply["result"]["protocolVersion"], "2025-03-26", "the client's version is kept when it's one that's known");
        let name = reply["result"]["serverInfo"]["name"].as_str().unwrap();
        assert_eq!(name, "Claude Code Limoniter MCP");
        assert!(regex::Regex::new(r"^Claude Code .+ MCP$").unwrap().is_match(name), "Claude Code matches on this");
        assert!(reply["result"]["capabilities"]["tools"].is_object());
        socket.send(Message::text(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }).to_string())).unwrap();
        ask(&mut socket, 8, "tools/list", json!({}));
        let listed = next_message(&mut socket);
        let names: Vec<&str> = listed["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names[..5], ["openDiff", "getDiagnostics", "close_tab", "openFile", "closeAllDiffTabs"], "executeCode is a Jupyter tool, which Limoniter has no use for");
        assert_eq!(names[5..].len(), 8, "and the task board's tools");
        for tool in listed["result"]["tools"].as_array().unwrap() {
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
        ask(&mut socket, 9, "ping", json!({}));
        assert_eq!(next_message(&mut socket)["result"], json!({}));
        ask(&mut socket, 10, "resources/list", json!({}));
        assert_eq!(next_message(&mut socket)["error"]["code"], -32601);
        ask(&mut socket, 11, "initialize", json!({ "protocolVersion": "1999-01-01" }));
        assert_eq!(next_message(&mut socket)["result"]["protocolVersion"], "2025-06-18", "an unknown version is answered with the newest");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn open_diff_waits_for_the_window_and_gives_its_answer() {
        let (server, dir) = start("diff");
        let mut socket = hello(&server);
        let arguments = json!({ "old_file_path": "/work/a.rs", "new_file_path": "/work/a.rs", "new_file_contents": "fn main() {}\n", "tab_name": "\u{273B} [Claude Code] a.rs (1a2b3c) \u{29C9}" });
        ask(&mut socket, 20, "tools/call", json!({ "name": "openDiff", "arguments": arguments }));
        let Event::OpenDiff { request, reply, .. } = next_event(&server) else { panic!("not a diff") };
        assert_eq!((request.new_file_path.as_str(), request.new_file_contents.as_str()), ("/work/a.rs", "fn main() {}\n"));
        assert!(request.tab_name.contains("a.rs"));
        // Nothing comes back until the person decides; the connection is still good for other things meanwhile.
        ask(&mut socket, 21, "ping", json!({}));
        assert_eq!(next_message(&mut socket)["id"], 21);
        reply.send(diff_answer(&crate::review::Decision::Accepted("fn main() { edited }\n".into()), &request.tab_name)).unwrap();
        let answer = next_message(&mut socket);
        assert_eq!(answer["id"], 20);
        assert_eq!(answer["result"]["content"], json!([{ "type": "text", "text": "FILE_SAVED" }, { "type": "text", "text": "fn main() { edited }\n" }]));
        // Rejected, and closed without a decision.
        for (id, decision, word) in [(22, crate::review::Decision::Rejected, "DIFF_REJECTED"), (23, crate::review::Decision::Closed, "TAB_CLOSED")] {
            ask(&mut socket, id, "tools/call", json!({ "name": "openDiff", "arguments": arguments }));
            let Event::OpenDiff { request, reply, .. } = next_event(&server) else { panic!("not a diff") };
            reply.send(diff_answer(&decision, &request.tab_name)).unwrap();
            let answer = next_message(&mut socket);
            assert_eq!(answer["id"], id);
            assert_eq!(answer["result"]["content"][0]["text"], word);
            assert_eq!(answer["result"]["content"][1]["text"], request.tab_name.as_str());
        }
        // A request missing what it needs is an error, not a hang.
        ask(&mut socket, 24, "tools/call", json!({ "name": "openDiff", "arguments": { "tab_name": "x" } }));
        assert_eq!(next_message(&mut socket)["error"]["code"], -32602);
        // The window failing to answer is the client's error too.
        ask(&mut socket, 25, "tools/call", json!({ "name": "openDiff", "arguments": arguments }));
        let Event::OpenDiff { reply, .. } = next_event(&server) else { panic!("not a diff") };
        drop(reply);
        assert!(next_message(&mut socket)["error"].is_object());
        // And a tool it hasn't got.
        ask(&mut socket, 26, "tools/call", json!({ "name": "executeCode", "arguments": { "code": "1" } }));
        assert_eq!(next_message(&mut socket)["result"]["isError"], true);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn open_file_asks_the_window_and_selects_by_text() {
        let (server, dir) = start("openfile");
        let mut socket = hello(&server);
        let arguments = json!({ "filePath": "/work/a.rs", "startText": "fn main", "endText": "}", "makeFrontmost": false });
        ask(&mut socket, 60, "tools/call", json!({ "name": "openFile", "arguments": arguments }));
        let Event::OpenFile { request, reply, .. } = next_event(&server) else { panic!("not an openFile") };
        assert_eq!(request, OpenFileRequest { file_path: "/work/a.rs".into(), start_text: Some("fn main".into()), end_text: Some("}".into()), select_to_end_of_line: false, make_frontmost: false });
        reply.send(vec!["Opened file: /work/a.rs".into()]).unwrap();
        assert_eq!(next_message(&mut socket)["result"]["content"][0]["text"], "Opened file: /work/a.rs");
        ask(&mut socket, 61, "tools/call", json!({ "name": "openFile", "arguments": { "filePath": "/work/b.rs" } }));
        let Event::OpenFile { request, reply, .. } = next_event(&server) else { panic!("not an openFile") };
        assert!(request.make_frontmost && request.start_text.is_none(), "in front unless it says not");
        reply.send(vec!["ok".into()]).unwrap();
        next_message(&mut socket);
        ask(&mut socket, 62, "tools/call", json!({ "name": "openFile", "arguments": {} }));
        assert_eq!(next_message(&mut socket)["error"]["code"], -32602);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_selection_is_found_from_its_text() {
        let lines: Vec<String> = ["use x;", "fn main() {", "    run();", "}", "fn other() {}"].map(String::from).to_vec();
        assert_eq!(selection_for(&lines, Some("fn main"), Some("}"), false), Some(((1, 0), (3, 1))));
        assert_eq!(selection_for(&lines, Some("run"), None, false), Some(((2, 4), (2, 7))), "no end text: just the start text");
        assert_eq!(selection_for(&lines, Some("run"), Some("("), true), Some(((2, 4), (2, 10))), "out to the end of the line");
        assert_eq!(selection_for(&lines, Some("nothing"), None, false), None);
        assert_eq!(selection_for(&lines, None, Some("}"), false), None);
        assert_eq!(selection_for(&lines, Some("fn main"), Some("not there"), false), Some(((1, 0), (1, 7))), "an end that isn't found is ignored");
        assert_eq!(selection_for(&["héllo wörld".to_string()], Some("wörld"), None, false), Some(((0, 6), (0, 11))), "characters, not bytes");
    }

    #[test]
    fn tabs_are_closed_by_name_or_all_together() {
        let (server, dir) = start("close");
        let mut socket = hello(&server);
        ask(&mut socket, 30, "tools/call", json!({ "name": "close_tab", "arguments": { "tab_name": "one" } }));
        let Event::CloseTab { tab_name, reply, .. } = next_event(&server) else { panic!("not a close") };
        assert_eq!(tab_name, "one");
        reply.send(vec!["TAB_CLOSED".into()]).unwrap();
        assert_eq!(next_message(&mut socket)["result"]["content"][0]["text"], "TAB_CLOSED");
        ask(&mut socket, 31, "tools/call", json!({ "name": "closeAllDiffTabs", "arguments": {} }));
        let Event::CloseAllDiffTabs { reply, .. } = next_event(&server) else { panic!("not a close") };
        reply.send(vec!["CLOSED_2_DIFF_TABS".into()]).unwrap();
        assert_eq!(next_message(&mut socket)["result"]["content"][0]["text"], "CLOSED_2_DIFF_TABS");
        drop(socket);
        assert!(matches!(next_event(&server), Event::Disconnected { .. }), "a client going is told to the window, which closes its diffs");
        assert_eq!(server.clients(), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn diagnostics_are_asked_for_and_given_as_json_text() {
        let (server, dir) = start("diagnostics");
        let mut socket = hello(&server);
        ask(&mut socket, 40, "tools/call", json!({ "name": "getDiagnostics", "arguments": { "uri": "file:///work/a.rs" } }));
        let Event::Diagnostics { uri, reply, .. } = next_event(&server) else { panic!("not diagnostics") };
        assert_eq!(uri.as_deref(), Some("file:///work/a.rs"));
        reply.send(vec![diagnostics_json(&[("file:///work/a.rs".into(), Some(3), vec![json!({ "severity": 1, "message": "oops", "range": { "start": { "line": 0, "character": 4 }, "end": { "line": 0, "character": 6 } } })])]).to_string()]).unwrap();
        let answer = next_message(&mut socket);
        let text = answer["result"]["content"][0]["text"].as_str().unwrap();
        assert_eq!(serde_json::from_str::<Value>(text).unwrap()[0]["diagnostics"][0]["message"], "oops");
        ask(&mut socket, 41, "tools/call", json!({ "name": "getDiagnostics", "arguments": {} }));
        let Event::Diagnostics { uri, reply, .. } = next_event(&server) else { panic!("not diagnostics") };
        assert_eq!(uri, None, "with no address, every file");
        reply.send(vec!["[]".into()]).unwrap();
        assert_eq!(next_message(&mut socket)["id"], 41);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// One plain HTTP request, as Codex's MCP client makes it, and the status and body of the answer.
    fn post(port: u16, method: &str, token: Option<&str>, body: &Value) -> (u16, Value) {
        use std::io::{Read, Write};
        let body = body.to_string();
        let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
        let request = format!("{method} /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\n{auth}Content-Length: {}\r\n\r\n{body}", body.len());
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut answer = String::new();
        let _ = stream.read_to_string(&mut answer);
        let status = answer.split(' ').nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let json = answer.split("\r\n\r\n").nth(1).and_then(|b| serde_json::from_str(b).ok()).unwrap_or(Value::Null);
        (status, json)
    }

    #[test]
    fn codex_asks_over_plain_http_with_the_key_and_gets_the_tools_for_what_is_open() {
        let (server, dir) = start("http");
        let (status, _) = post(server.port, "POST", None, &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }));
        assert_eq!(status, 401, "no key, no answer");
        let (status, _) = post(server.port, "POST", Some("wrong"), &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" }));
        assert_eq!(status, 401);
        let (status, hello) = post(server.port, "POST", Some(&server.token), &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "codex", "version": "0.172.0" } } }));
        assert_eq!(status, 200);
        assert_eq!(hello["result"]["protocolVersion"], "2025-06-18");
        let (status, _) = post(server.port, "POST", Some(&server.token), &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        assert_eq!(status, 202, "a notification is accepted and not answered");
        let (_, tools) = post(server.port, "POST", Some(&server.token), &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }));
        let names: Vec<&str> = tools["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
        assert_eq!(names[..4], ["getDiagnostics", "openFile", "getCurrentSelection", "getOpenEditors"], "no diffs: Codex's changes go through its own approvals");
        assert!(names.contains(&"claim_paths") && names.contains(&"leave_note"), "and the task board's tools");
        assert!(tools["result"]["tools"].as_array().unwrap().iter().all(|t| t["annotations"]["destructiveHint"] == false), "none asks Codex to ask the person first");
        let (status, _) = post(server.port, "GET", Some(&server.token), &json!({}));
        assert_eq!(status, 405, "no stream of server messages is offered");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_tool_called_over_http_is_asked_of_the_window_and_answered_in_the_reply() {
        let (server, dir) = start("http-call");
        let port = server.port;
        let token = server.token.clone();
        let caller = std::thread::spawn(move || post(port, "POST", Some(&token), &json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": { "name": "getCurrentSelection", "arguments": {} } })));
        let Event::CurrentSelection { reply, .. } = next_event(&server) else { panic!("not a selection") };
        reply.send(vec![json!({ "success": true, "text": "let x" }).to_string()]).unwrap();
        let (status, answer) = caller.join().unwrap();
        assert_eq!(status, 200);
        assert_eq!(answer["id"], 7);
        assert!(answer["result"]["content"][0]["text"].as_str().unwrap().contains("let x"));
        let token = server.token.clone();
        let caller = std::thread::spawn(move || post(port, "POST", Some(&token), &json!({ "jsonrpc": "2.0", "id": 8, "method": "tools/call", "params": { "name": "getOpenEditors" } })));
        let Event::OpenEditors { reply, .. } = next_event(&server) else { panic!("not the editors") };
        reply.send(vec!["{\"tabs\":[]}".into()]).unwrap();
        assert_eq!(caller.join().unwrap().1["id"], 8);
        // A WebSocket client still gets in beside it.
        let mut socket = hello(&server);
        ask(&mut socket, 3, "tools/list", json!({}));
        assert!(next_message(&mut socket)["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "openDiff"), "Claude Code's tools are as they were");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn problems_are_written_as_vs_code_writes_them() {
        let from_server = json!({
            "range": { "start": { "line": 2, "character": 5 }, "end": { "line": 2, "character": 9 } },
            "severity": 2, "code": { "value": "E0308", "target": "https://doc.rust-lang.org" }, "source": "rustc", "message": "mismatched types", "tags": [1]
        });
        assert_eq!(
            diagnostic_json(&from_server),
            json!({ "message": "mismatched types", "severity": "Warning", "range": { "start": { "line": 2, "character": 5 }, "end": { "line": 2, "character": 9 } }, "source": "rustc", "code": "E0308" })
        );
        let severities: Vec<Value> = (1..=4).map(|s| diagnostic_json(&json!({ "severity": s, "message": "m" }))["severity"].clone()).collect();
        assert_eq!(severities, ["Error", "Warning", "Information", "Hint"]);
        assert_eq!(diagnostic_json(&json!({ "message": "m", "code": 7 }))["code"], 7);
        let all = diagnostics_json(&[("file:///a".into(), Some(10), vec![from_server]), ("file:///b".into(), None, Vec::new())]);
        assert_eq!(all[0]["linesInFile"], 10);
        assert!(all[1].get("linesInFile").is_none());
        assert_eq!(all[1]["diagnostics"], json!([]));
    }

    #[test]
    fn notifications_reach_the_clients_in_their_own_paths() {
        let (server, dir) = start("notify");
        let mut socket = hello(&server);
        let selection = Selection { path: "C:\\Users\\a\\x.rs".into(), text: "hi".into(), start: (1, 0), end: (1, 2) };
        server.notify_all("selection_changed", |style| selection.params(style, Some("Ubuntu")));
        let told = next_message(&mut socket);
        assert_eq!(told["method"], "selection_changed");
        assert!(told.get("id").is_none());
        assert_eq!(told["params"]["filePath"], "C:\\Users\\a\\x.rs", "a client that writes Windows paths gets them");
        assert_eq!(told["params"]["selection"], json!({ "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 2 }, "isEmpty": false }));
        assert_eq!(told["params"]["text"], "hi");
        // A client's first request that shows it uses Linux paths changes what it's told.
        ask(&mut socket, 50, "tools/call", json!({ "name": "getDiagnostics", "arguments": { "uri": "file:///home/a/x.rs" } }));
        let Event::Diagnostics { client, reply, .. } = next_event(&server) else { panic!() };
        reply.send(vec!["[]".into()]).unwrap();
        next_message(&mut socket);
        assert_eq!(server.style(client), PathStyle::Wsl);
        server.notify_all("selection_changed", |style| selection.params(style, Some("Ubuntu")));
        let told = next_message(&mut socket);
        assert_eq!(told["params"]["filePath"], "/mnt/c/Users/a/x.rs");
        assert_eq!(told["params"]["fileUrl"], "file:///mnt/c/Users/a/x.rs");
        server.notify_latest("at_mentioned", |_| json!({ "filePath": "/mnt/c/a", "lineStart": 2, "lineEnd": 4 }));
        let told = next_message(&mut socket);
        assert_eq!((told["method"].as_str(), told["params"]["lineEnd"].as_u64()), (Some("at_mentioned"), Some(4)));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_mention_goes_to_the_client_that_was_last_active() {
        let (server, dir) = start("latest");
        let mut first = hello(&server);
        let mut second = hello(&server);
        assert_eq!(server.clients(), 2);
        // The first asks something, so it's the one the person is at.
        ask(&mut first, 60, "ping", json!({}));
        next_message(&mut first);
        server.notify_latest("at_mentioned", |_| json!({ "filePath": "/a" }));
        assert_eq!(next_message(&mut first)["method"], "at_mentioned");
        ask(&mut second, 61, "ping", json!({}));
        next_message(&mut second);
        server.notify_latest("at_mentioned", |_| json!({ "filePath": "/b" }));
        assert_eq!(next_message(&mut second)["params"]["filePath"], "/b");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_lock_file_is_written_read_back_changed_and_removed() {
        let (server, dir) = start("lock");
        let path = server.lock_path().unwrap().to_path_buf();
        assert_eq!(path, dir.join(format!("{}.lock", server.port)));
        let lock = read_lock(&path).unwrap();
        assert_eq!(lock, LockContents { pid: std::process::id(), workspace_folders: vec!["/work/project".into()], ide_name: "Limoniter".into(), running_in_windows: false, auth_token: server.token.clone() });
        let raw: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(raw["transport"], "ws", "the client picks its kind of connection from this");
        assert_eq!(raw["authToken"].as_str().map(str::len), Some(36), "a UUID");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600, "it holds the key");
        }
        server.set_folders(vec!["/elsewhere".into()]);
        assert_eq!(read_lock(&path).unwrap().workspace_folders, vec!["/elsewhere".to_string()]);
        drop(server);
        assert!(!path.exists(), "gone with the server");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_lock_file_left_by_a_crash_is_replaced_and_anothers_is_not() {
        let dir = folder("stale");
        let dead_port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port();
        let mine = |token: &str, name: &str| LockContents { pid: 1, workspace_folders: vec![], ide_name: name.into(), running_in_windows: false, auth_token: token.into() };
        std::fs::write(dir.join(format!("{dead_port}.lock")), serde_json::to_string(&mine("old", IDE_NAME).to_json()).unwrap()).unwrap();
        // VS Code's, on a port nobody answers on, is VS Code's business.
        let other_port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port();
        std::fs::write(dir.join(format!("{other_port}.lock")), serde_json::to_string(&mine("vs", "Visual Studio Code").to_json()).unwrap()).unwrap();
        // A running Limoniter's.
        let alive = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let alive_port = alive.local_addr().unwrap().port();
        std::fs::write(dir.join(format!("{alive_port}.lock")), serde_json::to_string(&mine("live", IDE_NAME).to_json()).unwrap()).unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        let server = Server::start(Options { lock_dir: Some(dir.clone()), folders: vec![], running_in_windows: false, also_on: Vec::new(), wake: Arc::new(|| {}) }).unwrap();
        assert!(!dir.join(format!("{dead_port}.lock")).exists(), "the stale one went");
        assert!(dir.join(format!("{other_port}.lock")).exists());
        assert!(dir.join(format!("{alive_port}.lock")).exists());
        assert!(dir.join("notes.txt").exists());
        assert!(server.lock_path().unwrap().exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_config_folder_can_be_moved_by_claude_config_dir() {
        // Only the rule is checked, not the environment, which other tests share.
        let dir = lock_dir();
        assert!(dir.is_none_or(|dir| dir.ends_with("ide")));
    }

    #[test]
    fn a_selection_is_told_once_it_has_stopped_changing_and_once_only() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut debounce = Debounce::new(ms(300));
        assert_eq!(debounce.ready(t0), None);
        debounce.offer(Some(1), t0);
        assert_eq!(debounce.ready(t0 + ms(100)), None, "too soon");
        debounce.offer(Some(2), t0 + ms(200));
        assert_eq!(debounce.ready(t0 + ms(400)), None, "it changed again, so the wait began again");
        assert_eq!(debounce.wait(t0 + ms(400)), Some(ms(100)));
        assert_eq!(debounce.ready(t0 + ms(500)), Some(2));
        assert_eq!(debounce.ready(t0 + ms(900)), None, "once");
        debounce.offer(Some(2), t0 + ms(1000));
        assert_eq!(debounce.wait(t0 + ms(1000)), None, "the same again is no change");
        debounce.offer(Some(3), t0 + ms(1100));
        debounce.offer(Some(2), t0 + ms(1150));
        assert_eq!(debounce.ready(t0 + ms(1500)), None, "back to what was told is nothing new");
        // A new client is told again, after a little while.
        debounce.again(t0 + ms(2000), ms(500));
        assert_eq!(debounce.ready(t0 + ms(2400)), None);
        assert_eq!(debounce.ready(t0 + ms(2500)), Some(2));
    }

    #[test]
    fn paths_are_translated_between_windows_and_wsl() {
        let ubuntu = Some("Ubuntu");
        assert_eq!(to_wsl("C:\\Users\\a\\my project\\x.rs", ubuntu), "/mnt/c/Users/a/my project/x.rs");
        assert_eq!(to_wsl("d:/data", ubuntu), "/mnt/d/data");
        assert_eq!(to_wsl("C:\\", ubuntu), "/mnt/c/");
        assert_eq!(to_wsl("\\\\wsl.localhost\\Ubuntu\\home\\a\\x.rs", ubuntu), "/home/a/x.rs");
        assert_eq!(to_wsl("\\\\wsl$\\ubuntu\\home\\a", ubuntu), "/home/a", "the old spelling, and no matter the case");
        assert_eq!(to_wsl("\\\\wsl.localhost\\Debian\\home\\a", ubuntu), "\\\\wsl.localhost\\Debian\\home\\a", "another distribution's file can't be named from this one");
        assert_eq!(to_wsl("\\\\wsl.localhost\\Debian\\home\\a", None), "/home/a");
        assert_eq!(to_wsl("/home/a", ubuntu), "/home/a", "already Linux's");
        assert_eq!(from_wsl("/mnt/c/Users/a/x.rs", "Ubuntu"), "C:\\Users\\a\\x.rs");
        assert_eq!(from_wsl("/mnt/d", "Ubuntu"), "D:\\");
        assert_eq!(from_wsl("/home/a/x.rs", "Ubuntu"), "\\\\wsl.localhost\\Ubuntu\\home\\a\\x.rs");
        assert_eq!(from_wsl("/mnt/wsl/x", "Ubuntu"), "\\\\wsl.localhost\\Ubuntu\\mnt\\wsl\\x", "not a drive");
        assert_eq!(from_wsl("C:\\already", "Ubuntu"), "C:\\already");
        // Both ways agree.
        for path in ["C:\\Users\\a\\x.rs", "\\\\wsl.localhost\\Ubuntu\\home\\a\\x.rs"] {
            assert_eq!(from_wsl(&to_wsl(path, ubuntu), "Ubuntu"), path);
        }
        assert_eq!(style_of_path("/home/a"), Some(PathStyle::Wsl));
        assert_eq!(style_of_path("C:\\a"), Some(PathStyle::Native));
        assert_eq!(style_of_path("\\\\wsl.localhost\\Ubuntu\\home"), Some(PathStyle::Wsl));
        assert_eq!(style_of_path("relative/x"), None);
    }

    #[test]
    fn addresses_of_files_are_read_and_written_as_the_client_does() {
        assert_eq!(file_uri("/home/a b/c#1.rs"), "file:///home/a b/c%231.rs", "spaces stay as VS Code leaves them");
        assert_eq!(file_uri("C:\\Users\\a\\x.rs"), "file:///C:/Users/a/x.rs");
        assert_eq!(path_of_uri("file:///home/a b/c.rs").as_deref(), Some("/home/a b/c.rs"));
        assert_eq!(path_of_uri("file:///home/a%20b/c%231.rs").as_deref(), Some("/home/a b/c#1.rs"));
        assert_eq!(path_of_uri("file:///C:/Users/a/x.rs").as_deref(), Some("C:\\Users\\a\\x.rs"));
        assert_eq!(path_of_uri("file://C:\\Users\\a\\x.rs").as_deref(), Some("C:\\Users\\a\\x.rs"), "the CLI on Windows doesn't turn the slashes");
        assert_eq!(path_of_uri("file:///100%/x").as_deref(), Some("/100%/x"), "a % that isn't a code is a %");
        assert_eq!(path_of_uri("https://x"), None);
        assert_eq!(path_of_uri(&file_uri("/mnt/c/Users/a/r\u{e9}sum\u{e9}.rs")).as_deref(), Some("/mnt/c/Users/a/r\u{e9}sum\u{e9}.rs"));
    }

    #[test]
    fn the_address_wsl_reaches_windows_by_is_found_in_ipconfig() {
        let text = "Windows IP Configuration\n\nEthernet adapter Ethernet:\n\n   Connection-specific DNS Suffix  . :\n   IPv4 Address. . . . . . . . . . . : 192.168.1.20\n   Default Gateway . . . . . . . . . : 192.168.1.1\n\nEthernet adapter vEthernet (WSL (Hyper-V firewall)):\n\n   Connection-specific DNS Suffix  . :\n   Link-local IPv6 Address . . . . . : fe80::1%45\n   IPv4 Address. . . . . . . . . . . : 172.22.48.1\n   Subnet Mask . . . . . . . . . . . : 255.255.240.0\n";
        assert_eq!(wsl_host_address(text), Some(Ipv4Addr::new(172, 22, 48, 1)));
        assert_eq!(wsl_host_address("Ethernet adapter Ethernet:\n   IPv4 Address. . . : 10.0.0.2\n"), None, "with mirrored networking there's none");
        assert_eq!(wsl_host_address(""), None);
    }

    #[test]
    fn shells_are_given_the_port_and_wsl_is_told_to_pass_it_on() {
        publish_port(Some(41234));
        assert_eq!(terminal_env(false, Some("ignored")), vec![("CLAUDE_CODE_SSE_PORT".to_string(), "41234".to_string())]);
        let wsl = terminal_env(true, None);
        assert_eq!(wsl[1], ("WSLENV".to_string(), "CLAUDE_CODE_SSE_PORT".to_string()));
        assert_eq!(terminal_env(true, Some("PATH/l:FOO"))[1].1, "PATH/l:FOO:CLAUDE_CODE_SSE_PORT", "what's there is kept");
        assert_eq!(terminal_env(true, Some("A:CLAUDE_CODE_SSE_PORT/u"))[1].1, "A:CLAUDE_CODE_SSE_PORT/u", "not named twice");
        publish_port(None);
        assert!(terminal_env(true, None).is_empty(), "with no server, nothing is set");
    }

    #[test]
    fn the_answer_to_a_decision_is_what_claude_code_reads() {
        use crate::review::Decision;
        assert_eq!(diff_answer(&Decision::Accepted("x".into()), "t"), ["FILE_SAVED", "x"]);
        assert_eq!(diff_answer(&Decision::Rejected, "t"), ["DIFF_REJECTED", "t"]);
        assert_eq!(diff_answer(&Decision::Closed, "t"), ["TAB_CLOSED", "t"]);
    }
}
