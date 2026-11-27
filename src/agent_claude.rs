//! Claude Code as an agent session's driver. It runs the `claude` program as Claude Code's own VS Code extension
//! does: `claude -p` with JSON lines in both directions (`--input-format stream-json --output-format stream-json`)
//! and every tool use put to Limoniter first (`--permission-prompt-tool stdio`). Here the lines are turned into
//! [`Event`]s and the session's [`Command`]s into lines; the process and its threads are at the end.
//!
//! What's known of the lines comes from Claude Code 2.1.332's own schemas and from running it; where that differs
//! from what the research notes guessed, it's said in the comments.

use crate::agent_session::{Answer, Command, Context, Event, Handle, Limit, Limits, Mode, ModelChoice, Permission, PermissionKind, Task, TaskState, Tokens, ToolKind};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ---------- Naming things as Claude Code does ----------

pub fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Ask => "default",
        Mode::AcceptEdits => "acceptEdits",
        Mode::Plan => "plan",
        Mode::Bypass => "bypassPermissions",
    }
}

/// A mode as Claude Code names it. One that Limoniter has no word for (`dontAsk`, `auto`) is taken as asking.
pub fn mode_from(name: &str) -> Mode {
    match name {
        "acceptEdits" => Mode::AcceptEdits,
        "plan" => Mode::Plan,
        "bypassPermissions" => Mode::Bypass,
        _ => Mode::Ask,
    }
}

/// What a new session is started with.
pub struct Options {
    /// The program to run, `claude`; and anything that goes before its own arguments (what a test's stand-in
    /// is run by, or `claude` itself when something else runs it, as `wsl.exe` would).
    pub program: String,
    pub prefix: Vec<String>,
    pub folder: PathBuf,
    pub mode: Mode,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// A conversation to take up again.
    pub resume: Option<String>,
    /// The port of Limoniter's IDE server, if the session may use it.
    pub ide_port: Option<u16>,
    /// Called when there's something new for the window to look at.
    pub wake: Arc<dyn Fn() + Send + Sync>,
}

/// The arguments the program is run with. Research said `--permission-prompt-tool stdio` and the two formats were
/// all it needed; the CLI also needs `-p`, since a stream of JSON in is only taken in print mode.
pub fn arguments(options: &Options) -> Vec<String> {
    let mut args: Vec<String> = options.prefix.clone();
    for part in ["-p", "--output-format", "stream-json", "--input-format", "stream-json", "--verbose", "--include-partial-messages", "--permission-prompt-tool", "stdio", "--permission-mode", mode_name(options.mode)] {
        args.push(part.to_string());
    }
    // The CLI only lets the mode be changed to Bypass later if it was started with this, and it refuses it
    // altogether as root, so it's given only to a session that starts in Bypass.
    if options.mode == Mode::Bypass {
        args.push("--allow-dangerously-skip-permissions".to_string());
    }
    if let Some(model) = options.model.as_deref().filter(|m| !m.is_empty() && *m != "default") {
        args.extend(["--model".to_string(), model.to_string()]);
    }
    if let Some(effort) = options.effort.as_deref().filter(|e| !e.is_empty()) {
        args.extend(["--effort".to_string(), effort.to_string()]);
    }
    if let Some(id) = options.resume.as_deref() {
        args.extend(["--resume".to_string(), id.to_string()]);
    }
    if options.ide_port.is_some() {
        args.push("--ide".to_string());
    }
    args
}

// ---------- What a tool use looks like in the transcript ----------

/// A path as the person knows it: relative to the project's folder when it's inside.
pub fn short_path(path: &Path, folder: &Path) -> String {
    path.strip_prefix(folder).map(|p| p.display().to_string()).ok().filter(|p| !p.is_empty()).unwrap_or_else(|| path.display().to_string())
}

fn first_line(text: &str, most: usize) -> String {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    if line.chars().count() > most { format!("{}\u{2026}", line.chars().take(most).collect::<String>()) } else { line.to_string() }
}

fn text_of<'a>(input: &'a Value, key: &str) -> &'a str {
    input.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The path a file tool works on.
fn file_of(input: &Value) -> Option<PathBuf> {
    ["file_path", "notebook_path", "path"].iter().find_map(|key| input.get(key).and_then(Value::as_str)).map(PathBuf::from)
}

