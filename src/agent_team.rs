//! Claude Code and Codex working on one project together, with Limoniter as the referee: who has claimed which
//! files, one build or test at a time, and a task board with notes between them. Nothing here draws anything or
//! knows how a tool talks; sessions (`agent_session.rs`) ask it, the MCP tools (`claude_ide.rs`) call it, and the
//! window (`agent_team_host.rs`) shows it.
//!
//! The state is one `Team` shared by all of them, because the MCP server answers on threads of its own. The
//! board (tasks, notes and the list of build commands) is kept in the project, in `.limoniter/tasks.json`, so it
//! survives a restart and can be committed or left alone as the person likes. Claims and the build queue only
//! make sense while the sessions are there, so they aren't kept.

use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

/// The person's own name as a holder of claims and a writer of notes.
pub const PERSON: &str = "You";

/// A running build that has gone on this long is taken to have been forgotten, so the queue doesn't stick.
const STALE_BUILD: Duration = Duration::from_secs(30 * 60);

/// What is taken to be a build or a test unless the person says otherwise. Each is the first words of a command.
pub const DEFAULT_BUILD_COMMANDS: [&str; 22] = [
    "cargo build",
    "cargo test",
    "cargo check",
    "cargo clippy",
    "cargo bench",
    "cargo nextest",
    "npm test",
    "npm run build",
    "npm run test",
    "pnpm test",
    "pnpm build",
    "pnpm run build",
    "yarn test",
    "yarn build",
    "make",
    "pytest",
    "python -m pytest",
    "go test",
    "go build",
    "gradle",
    "gradlew",
    "dotnet build",
];

// ---------- The board ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Open,
    Taken,
    Done,
}

