//! An AI agent's session as the window sees it, whichever tool is behind it: the transcript, what the agent is
//! doing, what it has used, and what it's waiting to be allowed. Nothing here knows Claude Code's messages or
//! draws anything.
//!
//! A tool is plugged in by a *driver*: code that starts the tool, turns what it says into [`Event`]s, and does
//! what a [`Command`] asks. A driver gives a [`Handle`], the two ends of a pair of channels, and the session does
//! the rest. Claude Code's is in `agent_claude.rs`; Codex's will be another.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

/// Which tool a session runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Claude,
    Codex,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Claude => "Claude Code",
            Tool::Codex => "Codex",
        }
    }

    /// The modes in the order the tool's own settings list them.
    pub fn modes(self) -> [Mode; 4] {
        match self {
            Tool::Claude => Mode::ALL,
            Tool::Codex => [Mode::Plan, Mode::Ask, Mode::AcceptEdits, Mode::Bypass],
        }
    }
}

/// How much the agent may do without asking.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Asks before each edit and each command.
    #[default]
    Ask,
    /// Edits files without asking; still asks before commands.
    AcceptEdits,
    /// Only looks and plans; changes nothing.
    Plan,
    /// Asks nothing at all.
    Bypass,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Ask, Mode::AcceptEdits, Mode::Plan, Mode::Bypass];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Ask => "Ask before edits",
            Mode::AcceptEdits => "Accept edits",
            Mode::Plan => "Plan only",
            Mode::Bypass => "Bypass permissions",
        }
    }

    /// What the tool itself calls the mode. Codex has three levels of its own (read only, workspace, full access)
    /// and a way of asking, which these four are made from.
    pub fn label_for(self, tool: Tool) -> &'static str {
        match (tool, self) {
            (Tool::Codex, Mode::Plan) => "Read only",
            (Tool::Codex, Mode::Ask) => "Ask before changes (untrusted)",
            (Tool::Codex, Mode::AcceptEdits) => "Workspace write",
            (Tool::Codex, Mode::Bypass) => "Full access",
            _ => self.label(),
        }
    }

    pub fn explain_for(self, tool: Tool) -> &'static str {
        match (tool, self) {
            (Tool::Codex, Mode::Plan) => "Codex can read files and run safe commands, and can't change anything in this folder.",
            (Tool::Codex, Mode::Ask) => "Codex works inside this folder, and asks before each edit and each command it can't be sure is harmless.",
            (Tool::Codex, Mode::AcceptEdits) => "Codex edits files in this folder without asking, and asks before a command that needs more than that.",
            (Tool::Codex, Mode::Bypass) => "Warning: Codex may change or delete any file and run any command, without asking you. Only use this in a folder you could lose.",
            _ => self.explain(),
        }
    }

    /// A line that says what the mode means, for under the choice.
    pub fn explain(self) -> &'static str {
        match self {
            Mode::Ask => "Each edit is shown to you first, and each command asked about.",
            Mode::AcceptEdits => "Edits to files go ahead without asking. Commands are still asked about.",
            Mode::Plan => "The agent reads and makes a plan, and changes nothing until you agree to the plan.",
            Mode::Bypass => "Warning: the agent may change or delete any file and run any command, without asking you. Only use this in a folder you could lose.",
        }
    }
}

/// A model the tool offers, and the efforts it can be asked to put in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelChoice {
    /// What's passed to the tool to choose it.
    pub value: String,
    pub name: String,
    pub efforts: Vec<String>,
}

/// The efforts a tool takes when it doesn't say which a model does.
pub const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// What the person tells the session to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Something for the agent to do. The driver is only sent one while it's idle.
    Prompt(String),
    /// Stops what it's doing now.
    Interrupt,
    /// A model by its `value`, or None for the tool's own choice.
    SetModel(Option<String>),
    SetEffort(String),
    SetMode(Mode),
    Answer { id: String, answer: Answer },
    /// Stops a subagent or task.
    StopTask(String),
    /// Asks how full the context is.
    AskContext,
    /// Asks what's left of the plan's limits.
    AskUsage,
    /// Ends the session.
    Close,
}

/// The person's answer to a request for permission.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// Allowed. `text` is the whole of the file as it was reviewed, when it differs from what was proposed;
    /// `for_session` also allows the same again for the rest of the session.
    Allow { text: Option<String>, for_session: bool },
    Refuse(String),
}