/// One line for a tool use, and what kind of thing it is.
pub fn describe(name: &str, input: &Value, folder: &Path) -> (String, ToolKind) {
    let file = file_of(input);
    let shown = file.as_deref().map(|p| short_path(p, folder)).unwrap_or_default();
    match name {
        "Read" => (crate::i18n::tf!("Read {}", shown), ToolKind::Other),
        "Edit" | "MultiEdit" | "NotebookEdit" => (crate::i18n::tf!("Edit {}", shown), file.map_or(ToolKind::Other, ToolKind::FileChange)),
        "Write" => (crate::i18n::tf!("Write {}", shown), file.map_or(ToolKind::Other, ToolKind::FileChange)),
        "Bash" => {
            let command = text_of(input, "command");
            (crate::i18n::tf!("Ran {}", first_line(command, 80)), ToolKind::Command(command.to_string()))
        }
        "Grep" => ({ let shown = text_of(input, "pattern"); crate::i18n::tf!("Searched for {}", shown) }, ToolKind::Other),
        "Glob" => ({ let shown = text_of(input, "pattern"); crate::i18n::tf!("Listed files matching {}", shown) }, ToolKind::Other),
        "WebFetch" => ({ let shown = text_of(input, "url"); crate::i18n::tf!("Fetched {}", shown) }, ToolKind::Other),
        "WebSearch" => ({ let shown = text_of(input, "query"); crate::i18n::tf!("Searched the web for {}", shown) }, ToolKind::Other),
        "TodoWrite" => (crate::i18n::tr("Updated the to-do list").to_string(), ToolKind::Other),
        // Claude Code 2.1.332 calls the subagent tool "Agent"; it was "Task" before.
        "Agent" | "Task" => ({ let shown = text_of(input, "description"); crate::i18n::tf!("Subagent: {}", shown) }, ToolKind::Other),
        _ => {
            let first = input.as_object().and_then(|o| o.values().find_map(Value::as_str)).map(|s| first_line(s, 60)).unwrap_or_default();
            (if first.is_empty() { name.to_string() } else { format!("{name} {first}") }, ToolKind::Other)
        }
    }
}

// ---------- The file an edit would leave ----------

fn replace_in(text: &str, old: &str, new: &str, all: bool) -> Option<String> {
    if old.is_empty() {
        return text.is_empty().then(|| new.to_string());
    }
    let count = text.matches(old).count();
    if count == 0 || (count > 1 && !all) {
        return None;
    }
    Some(if all { text.replace(old, new) } else { text.replacen(old, new, 1) })
}

/// The same, trying the file's own line ends if it uses CRLF and the tool wrote LF.
fn replace_like_the_tool(text: &str, old: &str, new: &str, all: bool) -> Option<String> {
    replace_in(text, old, new, all).or_else(|| {
        if text.contains("\r\n") && !old.contains('\r') {
            let crlf = |s: &str| s.replace("\r\n", "\n").replace('\n', "\r\n");
            replace_in(text, &crlf(old), &crlf(new), all)
        } else {
            None
        }
    })
}

/// What a file would be after an edit tool's change, with the path. `current` is the file now (None if there's
/// none yet). None for a tool this doesn't know, or an edit that doesn't fit the file.
pub fn proposed_text(tool: &str, input: &Value, current: Option<&str>) -> Option<(PathBuf, String)> {
    let path = file_of(input)?;
    let now = current.unwrap_or("");
    match tool {
        "Write" => Some((path, text_of(input, "content").to_string())),
        "Edit" => {
            let all = input.get("replace_all").and_then(Value::as_bool).unwrap_or(false);
            let text = replace_like_the_tool(now, text_of(input, "old_string"), text_of(input, "new_string"), all)?;
            Some((path, text))
        }
        "MultiEdit" => {
            let mut text = now.to_string();
            for edit in input.get("edits")?.as_array()? {
                let all = edit.get("replace_all").and_then(Value::as_bool).unwrap_or(false);
                text = replace_like_the_tool(&text, text_of(edit, "old_string"), text_of(edit, "new_string"), all)?;
            }
            Some((path, text))
        }
        _ => None,
    }
}

/// The tool's input after the person has changed the proposed file in the review: the same tool, made to leave
/// the file as reviewed. `current` is the file as it is now.
pub fn reviewed_input(tool: &str, input: &Value, current: Option<&str>, reviewed: &str) -> Value {
    let mut updated = input.clone();
    let Some(object) = updated.as_object_mut() else { return updated };
    match tool {
        "Write" => {
            object.insert("content".into(), json!(reviewed));
        }
        "Edit" => {
            object.insert("old_string".into(), json!(current.unwrap_or("")));
            object.insert("new_string".into(), json!(reviewed));
            object.insert("replace_all".into(), json!(false));
        }
        "MultiEdit" => {
            object.insert("edits".into(), json!([{"old_string": current.unwrap_or(""), "new_string": reviewed}]));
        }
        _ => {}
    }
    updated
}

// ---------- The lines ----------