impl Status {
    pub fn word(self) -> &'static str {
        match self {
            Status::Open => "open",
            Status::Taken => "taken",
            Status::Done => "done",
        }
    }

    fn parse(word: &str) -> Status {
        match word {
            "taken" => Status::Taken,
            "done" => Status::Done,
            _ => Status::Open,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub id: u32,
    pub title: String,
    pub details: String,
    pub status: Status,
    /// The session it's given to or has taken, by its name ("Codex 1").
    pub assignee: Option<String>,
    /// What was said when it was finished.
    pub notes: String,
    /// Who wrote it.
    pub by: String,
}

impl Task {
    fn to_json(&self) -> Value {
        json!({ "id": self.id, "title": self.title, "details": self.details, "status": self.status.word(), "assignee": self.assignee, "notes": self.notes, "by": self.by })
    }

    fn from_json(value: &Value) -> Option<Task> {
        let text = |key: &str| value.get(key).and_then(Value::as_str).unwrap_or("").to_string();
        Some(Task {
            id: value.get("id").and_then(Value::as_u64)? as u32,
            title: text("title"),
            details: text("details"),
            status: Status::parse(&text("status")),
            assignee: value.get("assignee").and_then(Value::as_str).map(str::to_string),
            notes: text("notes"),
            by: text("by"),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub from: String,
    /// A session's name ("Codex 1"), a tool's ("Codex", for any of its sessions), "all", or "You".
    pub to: String,
    pub text: String,
    /// Who has read it.
    pub read_by: Vec<String>,
}

impl Note {
    fn to_json(&self) -> Value {
        json!({ "from": self.from, "to": self.to, "text": self.text, "read_by": self.read_by })
    }

    fn from_json(value: &Value) -> Option<Note> {
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
        Some(Note {
            from: text("from")?,
            to: text("to")?,
            text: text("text")?,
            read_by: value.get("read_by").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default(),
        })
    }

    /// Whether a note is for the session with this name.
    fn is_for(&self, key: &str) -> bool {
        let to = self.to.trim().to_lowercase();
        let key = key.to_lowercase();
        !to.is_empty() && (to == "all" || to == "everyone" || key == to || (key.starts_with(&to) && key[to.len()..].starts_with(' ')))
    }
}

/// A file or folder somebody has claimed.
#[derive(Clone, Debug, PartialEq)]
pub struct PathClaim {
    pub path: PathBuf,
    pub holder: String,
}

/// A session as the team knows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub key: String,
    /// "Claude Code" or "Codex".
    pub tool: String,
    pub title: String,
}

#[derive(Clone, Debug)]
struct Entry {
    id: u64,
    key: String,
    command: String,
    since: Instant,
}

/// What a session is told when it asks to run a build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turn {
    /// Go ahead.
    Go,
    /// Wait; the number says when it's this one's turn (`is_its_turn`).
    Wait(u64),
}

/// One line of the build queue, for showing.
#[derive(Clone, Debug, PartialEq)]
pub struct QueueRow {
    pub id: u64,
    pub who: String,
    pub command: String,
    pub running: bool,
}

pub struct Team {
    folder: Option<PathBuf>,
    pub tasks: Vec<Task>,
    pub notes: Vec<Note>,
    build_commands: Option<Vec<String>>,
    pub claims: Vec<PathClaim>,
    pub members: Vec<Member>,
    counters: HashMap<String, u32>,
    running: Option<Entry>,
    queue: Vec<Entry>,
    next_entry: u64,
    outbox: HashMap<String, Vec<String>>,
    /// When the board file was last read or written, to see another program's change.
    stamp: Option<SystemTime>,
}

impl Team {
    pub fn new() -> Team {
        Team { folder: None, tasks: Vec::new(), notes: Vec::new(), build_commands: None, claims: Vec::new(), members: Vec::new(), counters: HashMap::new(), running: None, queue: Vec::new(), next_entry: 1, outbox: HashMap::new(), stamp: None }
    }

    // ----- The project and its file -----

    pub fn board_file(folder: &Path) -> PathBuf {
        folder.join(".limoniter").join("tasks.json")
    }

    /// Says which project the board belongs to, reading its file if it has one. Returns whether it changed.
    pub fn set_folder(&mut self, folder: &Path) -> bool {
        if self.folder.as_deref() == Some(folder) {
            return false;
        }
        self.folder = Some(folder.to_path_buf());
        self.tasks.clear();
        self.notes.clear();
        self.build_commands = None;
        self.stamp = None;
        self.load();
        true
    }

    fn load(&mut self) {
        let Some(folder) = self.folder.clone() else { return };
        let file = Self::board_file(&folder);
        self.stamp = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
        let Some(value) = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { return };
        self.tasks = value.get("tasks").and_then(Value::as_array).map(|a| a.iter().filter_map(Task::from_json).collect()).unwrap_or_default();
        self.notes = value.get("notes").and_then(Value::as_array).map(|a| a.iter().filter_map(Note::from_json).collect()).unwrap_or_default();
        self.build_commands = value.get("build_commands").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect());
        // The sessions a task was given to may be gone (the program was restarted): such a task is open again.
        let keys: Vec<String> = self.members.iter().map(|m| m.key.clone()).collect();
        for task in &mut self.tasks {
            if task.assignee.as_ref().is_some_and(|a| a != PERSON && !keys.contains(a)) && task.status != Status::Done {
                task.assignee = None;
                task.status = Status::Open;
            }
        }
    }

    /// Writes the board to its file in the project.
    pub fn save(&mut self) {
        let Some(folder) = self.folder.clone() else { return };
        let file = Self::board_file(&folder);
        let mut value = json!({ "version": 1, "tasks": self.tasks.iter().map(Task::to_json).collect::<Vec<_>>(), "notes": self.notes.iter().map(Note::to_json).collect::<Vec<_>>() });
        if let Some(commands) = &self.build_commands {
            value["build_commands"] = json!(commands);
        }
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(&value) {
            let _ = std::fs::write(&file, text + "\n");
        }
        self.stamp = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
    }

    /// Reads the board again if something else (another window, the person) has changed its file.
    pub fn refresh(&mut self) -> bool {
        let Some(folder) = self.folder.clone() else { return false };
        let now = std::fs::metadata(Self::board_file(&folder)).and_then(|m| m.modified()).ok();
        if now.is_some() && now != self.stamp {
            self.load();
            return true;
        }
        false
    }

    // ----- Who is in the team -----

    /// A name for a new session of a tool: "Codex 1", then "Codex 2".
    pub fn new_handle(&mut self, tool: &str) -> String {
        let count = self.counters.entry(tool.to_string()).or_insert(0);
        *count += 1;
        format!("{tool} {count}")
    }

    pub fn add_member(&mut self, key: &str, tool: &str, title: &str) {
        match self.members.iter_mut().find(|m| m.key == key) {
            Some(member) => member.title = title.to_string(),
            None => self.members.push(Member { key: key.to_string(), tool: tool.to_string(), title: title.to_string() }),
        }
    }

    pub fn set_title(&mut self, key: &str, title: &str) {
        if let Some(member) = self.members.iter_mut().find(|m| m.key == key) {
            member.title = title.to_string();
        }
    }

    /// A session has ended: what it held is let go, and its unfinished tasks are open again.
    pub fn remove_member(&mut self, key: &str) {
        if key.is_empty() {
            return;
        }
        self.members.retain(|m| m.key != key);
        self.claims.retain(|c| c.holder != key);
        self.queue.retain(|e| e.key != key);
        self.build_done(key);
        self.outbox.remove(key);
        let mut changed = false;
        for task in &mut self.tasks {
            if task.assignee.as_deref() == Some(key) && task.status != Status::Done {
                task.assignee = None;
                task.status = Status::Open;
                changed = true;
            }
        }
        if changed {
            self.save();
        }
    }

    /// A session as the person is told of it: "Claude Code 1 (Parser)".
    pub fn label(&self, key: &str) -> String {
        if key == PERSON {
            return crate::i18n::tr("you").to_string();
        }
        match self.members.iter().find(|m| m.key == key) {
            Some(member) if !member.title.is_empty() && member.title != crate::i18n::tr("New session") => format!("{} ({})", member.key, member.title),
            _ => key.to_string(),
        }
    }

    // ----- Claims -----

    fn absolute(&self, path: &str) -> PathBuf {
        let path = Path::new(path.trim());
        let joined = match (&self.folder, path.is_absolute()) {
            (Some(folder), false) => folder.join(path),
            _ => path.to_path_buf(),
        };
        normal(&joined)
    }

    /// Claims files or folders for `who`. Nothing is claimed if any is held by another.
    pub fn claim_paths(&mut self, who: &str, paths: &[String]) -> Result<String, String> {
        let wanted: Vec<PathBuf> = paths.iter().filter(|p| !p.trim().is_empty()).map(|p| self.absolute(p)).collect();
        if wanted.is_empty() {
            return Err("Give at least one path to claim.".into());
        }
        for path in &wanted {
            if let Some(claim) = self.claims.iter().find(|c| c.holder != who && (path.starts_with(&c.path) || c.path.starts_with(path))) {
                return Err(format!("{} is held by {}. Ask them with leave_note, or ask the person to release it.", self.shown(&claim.path), self.label(&claim.holder)));
            }
        }
        for path in wanted {
            if !self.claims.iter().any(|c| c.holder == who && path.starts_with(&c.path)) {
                // A claim of a folder takes in the ones this holder already had inside it.
                self.claims.retain(|c| !(c.holder == who && c.path.starts_with(&path)));
                self.claims.push(PathClaim { path, holder: who.to_string() });
            }
        }
        Ok(format!("Claimed. {} now holds {}.", self.label(who), self.claims.iter().filter(|c| c.holder == who).map(|c| self.shown(&c.path)).collect::<Vec<_>>().join(", ")))
    }

    pub fn release_paths(&mut self, who: &str, paths: &[String]) -> String {
        let released: Vec<PathBuf> = paths.iter().map(|p| self.absolute(p)).collect();
        let before = self.claims.len();
        self.claims.retain(|c| !(c.holder == who && released.iter().any(|r| c.path.starts_with(r))));
        format!("Released {} claim(s).", before - self.claims.len())
    }

    /// The path as the person wrote it: relative to the project when it's inside it.
    pub fn shown(&self, path: &Path) -> String {
        match self.folder.as_deref().and_then(|f| path.strip_prefix(f).ok()) {
            Some(rest) if !rest.as_os_str().is_empty() => rest.display().to_string(),
            _ => path.display().to_string(),
        }
    }

    /// Who holds the claim that covers this path, other than `key`.
    pub fn holder_other_than(&self, key: &str, path: &Path) -> Option<String> {
        let path = normal(path);
        self.claims.iter().find(|c| c.holder != key && path.starts_with(&c.path)).map(|c| c.holder.clone())
    }

    /// Who holds this path, if anyone: for marking it in the Explorer and Files changing.
    pub fn holder_of(&self, path: &Path) -> Option<String> {
        self.holder_other_than("", path)
    }

    /// What is said to an agent that is refused a file.
    pub fn refusal(&self, holder: &str, path: &Path) -> String {
        let who = self.label(holder);
        let mut text = format!("Refused: {} is claimed by {who}, so it can't be changed by you now.", self.shown(&normal(path)));
        if holder == PERSON {
            text.push_str(" The person has kept it for themselves; ask them to release the claim.");
        } else {
            text.push_str(&format!(" To ask for it, use leave_note to {holder} (or ask the person to release the claim), and carry on with something else meanwhile."));
        }
        text
    }

    /// What the claims held by anyone but `key` cover, for seeing what a command changed.
    pub fn others_claims(&self, key: &str) -> Vec<PathClaim> {
        self.claims.iter().filter(|c| c.holder != key).cloned().collect()
    }

    /// Reports files that `key` changed which others hold: a note in the holder's way and a line for `key`.
    pub fn report_changes(&mut self, key: &str, changed: &[PathBuf]) -> Vec<String> {
        let mut lines = Vec::new();
        for path in changed {
            if let Some(holder) = self.holder_other_than(key, path) {
                let (who, owner, shown) = (self.label(key), self.label(&holder), self.shown(path));
                lines.push(crate::i18n::tf!("{who} changed {shown}, which {owner} has claimed.", who = who, shown = shown, owner = owner));
                if holder != PERSON {
                    let text = format!("{key} changed {shown}, which you have claimed.");
                    self.notes.push(Note { from: "Limoniter".into(), to: holder.clone(), text: text.clone(), read_by: Vec::new() });
                    let line = crate::i18n::tf!("{who} changed {shown}, which you have claimed.", who = who, shown = shown);
                    self.outbox.entry(holder).or_default().push(line);
                }
            }
        }
        if !lines.is_empty() {
            self.save();
        }
        lines
    }

    // ----- Builds and tests, one at a time -----

    pub fn build_commands(&self) -> Vec<String> {
        self.build_commands.clone().unwrap_or_else(|| DEFAULT_BUILD_COMMANDS.iter().map(|s| s.to_string()).collect())
    }

    pub fn set_build_commands(&mut self, commands: Vec<String>) {
        let commands: Vec<String> = commands.into_iter().map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect();
        self.build_commands = if commands == DEFAULT_BUILD_COMMANDS.iter().map(|s| s.to_string()).collect::<Vec<_>>() { None } else { Some(commands) };
        self.save();
    }

    /// The part of a command that builds or tests, if any does.
    pub fn build_part(&self, command: &str) -> Option<String> {
        build_part(command, &self.build_commands())
    }

    /// A session wants to run a build. It's told to go, or to wait its turn.
    pub fn build_request(&mut self, key: &str, command: &str) -> Turn {
        self.expire();
        let what = self.build_part(command).unwrap_or_else(|| command.to_string());
        let id = self.next_entry;
        self.next_entry += 1;
        let entry = Entry { id, key: key.to_string(), command: what.clone(), since: Instant::now() };
        match &self.running {
            Some(running) if running.key != key => {
                let (runner, runner_label, own) = (running.command.clone(), self.label(&running.key), self.label(key));
                let running_key = running.key.clone();
                self.outbox.entry(key.to_string()).or_default().push(crate::i18n::tf!("Waiting for {who}'s {what}", who = runner_label, what = runner));
                self.outbox.entry(running_key).or_default().push(crate::i18n::tf!("{who} is waiting to run {what} until this has finished.", who = own, what = what));
                self.queue.push(entry);
                Turn::Wait(id)
            }
            Some(_) => Turn::Go,
            None => {
                self.running = Some(entry);
                Turn::Go
            }
        }
    }

    /// Whether the queued build with this number may run now.
    pub fn is_its_turn(&self, id: u64) -> bool {
        self.running.as_ref().is_some_and(|r| r.id == id)
    }

    /// A session's build has finished (or it has gone): the next in the queue goes.
    pub fn build_done(&mut self, key: &str) {
        if self.running.as_ref().is_some_and(|r| r.key == key) {
            self.running = None;
            self.promote();
        }
    }

    fn promote(&mut self) {
        if self.running.is_none() && !self.queue.is_empty() {
            let mut next = self.queue.remove(0);
            next.since = Instant::now();
            self.running = Some(next);
        }
    }

    fn expire(&mut self) {
        if self.running.as_ref().is_some_and(|r| r.since.elapsed() > STALE_BUILD) {
            self.running = None;
            self.promote();
        }
    }

    /// The person lets a waiting build go next.
    pub fn jump(&mut self, id: u64) {
        if let Some(at) = self.queue.iter().position(|e| e.id == id) {
            let entry = self.queue.remove(at);
            self.queue.insert(0, entry);
        }
    }

    /// A session has stopped waiting: its places in the queue go.
    pub fn cancel_waiting(&mut self, key: &str) {
        self.queue.retain(|e| e.key != key);
    }

    pub fn queue_rows(&self) -> Vec<QueueRow> {
        let row = |e: &Entry, running: bool| QueueRow { id: e.id, who: self.label(&e.key), command: e.command.clone(), running };
        self.running.iter().map(|e| row(e, true)).chain(self.queue.iter().map(|e| row(e, false))).collect()
    }

    pub fn take_notices(&mut self, key: &str) -> Vec<String> {
        self.outbox.remove(key).unwrap_or_default()
    }

    // ----- The tools -----

    fn resolve(&self, name: &str) -> Option<String> {
        let name = name.trim();
        if name.eq_ignore_ascii_case("you") || name.eq_ignore_ascii_case("person") {
            return Some(PERSON.to_string());
        }
        let lower = name.to_lowercase();
        self.members.iter().find(|m| m.key.to_lowercase() == lower).or_else(|| self.members.iter().find(|m| m.tool.to_lowercase().starts_with(&lower) || m.key.to_lowercase().starts_with(&lower))).map(|m| m.key.clone())
    }

    pub fn add_task(&mut self, by: &str, title: &str, details: &str, assignee: Option<String>) -> u32 {
        let id = self.tasks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        self.tasks.push(Task { id, title: title.trim().to_string(), details: details.trim().to_string(), status: Status::Open, assignee, notes: String::new(), by: by.to_string() });
        self.save();
        id
    }

    /// Calls one of the tools the agents are offered. The text is what the agent reads; the flag says it failed.
    pub fn call(&mut self, who: &str, tool: &str, args: &Value) -> (String, bool) {
        let result = self.call_inner(who, tool, args);
        match result {
            Ok(text) => (text, false),
            Err(text) => (text, true),
        }
    }

    fn call_inner(&mut self, who: &str, tool: &str, args: &Value) -> Result<String, String> {
        let text = |key: &str| args.get(key).and_then(Value::as_str).map(str::to_string);
        let id = || args.get("id").and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))).map(|n| n as u32).ok_or_else(|| "Give the task's id.".to_string());
        let paths = || -> Vec<String> {
            match args.get("paths") {
                Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
                Some(Value::String(s)) => vec![s.clone()],
                _ => text("path").into_iter().collect(),
            }
        };
        match tool {
            "list_tasks" => {
                let only = text("status");
                let list: Vec<Value> = self.tasks.iter().filter(|t| only.as_deref().is_none_or(|s| s == t.status.word())).map(Task::to_json).collect();
                Ok(serde_json::to_string_pretty(&json!({ "you_are": who, "tasks": list })).unwrap_or_default())
            }
            "claim_task" => {
                let id = id()?;
                let task = self.tasks.iter_mut().find(|t| t.id == id).ok_or_else(|| format!("There is no task {id}."))?;
                if task.status == Status::Done {
                    return Err(format!("Task {id} is already done."));
                }
                if let Some(other) = task.assignee.as_ref().filter(|a| a.as_str() != who) {
                    // Said to the agent, in English.
                    let taken = task.status == Status::Taken;
                    return Err(if taken { format!("Task {id} is taken by {other}.") } else { format!("Task {id} is given to {other}.") });
                }
                task.status = Status::Taken;
                task.assignee = Some(who.to_string());
                let reply = format!("Task {id} is yours: {}. {}", task.title, task.details);
                self.save();
                Ok(reply)
            }
            "finish_task" => {
                let id = id()?;
                let notes = text("notes").unwrap_or_default();
                let task = self.tasks.iter_mut().find(|t| t.id == id).ok_or_else(|| format!("There is no task {id}."))?;
                if task.assignee.as_ref().is_some_and(|a| a != who) {
                    return Err(format!("Task {id} belongs to {}.", task.assignee.clone().unwrap_or_default()));
                }
                task.status = Status::Done;
                task.assignee = Some(who.to_string());
                task.notes = notes;
                self.save();
                Ok(format!("Task {id} is marked done."))
            }
            "add_task" => {
                let title = text("title").filter(|t| !t.trim().is_empty()).ok_or("Give the task a title.")?;
                let assignee = match text("assignee").filter(|a| !a.trim().is_empty()) {
                    Some(name) => Some(self.resolve(&name).ok_or_else(|| format!("There is no session called {name}."))?),
                    None => None,
                };
                let id = self.add_task(who, &title, &text("details").unwrap_or_default(), assignee);
                Ok(format!("Added task {id}."))
            }
            "claim_paths" => self.claim_paths(who, &paths()),
            "release_paths" => Ok(self.release_paths(who, &paths())),
            "leave_note" => {
                let to = text("to").filter(|t| !t.trim().is_empty()).ok_or("Say who the note is to.")?;
                let body = text("text").filter(|t| !t.trim().is_empty()).ok_or("Give the note's text.")?;
                let to = if to.eq_ignore_ascii_case("all") || to.eq_ignore_ascii_case("everyone") { "all".to_string() } else { self.resolve(&to).ok_or_else(|| format!("There is no session called {to}."))? };
                self.notes.push(Note { from: who.to_string(), to: to.clone(), text: body, read_by: Vec::new() });
                let line = crate::i18n::tf!("{who} left a note for {to}.", who = self.label(who), to = to);
                self.outbox.entry(to.clone()).or_default().push(line);
                self.save();
                Ok(format!("Note left for {to}."))
            }
            "read_notes" => {
                let mut mine = Vec::new();
                for note in &mut self.notes {
                    if note.is_for(who) && !note.read_by.iter().any(|r| r == who) {
                        note.read_by.push(who.to_string());
                        mine.push(json!({ "from": note.from, "text": note.text }));
                    }
                }
                self.save();
                Ok(serde_json::to_string_pretty(&json!({ "notes": mine })).unwrap_or_default())
            }
            other => Err(format!("Limoniter doesn't have a tool called {other}")),
        }
    }
}