/// What an agent asks to do.
#[derive(Clone, Debug, PartialEq)]
pub enum PermissionKind {
    /// Change a file: the whole of it as it would be afterwards. It's shown for review.
    Edit { path: PathBuf, proposed: String },
    Command(String),
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Permission {
    pub id: String,
    /// The tool's own name for what it wants to use.
    pub tool: String,
    /// One line: "Edit src/main.rs", "Run cargo test".
    pub title: String,
    /// More: the command in full, or the plan.
    pub detail: String,
    pub kind: PermissionKind,
    /// Whether "Allow for this session" means anything for this one.
    pub can_remember: bool,
}

/// What a tool use did to the world, for the transcript's one line and for Files changing.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolKind {
    Command(String),
    FileChange(PathBuf),
    Other,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TaskState {
    #[default]
    Running,
    Done,
    Failed,
    Stopped,
}

/// A subagent or background task.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Task {
    pub id: String,
    pub description: String,
    /// What kind of helper: "general-purpose", or "command".
    pub kind: String,
    /// What it's doing now: the last tool it used.
    pub last_tool: Option<String>,
    pub tokens: u64,
    pub tool_uses: u32,
    pub state: TaskState,
}

/// One of the plan's limits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limit {
    /// How much is used, 0 to 100.
    pub percent: f32,
    /// When it resets, as seconds since 1970.
    pub resets: Option<i64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Limits {
    pub five_hour: Option<Limit>,
    pub week: Option<Limit>,
}

impl Limits {
    pub fn any(&self) -> bool {
        self.five_hour.is_some() || self.week.is_some()
    }

    /// Takes in what's newer, keeping what it doesn't mention.
    pub fn merge(&mut self, newer: Limits) {
        if newer.five_hour.is_some() {
            self.five_hour = newer.five_hour;
        }
        if newer.week.is_some() {
            self.week = newer.week;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    /// Of the input, how much was read back from a cache.
    pub cached: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Context {
    pub used: u64,
    pub max: u64,
}

impl Context {
    pub fn percent(&self) -> f32 {
        if self.max == 0 { 0.0 } else { (self.used as f32 / self.max as f32 * 100.0).clamp(0.0, 100.0) }
    }
}

/// What a driver tells the session.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The tool is up: the models it offers, and the mode it's in.
    Ready { models: Vec<ModelChoice>, mode: Mode },
    /// A conversation has begun (or been taken up again) under this name, with this model.
    Started { session_id: String, model: String, mode: Mode },
    /// All the text of one message from the agent so far. The same `id` again replaces it.
    Text { id: String, text: String, done: bool },
    /// More text of the message with this `id`.
    TextDelta { id: String, text: String },
    Thinking { id: String, text: String },
    /// The agent began something: a tool, a command, a change to a file. `parent` is the subagent's tool use it
    /// is in, if any.
    ToolUse { id: String, name: String, summary: String, kind: ToolKind, parent: Option<String> },
    ToolResult { id: String, text: String, is_error: bool },
    Permission(Permission),
    /// A request is no longer waiting (the tool took it back).
    PermissionGone { id: String },
    Task(Task),
    /// What's been used in the whole session so far, and what it cost if the tool says.
    Usage { tokens: Tokens, cost: Option<f64> },
    Context(Context),
    Limits(Limits),
    ModeChanged(Mode),
    ModelChanged(String),
    EffortChanged(String),
    Title(String),
    /// Something to tell the person, that isn't an error.
    Notice(String),
    Error(String),
    /// The agent has finished what it was asked (or was stopped).
    TurnFinished,
    /// The tool has gone, and with it the session.
    Finished(String),
}

/// The ends of the channels between a session and its driver.
pub struct Handle {
    pub commands: Sender<Command>,
    pub events: Receiver<Event>,
}

/// How the person's command went in the transcript.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decided {
    Waiting,
    Allowed { for_session: bool },
    Refused,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolResult {
    pub text: String,
    pub is_error: bool,
}

/// A line of the transcript.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    You(String),
    Text { id: String, text: String, done: bool },
    Thinking { id: String, text: String },
    Tool { id: String, name: String, summary: String, kind: ToolKind, parent: Option<String>, result: Option<ToolResult> },
    Permission { permission: Permission, decided: Decided },
    Notice(String),
    Error(String),
}

pub struct Session {
    pub tool: Tool,
    pub folder: PathBuf,
    /// What the tab says: the tool's own title for it, or the start of the first message.
    pub title: String,
    pub items: Vec<Item>,
    /// The conversation's name for the tool, which brings it back (`--resume`).
    pub session_id: Option<String>,
    pub model: Option<String>,
    /// The model as it was asked for (`opus`, `haiku`), which `model` names in full once it's running.
    pub chosen_model: Option<String>,
    pub effort: Option<String>,
    pub mode: Mode,
    pub models: Vec<ModelChoice>,
    pub tasks: Vec<Task>,
    pub tokens: Tokens,
    pub cost: Option<f64>,
    pub context: Option<Context>,
    pub limits: Limits,
    /// At work on something now.
    pub busy: bool,
    /// What was typed while it worked, to be sent as it's free.
    pub queue: Vec<String>,
    /// The tool has gone; why.
    pub finished: Option<String>,
    /// The text box's content.
    pub draft: String,
    commands: Sender<Command>,
    events: Receiver<Event>,
    /// Whether the title is the tool's, which the first message mustn't replace.
    titled: bool,
}

/// The most of a first message that makes a title.
const TITLE_LENGTH: usize = 40;

/// A short title from the first words of a message.
pub fn title_from(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    let mut title: String = line.chars().take(TITLE_LENGTH).collect();
    if line.chars().count() > TITLE_LENGTH {
        title.push('\u{2026}');
    }
    title
}

impl Session {
    pub fn new(tool: Tool, folder: &Path, handle: Handle) -> Self {
        Session {
            tool,
            folder: folder.to_path_buf(),
            title: crate::i18n::tr("New session").to_string(),
            items: Vec::new(),
            session_id: None,
            model: None,
            chosen_model: None,
            effort: None,
            mode: Mode::Ask,
            models: Vec::new(),
            tasks: Vec::new(),
            tokens: Tokens::default(),
            cost: None,
            context: None,
            limits: Limits::default(),
            busy: false,
            queue: Vec::new(),
            finished: None,
            draft: String::new(),
            commands: handle.commands,
            events: handle.events,
            titled: false,
        }
    }