/// What was asked by a control request of ours, to know what its answer is.
enum Asked {
    Initialize,
    Context,
    Usage,
    Model(Option<String>),
    Effort(String),
    Mode(Mode),
    Other,
}

/// A permission request, kept until it's answered.
struct Pending {
    tool: String,
    input: Value,
    suggestions: Vec<Value>,
    /// The file as it was when the edit was proposed.
    current: Option<String>,
    proposed: Option<String>,
}

#[derive(Default)]
pub struct State {
    folder: PathBuf,
    asked: HashMap<String, Asked>,
    pending: HashMap<String, Pending>,
    next: u64,
    /// The message being streamed, and for each of its blocks (by index) the id it has in the transcript.
    message: String,
    blocks: HashMap<u64, String>,
    thinking: HashMap<String, String>,
    /// How many text and thinking blocks of each message have been seen streaming, and in the finished messages,
    /// to give the same block the same id both ways.
    streamed: HashMap<String, (u32, u32)>,
    finished: HashMap<String, (u32, u32)>,
    /// The turn is being stopped, so its ending is told so.
    interrupting: bool,
    /// Tokens added up from each turn's own count, for when the CLI doesn't give its running total.
    tokens: Tokens,
}

fn text_content(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// The models in an `initialize` or `list_models` answer.
pub fn models_from(response: &Value) -> Vec<ModelChoice> {
    let Some(models) = response.get("models").and_then(Value::as_array) else { return Vec::new() };
    models
        .iter()
        .filter_map(|m| {
            let value = m.get("value")?.as_str()?.to_string();
            let name = m.get("displayName").and_then(Value::as_str).unwrap_or(&value).to_string();
            let efforts = m.get("supportedEffortLevels").and_then(Value::as_array).map(|levels| levels.iter().filter_map(|l| l.as_str().map(str::to_string)).collect()).unwrap_or_default();
            Some(ModelChoice { value, name, efforts })
        })
        .collect()
}

/// A time as seconds since 1970, from the ISO 8601 `get_usage` gives.
fn epoch_of(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text).ok().map(|t| t.timestamp())
}

fn limits_from_usage(response: &Value) -> Limits {
    let one = |key: &str| {
        let window = response.get("rate_limits")?.get(key)?;
        let percent = window.get("utilization")?.as_f64()? as f32;
        Some(Limit { percent, resets: window.get("resets_at").and_then(Value::as_str).and_then(epoch_of) })
    };
    Limits { five_hour: one("five_hour"), week: one("seven_day") }
}

/// A `rate_limit_event`'s windows. They say use as a fraction (0.42 is 42%), where `get_usage` says it in percent.
fn limits_from_event(event: &Value) -> Limits {
    let Some(windows) = event.get("rate_limit_info").and_then(|i| i.get("unifiedWindows")) else { return Limits::default() };
    let one = |key: &str| {
        let window = windows.get(key)?;
        let fraction = window.get("utilization")?.as_f64()? as f32;
        Some(Limit { percent: fraction * 100.0, resets: window.get("resetsAt").and_then(Value::as_i64) })
    };
    Limits { five_hour: one("five_hour"), week: one("seven_day") }
}

impl State {
    pub fn new(folder: &Path) -> Self {
        State { folder: folder.to_path_buf(), ..State::default() }
    }

    fn request(&mut self, asked: Asked, body: Value) -> String {
        self.next += 1;
        let id = format!("lim-{}", self.next);
        self.asked.insert(id.clone(), asked);
        json!({"type": "control_request", "request_id": id, "request": body}).to_string()
    }

    /// The line that begins the conversation with the program.
    pub fn initialize(&mut self) -> String {
        self.request(Asked::Initialize, json!({"subtype": "initialize"}))
    }

    /// What the person asked, as lines to send.
    pub fn command(&mut self, command: Command) -> Vec<String> {
        match command {
            Command::Prompt(text) => {
                self.interrupting = false;
                vec![json!({"type": "user", "message": {"role": "user", "content": text}}).to_string()]
            }
            Command::Interrupt => {
                self.interrupting = true;
                vec![self.request(Asked::Other, json!({"subtype": "interrupt"}))]
            }
            Command::SetModel(model) => vec![self.request(Asked::Model(model.clone()), json!({"subtype": "set_model", "model": model}))],
            // Claude Code has no request for effort of its own; it's a setting, as `/effort` changes it.
            Command::SetEffort(effort) => vec![self.request(Asked::Effort(effort.clone()), json!({"subtype": "apply_flag_settings", "settings": {"effortLevel": effort}}))],
            Command::SetMode(mode) => vec![self.request(Asked::Mode(mode), json!({"subtype": "set_permission_mode", "mode": mode_name(mode)}))],
            Command::StopTask(id) => vec![self.request(Asked::Other, json!({"subtype": "stop_task", "task_id": id}))],
            Command::AskContext => vec![self.request(Asked::Context, json!({"subtype": "get_context_usage"}))],
            Command::AskUsage => vec![self.request(Asked::Usage, json!({"subtype": "get_usage"}))],
            Command::Answer { id, answer } => self.answer(&id, answer),
            Command::Close => Vec::new(),
        }
    }