impl Default for Team {
    fn default() -> Self {
        Team::new()
    }
}

/// A path with `.` and `..` worked out, and no trailing slash, without asking the disk.
pub fn normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The part of a command that builds or tests: its words, up to the first ones that don't look like arguments of
/// it. A command may be several joined by `&&`, `;` or `|`; any of them counts.
pub fn build_part(command: &str, patterns: &[String]) -> Option<String> {
    for segment in command.split(['\n', ';', '|', '&']) {
        let mut words: Vec<&str> = segment.split_whitespace().collect();
        // Settings put before a command (`CARGO_BUILD_JOBS=4 cargo test`), and `time` or `sudo`.
        while let Some(first) = words.first() {
            if (first.contains('=') && !first.starts_with('-')) || matches!(*first, "time" | "sudo" | "nice") {
                words.remove(0);
            } else {
                break;
            }
        }
        if words.is_empty() {
            continue;
        }
        let program = words[0].trim_start_matches("./").trim_end_matches(".exe").trim_end_matches(".cmd").trim_end_matches(".bat");
        let program = program.rsplit(['/', '\\']).next().unwrap_or(program);
        let mut normalised = vec![program];
        normalised.extend(words[1..].iter().copied());
        for pattern in patterns {
            let wanted: Vec<&str> = pattern.split_whitespace().collect();
            if !wanted.is_empty() && normalised.len() >= wanted.len() && normalised.iter().zip(&wanted).all(|(a, b)| a == b) {
                return Some(normalised.iter().take(wanted.len()).copied().collect::<Vec<_>>().join(" "));
            }
        }
    }
    None
}