    /// A session a driver isn't behind, which does nothing but show what it was given: a past one.
    pub fn without_driver(tool: Tool, folder: &Path) -> Self {
        let (commands, _) = channel();
        let (_, events) = channel();
        let mut session = Session::new(tool, folder, Handle { commands, events });
        session.finished = Some(String::new());
        session
    }

    pub fn send_command(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// What the person typed: sent now if the agent is free, otherwise kept to send as it is.
    pub fn say(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() || self.finished.is_some() {
            return;
        }
        if self.busy {
            self.queue.push(text.to_string());
        } else {
            self.prompt(text);
        }
    }

    fn prompt(&mut self, text: &str) {
        if !self.titled && !self.items.iter().any(|item| matches!(item, Item::You(_))) {
            self.title = title_from(text);
        }
        self.items.push(Item::You(text.to_string()));
        self.busy = true;
        self.send_command(Command::Prompt(text.to_string()));
    }

    /// Stops the agent's work. What was queued still goes when it has stopped.
    pub fn stop(&mut self) {
        if self.busy {
            self.send_command(Command::Interrupt);
        }
    }

    pub fn answer(&mut self, id: &str, answer: Answer) {
        for item in &mut self.items {
            if let Item::Permission { permission, decided } = item
                && permission.id == id
                && *decided == Decided::Waiting
            {
                *decided = match &answer {
                    Answer::Allow { for_session, .. } => Decided::Allowed { for_session: *for_session },
                    Answer::Refuse(_) => Decided::Refused,
                };
            }
        }
        self.send_command(Command::Answer { id: id.to_string(), answer });
    }

    /// The requests waiting for the person.
    pub fn waiting(&self) -> Vec<&Permission> {
        self.items
            .iter()
            .filter_map(|item| match item {
                Item::Permission { permission, decided: Decided::Waiting } => Some(permission),
                _ => None,
            })
            .collect()
    }

    pub fn set_model(&mut self, value: Option<&str>) {
        self.chosen_model = value.map(str::to_string);
        self.send_command(Command::SetModel(value.map(str::to_string)));
    }

    pub fn set_effort(&mut self, effort: &str) {
        self.send_command(Command::SetEffort(effort.to_string()));
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.send_command(Command::SetMode(mode));
    }

    pub fn stop_task(&mut self, id: &str) {
        self.send_command(Command::StopTask(id.to_string()));
    }

    /// The efforts the chosen model takes.
    pub fn efforts(&self) -> Vec<String> {
        let wanted = self.chosen_model.as_deref().or(self.model.as_deref());
        let found = self.models.iter().find(|m| Some(m.value.as_str()) == wanted || (wanted.is_none() && m.value == "default"));
        match found {
            Some(model) if !model.efforts.is_empty() => model.efforts.clone(),
            _ => EFFORTS.iter().map(|e| e.to_string()).collect(),
        }
    }

    /// Takes what the driver has said and acts on it. Returns the events, for the window to do what only it can
    /// (open a review, label a file as the agent's).
    pub fn pump(&mut self) -> Vec<Event> {
        let mut seen = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            self.apply(&event);
            seen.push(event);
        }
        seen
    }