    fn answer(&mut self, id: &str, answer: Answer) -> Vec<String> {
        let Some(pending) = self.pending.remove(id) else { return Vec::new() };
        let response = match answer {
            Answer::Allow { text, for_session } => {
                let input = match (&text, &pending.proposed) {
                    (Some(reviewed), Some(proposed)) if reviewed != proposed => reviewed_input(&pending.tool, &pending.input, pending.current.as_deref(), reviewed),
                    _ => pending.input.clone(),
                };
                let mut allow = json!({"behavior": "allow", "updatedInput": input});
                if for_session {
                    allow["updatedPermissions"] = Value::Array(session_permissions(&pending));
                }
                allow
            }
            Answer::Refuse(message) => json!({"behavior": "deny", "message": message}),
        };
        vec![json!({"type": "control_response", "response": {"subtype": "success", "request_id": id, "response": response}}).to_string()]
    }

    /// What one line from the program means: the events, and any lines that must be sent back at once.
    pub fn line(&mut self, line: &str) -> (Vec<Event>, Vec<String>) {
        let line = line.trim();
        if line.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            // Something that isn't a message: a warning the program printed, or a line cut short. Said, not dropped.
            return (vec![Event::Notice(crate::i18n::tf!("Claude Code said: {}", first_line(line, 200)))], Vec::new());
        };
        let kind = message.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "system" => (self.system(&message), Vec::new()),
            "stream_event" => (self.stream(&message), Vec::new()),
            "assistant" => (self.assistant(&message), Vec::new()),
            "user" => (self.user(&message), Vec::new()),
            "result" => (self.result(&message), Vec::new()),
            "rate_limit_event" => {
                let limits = limits_from_event(&message);
                (if limits.any() { vec![Event::Limits(limits)] } else { Vec::new() }, Vec::new())
            }
            "control_request" => self.control_request(&message),
            "control_response" => (self.control_response(&message), Vec::new()),
            "control_cancel_request" => {
                let id = message.get("request_id").and_then(Value::as_str).unwrap_or("").to_string();
                self.pending.remove(&id);
                (vec![Event::PermissionGone { id }], Vec::new())
            }
            _ => (Vec::new(), Vec::new()),
        }
    }

    fn system(&mut self, message: &Value) -> Vec<Event> {
        let subtype = message.get("subtype").and_then(Value::as_str).unwrap_or("");
        let text = |key: &str| message.get(key).and_then(Value::as_str).unwrap_or("").to_string();
        match subtype {
            "init" => vec![Event::Started { session_id: text("session_id"), model: text("model"), mode: mode_from(&text("permissionMode")) }],
            "status" => match message.get("permissionMode").and_then(Value::as_str) {
                Some(mode) => vec![Event::ModeChanged(mode_from(mode))],
                None => Vec::new(),
            },
            "session_title_changed" => vec![Event::Title(text("title"))],
            "api_retry" => vec![Event::Notice({ let (attempt, most) = (message.get("attempt").and_then(Value::as_i64).unwrap_or(0), message.get("max_retries").and_then(Value::as_i64).unwrap_or(0)); crate::i18n::tf!("Claude Code is trying again after a problem ({} of {}).", attempt, most) })],
            "compact_boundary" => vec![Event::Notice(crate::i18n::tr("The conversation was condensed to make room.").to_string())],
            "task_started" | "task_progress" => {
                let usage = message.get("usage");
                let number = |key: &str| usage.and_then(|u| u.get(key)).and_then(Value::as_u64).unwrap_or(0);
                vec![Event::Task(Task {
                    id: text("task_id"),
                    description: text("description"),
                    kind: message.get("subagent_type").and_then(Value::as_str).unwrap_or(message.get("task_type").and_then(Value::as_str).unwrap_or("")).to_string(),
                    last_tool: message.get("last_tool_name").and_then(Value::as_str).map(str::to_string),
                    tokens: number("total_tokens"),
                    tool_uses: number("tool_uses") as u32,
                    state: TaskState::Running,
                })]
            }
            "task_updated" => {
                let state = match message.get("patch").and_then(|p| p.get("status")).and_then(Value::as_str) {
                    Some("completed") => TaskState::Done,
                    Some("failed") => TaskState::Failed,
                    Some("killed") => TaskState::Stopped,
                    _ => return Vec::new(),
                };
                vec![Event::Task(Task { id: text("task_id"), state, ..Task::default() })]
            }
            "task_notification" => {
                let state = match message.get("status").and_then(Value::as_str) {
                    Some("completed") => TaskState::Done,
                    Some("failed") => TaskState::Failed,
                    _ => TaskState::Stopped,
                };
                vec![Event::Task(Task { id: text("task_id"), description: text("summary"), state, ..Task::default() })]
            }
            _ => Vec::new(),
        }
    }

    /// The id of a block of a message, counting text and thinking separately.
    fn block_id(counts: &mut HashMap<String, (u32, u32)>, message: &str, thinking: bool) -> String {
        let entry = counts.entry(message.to_string()).or_default();
        if thinking {
            entry.1 += 1;
            format!("{message}:k{}", entry.1)
        } else {
            entry.0 += 1;
            format!("{message}:t{}", entry.0)
        }
    }

    fn stream(&mut self, message: &Value) -> Vec<Event> {
        // What a subagent says isn't streamed into the transcript.
        if message.get("parent_tool_use_id").is_some_and(|p| !p.is_null()) {
            return Vec::new();
        }
        let Some(event) = message.get("event") else { return Vec::new() };
        match event.get("type").and_then(Value::as_str).unwrap_or("") {
            "message_start" => {
                self.message = event.get("message").and_then(|m| m.get("id")).and_then(Value::as_str).unwrap_or("").to_string();
                self.blocks.clear();
                Vec::new()
            }
            "content_block_start" => {
                let index = event.get("index").and_then(Value::as_u64).unwrap_or(0);
                match event.get("content_block").and_then(|b| b.get("type")).and_then(Value::as_str) {
                    Some("text") => {
                        let id = Self::block_id(&mut self.streamed, &self.message, false);
                        self.blocks.insert(index, id);
                        Vec::new()
                    }
                    Some("thinking") => {
                        let id = Self::block_id(&mut self.streamed, &self.message, true);
                        self.blocks.insert(index, id);
                        Vec::new()
                    }
                    _ => Vec::new(),
                }
            }
            "content_block_delta" => {
                let index = event.get("index").and_then(Value::as_u64).unwrap_or(0);
                let Some(id) = self.blocks.get(&index).cloned() else { return Vec::new() };
                let delta = event.get("delta");
                match delta.and_then(|d| d.get("type")).and_then(Value::as_str) {
                    Some("text_delta") => vec![Event::TextDelta { id, text: delta.and_then(|d| d.get("text")).and_then(Value::as_str).unwrap_or("").to_string() }],
                    Some("thinking_delta") => {
                        let more = delta.and_then(|d| d.get("thinking")).and_then(Value::as_str).unwrap_or("");
                        let whole = self.thinking.entry(id.clone()).or_default();
                        whole.push_str(more);
                        vec![Event::Thinking { id, text: whole.clone() }]
                    }
                    _ => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    }

    fn assistant(&mut self, message: &Value) -> Vec<Event> {
        let parent = message.get("parent_tool_use_id").and_then(Value::as_str).map(str::to_string);
        let body = message.get("message");
        let id = body.and_then(|m| m.get("id")).and_then(Value::as_str).unwrap_or("").to_string();
        let mut events = Vec::new();
        let Some(blocks) = body.and_then(|m| m.get("content")).and_then(Value::as_array) else { return events };
        for block in blocks {
            match block.get("type").and_then(Value::as_str).unwrap_or("") {
                "text" if parent.is_none() => {
                    let block_id = Self::block_id(&mut self.finished, &id, false);
                    events.push(Event::Text { id: block_id, text: text_of(block, "text").to_string(), done: true });
                }
                "thinking" if parent.is_none() => {
                    let block_id = Self::block_id(&mut self.finished, &id, true);
                    // The finished message can come with the thinking left out; what streamed stays.
                    let text = text_of(block, "thinking");
                    if !text.is_empty() {
                        events.push(Event::Thinking { id: block_id, text: text.to_string() });
                    }
                }
                "tool_use" => {
                    let tool_id = text_of(block, "id").to_string();
                    let name = text_of(block, "name");
                    let input = block.get("input").cloned().unwrap_or(Value::Null);
                    let (summary, kind) = describe(name, &input, &self.folder);
                    events.push(Event::ToolUse { id: tool_id, name: name.to_string(), summary, kind, parent: parent.clone() });
                }
                _ => {}
            }
        }
        events
    }

    fn user(&mut self, message: &Value) -> Vec<Event> {
        let Some(blocks) = message.get("message").and_then(|m| m.get("content")).and_then(Value::as_array) else { return Vec::new() };
        blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
            .map(|b| Event::ToolResult { id: text_of(b, "tool_use_id").to_string(), text: text_content(b.get("content").unwrap_or(&Value::Null)), is_error: b.get("is_error").and_then(Value::as_bool).unwrap_or(false) })
            .collect()
    }

    fn result(&mut self, message: &Value) -> Vec<Event> {
        let mut events = Vec::new();
        // `modelUsage` is the whole session so far; `usage` is this turn's only.
        let total = message.get("modelUsage").and_then(Value::as_object).filter(|models| !models.is_empty()).map(|models| {
            let mut tokens = Tokens::default();
            for usage in models.values() {
                let number = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
                tokens.input += number("inputTokens") + number("cacheReadInputTokens") + number("cacheCreationInputTokens");
                tokens.cached += number("cacheReadInputTokens");
                tokens.output += number("outputTokens");
            }
            tokens
        });
        let tokens = total.unwrap_or_else(|| {
            let usage = message.get("usage");
            let number = |key: &str| usage.and_then(|u| u.get(key)).and_then(Value::as_u64).unwrap_or(0);
            self.tokens.input += number("input_tokens") + number("cache_read_input_tokens") + number("cache_creation_input_tokens");
            self.tokens.cached += number("cache_read_input_tokens");
            self.tokens.output += number("output_tokens");
            self.tokens
        });
        events.push(Event::Usage { tokens, cost: message.get("total_cost_usd").and_then(Value::as_f64) });
        let subtype = message.get("subtype").and_then(Value::as_str).unwrap_or("");
        let failed = message.get("is_error").and_then(Value::as_bool).unwrap_or(false) || subtype.starts_with("error");
        if self.interrupting && failed {
            events.push(Event::Notice(crate::i18n::tr("Stopped.").to_string()));
        } else if failed {
            let why = message.get("result").and_then(Value::as_str).filter(|r| !r.is_empty()).map(str::to_string).or_else(|| message.get("errors").and_then(Value::as_array).map(|e| e.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"))).filter(|r| !r.is_empty()).unwrap_or_else(|| subtype.replace('_', " "));
            events.push(Event::Error(why));
        }
        self.interrupting = false;
        events.push(Event::TurnFinished);
        events
    }

    fn control_request(&mut self, message: &Value) -> (Vec<Event>, Vec<String>) {
        let id = text_of(message, "request_id").to_string();
        let request = message.get("request").cloned().unwrap_or(Value::Null);
        match text_of(&request, "subtype") {
            "can_use_tool" => self.permission(id, &request),
            // Anything else the program asks of its host (hooks, MCP calls): this host does none of it.
            other => {
                let error = format!("Limoniter doesn't handle {other}");
                (Vec::new(), vec![json!({"type": "control_response", "response": {"subtype": "error", "request_id": id, "error": error}}).to_string()])
            }
        }
    }

    fn permission(&mut self, id: String, request: &Value) -> (Vec<Event>, Vec<String>) {
        let tool = text_of(request, "tool_name").to_string();
        let input = request.get("input").cloned().unwrap_or(Value::Null);
        let suggestions = request.get("permission_suggestions").and_then(Value::as_array).cloned().unwrap_or_default();
        // Asking a question of the person needs a form this panel doesn't have yet: the agent is told to ask in words.
        if tool == "AskUserQuestion" {
            let reply = json!({"type": "control_response", "response": {"subtype": "success", "request_id": id, "response": {"behavior": "deny", "message": "This editor can't show multiple-choice questions. Ask the question in plain text instead and wait for the answer."}}}).to_string();
            return (vec![Event::Notice(crate::i18n::tr("Claude Code tried to ask a question with choices, which this panel can't show yet. It was told to ask in plain words.").to_string())], vec![reply]);
        }
        let (title, kind) = describe(&tool, &input, &self.folder);
        let title = match tool.as_str() {
            "Bash" => { let shown = first_line(text_of(&input, "command"), 80); crate::i18n::tf!("Run {}", shown) },
            "Read" => crate::i18n::tf!("Read {}", file_of(&input).map(|p| short_path(&p, &self.folder)).unwrap_or_default()),
            "ExitPlanMode" => crate::i18n::tr("Start on this plan").to_string(),
            "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => title,
            _ => { let shown = request.get("display_name").and_then(Value::as_str).unwrap_or(&tool); crate::i18n::tf!("Use {}", shown) },
        };
        let detail = match tool.as_str() {
            "Bash" => text_of(&input, "command").to_string(),
            "ExitPlanMode" => text_of(&input, "plan").to_string(),
            _ => request.get("description").and_then(Value::as_str).filter(|d| !d.is_empty() && !title.contains(d)).map(str::to_string).unwrap_or_default(),
        };
        let mut current = None;
        let mut proposed = None;
        let mut permission_kind = match &kind {
            ToolKind::Command(command) => PermissionKind::Command(command.clone()),
            _ => PermissionKind::Other,
        };
        if matches!(tool.as_str(), "Edit" | "MultiEdit" | "Write")
            && let Some(path) = file_of(&input)
        {
            // A file that isn't text can't be shown as a change; the person is asked in words instead.
            let on_disk = match std::fs::read(&path) {
                Ok(bytes) => String::from_utf8(bytes).ok().map(Some),
                Err(_) => Some(None),
            };
            if let Some(now) = on_disk
                && let Some((path, text)) = proposed_text(&tool, &input, now.as_deref())
            {
                current = now;
                proposed = Some(text.clone());
                permission_kind = PermissionKind::Edit { path, proposed: text };
            }
        }
        let can_remember = !suggestions.is_empty() || !matches!(tool.as_str(), "ExitPlanMode");
        self.pending.insert(id.clone(), Pending { tool: tool.clone(), input, suggestions, current, proposed });
        (vec![Event::Permission(Permission { id, tool, title, detail, kind: permission_kind, can_remember })], Vec::new())
    }

    fn control_response(&mut self, message: &Value) -> Vec<Event> {
        let response = message.get("response").cloned().unwrap_or(Value::Null);
        let id = text_of(&response, "request_id");
        let Some(asked) = self.asked.remove(id) else { return Vec::new() };
        if text_of(&response, "subtype") == "error" {
            let why = text_of(&response, "error");
            return match asked {
                Asked::Other => Vec::new(),
                _ => vec![Event::Error(why.to_string())],
            };
        }
        let body = response.get("response").cloned().unwrap_or(Value::Null);
        match asked {
            Asked::Initialize => vec![Event::Ready { models: models_from(&body), mode: mode_from(text_of(&body, "current_permission_mode")) }],
            Asked::Context => {
                let max = body.get("maxTokens").and_then(Value::as_u64).unwrap_or(0);
                match body.get("totalTokens").and_then(Value::as_u64) {
                    Some(used) if max > 0 => vec![Event::Context(Context { used, max })],
                    _ => Vec::new(),
                }
            }
            Asked::Usage => {
                // Only said when the CLI has them: a plan with limits, and a first answer from the service.
                let limits = limits_from_usage(&body);
                if limits.any() { vec![Event::Limits(limits)] } else { Vec::new() }
            }
            Asked::Model(model) => vec![Event::ModelChanged(body.get("model").and_then(Value::as_str).map(str::to_string).or(model).unwrap_or_else(|| "default".into()))],
            Asked::Effort(effort) => vec![Event::EffortChanged(effort)],
            Asked::Mode(mode) => vec![Event::ModeChanged(mode)],
            Asked::Other => Vec::new(),
        }
    }
}

/// What "Allow for this session" sends: the rules the program suggested, made to last only as long as the session
/// (its suggestions say `localSettings`, which would be kept in a file), or the whole tool allowed if it suggested none.
fn session_permissions(pending: &Pending) -> Vec<Value> {
    if pending.suggestions.is_empty() {
        return vec![json!({"type": "addRules", "rules": [{"toolName": pending.tool}], "behavior": "allow", "destination": "session"})];
    }
    pending
        .suggestions
        .iter()
        .map(|suggestion| {
            let mut suggestion = suggestion.clone();
            if let Some(object) = suggestion.as_object_mut() {
                object.insert("destination".into(), json!("session"));
            }
            suggestion
        })
        .collect()
}

// ---------- The process ----------

/// The process, and what to say about how it ended.
struct Running {
    child: Mutex<Child>,
    stderr: Arc<Mutex<String>>,
}

/// Starts `claude` and gives the session its two channels.
pub fn start(options: Options) -> Result<Handle, String> {
    let mut command = crate::platform::command(&options.program);
    command.args(arguments(&options)).current_dir(&options.folder).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(port) = options.ide_port {
        command.env("CLAUDE_CODE_SSE_PORT", port.to_string()).env("CLAUDE_CODE_AUTO_CONNECT_IDE", "true");
    }
    let mut child = command.spawn().map_err(|e| crate::i18n::tf!("Couldn't start {}: {}", options.program, e))?;
    let stdin = child.stdin.take().ok_or("no input")?;
    let stdout = child.stdout.take().ok_or("no output")?;
    let stderr_pipe = child.stderr.take().ok_or("no error output")?;
    let stderr = Arc::new(Mutex::new(String::new()));
    let kept = stderr.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr_pipe).lines().map_while(Result::ok) {
            if let Ok(mut text) = kept.lock() {
                text.push_str(&line);
                text.push('\n');
                let excess = text.len().saturating_sub(2000);
                if excess > 0 {
                    let cut = (excess..=text.len()).find(|&i| text.is_char_boundary(i)).unwrap_or(text.len());
                    text.drain(..cut);
                }
            }
        }
    });
    let running = Arc::new(Running { child: Mutex::new(child), stderr });
    let state = Arc::new(Mutex::new(State::new(&options.folder)));
    let stdin = Arc::new(Mutex::new(Some(stdin)));
    let (event_tx, events) = channel::<Event>();
    let (commands, command_rx) = channel::<Command>();

    let wake = options.wake.clone();
    let send_lines = {
        let stdin = stdin.clone();
        move |lines: Vec<String>| -> bool {
            let Ok(mut stdin) = stdin.lock() else { return false };
            let Some(stdin) = stdin.as_mut() else { return false };
            lines.iter().all(|line| writeln!(stdin, "{line}").and_then(|()| stdin.flush()).is_ok())
        }
    };
    let first = state.lock().map(|mut s| s.initialize()).unwrap_or_default();
    send_lines(vec![first]);

    // Reading: what the program says.
    {
        let (state, tx, running, wake, send_lines) = (state.clone(), event_tx.clone(), running.clone(), wake.clone(), send_lines.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let (events, replies) = state.lock().map(|mut s| s.line(&line)).unwrap_or_default();
                if !replies.is_empty() {
                    send_lines(replies);
                }
                if !events.is_empty() {
                    for event in events {
                        let _ = tx.send(event);
                    }
                    wake();
                }
            }
            // The output has ended: the program is going, or has gone.
            let status = loop {
                match running.child.lock().map(|mut c| c.try_wait()) {
                    Ok(Ok(Some(status))) => break Some(status),
                    Ok(Ok(None)) => std::thread::sleep(Duration::from_millis(50)),
                    _ => break None,
                }
            };
            let said = running.stderr.lock().map(|s| s.trim().to_string()).unwrap_or_default();
            let why = match status {
                Some(status) if status.success() && said.is_empty() => crate::i18n::tr("Claude Code has finished.").to_string(),
                Some(status) => crate::i18n::tf!("Claude Code stopped ({}). {}", status, said),
                None => crate::i18n::tr("Claude Code has gone.").to_string(),
            };
            let _ = tx.send(Event::Finished(why.trim().to_string()));
            wake();
        });
    }

    // Writing: what the person tells it.
    {
        let (state, running, stdin) = (state.clone(), running.clone(), stdin.clone());
        std::thread::spawn(move || {
            while let Ok(command) = command_rx.recv() {
                if command == Command::Close {
                    break;
                }
                let lines = state.lock().map(|mut s| s.command(command)).unwrap_or_default();
                if !lines.is_empty() && !send_lines(lines) {
                    break;
                }
            }
            // Closing its input ends it, after it has finished what it's doing; it isn't waited for long.
            if let Ok(mut stdin) = stdin.lock() {
                stdin.take();
            }
            std::thread::sleep(Duration::from_millis(1500));
            if let Ok(mut child) = running.child.lock() {
                let _ = child.kill();
            }
        });
    }
    Ok(Handle { commands, events })
}

/// Asks a `claude` which models it has, without starting a conversation (nothing is sent to the service).
pub fn probe_models(program: &str, prefix: &[String], folder: &Path) -> Result<Vec<ModelChoice>, String> {
    let mut command = crate::platform::command(program);
    command.args(prefix).args(["-p", "--output-format", "stream-json", "--input-format", "stream-json", "--verbose"]).current_dir(folder).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().ok_or("no input")?;
    let stdout = child.stdout.take().ok_or("no output")?;
    let mut state = State::new(folder);
    let first = state.initialize();
    writeln!(stdin, "{first}").map_err(|e| e.to_string())?;
    let (tx, rx): (Sender<Vec<ModelChoice>>, _) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let (events, _) = state.line(&line);
            for event in events {
                if let Event::Ready { models, .. } = event {
                    let _ = tx.send(models);
                    return;
                }
            }
        }
    });
    let models = rx.recv_timeout(Duration::from_secs(20)).map_err(|_| crate::i18n::tr("Claude Code didn't answer.").to_string());
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    models
}

#[cfg(test)]
#[path = "agent_claude_tests.rs"]
pub(crate) mod agent_claude_tests;