// ---------- What a command changed ----------

/// What a file looked like: its size and when it was last changed.
pub type Prints = HashMap<PathBuf, (u64, Option<SystemTime>)>;

/// The most files looked at, so a huge tree can't make a command's start or end slow.
const MOST_FILES: usize = 20_000;

/// Notes every file under these paths (they may be files or folders), skipping the folders nobody edits by hand.
pub fn fingerprint(paths: &[PathBuf]) -> Prints {
    let mut prints = Prints::new();
    let mut todo: Vec<PathBuf> = paths.to_vec();
    while let Some(path) = todo.pop() {
        if prints.len() >= MOST_FILES {
            break;
        }
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if meta.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    if !matches!(name.to_string_lossy().as_ref(), ".git" | "target" | "node_modules" | ".limoniter") {
                        todo.push(entry.path());
                    }
                }
            }
        } else {
            prints.insert(path, (meta.len(), meta.modified().ok()));
        }
    }
    prints
}

/// The files that are new, gone or different.
pub fn changed_between(before: &Prints, after: &Prints) -> Vec<PathBuf> {
    let mut changed: Vec<PathBuf> = after.iter().filter(|(p, v)| before.get(*p) != Some(*v)).map(|(p, _)| p.clone()).chain(before.keys().filter(|p| !after.contains_key(*p)).cloned()).collect();
    changed.sort();
    changed
}

