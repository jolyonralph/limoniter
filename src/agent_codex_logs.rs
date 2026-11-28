//! Codex's own logs, its "rollouts": `sessions/YYYY/MM/DD/rollout-<time>-<thread>.jsonl` in its home folder
//! (`CODEX_HOME`, or `.codex` at home). They're read for three things:
//!
//! - past sessions of a project, to be listed and taken up again (`thread/resume`), with the transcript built from
//!   what's in the log;
//! - a Codex running in Limoniter's terminal (or anywhere else) is followed as its log grows, so the files it
//!   changes are put down to Codex in Files changing, as a session in the panel's are;
//! - the tokens, context and limits such a session shows, for the Usage window: [`watched_sessions`] is the plain
//!   function to call for them.
//!
//! The format is Codex's own and not promised to stay; whatever can't be read is left out.

use crate::agent_codex::unwrap_shell;
use crate::agent_logs::Past;
use crate::agent_session::{Context, Item, Limit, Limits, Tokens, Tool, ToolKind, ToolResult};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

/// The most sessions listed.
const MOST_LISTED: usize = 50;
/// The most log files looked at to list the sessions of one folder.
const MOST_LOOKED_AT: usize = 600;
/// The most of one log read to list it.
const MOST_READ: usize = 4 * 1024 * 1024;
/// The most items a transcript built from a log has.
const MOST_ITEMS: usize = 3000;
/// A log not written to for this long isn't followed.
const FOLLOWED_FOR: Duration = Duration::from_secs(10 * 60);

/// The folder Codex keeps its logs and settings in.
pub fn codex_home() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CODEX_HOME").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).filter(|home| !home.is_empty())?;
    Some(PathBuf::from(home).join(".codex"))
}

/// A folder written the same way whichever way it was given, to compare: the separators alike, no end one, and on
/// Windows, no difference of case. Codex says `file:///` for some, and WSL's names start `\\wsl.localhost\`.
fn same_folder_key(path: &str) -> String {
    let mut text = path.trim().strip_prefix("file://").unwrap_or(path.trim()).replace('\\', "/");
    while text.len() > 1 && text.ends_with('/') {
        text.pop();
    }
    if cfg!(windows) { text.to_lowercase() } else { text }
}

/// What a log's first line says of its session.
#[derive(Clone, Debug, PartialEq)]
pub struct Meta {
    pub id: String,
    pub cwd: String,
    pub originator: String,
    /// A helper's, not the person's own conversation.
    pub is_helper: bool,
}

pub fn read_meta(path: &Path) -> Option<Meta> {
    let file = std::fs::File::open(path).ok()?;
    let mut first = String::new();
    BufReader::new(file).read_line(&mut first).ok()?;
    let line: Value = serde_json::from_str(first.trim()).ok()?;
    if line.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }
    let meta = line.get("payload")?;
    let text = |key: &str| meta.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let helper = meta.get("parent_thread_id").is_some_and(|p| !p.is_null()) || meta.get("source").is_some_and(Value::is_object);
    Some(Meta { id: text("id"), cwd: text("cwd"), originator: text("originator"), is_helper: helper })
}

/// Every log under `sessions`, the newest day first.
fn all_logs(home: &Path, most: usize) -> Vec<(SystemTime, PathBuf)> {
    let sorted = |dir: &Path| -> Vec<PathBuf> {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
        entries.sort();
        entries.reverse();
        entries
    };
    let mut found = Vec::new();
    for year in sorted(&home.join("sessions")) {
        for month in sorted(&year) {
            for day in sorted(&month) {
                for log in sorted(&day) {
                    let name = log.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    if name.starts_with("rollout-") && name.ends_with(".jsonl") {
                        let when = std::fs::metadata(&log).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
                        found.push((when, log));
                    }
                }
                if found.len() >= most {
                    return found;
                }
            }
        }
    }
    found
}