    /// Does what one event says to the session's state.
    pub fn apply(&mut self, event: &Event) {
        match event {
            Event::Ready { models, mode } => {
                self.models.clone_from(models);
                self.mode = *mode;
            }
            Event::Started { session_id, model, mode } => {
                self.session_id = Some(session_id.clone());
                self.model = Some(model.clone());
                self.mode = *mode;
            }
            Event::Text { id, text, done } => {
                if let Some(Item::Text { text: old, done: was, .. }) = self.items.iter_mut().rev().find(|item| matches!(item, Item::Text { id: i, .. } if i == id)) {
                    old.clone_from(text);
                    *was = *done;
                } else {
                    self.items.push(Item::Text { id: id.clone(), text: text.clone(), done: *done });
                }
            }
            Event::TextDelta { id, text } => {
                if let Some(Item::Text { text: old, .. }) = self.items.iter_mut().rev().find(|item| matches!(item, Item::Text { id: i, .. } if i == id)) {
                    old.push_str(text);
                } else {
                    self.items.push(Item::Text { id: id.clone(), text: text.clone(), done: false });
                }
            }
            Event::Thinking { id, text } => {
                if let Some(Item::Thinking { text: old, .. }) = self.items.iter_mut().rev().find(|item| matches!(item, Item::Thinking { id: i, .. } if i == id)) {
                    old.clone_from(text);
                } else {
                    self.items.push(Item::Thinking { id: id.clone(), text: text.clone() });
                }
            }
            Event::ToolUse { id, name, summary, kind, parent } => {
                if !self.items.iter().any(|item| matches!(item, Item::Tool { id: i, .. } if i == id)) {
                    self.items.push(Item::Tool { id: id.clone(), name: name.clone(), summary: summary.clone(), kind: kind.clone(), parent: parent.clone(), result: None });
                }
            }
            Event::ToolResult { id, text, is_error } => {
                if let Some(Item::Tool { result, .. }) = self.items.iter_mut().rev().find(|item| matches!(item, Item::Tool { id: i, .. } if i == id)) {
                    *result = Some(ToolResult { text: text.clone(), is_error: *is_error });
                }
            }
            Event::Permission(permission) => {
                if !self.items.iter().any(|item| matches!(item, Item::Permission { permission: p, .. } if p.id == permission.id)) {
                    self.items.push(Item::Permission { permission: permission.clone(), decided: Decided::Waiting });
                }
            }
            Event::PermissionGone { id } => {
                for item in &mut self.items {
                    if let Item::Permission { permission, decided } = item
                        && permission.id == *id
                        && *decided == Decided::Waiting
                    {
                        *decided = Decided::Refused;
                    }
                }
            }
            Event::Task(task) => match self.tasks.iter_mut().find(|t| t.id == task.id) {
                // An update may say only what changed: what it leaves empty is kept, and the counts only grow.
                Some(old) => {
                    if !task.description.is_empty() {
                        old.description.clone_from(&task.description);
                    }
                    if !task.kind.is_empty() {
                        old.kind.clone_from(&task.kind);
                    }
                    if task.last_tool.is_some() {
                        old.last_tool.clone_from(&task.last_tool);
                    }
                    old.tokens = old.tokens.max(task.tokens);
                    old.tool_uses = old.tool_uses.max(task.tool_uses);
                    old.state = task.state;
                }
                None => self.tasks.push(task.clone()),
            },
            Event::Usage { tokens, cost } => {
                self.tokens = *tokens;
                if cost.is_some() {
                    self.cost = *cost;
                }
            }
            Event::Context(context) => self.context = Some(*context),
            Event::Limits(limits) => self.limits.merge(*limits),
            Event::ModeChanged(mode) => self.mode = *mode,
            Event::ModelChanged(model) => self.model = Some(model.clone()),
            Event::EffortChanged(effort) => self.effort = Some(effort.clone()),
            Event::Title(title) => {
                if !title.trim().is_empty() {
                    self.title.clone_from(title);
                    self.titled = true;
                }
            }
            Event::Notice(text) => self.items.push(Item::Notice(text.clone())),
            Event::Error(text) => self.items.push(Item::Error(text.clone())),
            Event::TurnFinished => {
                self.busy = false;
                if self.queue.is_empty() {
                    self.ask_after_turn();
                } else {
                    let next = self.queue.remove(0);
                    self.ask_after_turn();
                    self.prompt(&next);
                }
            }
            Event::Finished(why) => {
                self.busy = false;
                self.finished = Some(why.clone());
                for item in &mut self.items {
                    if let Item::Permission { decided, .. } = item
                        && *decided == Decided::Waiting
                    {
                        *decided = Decided::Refused;
                    }
                }
            }
        }
    }