// ---------- The one shared team ----------

static TEAM: LazyLock<Mutex<Team>> = LazyLock::new(|| Mutex::new(Team::new()));

/// The team, to look at or change. (A thread that panicked with it held doesn't stop the others.)
pub fn team() -> MutexGuard<'static, Team> {
    TEAM.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the agents are offered over MCP, as `tools/list` gives it.
pub fn tools() -> Value {
    let object = |properties: Value, required: &[&str]| json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false });
    let tool = |name: &str, description: &str, schema: Value, read_only: bool| json!({ "name": name, "description": description, "inputSchema": schema, "annotations": { "readOnlyHint": read_only, "destructiveHint": false, "openWorldHint": false } });
    json!([
        tool("list_tasks", "List the tasks on the shared task board (who has which, and what is done). Several agents and the person share this board.", object(json!({ "status": { "type": "string", "enum": ["open", "taken", "done"] } }), &[]), true),
        tool("claim_task", "Take a task from the board for yourself, so the others leave it alone.", object(json!({ "id": { "type": "integer" } }), &["id"]), false),
        tool("finish_task", "Mark your task done, with notes on what you did for whoever reads the board next.", object(json!({ "id": { "type": "integer" }, "notes": { "type": "string" } }), &["id"]), false),
        tool("add_task", "Add a task to the board, optionally giving it to a session by name (for example \"Codex 1\").", object(json!({ "title": { "type": "string" }, "details": { "type": "string" }, "assignee": { "type": "string" } }), &["title"]), false),
        tool("claim_paths", "Claim files or folders (relative to the project) as yours. Another agent's edits there are refused until you release them or your session ends.", object(json!({ "paths": { "type": "array", "items": { "type": "string" } } }), &["paths"]), false),
        tool("release_paths", "Let go of files or folders you claimed.", object(json!({ "paths": { "type": "array", "items": { "type": "string" } } }), &["paths"]), false),
        tool("leave_note", "Leave a note for another session (by name, or a tool's name such as Codex, or all) or for the person (to: You).", object(json!({ "to": { "type": "string" }, "text": { "type": "string" } }), &["to", "text"]), false),
        tool("read_notes", "Read the notes left for you that you haven't read yet.", object(json!({}), &[]), false),
    ])
}

