//! Codex as an agent session's driver. It runs `codex app-server`, which is how Codex's own VS Code extension
//! drives it: JSON-RPC 2.0 over standard input and output, one message to a line (the `"jsonrpc"` field left
//! out). Here the lines are turned into [`Event`]s and the session's [`Command`]s into lines; the process and its
//! threads are at the end.
//!
//! What's known of the messages comes from the schema the installed Codex prints (`codex app-server
//! generate-json-schema`) and from running it (codex-cli 0.172.0); where that differs from what the research
//! notes said, it's said in the comments.

use crate::agent_session::{Answer, Command, Context, Event, Handle, Limit, Limits, Mode, ModelChoice, Permission, PermissionKind, Task, TaskState, Tokens, ToolKind};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ---------- Naming things as Codex does ----------

/// What Codex is told for a mode: when it asks (`approvalPolicy`), how far it can reach (`sandbox`, as
/// `thread/start` writes it) and the same as `turn/start` writes it.
///
/// Codex has three levels, which are Limoniter's four modes less one: Plan is "read only", Accept edits is the
/// workspace one Codex itself starts in, Bypass is full access, and Ask is the workspace one with `untrusted`,
/// which asks before everything that isn't known to be harmless (including every edit).
pub fn policy(mode: Mode) -> (&'static str, &'static str, Value) {
    match mode {
        Mode::Plan => ("on-request", "read-only", json!({"type": "readOnly"})),
        Mode::Ask => ("untrusted", "workspace-write", json!({"type": "workspaceWrite"})),
        Mode::AcceptEdits => ("on-request", "workspace-write", json!({"type": "workspaceWrite"})),
        Mode::Bypass => ("never", "danger-full-access", json!({"type": "dangerFullAccess"})),
    }
}

/// The mode that an approval policy and a sandbox (as a name, or as an object with a `type`) make.
pub fn mode_from(approval: &Value, sandbox: &Value) -> Option<Mode> {
    let sandbox = sandbox.as_str().or_else(|| sandbox.get("type").and_then(Value::as_str))?;
    let approval = approval.as_str().unwrap_or("");
    match sandbox {
        "read-only" | "readOnly" => Some(Mode::Plan),
        "danger-full-access" | "dangerFullAccess" => Some(Mode::Bypass),
        "workspace-write" | "workspaceWrite" => Some(if approval == "untrusted" { Mode::Ask } else { Mode::AcceptEdits }),
        _ => None,
    }
}

/// Where Limoniter offers Codex its tools: a streamable HTTP MCP server on this computer.
#[derive(Clone, Debug, PartialEq)]
pub struct Mcp {
    pub url: String,
    pub token: String,
}

/// What a new session is started with.
pub struct Options {
    /// The program to run, `codex`; and anything that goes before its own arguments (what a test's stand-in is
    /// run by).
    pub program: String,
    pub prefix: Vec<String>,
    pub folder: PathBuf,
    pub mode: Mode,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// A thread to take up again.
    pub resume: Option<String>,
    /// Limoniter's tools for Codex, if it may use them.
    pub mcp: Option<Mcp>,
    /// Set when the program runs in WSL: how the paths in what's said each way are translated.
    pub wsl: Option<crate::agent_wsl::Paths>,
    /// Called when there's something new for the window to look at.
    pub wake: Arc<dyn Fn() + Send + Sync>,
}

pub fn arguments(options: &Options) -> Vec<String> {
    let mut args = options.prefix.clone();
    args.push("app-server".to_string());
    args
}

/// The settings given to one thread in place of what's in the person's `config.toml`, which isn't touched: the
/// MCP server Limoniter offers. `thread/start` takes them as `config`, found by running it.
pub fn thread_config(mcp: Option<&Mcp>) -> Option<Value> {
    let mcp = mcp?;
    Some(json!({"mcp_servers": {"limoniter": {"url": mcp.url, "http_headers": {"Authorization": format!("Bearer {}", mcp.token)}, "startup_timeout_sec": 10, "tool_timeout_sec": 600}}}))
}

// ---------- What the editor tells Codex it's looking at ----------

static IDE_CONTEXT: Mutex<String> = Mutex::new(String::new());

/// The window keeps this up to date (see `ide_context_text`); each turn is sent with whatever it is then.
pub fn set_ide_context(text: String) {
    if let Ok(mut now) = IDE_CONTEXT.lock() {
        *now = text;
    }
}

fn ide_context() -> String {
    IDE_CONTEXT.lock().map(|text| text.clone()).unwrap_or_default()
}

/// The most of a selection that's sent.
const MOST_SELECTED: usize = 200_000;
/// The most open tabs named.
const MOST_TABS: usize = 5;

/// The text Codex's own VS Code extension sends with each turn to say what's open, as that extension writes it.
/// `file` is the file in front, `selection` what's selected in it, `tabs` the other open files (label, path).
pub fn ide_context_text(folder: &Path, file: Option<&Path>, selection: &str, tabs: &[(String, PathBuf)]) -> String {
    let shown = |path: &Path| crate::agent_claude::short_path(path, folder);
    let mut text = String::from("# Context from my IDE setup:\n");
    if let Some(file) = file {
        text.push_str(&format!("\n## Active file: {}\n", shown(file)));
        if !selection.is_empty() {
            let cut: String = selection.chars().take(MOST_SELECTED).collect();
            text.push_str(&format!("\n## Active selection of the file:\n{cut}\n"));
        }
    }
    if !tabs.is_empty() {
        text.push_str("\n## Open tabs:\n");
        for (label, path) in tabs.iter().take(MOST_TABS) {
            text.push_str(&format!("- {label}: {}\n", shown(path)));
        }
    }
    if file.is_none() && tabs.is_empty() { String::new() } else { text }
}

// ---------- What a command or change looks like in the transcript ----------

fn first_line(text: &str, most: usize) -> String {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    if line.chars().count() > most { format!("{}\u{2026}", line.chars().take(most).collect::<String>()) } else { line.to_string() }
}

fn text_of<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

/// A command as the person typed it, without the shell Codex wraps it in: `/bin/bash -lc 'ls -l'` is `ls -l`.
pub fn unwrap_shell(command: &str) -> String {
    let trimmed = command.trim();
    for marker in [" -lc ", " -c ", " /c ", " -Command "] {
        if let Some(at) = trimmed.find(marker) {
            let program = trimmed[..at].rsplit(['/', '\\']).next().unwrap_or("").to_lowercase();
            if !["bash", "sh", "zsh", "cmd", "cmd.exe", "powershell", "powershell.exe", "pwsh", "pwsh.exe"].contains(&program.as_str()) {
                break;
            }
            let rest = trimmed[at + marker.len()..].trim();
            if let Some(inner) = rest.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')) {
                return inner.replace("'\\''", "'");
            }
            if let Some(inner) = rest.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
                return inner.replace("\\\"", "\"");
            }
            return rest.to_string();
        }
    }
    trimmed.to_string()
}