/// Names Codex gave threads: `session_index.jsonl`, lines of `{"id", "thread_name"}`. It may not be there.
fn thread_names(home: &Path) -> HashMap<String, String> {
    let mut names = HashMap::new();
    let Ok(file) = std::fs::File::open(home.join("session_index.jsonl")) else { return names };
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if let Ok(value) = serde_json::from_str::<Value>(&line)
            && let (Some(id), Some(name)) = (value.get("id").and_then(Value::as_str), value.get("thread_name").and_then(Value::as_str))
            && !name.trim().is_empty()
        {
            names.insert(id.to_string(), name.to_string());
        }
    }
    names
}

/// The text of what the person said, from an `item_completed` item of that kind.
fn person_said(item: &Value) -> String {
    item.get("content").and_then(Value::as_array).map(|parts| parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n")).unwrap_or_default().trim().to_string()
}

/// The `item_completed` item of a log line, if it is one.
fn completed_item(line: &Value) -> Option<&Value> {
    let payload = line.get("payload")?;
    (line.get("type").and_then(Value::as_str) == Some("event_msg") && payload.get("type").and_then(Value::as_str) == Some("item_completed")).then(|| payload.get("item")).flatten()
}

/// One log, listed: its title (Codex's own name for it, or the first words said), when it was last used, and the
/// first message.
fn read_past(path: &Path, meta: &Meta, names: &HashMap<String, String>) -> Option<Past> {
    let when = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    let file = std::fs::File::open(path).ok()?;
    let mut first = String::new();
    let mut read = 0usize;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        read += line.len() + 1;
        if read > MOST_READ {
            break;
        }
        if !line.contains("\"UserMessage\"") {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(&line)
            && let Some(item) = completed_item(&value)
            && item.get("type").and_then(Value::as_str) == Some("UserMessage")
        {
            first = person_said(item);
            if !first.is_empty() {
                break;
            }
        }
    }
    if first.is_empty() {
        return None;
    }
    let title = names.get(&meta.id).cloned().unwrap_or_else(|| crate::agent_session::title_from(&first));
    Some(Past { id: meta.id.clone(), title, first_message: first, when, path: path.to_path_buf(), tool: Tool::Codex, wsl: None })
}

/// The past sessions of a project, the newest first.
pub fn list(home: &Path, folder: &Path) -> Vec<Past> {
    let wanted = same_folder_key(&folder.display().to_string());
    let names = thread_names(home);
    let mut logs = all_logs(home, MOST_LOOKED_AT);
    logs.sort_by_key(|log| std::cmp::Reverse(log.0));
    let mut found = Vec::new();
    for (_, path) in logs {
        let Some(meta) = read_meta(&path) else { continue };
        if meta.is_helper || same_folder_key(&meta.cwd) != wanted {
            continue;
        }
        if let Some(past) = read_past(&path, &meta, &names) {
            found.push(past);
            if found.len() >= MOST_LISTED {
                break;
            }
        }
    }
    found
}

/// What Codex ran, as a command line: the arguments after the shell's own `-lc`, or all of them.
fn command_text(command: &Value) -> String {
    match command {
        Value::Array(parts) => {
            let words: Vec<&str> = parts.iter().filter_map(Value::as_str).collect();
            unwrap_shell(&words.iter().map(|w| if w.contains(' ') { format!("'{w}'") } else { (*w).to_string() }).collect::<Vec<_>>().join(" "))
        }
        Value::String(text) => unwrap_shell(text),
        _ => String::new(),
    }
}

/// A session's transcript from its log: what was said, the commands run with their output, the files changed.
pub fn transcript(path: &Path, folder: &Path) -> Vec<Item> {
    let Ok(file) = std::fs::File::open(path) else { return Vec::new() };
    let mut items: Vec<Item> = Vec::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if !line.contains("\"item_completed\"") {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(item) = completed_item(&value) else { continue };
        let id = item.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        match item.get("type").and_then(Value::as_str) {
            Some("UserMessage") => {
                let text = person_said(item);
                if !text.is_empty() {
                    items.push(Item::You(text));
                }
            }
            Some("AgentMessage") => {
                let text = person_said(item);
                if !text.is_empty() {
                    items.push(Item::Text { id, text, done: true });
                }
            }
            Some("Reasoning") => {
                let text = item.get("summary_text").and_then(Value::as_array).map(|p| p.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n\n")).unwrap_or_default();
                if !text.trim().is_empty() {
                    items.push(Item::Thinking { id, text });
                }
            }
            Some("CommandExecution") => {
                let command = command_text(item.get("command").unwrap_or(&Value::Null));
                let shown = command.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").chars().take(80).collect::<String>();
                let output = item.get("aggregated_output").and_then(Value::as_str).unwrap_or("").to_string();
                let code = item.get("exit_code").and_then(Value::as_i64);
                items.push(Item::Tool {
                    id,
                    name: "Shell".into(),
                    summary: crate::i18n::tf!("Ran {}", shown),
                    kind: ToolKind::Command(command),
                    parent: None,
                    result: Some(ToolResult { text: output, is_error: code.is_some_and(|c| c != 0) }),
                });
            }
            Some("FileChange") => {
                let ok = item.get("status").and_then(Value::as_str) == Some("completed");
                if let Some(changes) = item.get("changes").and_then(Value::as_object) {
                    for (n, (path, change)) in changes.iter().enumerate() {
                        let path = PathBuf::from(path);
                        let shown = crate::agent_claude::short_path(&path, folder);
                        let summary = match change.get("type").and_then(Value::as_str) {
                            Some("add") => crate::i18n::tf!("Create {}", shown),
                            Some("delete") => crate::i18n::tf!("Delete {}", shown),
                            _ => crate::i18n::tf!("Edit {}", shown),
                        };
                        let result = ToolResult { text: if ok { crate::i18n::tr("Done.").to_string() } else { crate::i18n::tr("The change failed.").to_string() }, is_error: !ok };
                        items.push(Item::Tool { id: format!("{id}#{n}"), name: "Edit".into(), summary, kind: ToolKind::FileChange(path), parent: None, result: Some(result) });
                    }
                }
            }
            _ => {}
        }
        if items.len() > MOST_ITEMS {
            items.drain(..items.len() - MOST_ITEMS);
        }
    }
    items
}

// ---------- Following a Codex that isn't driven from here ----------

/// What's known of a session running somewhere else, from its log.
#[derive(Clone, Debug, PartialEq)]
pub struct WatchedSession {
    pub id: String,
    pub path: PathBuf,
    pub model: Option<String>,
    /// What the whole session has used.
    pub tokens: Tokens,
    pub context: Option<Context>,
    pub limits: Limits,
    pub updated: SystemTime,
}

impl WatchedSession {
    fn new(id: &str, path: &Path) -> Self {
        WatchedSession { id: id.to_string(), path: path.to_path_buf(), model: None, tokens: Tokens::default(), context: None, limits: Limits::default(), updated: SystemTime::UNIX_EPOCH }
    }
}

/// What a `token_count` event's `rate_limits` say: windows of a day or less are the five-hour one.
fn limits_of(rates: &Value) -> Limits {
    let mut limits = Limits::default();
    for key in ["primary", "secondary"] {
        let Some(window) = rates.get(key).filter(|w| w.is_object()) else { continue };
        let Some(percent) = window.get("used_percent").and_then(Value::as_f64) else { continue };
        let minutes = window.get("window_minutes").and_then(Value::as_i64).unwrap_or(if key == "primary" { 300 } else { 10_080 });
        let limit = Limit { percent: percent as f32, resets: window.get("resets_at").and_then(Value::as_i64) };
        if minutes <= 24 * 60 { limits.five_hour = Some(limit) } else { limits.week = Some(limit) }
    }
    limits
}

/// Takes one line of a log into what's known of its session, and says which files it changed (the paths of a
/// `FileChange` item, which is written once the change has been made).
pub fn take_line(session: &mut WatchedSession, line: &Value) -> Vec<PathBuf> {
    let payload = line.get("payload").unwrap_or(&Value::Null);
    let mut changed = Vec::new();
    match (line.get("type").and_then(Value::as_str), payload.get("type").and_then(Value::as_str)) {
        (Some("turn_context"), _) => {
            if let Some(model) = payload.get("model").and_then(Value::as_str) {
                session.model = Some(model.to_string());
            }
        }
        (Some("event_msg"), Some("token_count")) => {
            if let Some(info) = payload.get("info").filter(|i| i.is_object()) {
                let number = |part: &str, key: &str| info.get(part).and_then(|u| u.get(key)).and_then(Value::as_u64).unwrap_or(0);
                session.tokens = Tokens { input: number("total_token_usage", "input_tokens"), output: number("total_token_usage", "output_tokens"), cached: number("total_token_usage", "cached_input_tokens") };
                if let Some(max) = info.get("model_context_window").and_then(Value::as_u64).filter(|m| *m > 0) {
                    session.context = Some(Context { used: number("last_token_usage", "total_tokens"), max });
                }
            }
            if let Some(rates) = payload.get("rate_limits") {
                session.limits.merge(limits_of(rates));
            }
        }
        (Some("event_msg"), Some("item_completed")) => {
            if let Some(item) = payload.get("item")
                && item.get("type").and_then(Value::as_str) == Some("FileChange")
                && let Some(changes) = item.get("changes").and_then(Value::as_object)
            {
                changed.extend(changes.keys().map(PathBuf::from));
            }
        }
        _ => {}
    }
    if let Some(when) = line.get("timestamp").and_then(Value::as_str).and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()) {
        session.updated = SystemTime::UNIX_EPOCH + Duration::from_secs(when.timestamp().max(0) as u64);
    }
    changed
}

#[allow(dead_code, reason = "for the Usage window, which reads a log's use of tokens")]
/// What's known of one log, read whole: the plain function for a log's tokens, context and limits.
pub fn rollout_usage(path: &Path) -> Option<WatchedSession> {
    let meta = read_meta(path)?;
    let mut session = WatchedSession::new(&meta.id, path);
    for line in BufReader::new(std::fs::File::open(path).ok()?).lines().map_while(Result::ok) {
        if (line.contains("\"token_count\"") || line.contains("\"turn_context\""))
            && let Ok(value) = serde_json::from_str::<Value>(&line)
        {
            take_line(&mut session, &value);
        }
    }
    Some(session)
}

/// A log being followed.
struct Tail {
    session: WatchedSession,
    offset: u64,
    /// A change it makes is Codex's only once it's been seen to be made while following: the backlog isn't.
    following: bool,
    seen_at: SystemTime,
}

/// What the follower knows, for others to read.
static WATCHED: Mutex<Vec<WatchedSession>> = Mutex::new(Vec::new());
static FOLDER: Mutex<Option<PathBuf>> = Mutex::new(None);
static STARTED: std::sync::Once = std::sync::Once::new();

/// The sessions of Codex that were running in the folder being watched, in a terminal or elsewhere (not those
/// driven from the panel, which tell the window what they use themselves), with what each has used. A plain
/// function, to be called from anywhere, such as the Usage window.
#[allow(dead_code, reason = "for the Usage window")]
pub fn watched_sessions() -> Vec<WatchedSession> {
    WATCHED.lock().map(|list| list.clone()).unwrap_or_default()
}

/// Starts following Codex logs for the project in this folder (the first call starts the thread, later ones change
/// the folder).
pub fn watch(folder: &Path) {
    if let Ok(mut now) = FOLDER.lock() {
        *now = Some(folder.to_path_buf());
    }
    STARTED.call_once(|| {
        let _ = std::thread::Builder::new().name("codex-rollouts".into()).spawn(follow);
    });
}

/// The new whole lines of a log from `offset`, and where that leaves it.
fn new_lines(path: &Path, offset: u64) -> (Vec<String>, u64) {
    let Ok(mut file) = std::fs::File::open(path) else { return (Vec::new(), offset) };
    let length = file.metadata().map_or(0, |m| m.len());
    if length < offset {
        // Shorter than it was: it's been replaced, and is read again from the start.
        return read_from(&mut file, 0);
    }
    if length == offset {
        return (Vec::new(), offset);
    }
    read_from(&mut file, offset)
}

fn read_from(file: &mut std::fs::File, offset: u64) -> (Vec<String>, u64) {
    if file.seek(SeekFrom::Start(offset)).is_err() {
        return (Vec::new(), offset);
    }
    let mut bytes = Vec::new();
    if file.take(16 * 1024 * 1024).read_to_end(&mut bytes).is_err() {
        return (Vec::new(), offset);
    }
    // Only whole lines: a line still being written is left for next time.
    let end = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |at| at + 1);
    let text = String::from_utf8_lossy(&bytes[..end]);
    (text.lines().map(str::to_string).collect(), offset + end as u64)
}