    /// After each turn: how full the context is, and what's left of the plan.
    fn ask_after_turn(&mut self) {
        self.send_command(Command::AskContext);
        self.send_command(Command::AskUsage);
    }

    /// The tasks still running.
    pub fn running_tasks(&self) -> Vec<&Task> {
        self.tasks.iter().filter(|task| task.state == TaskState::Running).collect()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Close);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> (Session, Sender<Event>, Receiver<Command>) {
        let (commands, command_rx) = channel();
        let (event_tx, events) = channel();
        (Session::new(Tool::Claude, Path::new("/p"), Handle { commands, events }), event_tx, command_rx)
    }

    fn commands(rx: &Receiver<Command>) -> Vec<Command> {
        rx.try_iter().collect()
    }

    #[test]
    fn a_message_while_busy_is_queued_and_sent_when_the_turn_ends() {
        let (mut s, events, rx) = session();
        s.say("first");
        s.say("second");
        assert_eq!(s.queue, vec!["second".to_string()]);
        assert_eq!(commands(&rx), vec![Command::Prompt("first".into())]);
        events.send(Event::TurnFinished).unwrap();
        s.pump();
        assert!(s.queue.is_empty());
        assert!(s.busy, "the queued one is now being worked on");
        assert_eq!(commands(&rx), vec![Command::AskContext, Command::AskUsage, Command::Prompt("second".into())]);
        assert_eq!(s.items, vec![Item::You("first".into()), Item::You("second".into())]);
    }

    #[test]
    fn the_first_message_makes_the_title_unless_the_tool_gave_one() {
        let (mut s, events, _rx) = session();
        s.say("Fix the bug in the parser, which loses the last line of every file it reads");
        assert_eq!(s.title, "Fix the bug in the parser, which loses t\u{2026}");
        events.send(Event::Title("Parser bug".into())).unwrap();
        s.pump();
        assert_eq!(s.title, "Parser bug");
        let (mut t, events, _rx) = session();
        events.send(Event::Title("Named first".into())).unwrap();
        t.pump();
        t.say("something else");
        assert_eq!(t.title, "Named first");
    }

    #[test]
    fn streamed_text_grows_and_the_whole_message_replaces_it() {
        let (mut s, _e, _rx) = session();
        s.apply(&Event::TextDelta { id: "m1".into(), text: "Hel".into() });
        s.apply(&Event::TextDelta { id: "m1".into(), text: "lo".into() });
        assert_eq!(s.items, vec![Item::Text { id: "m1".into(), text: "Hello".into(), done: false }]);
        s.apply(&Event::Text { id: "m1".into(), text: "Hello there".into(), done: true });
        assert_eq!(s.items, vec![Item::Text { id: "m1".into(), text: "Hello there".into(), done: true }]);
    }