/// Whether a tool name is one of the board's.
pub fn is_team_tool(name: &str) -> bool {
    matches!(name, "list_tasks" | "claim_task" | "finish_task" | "add_task" | "claim_paths" | "release_paths" | "leave_note" | "read_notes")
}

/// The name of whoever has claimed this path, as the Explorer shows it, if anyone has.
pub fn holder_name(path: &Path) -> Option<String> {
    let team = team();
    if team.claims.is_empty() {
        return None;
    }
    team.holder_of(path).map(|key| if key == PERSON { crate::i18n::tr("You").to_string() } else { key })
}

/// What a session is asked when it is to split a goal into tasks.
pub fn planner_prompt(goal: &str) -> String {
    crate::i18n::tf!("You are the planner for this project. The goal: {goal}\n\nLook at the project if you need to. Then split the goal into small tasks that can be done separately, perhaps by different agents, and add each to the shared task board with the add_task tool (a title, details, and the files or folders it will be in). Don't change any files yourself. When you've added them, say in a sentence what you added.", goal = goal.trim())
}

/// What the MCP server does for one of the board's tools; `who` is the session's name from the request.
pub fn call(who: Option<&str>, tool: &str, args: &Value) -> (String, bool) {
    team().call(who.filter(|w| !w.is_empty()).unwrap_or("Claude Code (terminal)"), tool, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("limoniter-team-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn team_with(folder: &Path) -> Team {
        let mut team = Team::new();
        team.set_folder(folder);
        team.add_member("Claude Code 1", "Claude Code", "Parser");
        team.add_member("Codex 1", "Codex", "Writer");
        team
    }

    #[test]
    fn a_claim_covers_what_is_inside_it_and_refuses_the_others() {
        let folder = dir("claims");
        let mut team = team_with(&folder);
        team.claim_paths("Claude Code 1", &["src/parser/".into()]).unwrap();
        assert_eq!(team.holder_other_than("Codex 1", &folder.join("src/parser/lex.rs")).as_deref(), Some("Claude Code 1"));
        assert_eq!(team.holder_other_than("Claude Code 1", &folder.join("src/parser/lex.rs")), None, "its own is its own");
        assert_eq!(team.holder_other_than("Codex 1", &folder.join("src/other.rs")), None);
        assert_eq!(team.holder_other_than("Codex 1", &folder.join("src/parser/../other.rs")), None, "dots are worked out");
        let refused = team.claim_paths("Codex 1", &["src".into()]).unwrap_err();
        assert!(refused.contains("Claude Code 1 (Parser)"), "{refused}");
        assert!(team.claim_paths("Codex 1", &["src/writer".into(), "docs".into()]).is_ok());
        let words = team.refusal("Claude Code 1", &folder.join("src/parser/lex.rs"));
        // Named as the system writes paths (src\\parser\\lex.rs on Windows).
        let named = Path::new("src").join("parser").join("lex.rs").display().to_string();
        assert!(words.contains(&named) && words.contains("leave_note to Claude Code 1"), "{words}");
    }

    #[test]
    fn claims_are_let_go_by_hand_and_when_the_session_ends() {
        let folder = dir("release");
        let mut team = team_with(&folder);
        team.claim_paths("Codex 1", &["a.txt".into(), "b".into()]).unwrap();
        team.release_paths("Codex 1", &["a.txt".into()]);
        assert_eq!(team.claims.len(), 1);
        team.remove_member("Codex 1");
        assert!(team.claims.is_empty());
        team.claim_paths(PERSON, &["b".into()]).unwrap();
        assert!(team.refusal(PERSON, &folder.join("b")).contains("The person has kept it"));
    }

    #[test]
    fn builds_go_one_at_a_time_in_order_and_one_can_jump() {
        let folder = dir("queue");
        let mut team = team_with(&folder);
        team.add_member("Codex 2", "Codex", "Third");
        assert_eq!(team.build_request("Codex 1", "CARGO_BUILD_JOBS=4 cargo test --release"), Turn::Go);
        let Turn::Wait(second) = team.build_request("Claude Code 1", "cargo build") else { panic!("should wait") };
        let Turn::Wait(third) = team.build_request("Codex 2", "make") else { panic!("should wait") };
        assert!(!team.is_its_turn(second));
        assert_eq!(team.take_notices("Claude Code 1"), vec!["Waiting for Codex 1 (Writer)'s cargo test".to_string()]);
        assert!(team.take_notices("Codex 1")[0].contains("Claude Code 1 (Parser) is waiting to run cargo build"));
        team.jump(third);
        team.build_done("Codex 1");
        assert!(team.is_its_turn(third), "the one that jumped goes first");
        assert!(!team.is_its_turn(second));
        team.build_done("Codex 2");
        assert!(team.is_its_turn(second));
        assert_eq!(team.queue_rows().len(), 1);
        team.remove_member("Claude Code 1");
        assert!(team.queue_rows().is_empty(), "a session that ends frees the queue");
    }

    #[test]
    fn a_session_may_run_two_of_its_own_builds_together() {
        let mut team = Team::new();
        assert_eq!(team.build_request("a", "cargo build"), Turn::Go);
        assert_eq!(team.build_request("a", "cargo test"), Turn::Go);
    }

    #[test]
    fn builds_are_told_from_other_commands() {
        let patterns: Vec<String> = DEFAULT_BUILD_COMMANDS.iter().map(|s| s.to_string()).collect();
        for yes in ["cargo test", "cargo.exe build --release", "cd app && npm run build", "FOO=1 pytest -x tests", "time make -j4", "git status; go test ./...", "./gradlew build", "python -m pytest"] {
            assert!(build_part(yes, &patterns).is_some(), "{yes}");
        }
        for no in ["ls -la", "cargo fmt", "git commit -m 'cargo test'", "echo cargo test", "npm install", "cat Makefile"] {
            assert!(build_part(no, &patterns).is_none(), "{no}");
        }
        assert_eq!(build_part("cd x && cargo test --release", &patterns).as_deref(), Some("cargo test"));
        assert!(build_part("./build.sh", &["build.sh".to_string()]).is_some(), "the person's own list");
    }

    #[test]
    fn the_board_is_kept_in_the_project_and_read_back() {
        let folder = dir("board");
        let mut team = team_with(&folder);
        let (reply, failed) = team.call("Claude Code 1", "add_task", &json!({ "title": "Write the lexer", "details": "in src/parser", "assignee": "codex" }));
        assert!(!failed, "{reply}");
        team.call("Claude Code 1", "add_task", &json!({ "title": "Second" }));
        let (_, failed) = team.call("Codex 1", "claim_task", &json!({ "id": 1 }));
        assert!(!failed);
        let (taken, failed) = team.call("Claude Code 1", "claim_task", &json!({ "id": 1 }));
        assert!(failed && taken.contains("Codex 1"), "{taken}");
        team.call("Codex 1", "finish_task", &json!({ "id": 1, "notes": "done in two files" }));
        team.set_build_commands(vec!["cargo test".into(), "just check".into()]);
        assert!(Team::board_file(&folder).exists());

        let mut again = Team::new();
        again.add_member("Codex 1", "Codex", "Writer");
        again.set_folder(&folder);
        assert_eq!(again.tasks.len(), 2);
        assert_eq!(again.tasks[0].status, Status::Done);
        assert_eq!(again.tasks[0].notes, "done in two files");
        assert_eq!(again.build_commands(), vec!["cargo test".to_string(), "just check".to_string()]);
        let (list, _) = again.call("Codex 1", "list_tasks", &json!({ "status": "open" }));
        assert!(list.contains("Second") && !list.contains("Write the lexer"), "{list}");
    }

    #[test]
    fn a_task_given_to_a_session_that_has_gone_is_open_again() {
        let folder = dir("gone");
        let mut team = team_with(&folder);
        team.add_task(PERSON, "Do it", "", Some("Codex 1".into()));
        team.call("Codex 1", "claim_task", &json!({ "id": 1 }));
        team.remove_member("Codex 1");
        assert_eq!((team.tasks[0].status, team.tasks[0].assignee.clone()), (Status::Open, None));
        // A restart: nobody is there to hold what the file says is taken.
        team.add_member("Codex 1", "Codex", "");
        team.call("Codex 1", "claim_task", &json!({ "id": 1 }));
        let mut fresh = Team::new();
        fresh.set_folder(&folder);
        assert_eq!(fresh.tasks[0].status, Status::Open);
    }

    #[test]
    fn notes_go_to_a_session_a_tool_or_everyone_and_are_read_once() {
        let folder = dir("notes");
        let mut team = team_with(&folder);
        team.call("Claude Code 1", "leave_note", &json!({ "to": "Codex", "text": "the parser is yours to call" }));
        team.call("Codex 1", "leave_note", &json!({ "to": "you", "text": "tests pass" }));
        team.call("Codex 1", "leave_note", &json!({ "to": "all", "text": "lunch" }));
        let (first, _) = team.call("Codex 1", "read_notes", &json!({}));
        assert!(first.contains("the parser is yours") && first.contains("lunch") && !first.contains("tests pass"), "{first}");
        let (again, _) = team.call("Codex 1", "read_notes", &json!({}));
        assert!(!again.contains("lunch"), "{again}");
        let (other, _) = team.call("Claude Code 1", "read_notes", &json!({}));
        assert!(other.contains("lunch") && !other.contains("the parser"), "{other}");
        assert!(team.call("Codex 1", "leave_note", &json!({ "to": "nobody", "text": "x" })).1);
        assert!(team.notes.iter().any(|n| n.to == PERSON && n.text == "tests pass"));
        assert!(team.take_notices("Codex 1")[0].contains("left a note"));
    }

    #[test]
    fn a_command_that_changes_what_another_holds_is_reported_to_both() {
        let folder = dir("report");
        std::fs::create_dir_all(folder.join("src")).unwrap();
        std::fs::write(folder.join("src/a.txt"), "one").unwrap();
        let mut team = team_with(&folder);
        team.claim_paths("Claude Code 1", &["src".into()]).unwrap();
        let held: Vec<PathBuf> = team.others_claims("Codex 1").into_iter().map(|c| c.path).collect();
        let before = fingerprint(&held);
        std::fs::write(folder.join("src/a.txt"), "one two").unwrap();
        std::fs::write(folder.join("src/new.txt"), "x").unwrap();
        let changed = changed_between(&before, &fingerprint(&held));
        assert_eq!(changed.len(), 2);
        let lines = team.report_changes("Codex 1", &changed);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("Codex 1") && lines[0].contains("Claude Code 1"), "{:?}", lines);
        assert_eq!(team.take_notices("Claude Code 1").len(), 2, "the holder is told too");
        assert!(team.report_changes("Claude Code 1", &changed).is_empty(), "its own files are its own");
    }

    #[test]
    fn the_tools_listed_are_the_tools_handled() {
        for tool in tools().as_array().unwrap() {
            assert!(is_team_tool(tool["name"].as_str().unwrap()));
        }
        assert_eq!(tools().as_array().unwrap().len(), 8);
    }
}

#[cfg(test)]
#[path = "agent_team_tests.rs"]
mod agent_team_tests;