fn follow() {
    let mut tails: HashMap<PathBuf, Tail> = HashMap::new();
    let mut last_scan: Option<std::time::Instant> = None;
    loop {
        std::thread::sleep(Duration::from_millis(200));
        let Some(folder) = FOLDER.lock().ok().and_then(|f| f.clone()) else { continue };
        let Some(home) = codex_home() else { continue };
        let scan = last_scan.is_none_or(|at| at.elapsed() > Duration::from_secs(2));
        if scan {
            last_scan = Some(std::time::Instant::now());
        }
        for path in step(&home, &folder, &mut tails, scan) {
            crate::files_changing::claim(&path, "Codex");
        }
        if let Ok(mut list) = WATCHED.lock() {
            *list = tails.values().map(|t| t.session.clone()).collect();
        }
    }
}

/// One look at the logs being followed: new ones are found (when `scan`), what each has added is taken in, and the
/// files Codex has changed since the last look are returned.
fn step(home: &Path, folder: &Path, tails: &mut HashMap<PathBuf, Tail>, scan: bool) -> Vec<PathBuf> {
    if scan {
        find_new(home, folder, tails);
    }
    let mut claims = Vec::new();
    for (path, tail) in tails.iter_mut() {
        let (lines, offset) = new_lines(path, tail.offset);
        tail.offset = offset;
        for line in lines {
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                let changed = take_line(&mut tail.session, &value);
                if tail.following {
                    claims.extend(changed);
                }
            }
        }
        tail.following = true;
        if std::fs::metadata(path).and_then(|m| m.modified()).is_ok_and(|m| m.elapsed().is_ok_and(|age| age < Duration::from_secs(5))) {
            tail.seen_at = SystemTime::now();
        }
    }
    tails.retain(|_, tail| tail.seen_at.elapsed().is_ok_and(|age| age < FOLLOWED_FOR));
    claims
}