    #[test]
    fn a_tool_use_gets_its_result_once() {
        let (mut s, _e, _rx) = session();
        let use_ = Event::ToolUse { id: "t".into(), name: "Read".into(), summary: "Read a.rs".into(), kind: ToolKind::Other, parent: None };
        s.apply(&use_);
        s.apply(&use_);
        s.apply(&Event::ToolResult { id: "t".into(), text: "ok".into(), is_error: false });
        assert_eq!(s.items.len(), 1);
        assert!(matches!(&s.items[0], Item::Tool { result: Some(ToolResult { text, .. }), .. } if text == "ok"));
    }

    #[test]
    fn answering_marks_the_request_and_tells_the_driver() {
        let (mut s, _e, rx) = session();
        let permission = Permission { id: "p1".into(), tool: "Bash".into(), title: "Run ls".into(), detail: String::new(), kind: PermissionKind::Command("ls".into()), can_remember: true };
        s.apply(&Event::Permission(permission));
        assert_eq!(s.waiting().len(), 1);
        s.answer("p1", Answer::Allow { text: None, for_session: true });
        assert!(s.waiting().is_empty());
        assert!(matches!(&s.items[0], Item::Permission { decided: Decided::Allowed { for_session: true }, .. }));
        assert_eq!(commands(&rx), vec![Command::Answer { id: "p1".into(), answer: Answer::Allow { text: None, for_session: true } }]);
    }

    #[test]
    fn tasks_update_in_place_and_the_running_ones_are_counted() {
        let (mut s, _e, _rx) = session();
        s.apply(&Event::Task(Task { id: "a".into(), description: "Look".into(), ..Task::default() }));
        s.apply(&Event::Task(Task { id: "a".into(), description: "Look".into(), tool_uses: 3, tokens: 900, last_tool: Some("Read".into()), ..Task::default() }));
        assert_eq!(s.tasks.len(), 1);
        assert_eq!(s.running_tasks()[0].tool_uses, 3);
        s.apply(&Event::Task(Task { id: "a".into(), state: TaskState::Done, ..Task::default() }));
        assert!(s.running_tasks().is_empty());
    }

    #[test]
    fn limits_merge_and_keep_what_the_newer_leaves_out() {
        let mut limits = Limits { five_hour: Some(Limit { percent: 40.0, resets: Some(1) }), week: None };
        limits.merge(Limits { five_hour: None, week: Some(Limit { percent: 10.0, resets: None }) });
        assert_eq!(limits.five_hour.unwrap().percent, 40.0);
        assert_eq!(limits.week.unwrap().percent, 10.0);
    }

    #[test]
    fn the_tool_going_refuses_what_was_waiting_and_stops_further_messages() {
        let (mut s, _e, rx) = session();
        let permission = Permission { id: "p1".into(), tool: "Bash".into(), title: "x".into(), detail: String::new(), kind: PermissionKind::Other, can_remember: false };
        s.apply(&Event::Permission(permission));
        s.apply(&Event::Finished("exited".into()));
        assert!(s.waiting().is_empty());
        s.say("hello");
        assert!(commands(&rx).is_empty());
        assert!(s.items.iter().all(|item| !matches!(item, Item::You(_))));
    }

    #[test]
    fn efforts_follow_the_chosen_model() {
        let (mut s, _e, _rx) = session();
        s.models = vec![
            ModelChoice { value: "default".into(), name: "Default".into(), efforts: vec!["low".into(), "high".into()] },
            ModelChoice { value: "old".into(), name: "Old".into(), efforts: vec![] },
        ];
        assert_eq!(s.efforts(), vec!["low", "high"], "the default model's");
        s.chosen_model = Some("old".into());
        assert_eq!(s.efforts().len(), 5, "a model that doesn't say takes the usual");
    }

    #[test]
    fn context_percentages_are_kept_in_range() {
        assert_eq!(Context { used: 250, max: 1000 }.percent(), 25.0);
        assert_eq!(Context { used: 5, max: 0 }.percent(), 0.0);
        assert_eq!(Context { used: 2000, max: 1000 }.percent(), 100.0);
    }
}