// ---------- The file a patch would leave ----------

/// What Codex's patch does to one file.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// A new file, with all its text.
    Add(String),
    /// Hunks of a unified diff.
    Update(String),
    Delete,
    /// Moved to another name (with hunks, maybe).
    Move(String, String),
}

/// One file of a patch, as the `fileChange` item gives it.
#[derive(Clone, Debug, PartialEq)]
pub struct Touched {
    pub path: PathBuf,
    pub change: Change,
}

fn touched_from(changes: &Value) -> Vec<Touched> {
    let Some(changes) = changes.as_array() else { return Vec::new() };
    changes
        .iter()
        .filter_map(|c| {
            let path = PathBuf::from(c.get("path")?.as_str()?);
            let diff = text_of(c, "diff").to_string();
            let kind = c.get("kind")?;
            let change = match text_of(kind, "type") {
                "add" => Change::Add(diff),
                "delete" => Change::Delete,
                _ => match kind.get("move_path").and_then(Value::as_str) {
                    Some(to) => Change::Move(to.to_string(), diff),
                    None => Change::Update(diff),
                },
            };
            Some(Touched { path, change })
        })
        .collect()
}

/// A text as lines that keep their own endings.
fn lines_with_endings(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

fn without_ending(line: &str) -> &str {
    line.strip_suffix('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).unwrap_or(line)
}

/// What the file `now` would be after the hunks of a unified diff (without the file headers, as Codex gives
/// them) are applied. Each hunk is looked for where it says, or failing that, the nearest place its old lines
/// are found. None if a hunk doesn't fit.
pub fn apply_diff(now: &str, diff: &str) -> Option<String> {
    let old = lines_with_endings(now);
    let ending = if now.contains("\r\n") { "\r\n" } else { "\n" };
    let mut result: Vec<String> = Vec::new();
    // How many of the old lines are already dealt with.
    let mut done = 0usize;
    let mut hunks: Vec<(usize, Vec<(char, String)>)> = Vec::new();
    for line in diff.split_inclusive('\n') {
        let body = without_ending(line);
        if let Some(header) = body.strip_prefix("@@ -") {
            let start = header.split([',', ' ']).next()?.parse::<usize>().ok()?;
            hunks.push((start, Vec::new()));
        } else if let Some((_, lines)) = hunks.last_mut() {
            let mut chars = body.chars();
            match chars.next() {
                Some(c @ (' ' | '-' | '+')) => lines.push((c, chars.as_str().to_string())),
                // "No newline at end of file", said of the line before.
                Some('\\') => lines.push(('\\', String::new())),
                None => lines.push((' ', String::new())),
                _ => {}
            }
        }
    }
    if hunks.is_empty() {
        return None;
    }
    for (start, lines) in hunks {
        let expected: Vec<&str> = lines.iter().filter(|(c, _)| *c == ' ' || *c == '-').map(|(_, l)| l.as_str()).collect();
        let fits = |at: usize| at >= done && at + expected.len() <= old.len() && expected.iter().enumerate().all(|(n, l)| without_ending(old[at + n]) == *l);
        let wanted = start.saturating_sub(1).max(done);
        let at = if fits(wanted) { wanted } else { (done..=old.len().saturating_sub(expected.len())).filter(|&at| fits(at)).min_by_key(|&at| at.abs_diff(wanted))? };
        for line in &old[done..at] {
            result.push((*line).to_string());
        }
        let mut used = at;
        let mut before = ' ';
        for (c, text) in &lines {
            match c {
                ' ' => {
                    result.push(old[used].to_string());
                    used += 1;
                }
                '-' => used += 1,
                '+' => result.push(format!("{text}{ending}")),
                _ => {
                    // The line just written has no ending (if it was the old file's, it has none already).
                    if before != '-'
                        && let Some(last) = result.last_mut()
                    {
                        *last = without_ending(last).to_string();
                    }
                }
            }
            before = *c;
        }
        done = used;
    }
    for line in &old[done..] {
        result.push((*line).to_string());
    }
    Some(result.concat())
}

// ---------- The lines ----------

/// What was asked by a request of ours, to know what its answer is.
#[derive(Clone, Debug, PartialEq)]
enum Asked {
    Initialize,
    Models,
    Start,
    Turn,
    Limits,
    Other,
}

/// One file of a change that's waiting to be allowed.
struct Part {
    id: String,
    path: PathBuf,
    /// The text the file is to have, if it was shown for review.
    proposed: Option<String>,
    answer: Option<Answer>,
}

/// A request from Codex that's waiting for the person.
enum Approval {
    Command { rpc: Value },
    Files { rpc: Value, item: String, parts: Vec<Part> },
    Permissions { rpc: Value, requested: Value },
    /// An MCP tool asking to be run.
    Tool { rpc: Value },
}

pub struct State {
    folder: PathBuf,
    /// Asking the models only: no thread is started.
    probe: bool,
    next: i64,
    asked: HashMap<i64, Asked>,
    thread: Option<String>,
    turn: Option<String>,
    mode: Mode,
    model: Option<String>,
    effort: Option<String>,
    default_model: Option<String>,
    resume: Option<String>,
    config: Option<Value>,
    /// What was said before the thread was ready.
    waiting: Option<String>,
    /// The last prompt sent, kept in case the context has to be sent another way.
    last_prompt: Option<String>,
    context_in_text: bool,
    /// Requests waiting for the person, by our key for them.
    approvals: HashMap<String, Approval>,
    /// Our id for each part of a change, to the key of its request.
    part_keys: HashMap<String, String>,
    /// The files each `fileChange` item touches, from when it began.
    changes: HashMap<String, Vec<Touched>>,
    /// What the person made of a file in the review, to be written over Codex's change once it has made it.
    rewrites: HashMap<String, Vec<(PathBuf, String)>>,
    /// What Codex is to be told with the next prompt.
    notes: Vec<String>,
    /// The summary parts of the reasoning being streamed, by item.
    thinking: HashMap<String, Vec<String>>,
    tasks: HashMap<String, Task>,
    /// The turn running on each thread, to stop a helper's.
    turns: HashMap<String, String>,
    context: Option<Context>,
    heard_limits: bool,
}

/// The models in a `model/list` answer. The first is "default", which leaves the choice to Codex (and means the
/// model it says is its default).
pub fn models_from(response: &Value) -> Vec<ModelChoice> {
    let Some(models) = response.get("data").and_then(Value::as_array) else { return Vec::new() };
    let efforts_of = |m: &Value| -> Vec<String> { m.get("supportedReasoningEfforts").and_then(Value::as_array).map(|e| e.iter().filter_map(|e| e.get("reasoningEffort").and_then(Value::as_str).map(str::to_string)).collect()).unwrap_or_default() };
    let shown: Vec<&Value> = models.iter().filter(|m| !m.get("hidden").and_then(Value::as_bool).unwrap_or(false)).collect();
    let mut list = Vec::new();
    if let Some(default) = shown.iter().find(|m| m.get("isDefault").and_then(Value::as_bool).unwrap_or(false)).or(shown.first()) {
        let name = text_of(default, "displayName");
        list.push(ModelChoice { value: "default".into(), name: crate::i18n::tf!("Default ({})", name), efforts: efforts_of(default) });
    }
    for m in shown {
        let value = text_of(m, "model").to_string();
        if value.is_empty() {
            continue;
        }
        let name = m.get("displayName").and_then(Value::as_str).unwrap_or(&value).to_string();
        list.push(ModelChoice { value, name, efforts: efforts_of(m) });
    }
    list
}

fn default_model_of(response: &Value) -> Option<String> {
    let models = response.get("data")?.as_array()?;
    models.iter().find(|m| m.get("isDefault").and_then(Value::as_bool).unwrap_or(false)).and_then(|m| m.get("model")).and_then(Value::as_str).map(str::to_string)
}

/// A rate limit window as Limoniter has them: the five-hour one is the one of a day or less, the week's the longer.
pub fn limits_from(snapshot: &Value) -> Limits {
    let mut limits = Limits::default();
    for key in ["primary", "secondary"] {
        let Some(window) = snapshot.get(key).filter(|w| w.is_object()) else { continue };
        let Some(percent) = window.get("usedPercent").and_then(Value::as_f64) else { continue };
        let minutes = window.get("windowDurationMins").and_then(Value::as_i64).unwrap_or(if key == "primary" { 300 } else { 10_080 });
        let limit = Limit { percent: percent as f32, resets: window.get("resetsAt").and_then(Value::as_i64) };
        if minutes <= 24 * 60 { limits.five_hour = Some(limit) } else { limits.week = Some(limit) }
    }
    limits
}

fn task_state(status: &str) -> TaskState {
    match status {
        "completed" => TaskState::Done,
        "errored" | "notFound" => TaskState::Failed,
        "interrupted" | "shutdown" => TaskState::Stopped,
        _ => TaskState::Running,
    }
}

/// The app server's methods Limoniter calls, by their names in its JSON-RPC.
mod method {
    pub const INITIALIZE: &str = "initialize";
    pub const THREAD_RESUME: &str = "thread/resume";
    pub const THREAD_START: &str = "thread/start";
    pub const TURN_START: &str = "turn/start";
    pub const TURN_INTERRUPT: &str = "turn/interrupt";
    pub const READ_LIMITS: &str = "account/rateLimits/read";
    pub const MODEL_LIST: &str = "model/list";
}

impl State {
    pub fn new(options: &Options) -> Self {
        State {
            folder: options.folder.clone(),
            probe: false,
            next: 0,
            asked: HashMap::new(),
            thread: None,
            turn: None,
            mode: options.mode,
            model: options.model.clone().filter(|m| !m.is_empty() && m != "default"),
            effort: options.effort.clone().filter(|e| !e.is_empty()),
            default_model: None,
            resume: options.resume.clone(),
            config: thread_config(options.mcp.as_ref()),
            waiting: None,
            last_prompt: None,
            context_in_text: false,
            approvals: HashMap::new(),
            part_keys: HashMap::new(),
            changes: HashMap::new(),
            rewrites: HashMap::new(),
            notes: Vec::new(),
            thinking: HashMap::new(),
            tasks: HashMap::new(),
            turns: HashMap::new(),
            context: None,
            heard_limits: false,
        }
    }

    fn request(&mut self, asked: Asked, method: &str, params: Value) -> String {
        self.next += 1;
        self.asked.insert(self.next, asked);
        json!({"method": method, "id": self.next, "params": params}).to_string()
    }

    /// The line that begins the conversation with the program. `experimentalApi` is asked for because the editor's
    /// context is sent in a field that needs it.
    pub fn initialize(&mut self) -> String {
        let params = json!({"clientInfo": {"name": "limoniter", "title": "Limoniter", "version": env!("CARGO_PKG_VERSION")}, "capabilities": {"experimentalApi": true}});
        self.request(Asked::Initialize, method::INITIALIZE, params)
    }

    fn thread_params(&self) -> Value {
        let (approval, sandbox, _) = policy(self.mode);
        let mut params = json!({"cwd": self.folder.display().to_string(), "approvalPolicy": approval, "sandbox": sandbox, "serviceName": "limoniter"});
        if let Some(model) = &self.model {
            params["model"] = json!(model);
        }
        if let Some(config) = &self.config {
            params["config"] = config.clone();
        }
        params
    }

    fn start_thread(&mut self) -> String {
        let mut params = self.thread_params();
        match self.resume.clone() {
            Some(id) => {
                params["threadId"] = json!(id);
                params["excludeTurns"] = json!(true);
                self.request(Asked::Start, method::THREAD_RESUME, params)
            }
            None => self.request(Asked::Start, method::THREAD_START, params),
        }
    }

    fn turn_start(&mut self, text: &str) -> String {
        let Some(thread) = self.thread.clone() else { return String::new() };
        let (approval, _, sandbox) = policy(self.mode);
        let mut input: Vec<Value> = std::mem::take(&mut self.notes).into_iter().map(|note| json!({"type": "text", "text": note})).collect();
        let context = ide_context();
        if !context.is_empty() && self.context_in_text {
            input.push(json!({"type": "text", "text": context}));
        }
        input.push(json!({"type": "text", "text": text}));
        let mut params = json!({"threadId": thread, "input": input, "approvalPolicy": approval, "sandboxPolicy": sandbox, "summary": "concise"});
        if let Some(model) = &self.model {
            params["model"] = json!(model);
        }
        if let Some(effort) = &self.effort {
            params["effort"] = json!(effort);
        }
        if !context.is_empty() && !self.context_in_text {
            params["additionalContext"] = json!({"ide_context": {"kind": "untrusted", "name": "ide_context", "value": context}});
        }
        self.last_prompt = Some(text.to_string());
        self.request(Asked::Turn, method::TURN_START, params)
    }

    /// What the person asked, as lines to send.
    pub fn command(&mut self, command: Command) -> (Vec<Event>, Vec<String>) {
        let mut events = Vec::new();
        let mut lines = Vec::new();
        match command {
            Command::Prompt(text) => {
                if self.thread.is_some() {
                    lines.push(self.turn_start(&text));
                } else {
                    self.waiting = Some(text);
                }
            }
            Command::Interrupt => {
                let (answered, gone) = self.decline_all();
                lines.extend(answered);
                events.extend(gone);
                if let (Some(thread), Some(turn)) = (self.thread.clone(), self.turn.clone()) {
                    lines.push(self.request(Asked::Other, method::TURN_INTERRUPT, json!({"threadId": thread, "turnId": turn})));
                }
            }
            Command::SetModel(model) => {
                self.model = model.clone().filter(|m| m != "default");
                // Going back to Codex's own choice has to name it, since the thread keeps the last one it was given.
                if self.model.is_none() {
                    self.model.clone_from(&self.default_model);
                }
                events.push(Event::ModelChanged(self.model.clone().unwrap_or_else(|| "default".into())));
            }
            Command::SetEffort(effort) => {
                self.effort = Some(effort.clone());
                events.push(Event::EffortChanged(effort));
            }
            Command::SetMode(mode) => {
                self.mode = mode;
                events.push(Event::ModeChanged(mode));
            }
            Command::StopTask(id) => {
                if let Some(turn) = self.turns.get(&id).cloned() {
                    lines.push(self.request(Asked::Other, method::TURN_INTERRUPT, json!({"threadId": id, "turnId": turn})));
                }
            }
            Command::AskContext => events.extend(self.context.map(Event::Context)),
            Command::AskUsage => {
                if !self.heard_limits {
                    lines.push(self.request(Asked::Limits, method::READ_LIMITS, Value::Null));
                }
            }
            Command::Answer { id, answer } => lines.extend(self.answer(&id, answer)),
            Command::Close => {}
        }
        (events, lines)
    }

    /// Refuses every request that's waiting, because the turn is being stopped (which is what Codex's own extension
    /// does too).
    fn decline_all(&mut self) -> (Vec<String>, Vec<Event>) {
        let mut lines = Vec::new();
        let mut gone = Vec::new();
        for (key, approval) in std::mem::take(&mut self.approvals) {
            match approval {
                Approval::Command { rpc } | Approval::Files { rpc, .. } => lines.push(json!({"id": rpc, "result": {"decision": "cancel"}}).to_string()),
                Approval::Permissions { rpc, .. } => lines.push(json!({"id": rpc, "result": {"permissions": {}}}).to_string()),
                Approval::Tool { rpc } => lines.push(json!({"id": rpc, "result": {"action": "cancel", "content": null}}).to_string()),
            }
            gone.push(Event::PermissionGone { id: key });
        }
        for (part, _) in std::mem::take(&mut self.part_keys) {
            gone.push(Event::PermissionGone { id: part });
        }
        (lines, gone)
    }

    fn answer(&mut self, id: &str, answer: Answer) -> Vec<String> {
        let key = self.part_keys.remove(id).unwrap_or_else(|| id.to_string());
        let Some(approval) = self.approvals.remove(&key) else { return Vec::new() };
        match approval {
            Approval::Command { rpc } => {
                let decision = match answer {
                    Answer::Allow { for_session: true, .. } => "acceptForSession",
                    Answer::Allow { .. } => "accept",
                    Answer::Refuse(_) => "decline",
                };
                vec![json!({"id": rpc, "result": {"decision": decision}}).to_string()]
            }
            Approval::Tool { rpc } => {
                let result = match answer {
                    Answer::Allow { for_session: true, .. } => json!({"action": "accept", "content": null, "_meta": {"persist": "session"}}),
                    Answer::Allow { .. } => json!({"action": "accept", "content": null}),
                    Answer::Refuse(_) => json!({"action": "decline", "content": null}),
                };
                vec![json!({"id": rpc, "result": result}).to_string()]
            }
            Approval::Permissions { rpc, requested } => {
                let result = match answer {
                    Answer::Allow { for_session, .. } => json!({"permissions": requested, "scope": if for_session { "session" } else { "turn" }}),
                    Answer::Refuse(_) => json!({"permissions": {}}),
                };
                vec![json!({"id": rpc, "result": result}).to_string()]
            }
            Approval::Files { rpc, item, mut parts } => {
                if let Some(part) = parts.iter_mut().find(|p| p.id == id) {
                    part.answer = Some(answer);
                }
                if parts.iter().any(|p| p.answer.is_none()) {
                    self.approvals.insert(key, Approval::Files { rpc, item, parts });
                    return Vec::new();
                }
                // Codex is asked once for all of a patch's files, so it's allowed only if every file was.
                let allowed = parts.iter().all(|p| matches!(p.answer, Some(Answer::Allow { .. })));
                let for_session = parts.iter().any(|p| matches!(p.answer, Some(Answer::Allow { for_session: true, .. })));
                if allowed {
                    let rewrites: Vec<(PathBuf, String)> = parts
                        .iter()
                        .filter_map(|p| match (&p.answer, &p.proposed) {
                            (Some(Answer::Allow { text: Some(text), .. }), Some(proposed)) if text != proposed => Some((p.path.clone(), text.clone())),
                            _ => None,
                        })
                        .collect();
                    if !rewrites.is_empty() {
                        self.rewrites.insert(item, rewrites);
                    }
                }
                let decision = if !allowed { "decline" } else if for_session { "acceptForSession" } else { "accept" };
                vec![json!({"id": rpc, "result": {"decision": decision}}).to_string()]
            }
        }
    }

    /// What one line from the program means: the events, and any lines that must be sent back at once.
    pub fn line(&mut self, line: &str) -> (Vec<Event>, Vec<String>) {
        let line = line.trim();
        if line.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            // Something that isn't a message: a warning the program printed, or a line cut short. Said, not dropped.
            return (vec![Event::Notice(crate::i18n::tf!("Codex said: {}", first_line(line, 200)))], Vec::new());
        };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        match (message.get("method").and_then(Value::as_str), message.get("id")) {
            (Some(method), Some(id)) => self.server_request(method, id.clone(), &params),
            (Some(method), None) => (self.notification(method, &params), Vec::new()),
            (None, Some(id)) => self.response(id.as_i64().unwrap_or(-1), &message),
            (None, None) => (Vec::new(), Vec::new()),
        }
    }

    fn response(&mut self, id: i64, message: &Value) -> (Vec<Event>, Vec<String>) {
        let Some(asked) = self.asked.remove(&id) else { return (Vec::new(), Vec::new()) };
        let result = message.get("result").cloned().unwrap_or(Value::Null);
        let failed = message.get("error").map(|e| text_of(e, "message").to_string());
        match asked {
            Asked::Initialize => {
                if let Some(why) = failed {
                    return (vec![Event::Error(why)], Vec::new());
                }
                let mut lines = vec![json!({"method": "initialized", "params": {}}).to_string(), self.request(Asked::Models, method::MODEL_LIST, json!({}))];
                if !self.probe {
                    lines.push(self.start_thread());
                    lines.push(self.request(Asked::Limits, method::READ_LIMITS, Value::Null));
                }
                (Vec::new(), lines)
            }
            Asked::Models => {
                if failed.is_some() {
                    return (Vec::new(), Vec::new());
                }
                self.default_model = default_model_of(&result);
                (vec![Event::Ready { models: models_from(&result), mode: self.mode }], Vec::new())
            }
            Asked::Start => {
                if let Some(why) = failed {
                    return (vec![Event::Error(why), Event::Finished(crate::i18n::tr("Codex couldn't start a session.").to_string())], Vec::new());
                }
                let thread = result.get("thread").map(|t| text_of(t, "id")).unwrap_or("").to_string();
                self.thread = Some(thread.clone());
                if let Some(mode) = mode_from(result.get("approvalPolicy").unwrap_or(&Value::Null), result.get("sandbox").unwrap_or(&Value::Null)) {
                    self.mode = mode;
                }
                let model = text_of(&result, "model").to_string();
                if self.effort.is_none() {
                    self.effort = result.get("reasoningEffort").and_then(Value::as_str).map(str::to_string);
                }
                let mut events = vec![Event::Started { session_id: thread, model, mode: self.mode }];
                if let Some(effort) = &self.effort {
                    events.push(Event::EffortChanged(effort.clone()));
                }
                let mut lines = Vec::new();
                if let Some(text) = self.waiting.take() {
                    lines.push(self.turn_start(&text));
                }
                (events, lines)
            }
            Asked::Turn => {
                if let Some(why) = failed {
                    // The editor's context may have been refused (an older Codex, or one that doesn't take the field):
                    // it's sent in the text instead, and the prompt tried again.
                    if !self.context_in_text && !ide_context().is_empty() && (why.contains("additionalContext") || why.contains("experimental") || why.contains("unknown field")) && let Some(text) = self.last_prompt.clone() {
                        self.context_in_text = true;
                        return (Vec::new(), vec![self.turn_start(&text)]);
                    }
                    return (vec![Event::Error(why), Event::TurnFinished], Vec::new());
                }
                if let Some(turn) = result.get("turn") {
                    self.turn = Some(text_of(turn, "id").to_string());
                }
                (Vec::new(), Vec::new())
            }
            Asked::Limits => {
                let limits = limits_from(result.get("rateLimits").unwrap_or(&Value::Null));
                if limits.any() {
                    self.heard_limits = true;
                    (vec![Event::Limits(limits)], Vec::new())
                } else {
                    (Vec::new(), Vec::new())
                }
            }
            Asked::Other => (Vec::new(), Vec::new()),
        }
    }

    fn server_request(&mut self, method: &str, rpc: Value, params: &Value) -> (Vec<Event>, Vec<String>) {
        let key = format!("req-{rpc}");
        match method {
            "item/commandExecution/requestApproval" => {
                let command = unwrap_shell(text_of(params, "command"));
                let reason = text_of(params, "reason");
                let can_remember = params.get("availableDecisions").and_then(Value::as_array).is_none_or(|d| d.iter().any(|d| d.as_str() == Some("acceptForSession")));
                let shown = first_line(&command, 80);
                let detail = if reason.is_empty() { command.clone() } else { format!("{command}\n\n{reason}") };
                self.approvals.insert(key.clone(), Approval::Command { rpc });
                (vec![Event::Permission(Permission { id: key, tool: "Shell".into(), title: crate::i18n::tf!("Run {}", shown), detail, kind: PermissionKind::Command(command), can_remember })], Vec::new())
            }
            "item/fileChange/requestApproval" => self.file_approval(key, rpc, params),
            "item/permissions/requestApproval" => {
                let requested = params.get("permissions").cloned().unwrap_or(Value::Null);
                let reason = text_of(params, "reason");
                let detail = if reason.is_empty() { requested.to_string() } else { format!("{reason}\n\n{requested}") };
                self.approvals.insert(key.clone(), Approval::Permissions { rpc, requested });
                (vec![Event::Permission(Permission { id: key, tool: "Permissions".into(), title: crate::i18n::tr("Allow more access").to_string(), detail, kind: PermissionKind::Other, can_remember: true })], Vec::new())
            }
            // Questions with choices need a form this panel doesn't have: Codex is told there was no answer.
            "item/tool/requestUserInput" => (
                vec![Event::Notice(crate::i18n::tr("Codex tried to ask a question with choices, which this panel can't show yet.").to_string())],
                vec![json!({"id": rpc, "result": {"answers": {}}}).to_string()],
            ),
            "mcpServer/elicitation/request" => self.tool_approval(key, rpc, params),
            other => {
                let error = format!("Limoniter doesn't handle {other}");
                (Vec::new(), vec![json!({"id": rpc, "error": {"code": -32601, "message": error}}).to_string()])
            }
        }
    }

    /// An MCP server asking to be allowed to run a tool (Codex asks through the MCP way of asking the person
    /// something, with `_meta.codex_approval_kind` saying what for). Limoniter's own tools only look at what's open
    /// or open a file, so they're allowed; another server's are put to the person. Any other question (a form to
    /// fill in) is declined, since this panel has none.
    fn tool_approval(&mut self, key: String, rpc: Value, params: &Value) -> (Vec<Event>, Vec<String>) {
        let meta = params.get("_meta").cloned().unwrap_or(Value::Null);
        let server = text_of(params, "serverName");
        if text_of(&meta, "codex_approval_kind") != "mcp_tool_call" {
            return (Vec::new(), vec![json!({"id": rpc, "result": {"action": "decline", "content": null}}).to_string()]);
        }
        if server == "limoniter" {
            return (Vec::new(), vec![json!({"id": rpc, "result": {"action": "accept", "content": null}}).to_string()]);
        }
        let tool = text_of(&meta, "tool_name").to_string();
        let shown = format!("{server}: {tool}");
        let mut detail = text_of(params, "message").to_string();
        if let Some(arguments) = meta.get("tool_params").filter(|a| a.as_object().is_some_and(|o| !o.is_empty())) {
            detail = format!("{detail}\n\n{arguments}");
        }
        let can_remember = meta.get("persist").and_then(Value::as_array).is_some_and(|p| p.iter().any(|p| p.as_str() == Some("session")));
        self.approvals.insert(key.clone(), Approval::Tool { rpc });
        (vec![Event::Permission(Permission { id: key, tool, title: crate::i18n::tf!("Use {}", shown), detail, kind: PermissionKind::Other, can_remember })], Vec::new())
    }

    /// A request to change files: each file is put to the person as a change to review where the whole file after it
    /// can be worked out from the patch.
    fn file_approval(&mut self, key: String, rpc: Value, params: &Value) -> (Vec<Event>, Vec<String>) {
        let item = text_of(params, "itemId").to_string();
        let touched = self.changes.get(&item).cloned().unwrap_or_default();
        let mut events = Vec::new();
        let mut parts = Vec::new();
        let many = touched.len() > 1;
        for (n, one) in touched.iter().enumerate() {
            let id = if many { format!("{key}#{n}") } else { key.clone() };
            let shown = crate::agent_claude::short_path(&one.path, &self.folder);
            // A file that isn't text can't be shown as a change; the person is asked in words instead.
            let current = match std::fs::read(&one.path) {
                Ok(bytes) => String::from_utf8(bytes).ok().map(Some),
                Err(_) => Some(None),
            };
            let (title, proposed, detail) = match &one.change {
                Change::Add(text) => (crate::i18n::tf!("Create {}", shown), Some(text.clone()), String::new()),
                Change::Update(diff) => (crate::i18n::tf!("Edit {}", shown), current.as_ref().and_then(|now| apply_diff(now.as_deref().unwrap_or(""), diff)), diff.clone()),
                Change::Delete => (crate::i18n::tf!("Delete {}", shown), None, String::new()),
                Change::Move(to, diff) => {
                    let to = crate::agent_claude::short_path(Path::new(to), &self.folder);
                    (crate::i18n::tf!("Move {} to {}", shown, to), None, diff.clone())
                }
            };
            let kind = match &proposed {
                Some(text) => PermissionKind::Edit { path: one.path.clone(), proposed: text.clone() },
                None => PermissionKind::Other,
            };
            let detail = if proposed.is_some() { String::new() } else { detail };
            events.push(Event::Permission(Permission { id: id.clone(), tool: "apply_patch".into(), title, detail, kind, can_remember: true }));
            self.part_keys.insert(id.clone(), key.clone());
            parts.push(Part { id, path: one.path.clone(), proposed, answer: None });
        }
        if parts.is_empty() {
            let reason = text_of(params, "reason");
            events.push(Event::Permission(Permission { id: key.clone(), tool: "apply_patch".into(), title: crate::i18n::tr("Change files").to_string(), detail: reason.to_string(), kind: PermissionKind::Other, can_remember: true }));
            self.approvals.insert(key, Approval::Command { rpc });
            return (events, Vec::new());
        }
        self.approvals.insert(key, Approval::Files { rpc, item, parts });
        (events, Vec::new())
    }

    fn notification(&mut self, method: &str, params: &Value) -> Vec<Event> {
        let thread = text_of(params, "threadId");
        let mine = self.thread.as_deref().is_none_or(|t| t == thread || thread.is_empty());
        match method {
            "thread/tokenUsage/updated" => self.token_usage(thread, mine, params),
            "account/rateLimits/updated" => {
                let limits = limits_from(params.get("rateLimits").unwrap_or(&Value::Null));
                if limits.any() {
                    self.heard_limits = true;
                    vec![Event::Limits(limits)]
                } else {
                    Vec::new()
                }
            }
            "turn/started" => {
                if let Some(turn) = params.get("turn") {
                    self.turns.insert(thread.to_string(), text_of(turn, "id").to_string());
                    if mine {
                        self.turn = Some(text_of(turn, "id").to_string());
                    }
                }
                Vec::new()
            }
            "turn/completed" => {
                let status = params.get("turn").map(|t| text_of(t, "status")).unwrap_or("");
                self.turns.remove(thread);
                if !mine {
                    return Vec::new();
                }
                self.turn = None;
                let mut events = Vec::new();
                if status == "failed" {
                    let why = params.get("turn").and_then(|t| t.get("error")).map(|e| text_of(e, "message")).unwrap_or("");
                    events.push(Event::Error(if why.is_empty() { crate::i18n::tr("The turn failed.").to_string() } else { why.to_string() }));
                }
                events.push(Event::TurnFinished);
                events
            }
            "thread/name/updated" if mine => params.get("threadName").and_then(Value::as_str).map(|name| vec![Event::Title(name.to_string())]).unwrap_or_default(),
            "item/started" | "item/completed" => self.item(method == "item/completed", thread, mine, params.get("item").unwrap_or(&Value::Null)),
            "item/agentMessage/delta" if mine => vec![Event::TextDelta { id: text_of(params, "itemId").to_string(), text: text_of(params, "delta").to_string() }],
            "item/reasoning/summaryTextDelta" if mine => {
                let id = text_of(params, "itemId").to_string();
                let index = params.get("summaryIndex").and_then(Value::as_u64).unwrap_or(0) as usize;
                let parts = self.thinking.entry(id.clone()).or_default();
                if parts.len() <= index {
                    parts.resize(index + 1, String::new());
                }
                parts[index].push_str(text_of(params, "delta"));
                vec![Event::Thinking { id, text: parts.join("\n\n") }]
            }
            "item/fileChange/patchUpdated" => {
                self.changes.insert(text_of(params, "itemId").to_string(), touched_from(params.get("changes").unwrap_or(&Value::Null)));
                Vec::new()
            }
            "serverRequest/resolved" => {
                // The request was taken back (or answered elsewhere).
                let key = format!("req-{}", params.get("requestId").unwrap_or(&Value::Null));
                if self.approvals.remove(&key).is_some() {
                    let mut gone = vec![Event::PermissionGone { id: key.clone() }];
                    let parts: Vec<String> = self.part_keys.iter().filter(|(_, k)| **k == key).map(|(p, _)| p.clone()).collect();
                    for part in parts {
                        self.part_keys.remove(&part);
                        gone.push(Event::PermissionGone { id: part });
                    }
                    gone
                } else {
                    Vec::new()
                }
            }
            "error" => {
                let message = params.get("error").map(|e| text_of(e, "message")).unwrap_or("");
                let retrying = params.get("willRetry").and_then(Value::as_bool).unwrap_or(false);
                if message.is_empty() {
                    Vec::new()
                } else if retrying {
                    vec![Event::Notice(crate::i18n::tf!("Codex is trying again after a problem: {}", first_line(message, 160)))]
                } else {
                    vec![Event::Error(message.to_string())]
                }
            }
            "warning" if mine => vec![Event::Notice(text_of(params, "message").to_string())],
            "thread/compacted" if mine => vec![Event::Notice(crate::i18n::tr("The conversation was condensed to make room.").to_string())],
            _ => Vec::new(),
        }
    }

    fn token_usage(&mut self, thread: &str, mine: bool, params: &Value) -> Vec<Event> {
        let usage = params.get("tokenUsage").cloned().unwrap_or(Value::Null);
        let number = |part: &str, key: &str| usage.get(part).and_then(|u| u.get(key)).and_then(Value::as_u64).unwrap_or(0);
        if !mine {
            // A helper's own count.
            return match self.tasks.get_mut(thread) {
                Some(task) => {
                    task.tokens = number("total", "totalTokens");
                    vec![Event::Task(task.clone())]
                }
                None => Vec::new(),
            };
        }
        let mut events = vec![Event::Usage { tokens: Tokens { input: number("total", "inputTokens"), output: number("total", "outputTokens"), cached: number("total", "cachedInputTokens") }, cost: None }];
        if let Some(max) = usage.get("modelContextWindow").and_then(Value::as_u64).filter(|m| *m > 0) {
            let context = Context { used: number("last", "totalTokens"), max };
            self.context = Some(context);
            events.push(Event::Context(context));
        }
        events
    }

    /// An item began or was completed (`done`), on this session's thread or a helper's.
    fn item(&mut self, done: bool, thread: &str, mine: bool, item: &Value) -> Vec<Event> {
        let id = text_of(item, "id").to_string();
        let kind = text_of(item, "type");
        if !mine {
            // What a helper does isn't in the transcript, but counts in its task.
            if !done && matches!(kind, "commandExecution" | "fileChange" | "mcpToolCall" | "webSearch" | "dynamicToolCall")
                && let Some(task) = self.tasks.get_mut(thread)
            {
                task.tool_uses += 1;
                task.last_tool = Some(match kind {
                    "commandExecution" => "Shell".to_string(),
                    "fileChange" => "apply_patch".to_string(),
                    "webSearch" => "WebSearch".to_string(),
                    _ => text_of(item, "tool").to_string(),
                });
                return vec![Event::Task(task.clone())];
            }
            return Vec::new();
        }
        match kind {
            "agentMessage" if done => vec![Event::Text { id, text: text_of(item, "text").to_string(), done: true }],
            "plan" if done => vec![Event::Text { id, text: text_of(item, "text").to_string(), done: true }],
            "reasoning" if done => {
                self.thinking.remove(&id);
                let summary = item.get("summary").and_then(Value::as_array).map(|parts| parts.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n\n")).unwrap_or_default();
                if summary.is_empty() { Vec::new() } else { vec![Event::Thinking { id, text: summary }] }
            }
            "commandExecution" => {
                let command = unwrap_shell(text_of(item, "command"));
                if !done {
                    let shown = first_line(&command, 80);
                    return vec![Event::ToolUse { id, name: "Shell".into(), summary: crate::i18n::tf!("Ran {}", shown), kind: ToolKind::Command(command), parent: None }];
                }
                let status = text_of(item, "status");
                let code = item.get("exitCode").and_then(Value::as_i64);
                let output = item.get("aggregatedOutput").and_then(Value::as_str).unwrap_or("").to_string();
                let text = match status {
                    "declined" => crate::i18n::tr("It wasn't allowed to run.").to_string(),
                    _ if output.is_empty() => code.map(|c| crate::i18n::tf!("Finished with exit code {}.", c)).unwrap_or_default(),
                    _ => output,
                };
                vec![Event::ToolResult { id, text, is_error: status == "failed" || status == "declined" || code.is_some_and(|c| c != 0) }]
            }
            "fileChange" => self.file_change(done, &id, item),
            "mcpToolCall" => {
                if !done {
                    let shown = format!("{}: {}", text_of(item, "server"), text_of(item, "tool"));
                    return vec![Event::ToolUse { id, name: text_of(item, "tool").to_string(), summary: crate::i18n::tf!("Use {}", shown), kind: ToolKind::Other, parent: None }];
                }
                let failed = text_of(item, "status") == "failed";
                let text = item
                    .get("result")
                    .and_then(|r| r.get("content"))
                    .and_then(Value::as_array)
                    .map(|parts| parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"))
                    .or_else(|| item.get("error").map(|e| text_of(e, "message").to_string()))
                    .unwrap_or_default();
                vec![Event::ToolResult { id, text, is_error: failed }]
            }
            "dynamicToolCall" => {
                if !done {
                    return vec![Event::ToolUse { id, name: text_of(item, "tool").to_string(), summary: text_of(item, "tool").to_string(), kind: ToolKind::Other, parent: None }];
                }
                vec![Event::ToolResult { id, text: String::new(), is_error: item.get("success").and_then(Value::as_bool) == Some(false) }]
            }
            "webSearch" => {
                if !done {
                    let shown = text_of(item, "query").to_string();
                    return vec![Event::ToolUse { id, name: "WebSearch".into(), summary: crate::i18n::tf!("Searched the web for {}", shown), kind: ToolKind::Other, parent: None }];
                }
                Vec::new()
            }
            "collabAgentToolCall" => self.collab(done, &id, item),
            "contextCompaction" if done => vec![Event::Notice(crate::i18n::tr("The conversation was condensed to make room.").to_string())],
            _ => Vec::new(),
        }
    }

    fn file_change(&mut self, done: bool, id: &str, item: &Value) -> Vec<Event> {
        let touched = touched_from(item.get("changes").unwrap_or(&Value::Null));
        if !done {
            let mut events = Vec::new();
            for (n, one) in touched.iter().enumerate() {
                let shown = crate::agent_claude::short_path(&one.path, &self.folder);
                let (name, summary) = match &one.change {
                    Change::Add(_) => ("Write", crate::i18n::tf!("Create {}", shown)),
                    Change::Delete => ("Delete", crate::i18n::tf!("Delete {}", shown)),
                    _ => ("Edit", crate::i18n::tf!("Edit {}", shown)),
                };
                let tool_id = if n == 0 { id.to_string() } else { format!("{id}#{n}") };
                events.push(Event::ToolUse { id: tool_id, name: name.into(), summary, kind: ToolKind::FileChange(one.path.clone()), parent: None });
            }
            self.changes.insert(id.to_string(), touched);
            return events;
        }
        let status = text_of(item, "status");
        let ok = status == "completed";
        let mut events = Vec::new();
        // What the person changed in the review goes over Codex's own change, and Codex is told next time.
        if let Some(rewrites) = self.rewrites.remove(id)
            && ok
        {
            for (path, text) in rewrites {
                let shown = crate::agent_claude::short_path(&path, &self.folder);
                match std::fs::write(&path, text) {
                    Ok(()) => {
                        self.notes.push(format!("The person edited {shown} while reviewing your change to it, before accepting it. The file now holds their version, which differs a little from yours. Read it again before you change it further."));
                        events.push(Event::Notice(crate::i18n::tf!("Your changes to {} were kept over Codex's, and Codex will be told in your next message.", shown)));
                    }
                    Err(e) => events.push(Event::Error(crate::i18n::tf!("Couldn't write your changes to {}: {}", shown, e))),
                }
            }
        }
        let count = touched.len().max(1);
        for n in 0..count {
            let tool_id = if n == 0 { id.to_string() } else { format!("{id}#{n}") };
            let text = match status {
                "completed" => crate::i18n::tr("Done.").to_string(),
                "declined" => crate::i18n::tr("It wasn't allowed to make this change.").to_string(),
                _ => crate::i18n::tr("The change failed.").to_string(),
            };
            events.push(Event::ToolResult { id: tool_id, text, is_error: !ok });
        }
        self.changes.remove(id);
        events
    }

    /// Helpers Codex starts and stops are tasks. Each helper is a thread of its own, so its tasks are named by it.
    fn collab(&mut self, done: bool, id: &str, item: &Value) -> Vec<Event> {
        let tool = text_of(item, "tool");
        let prompt = first_line(text_of(item, "prompt"), 100);
        let mut events = Vec::new();
        if !done && tool == "spawnAgent" {
            events.push(Event::ToolUse { id: id.to_string(), name: "spawnAgent".into(), summary: crate::i18n::tf!("Subagent: {}", prompt), kind: ToolKind::Other, parent: None });
        }
        let states = item.get("agentsStates");
        for receiver in item.get("receiverThreadIds").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
            let status = states.and_then(|s| s.get(receiver)).map(|s| text_of(s, "status")).unwrap_or("");
            let mut state = if status.is_empty() { TaskState::Running } else { task_state(status) };
            if tool == "closeAgent" && state == TaskState::Running {
                state = TaskState::Stopped;
            }
            let task = self.tasks.entry(receiver.to_string()).or_insert_with(|| Task { id: receiver.to_string(), kind: "subagent".into(), ..Task::default() });
            if tool == "spawnAgent" && !prompt.is_empty() {
                task.description.clone_from(&prompt);
            } else if task.description.is_empty() {
                task.description = if prompt.is_empty() { receiver.to_string() } else { prompt.clone() };
            }
            task.state = state;
            events.push(Event::Task(task.clone()));
        }
        if done {
            let failed = text_of(item, "status") == "failed";
            events.push(Event::ToolResult { id: id.to_string(), text: String::new(), is_error: failed });
        }
        events
    }
}

// ---------- The process ----------

/// The process, and what to say about how it ended.
struct Running {
    child: Mutex<Child>,
    stderr: Arc<Mutex<String>>,
}

/// Runs the program with its error output kept (the last of it), and its input and output taken.
fn spawn(program: &str, args: &[String], folder: Option<&Path>) -> Result<(Running, std::process::ChildStdin, std::process::ChildStdout), String> {
    let mut command = crate::platform::command(program);
    command.args(args).env("RUST_LOG", "warn").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(folder) = folder {
        command.current_dir(folder);
    }
    let mut child = command.spawn().map_err(|e| crate::i18n::tf!("Couldn't start {}: {}", program, e))?;
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
    Ok((Running { child: Mutex::new(child), stderr }, stdin, stdout))
}

/// Starts `codex app-server` and gives the session its two channels.
pub fn start(options: Options) -> Result<Handle, String> {
    let (running, stdin, stdout) = spawn(&options.program, &arguments(&options), options.wsl.is_none().then_some(options.folder.as_path()))?;
    let running = Arc::new(running);
    let state = Arc::new(Mutex::new(State::new(&options)));
    let stdin = Arc::new(Mutex::new(Some(stdin)));
    let (event_tx, events) = channel::<Event>();
    let (commands, command_rx) = channel::<Command>();

    let wake = options.wake.clone();
    let paths = options.wsl.clone();
    let send_lines = {
        let (stdin, paths) = (stdin.clone(), paths.clone());
        move |lines: Vec<String>| -> bool {
            let Ok(mut stdin) = stdin.lock() else { return false };
            let Some(stdin) = stdin.as_mut() else { return false };
            lines.iter().filter(|line| !line.is_empty()).all(|line| {
                let line = paths.as_ref().map_or_else(|| line.clone(), |paths| paths.outgoing(line));
                writeln!(stdin, "{line}").and_then(|()| stdin.flush()).is_ok()
            })
        }
    };
    let first = state.lock().map(|mut s| s.initialize()).unwrap_or_default();
    send_lines(vec![first]);

    // Reading: what the program says.
    {
        let (state, tx, running, wake, send_lines, paths) = (state.clone(), event_tx.clone(), running.clone(), wake.clone(), send_lines.clone(), paths.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let line = paths.as_ref().map_or(line.clone(), |paths| paths.incoming(&line));
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
                Some(status) if status.success() && said.is_empty() => crate::i18n::tr("Codex has finished.").to_string(),
                Some(status) => crate::i18n::tf!("Codex stopped ({}). {}", status, said),
                None => crate::i18n::tr("Codex has gone.").to_string(),
            };
            let _ = tx.send(Event::Finished(why.trim().to_string()));
            wake();
        });
    }

    // Writing: what the person tells it.
    {
        let (state, running, stdin, tx, wake) = (state.clone(), running.clone(), stdin.clone(), event_tx, wake);
        std::thread::spawn(move || {
            while let Ok(command) = command_rx.recv() {
                if command == Command::Close {
                    break;
                }
                let (events, lines) = state.lock().map(|mut s| s.command(command)).unwrap_or_default();
                if !events.is_empty() {
                    for event in events {
                        let _ = tx.send(event);
                    }
                    wake();
                }
                if !lines.is_empty() && !send_lines(lines) {
                    break;
                }
            }
            // Closing its input ends it; it isn't waited for long.
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

/// Asks a `codex` which models it has, without starting a conversation (nothing is sent to the service).
pub fn probe_models(program: &str, prefix: &[String], folder: &Path) -> Result<Vec<ModelChoice>, String> {
    let options = Options { program: program.to_string(), prefix: prefix.to_vec(), folder: folder.to_path_buf(), mode: Mode::Ask, model: None, effort: None, resume: None, mcp: None, wsl: None, wake: Arc::new(|| {}) };
    let (running, mut stdin, stdout) = spawn(program, &arguments(&options), Some(folder))?;
    let mut state = State::new(&options);
    state.probe = true;
    let first = state.initialize();
    writeln!(stdin, "{first}").map_err(|e| e.to_string())?;
    let (tx, rx): (Sender<Vec<ModelChoice>>, _) = channel();
    let mut writer = stdin;
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let (events, replies) = state.line(&line);
            for reply in replies {
                let _ = writeln!(writer, "{reply}").and_then(|()| writer.flush());
            }
            for event in events {
                if let Event::Ready { models, .. } = event {
                    let _ = tx.send(models);
                    return;
                }
            }
        }
    });
    let models = rx.recv_timeout(Duration::from_secs(20)).map_err(|_| crate::i18n::tr("Codex didn't answer.").to_string());
    if let Ok(mut child) = running.child.lock() {
        let _ = child.kill();
        let _ = child.wait();
    }
    models
}

#[cfg(test)]
#[path = "agent_codex_tests.rs"]
pub(crate) mod agent_codex_tests;