/// Finds logs written to lately that are of this folder and aren't Limoniter's own.
fn find_new(home: &Path, folder: &Path, tails: &mut HashMap<PathBuf, Tail>) {
    let wanted = same_folder_key(&folder.display().to_string());
    for (when, path) in all_logs(home, 40) {
        if when.elapsed().is_ok_and(|age| age > FOLLOWED_FOR) {
            break;
        }
        if tails.contains_key(&path) {
            continue;
        }
        let Some(meta) = read_meta(&path) else { continue };
        if meta.originator == "limoniter" || meta.is_helper || same_folder_key(&meta.cwd) != wanted {
            continue;
        }
        tails.insert(path.clone(), Tail { session: WatchedSession::new(&meta.id, &path), offset: 0, following: false, seen_at: SystemTime::now() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-codex-logs-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Writes a log as Codex 0.172.0 does, with `\n` ends on every system.
    fn write_log(home: &Path, day: &str, id: &str, cwd: &str, extra_meta: &str, lines: &[&str]) -> PathBuf {
        let dir = home.join("sessions").join(day);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("rollout-2026-11-27T10-00-00-{id}.jsonl"));
        let meta = format!(r#"{{"timestamp":"2026-11-27T10:00:00.000Z","type":"session_meta","payload":{{"id":"{id}","cwd":"{cwd}","originator":"codex_cli_rs","cli_version":"0.172.0"{extra_meta}}}}}"#);
        let mut text = meta + "\n";
        for line in lines {
            text.push_str(line);
            text.push('\n');
        }
        std::fs::write(&path, text).unwrap();
        path
    }

    const USER: &str = r#"{"timestamp":"2026-11-27T10:00:01.000Z","type":"event_msg","payload":{"type":"item_completed","thread_id":"t","item":{"type":"UserMessage","id":"u1","content":[{"type":"text","text":"Fix the parser","text_elements":[]}]}}}"#;
    const THINK: &str = r#"{"timestamp":"2026-11-27T10:00:02.000Z","type":"event_msg","payload":{"type":"item_completed","item":{"type":"Reasoning","id":"r1","summary_text":["**Looking**"],"raw_content":[]}}}"#;
    const SAID: &str = r#"{"timestamp":"2026-11-27T10:00:03.000Z","type":"event_msg","payload":{"type":"item_completed","item":{"type":"AgentMessage","id":"m1","content":[{"type":"Text","text":"On it."}],"phase":"commentary"}}}"#;
    const RAN: &str = r#"{"timestamp":"2026-11-27T10:00:04.000Z","type":"event_msg","payload":{"type":"item_completed","item":{"type":"CommandExecution","id":"c1","command":["/bin/bash","-lc","cargo test"],"aggregated_output":"ok","exit_code":0,"status":"completed"}}}"#;
    const EDIT: &str = r#"{"timestamp":"2026-11-27T10:00:05.000Z","type":"event_msg","payload":{"type":"item_completed","item":{"type":"FileChange","id":"f1","changes":{"/p/src/a.rs":{"type":"update","unified_diff":"@@ -1 +1 @@\n-a\n+b\n","move_path":null}},"status":"completed"}}}"#;
    const COUNT: &str = r#"{"timestamp":"2026-11-27T10:00:06.000Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":600,"output_tokens":50,"total_tokens":1050},"last_token_usage":{"input_tokens":900,"cached_input_tokens":600,"output_tokens":10,"total_tokens":910},"model_context_window":258400},"rate_limits":{"primary":{"used_percent":11.0,"window_minutes":300,"resets_at":1795839145},"secondary":{"used_percent":59.0,"window_minutes":10080,"resets_at":1796252461}}}}"#;

    #[test]
    fn past_sessions_of_the_folder_are_listed_newest_first_and_helpers_left_out() {
        let home = temp("list");
        write_log(&home, "2026/11/26", "old", "/work/proj", "", &[USER]);
        std::thread::sleep(Duration::from_millis(30));
        write_log(&home, "2026/11/27", "new", "/work/proj/", "", &[USER, SAID]);
        write_log(&home, "2026/11/27", "other", "/work/else", "", &[USER]);
        write_log(&home, "2026/11/27", "helper", "/work/proj", r#","parent_thread_id":"new""#, &[USER]);
        write_log(&home, "2026/11/27", "silent", "/work/proj", "", &[SAID]);
        std::fs::write(home.join("session_index.jsonl"), "{\"id\":\"new\",\"thread_name\":\"Parser fix\",\"updated_at\":\"x\"}\n").unwrap();
        let listed = list(&home, Path::new("/work/proj"));
        let ids: Vec<_> = listed.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["new", "old"], "{ids:?}");
        assert_eq!(listed[0].title, "Parser fix");
        assert_eq!(listed[0].first_message, "Fix the parser");
        assert_eq!(listed[1].title, "Fix the parser", "without Codex's name, the first words");
        assert!(listed.iter().all(|p| p.tool == Tool::Codex));
    }

    #[test]
    fn a_transcript_has_what_was_said_the_commands_and_the_changes() {
        let home = temp("transcript");
        let path = write_log(&home, "2026/11/27", "s", "/p", "", &[USER, THINK, SAID, RAN, EDIT, "not json", COUNT]);
        let items = transcript(&path, Path::new("/p"));
        assert_eq!(items.len(), 5, "{items:?}");
        assert_eq!(items[0], Item::You("Fix the parser".into()));
        assert!(matches!(&items[1], Item::Thinking { text, .. } if text == "**Looking**"));
        assert!(matches!(&items[2], Item::Text { text, .. } if text == "On it."));
        match &items[3] {
            Item::Tool { summary, kind, result, .. } => {
                assert_eq!(summary, "Ran cargo test");
                assert_eq!(*kind, ToolKind::Command("cargo test".into()));
                assert_eq!(result.as_ref().unwrap().text, "ok");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(&items[4], Item::Tool { summary, kind: ToolKind::FileChange(p), .. } if summary == "Edit src/a.rs" && p == Path::new("/p/src/a.rs")));
    }

    #[test]
    fn a_log_gives_its_tokens_context_and_limits() {
        let home = temp("usage");
        let path = write_log(&home, "2026/11/27", "u", "/p", "", &[r#"{"type":"turn_context","payload":{"model":"gpt-5.6-luna"}}"#, COUNT]);
        let usage = rollout_usage(&path).unwrap();
        assert_eq!(usage.id, "u");
        assert_eq!(usage.model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(usage.tokens, Tokens { input: 1000, output: 50, cached: 600 });
        assert_eq!(usage.context, Some(Context { used: 910, max: 258_400 }));
        assert_eq!(usage.limits.five_hour.unwrap().percent, 11.0);
        assert_eq!(usage.limits.week.unwrap().resets, Some(1_796_252_461));
    }

    #[test]
    fn a_file_change_in_a_followed_log_names_the_files() {
        let mut session = WatchedSession::new("x", Path::new("/log"));
        let edit: Value = serde_json::from_str(EDIT).unwrap();
        assert_eq!(take_line(&mut session, &edit), vec![PathBuf::from("/p/src/a.rs")]);
        let user: Value = serde_json::from_str(USER).unwrap();
        assert!(take_line(&mut session, &user).is_empty());
    }

    #[test]
    fn only_whole_lines_are_taken_from_a_growing_log() {
        let dir = temp("tail");
        let path = dir.join("log.jsonl");
        std::fs::write(&path, "one\ntwo\nthr").unwrap();
        let (lines, offset) = new_lines(&path, 0);
        assert_eq!(lines, vec!["one", "two"]);
        assert_eq!(offset, 8);
        std::fs::write(&path, "one\ntwo\nthree\n").unwrap();
        let (lines, offset) = new_lines(&path, offset);
        assert_eq!(lines, vec!["three"]);
        assert_eq!(new_lines(&path, offset).0, Vec::<String>::new());
    }

    #[test]
    fn a_codex_in_a_terminal_is_followed_and_its_changes_named_but_not_limoniters_own_or_another_folders() {
        let home = temp("follow");
        let folder = Path::new("/work/proj");
        let log = write_log(&home, "2026/11/27", "term", "/work/proj", "", &[COUNT]);
        // The recent days' logs are the ones looked at: this one was just written.
        write_log(&home, "2026/11/27", "ours", "/work/proj", "", &[]);
        let ours = home.join("sessions/2026/11/27/rollout-2026-11-27T10-00-00-ours.jsonl");
        std::fs::write(&ours, std::fs::read_to_string(&ours).unwrap().replace("codex_cli_rs", "limoniter")).unwrap();
        write_log(&home, "2026/11/27", "elsewhere", "/work/other", "", &[COUNT]);
        let mut tails = HashMap::new();
        assert!(step(&home, folder, &mut tails, true).is_empty(), "what was there before isn't claimed");
        assert_eq!(tails.len(), 1, "only the terminal's log of this folder is followed: {:?}", tails.keys().collect::<Vec<_>>());
        let tail = tails.values().next().unwrap();
        assert_eq!(tail.session.id, "term");
        assert_eq!(tail.session.tokens.input, 1000, "its use is known from the start");
        // Codex changes a file, and the log says so.
        let mut text = std::fs::read_to_string(&log).unwrap();
        text.push_str(EDIT);
        text.push('\n');
        std::fs::write(&log, text).unwrap();
        assert_eq!(step(&home, folder, &mut tails, false), vec![PathBuf::from("/p/src/a.rs")]);
        assert!(step(&home, folder, &mut tails, false).is_empty(), "said once");
    }

    #[test]
    fn folders_are_compared_whatever_their_separators() {
        assert_eq!(same_folder_key("C:\\work\\proj\\"), same_folder_key("C:/work/proj"));
        assert_eq!(same_folder_key("file:///work/proj"), same_folder_key("/work/proj"));
    }
}
