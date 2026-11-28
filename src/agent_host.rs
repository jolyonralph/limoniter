//! The window's side of the AI agents panel (View, Panels, AI agents; Tools): the sessions as tabs in a side panel,
//! starting one (a box to choose how), taking a past one up again, and what only the window can do for a session:
//! show an edit it asks to make as a review to accept or reject, and mark the files it changes as its own.

use super::*;
use crate::agent_logs::{self, Past};
use crate::agent_panel::{self, Act, Tab};
use crate::agent_session::{Answer, Event, Mode, ModelChoice, PermissionKind, Session, Tool, ToolKind};
use crate::review::Decision;
use std::sync::mpsc::Receiver;

/// How the person wants a new session.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Choice {
    pub mode: Mode,
    /// A model's `value`, "default" for the tool's own choice.
    pub model: String,
    /// An effort, or empty for the tool's own.
    pub effort: String,
}

impl Default for Choice {
    fn default() -> Self {
        Choice { mode: Mode::Ask, model: "default".into(), effort: String::new() }
    }
}

/// A change a session asked to make, shown for review while it waits.
struct OpenReview {
    /// The session's tab number.
    session: u64,
    request: String,
    tab: u64,
    proposed: String,
    path: PathBuf,
    decisions: Receiver<Decision>,
}

/// The project's files, for `@` to name, found on a thread of their own.
type FileList = Arc<Mutex<Option<(PathBuf, Arc<Vec<String>>)>>>;

pub(super) struct State {
    pub tabs: Vec<Tab>,
    pub active: usize,
    next_number: u64,
    /// The box for a new session is open, with these choices.
    pub dialog: Option<Choice>,
    /// What's been chosen before, offered again.
    last: Choice,
    /// The models the tool has said it has, kept for the next box.
    models: Vec<ModelChoice>,
    probing: Option<Receiver<Result<Vec<ModelChoice>, String>>>,
    probed: bool,
    reviews: Vec<OpenReview>,
    /// Past sessions of the project, listed when asked for.
    past: Option<(PathBuf, Vec<Past>)>,
    showing_past: bool,
    files: FileList,
    scanning: Option<PathBuf>,
    /// The program that is Claude Code, and what goes before its own arguments. Tests put a stand-in here.
    pub program: (String, Vec<String>),
}

impl Default for State {
    fn default() -> Self {
        State {
            tabs: Vec::new(),
            active: 0,
            next_number: 1,
            dialog: None,
            last: Choice::default(),
            models: Vec::new(),
            probing: None,
            probed: false,
            reviews: Vec::new(),
            past: None,
            showing_past: false,
            files: Arc::new(Mutex::new(None)),
            scanning: None,
            program: ("claude".to_string(), Vec::new()),
        }
    }
}

/// The folders not looked into for `@` names.
const SKIPPED: [&str; 6] = [".git", "target", "node_modules", ".hg", ".svn", "__pycache__"];
const MOST_FILES: usize = 20_000;

/// The files under a folder as paths relative to it, for naming in a message.
pub(super) fn project_files(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                if !name.starts_with('.') && !SKIPPED.contains(&name.as_str()) {
                    stack.push(entry.path());
                }
            } else if kind.is_file()
                && let Ok(relative) = entry.path().strip_prefix(root)
            {
                found.push(relative.display().to_string().replace('\\', "/"));
                if found.len() >= MOST_FILES {
                    return found;
                }
            }
        }
    }
    found
}

impl Limoniter {
    /// The folder a session works in: the project's, or the folder of the file in front.
    pub(super) fn agent_folder(&self) -> Option<PathBuf> {
        self.project.as_ref().map(|p| p.folder()).or_else(|| self.docs.get(self.active).and_then(|d| d.path.as_deref()).and_then(Path::parent).map(Path::to_path_buf)).or_else(|| std::env::current_dir().ok())
    }

    /// Opens the box for a new session.
    pub(super) fn agent_new_session_box(&mut self) {
        self.panels.agents = true;
        self.agents.dialog = Some(self.agents.last.clone());
    }

    /// Starts a session on the program, in `folder`, and puts it in front.
    pub(super) fn agent_start(&mut self, ctx: &Context, folder: &Path, choice: &Choice, resume: Option<&Past>) -> Result<(), String> {
        let (program, prefix) = self.agents.program.clone();
        let wake = ctx.clone();
        // Tried with Claude Code 2.1.332 and Limoniter's own IDE server: a `claude -p` session didn't connect to it,
        // with `--ide`, the port in CLAUDE_CODE_SSE_PORT and a matching lock file, so no port is passed (see NOTES).
        let ide_port = None;
        let options = crate::agent_claude::Options {
            program,
            prefix,
            folder: folder.to_path_buf(),
            mode: choice.mode,
            model: Some(choice.model.clone()).filter(|m| m != "default"),
            effort: Some(choice.effort.clone()).filter(|e| !e.is_empty()),
            resume: resume.map(|p| p.id.clone()),
            ide_port,
            wake: Arc::new(move || wake.request_repaint()),
        };
        let handle = crate::agent_claude::start(options)?;
        let mut session = Session::new(Tool::Claude, folder, handle);
        session.mode = choice.mode;
        session.chosen_model = Some(choice.model.clone());
        session.effort = Some(choice.effort.clone()).filter(|e| !e.is_empty());
        session.models.clone_from(&self.agents.models);
        if let Some(past) = resume {
            session.items = agent_logs::transcript(&past.path, folder);
            session.title = past.title.clone();
            session.session_id = Some(past.id.clone());
        }
        let number = self.agents.next_number;
        self.agents.next_number += 1;
        self.agents.tabs.push(Tab::new(session, number));
        self.agents.active = self.agents.tabs.len() - 1;
        self.agents.last = choice.clone();
        self.panels.agents = true;
        Ok(())
    }

    /// Closes a session's tab: its reviews go, and the program with it.
    pub(super) fn agent_close(&mut self, index: usize) {
        if index >= self.agents.tabs.len() {
            return;
        }
        let tab = self.agents.tabs.remove(index);
        let reviews: Vec<u64> = self.agents.reviews.iter().filter(|r| r.session == tab.number).map(|r| r.tab).collect();
        self.agents.reviews.retain(|r| r.session != tab.number);
        for review in reviews {
            self.close_review(review);
        }
        if self.agents.active >= self.agents.tabs.len() {
            self.agents.active = self.agents.tabs.len().saturating_sub(1);
        }
    }

    /// The name Files changing gives a session's changes.
    fn agent_name(session: &Session) -> String {
        if session.title.is_empty() { session.tool.name().to_string() } else { format!("{}: {}", session.tool.name(), session.title) }
    }

    /// Everything the window does for the sessions, each time round.
    pub(super) fn agents_tick(&mut self, ctx: &Context) {
        if let Some(probing) = &self.agents.probing {
            match probing.try_recv() {
                Ok(result) => {
                    self.agents.probing = None;
                    if let Ok(models) = result
                        && !models.is_empty()
                    {
                        self.agents.models = models;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.agents.probing = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        let mut asked = Vec::new();
        let mut known = None;
        for tab in &mut self.agents.tabs {
            let name = Self::agent_name(&tab.session);
            let folder = tab.session.folder.clone();
            for event in tab.session.pump() {
                match event {
                    Event::Permission(permission) => {
                        match &permission.kind {
                            PermissionKind::Edit { path, .. } => crate::files_changing::claim(path, &name),
                            PermissionKind::Command(_) => crate::files_changing::claim(&folder, &name),
                            PermissionKind::Other => {}
                        }
                        asked.push((tab.number, permission));
                    }
                    // In the modes that don't ask, the first the window hears of a change is the tool using it.
                    Event::ToolUse { kind: ToolKind::FileChange(path), .. } => crate::files_changing::claim(&path, &name),
                    Event::ToolUse { kind: ToolKind::Command(_), .. } => crate::files_changing::claim(&folder, &name),
                    Event::Ready { models, .. } if !models.is_empty() => known = Some(models),
                    _ => {}
                }
            }
        }
        if let Some(models) = known {
            self.agents.models = models;
        }
        for (session, permission) in asked {
            let PermissionKind::Edit { path, proposed } = &permission.kind else { continue };
            match self.open_review(path, proposed, &permission.title) {
                Ok((tab, decisions)) => self.agents.reviews.push(OpenReview { session, request: permission.id.clone(), tab, proposed: proposed.clone(), path: path.clone(), decisions }),
                // It can't be shown as a change (too large, or not text): the person is asked in the panel instead.
                Err(why) => {
                    if let Some(tab) = self.agents.tabs.iter_mut().find(|t| t.number == session) {
                        tab.session.apply(&Event::Notice(why));
                    }
                }
            }
        }
        // The decisions made in the review tabs.
        let mut decided = Vec::new();
        self.agents.reviews.retain(|review| match review.decisions.try_recv() {
            Ok(decision) => {
                decided.push((review.session, review.request.clone(), review.path.clone(), review.proposed.clone(), decision));
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
        });
        for (session, request, path, proposed, decision) in decided {
            let Some(tab) = self.agents.tabs.iter_mut().find(|t| t.number == session) else { continue };
            let answer = match decision {
                Decision::Accepted(text) => {
                    crate::files_changing::claim(&path, &Self::agent_name(&tab.session));
                    Answer::Allow { text: (text != proposed).then_some(text), for_session: false }
                }
                Decision::Rejected => Answer::Refuse(crate::i18n::tr("The person rejected this change in the editor.").to_string()),
                Decision::Closed => Answer::Refuse(crate::i18n::tr("The person closed the review without accepting the change.").to_string()),
            };
            tab.session.answer(&request, answer);
        }
        // A refusal made in the panel closes the review it had open.
        let answered: Vec<(u64, String)> = self.agents.reviews.iter().filter(|r| self.agents.tabs.iter().find(|t| t.number == r.session).is_none_or(|t| !t.session.waiting().iter().any(|p| p.id == r.request))).map(|r| (r.tab, r.request.clone())).collect();
        for (tab, request) in answered {
            self.agents.reviews.retain(|r| r.request != request);
            self.close_review(tab);
        }
        if self.agents.tabs.iter().any(|t| t.session.busy) {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    /// The files `@` can name, started on a thread when the project is new to the panel.
    fn agent_files(&mut self, root: &Path, ctx: &Context) -> Arc<Vec<String>> {
        if self.agents.scanning.as_deref() != Some(root) {
            self.agents.scanning = Some(root.to_path_buf());
            let (root, list, ctx) = (root.to_path_buf(), self.agents.files.clone(), ctx.clone());
            std::thread::spawn(move || {
                let files = project_files(&root);
                if let Ok(mut list) = list.lock() {
                    *list = Some((root, Arc::new(files)));
                }
                ctx.request_repaint();
            });
        }
        self.agents.files.lock().ok().and_then(|list| list.as_ref().filter(|(r, _)| r == root).map(|(_, files)| files.clone())).unwrap_or_default()
    }

    /// The tabs of the sessions, and the one in front.
    fn agent_tab_row(&mut self, ui: &mut Ui) -> bool {
        let mut new = false;
        let mut closed = None;
        let mut usage = false;
        ui.horizontal_wrapped(|ui| {
            for (n, tab) in self.agents.tabs.iter().enumerate() {
                let label = egui::RichText::new(&tab.session.title).color(agent_panel::tab_colour(tab));
                if ui.selectable_label(n == self.agents.active, label).clicked() {
                    self.agents.active = n;
                }
                if ui.small_button("\u{D7}").on_hover_text(crate::i18n::tr("Close this session")).clicked() {
                    closed = Some(n);
                }
            }
            if ui.button("+").on_hover_text(crate::i18n::tr("New session")).clicked() {
                new = true;
            }
            if ui.button(crate::i18n::tr("Usage")).on_hover_text(crate::i18n::tr("What Claude Code and Codex have used today, and what is left of the plan's limits")).clicked() {
                usage = true;
            }
            if ui.button(crate::i18n::tr("Past sessions")).clicked() {
                self.agents.showing_past = !self.agents.showing_past;
                self.agents.past = None;
            }
        });
        if let Some(n) = closed {
            self.agent_close(n);
        }
        if usage {
            let ctx = ui.ctx().clone();
            self.open_usage(&ctx);
        }
        new
    }

    /// The past sessions of the folder, as rows to take up.
    fn agent_past_list(&mut self, ui: &mut Ui, folder: &Path) -> Option<Past> {
        if self.agents.past.as_ref().is_none_or(|(f, _)| f != folder) {
            let listed = crate::claude_ide::config_dir().map(|config| agent_logs::list(&config, folder)).unwrap_or_default();
            self.agents.past = Some((folder.to_path_buf(), listed));
        }
        let mut chosen = None;
        let Some((_, past)) = &self.agents.past else { return None };
        if past.is_empty() {
            ui.label(egui::RichText::new(crate::i18n::tr("No earlier sessions in this folder.")).color(theme::text_faint()));
        }
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for session in past {
                let when: chrono::DateTime<chrono::Local> = session.when.into();
                let response = ui.add(egui::Button::new(egui::RichText::new(&session.title).color(theme::text()).size(12.5)).frame(false));
                ui.label(egui::RichText::new(format!("{}  {}", when.format("%d %b %H:%M"), crate::agent_session::title_from(&session.first_message))).color(theme::text_faint()).size(11.0));
                if response.on_hover_text(crate::i18n::tr("Carry on with this session")).clicked() {
                    chosen = Some(session.clone());
                }
                ui.add_space(2.0);
            }
        });
        chosen
    }

    /// The panel, docked on the right while it's open.
    pub(super) fn agents_panel(&mut self, ui: &mut Ui) {
        if !self.panels.agents {
            return;
        }
        let Some(folder) = self.agent_folder() else { return };
        let ctx = ui.ctx().clone();
        let files = self.agent_files(&folder, &ctx);
        let mut shown = true;
        let mut new = false;
        let mut resume = None;
        let mut acts = Vec::new();
        crate::panels::dock_right(ui, "agents-panel", "AI agents", 400.0, &mut shown, |ui| {
            new = self.agent_tab_row(ui);
            if self.agents.showing_past {
                resume = self.agent_past_list(ui, &folder);
                ui.separator();
            }
            if self.agents.tabs.is_empty() {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(crate::i18n::tr("An AI agent works on this project beside you: it reads and changes files, runs commands, and asks before it does.")).color(theme::text_dim()));
                ui.add_space(6.0);
                if ui.button(crate::i18n::tr("New session")).clicked() {
                    new = true;
                }
                return;
            }
            let active = self.agents.active.min(self.agents.tabs.len() - 1);
            let State { tabs, reviews, .. } = &mut self.agents;
            let reviewing = |id: &str| reviews.iter().any(|r| r.request == id);
            acts = agent_panel::draw(ui, &mut tabs[active], &files, &reviewing);
        });
        self.panels.agents = shown;
        if new {
            self.agent_new_session_box();
        }
        if let Some(past) = resume {
            self.agents.showing_past = false;
            let choice = self.agents.last.clone();
            if let Err(why) = self.agent_start(&ctx, &folder, &Choice { mode: choice.mode, ..choice }, Some(&past)) {
                self.message("AI agents", why);
            }
        }
        for act in acts {
            match act {
                Act::OpenFile(path) => {
                    self.open_path(&path);
                }
                Act::ShowReview(request) => {
                    let tab = self.agents.reviews.iter().find(|r| r.request == request).map(|r| r.tab);
                    if let Some(index) = tab.and_then(|tab| self.docs.iter().position(|d| d.id == tab)) {
                        self.active = index;
                        self.pending_focus = true;
                    }
                }
            }
        }
    }

    /// The box for choosing how a new session goes.
    pub(super) fn agent_dialog(&mut self, ctx: &Context) {
        let Some(mut choice) = self.agents.dialog.clone() else { return };
        let Some(folder) = self.agent_folder() else {
            self.agents.dialog = None;
            return;
        };
        if self.agents.models.is_empty() && self.agents.probing.is_none() && !self.agents.probed {
            // The program is asked which models it has, once; nothing is sent to the service.
            self.agents.probed = true;
            let (tx, rx) = std::sync::mpsc::channel();
            let ((program, prefix), folder, wake) = (self.agents.program.clone(), folder.clone(), ctx.clone());
            std::thread::spawn(move || {
                let _ = tx.send(crate::agent_claude::probe_models(&program, &prefix, &folder));
                wake.request_repaint();
            });
            self.agents.probing = Some(rx);
        }
        let installed = !self.agents.program.1.is_empty() || assistant::installed(&self.agents.program.0);
        let models = if self.agents.models.is_empty() { vec![ModelChoice { value: "default".into(), name: crate::i18n::tr("Default").to_string(), efforts: Vec::new() }] } else { self.agents.models.clone() };
        let efforts = models.iter().find(|m| m.value == choice.model).map(|m| m.efforts.clone()).filter(|e| !e.is_empty()).unwrap_or_else(|| crate::agent_session::EFFORTS.iter().map(|e| e.to_string()).collect());
        let mut open = true;
        let mut start = false;
        let mut cancel = false;
        crate::fit::Dialog::new(crate::i18n::tr("New AI agent session")).open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(460.0);
            ui.label(crate::i18n::tf!("It will work in {}.", folder.display()));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Agent:"));
                let _ = ui.radio(true, "Claude Code");
                ui.add_enabled(false, egui::RadioButton::new(false, crate::i18n::tr("Codex (in the next batch)")));
            });
            if !installed {
                ui.label(egui::RichText::new(crate::i18n::tr("Claude Code isn't installed here, or isn't on the PATH. Install it from claude.com/claude-code, then try again.")).color(theme::rainbow_mistake()));
            }
            ui.add_space(6.0);
            egui::Grid::new("agent-new-grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label(crate::i18n::tr("Model:"));
                egui::ComboBox::from_id_salt("agent-new-model").selected_text(models.iter().find(|m| m.value == choice.model).map_or(choice.model.clone(), |m| m.name.clone())).show_ui(ui, |ui| {
                    for model in &models {
                        ui.selectable_value(&mut choice.model, model.value.clone(), &model.name);
                    }
                });
                ui.end_row();
                ui.label(crate::i18n::tr("Effort:"));
                egui::ComboBox::from_id_salt("agent-new-effort").selected_text(if choice.effort.is_empty() { crate::i18n::tr("Default").to_string() } else { choice.effort.clone() }).show_ui(ui, |ui| {
                    ui.selectable_value(&mut choice.effort, String::new(), crate::i18n::tr("Default"));
                    for effort in &efforts {
                        ui.selectable_value(&mut choice.effort, effort.clone(), effort);
                    }
                });
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.label(crate::i18n::tr("What it may do without asking:"));
            for mode in Mode::ALL {
                ui.radio_value(&mut choice.mode, mode, crate::i18n::tr(mode.label()));
            }
            let warning = choice.mode == Mode::Bypass;
            let colour = if warning { theme::rainbow_mistake() } else { theme::text_dim() };
            ui.label(egui::RichText::new(crate::i18n::tr(choice.mode.explain())).color(colour).size(12.0));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(installed, egui::Button::new(crate::i18n::tr("Start"))).clicked() {
                    start = true;
                }
                if ui.button(crate::i18n::tr("Cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        if start {
            self.agents.dialog = None;
            if let Err(why) = self.agent_start(ctx, &folder, &choice, None) {
                self.message("AI agents", why);
            }
        } else if open && !cancel {
            self.agents.dialog = Some(choice);
        } else {
            self.agents.dialog = None;
        }
    }

    /// A step before a screenshot: a session with a conversation in it, as it would look part way through.
    /// "top" has no request waiting but the subagents and the bar of what's used in view; the default has a
    /// command waiting to be allowed.
    pub(super) fn agents_screenshot_step(&mut self, value: &str) {
        use crate::agent_session::{Context as Fill, Limit, Limits, Permission, Task, TaskState, Tokens};
        self.panels.agents = true;
        let folder = self.agent_folder().unwrap_or_else(|| PathBuf::from("."));
        let mut session = Session::without_driver(Tool::Claude, &folder);
        session.finished = None;
        session.title = "Fix the parser's last line".into();
        let events = [
            Event::Ready { models: vec![ModelChoice { value: "default".into(), name: "Default (recommended)".into(), efforts: Vec::new() }], mode: Mode::Ask },
            Event::Started { session_id: "s".into(), model: "claude-opus-5-5".into(), mode: Mode::Ask },
            Event::Text { id: "m1:t1".into(), text: "I'll look at **parser.rs** first, then the test that covers the last line.".into(), done: true },
            Event::ToolUse { id: "t1".into(), name: "Read".into(), summary: "Read src/parser.rs".into(), kind: ToolKind::Other, parent: None },
            Event::ToolResult { id: "t1".into(), text: "fn parse() {}".into(), is_error: false },
            Event::ToolUse { id: "t2".into(), name: "Bash".into(), summary: "Ran cargo test parser".into(), kind: ToolKind::Command("cargo test parser".into()), parent: None },
            Event::ToolResult { id: "t2".into(), text: "running 4 tests\ntest result: FAILED. 1 passed, 3 failed".into(), is_error: false },
            Event::Text { id: "m2:t1".into(), text: "Three tests fail because the **last line** is dropped when a file has no final newline:\n\n- `read_lines` stops one early\n- the tests expect it\n\nI'll fix that and run the tests again.".into(), done: true },
            Event::Task(Task { id: "a1".into(), description: "Find other callers of read_lines".into(), kind: "general-purpose".into(), last_tool: Some("Grep".into()), tokens: 18_400, tool_uses: 6, state: TaskState::Running }),
            Event::Task(Task { id: "a2".into(), description: "Read the parser's tests".into(), kind: "general-purpose".into(), last_tool: Some("Read".into()), tokens: 9_200, tool_uses: 3, state: TaskState::Done }),
            Event::Usage { tokens: Tokens { input: 124_000, output: 3_900, cached: 98_000 }, cost: Some(0.412) },
            Event::Context(Fill { used: 620_000, max: 1_000_000 }),
            Event::Limits(Limits { five_hour: Some(Limit { percent: 42.0, resets: Some(now_plus(3 * 3600)) }), week: Some(Limit { percent: 19.0, resets: Some(now_plus(4 * 86400)) }) }),
        ];
        for event in &events {
            session.apply(event);
        }
        session.busy = true;
        if value != "top" {
            session.apply(&Event::Permission(Permission {
                id: "p1".into(),
                tool: "Bash".into(),
                title: "Run cargo test".into(),
                detail: "cargo test --release parser".into(),
                kind: PermissionKind::Command("cargo test --release parser".into()),
                can_remember: true,
            }));
        }
        let number = self.agents.next_number;
        self.agents.next_number += 1;
        self.agents.tabs = vec![Tab::new(session, number)];
        self.agents.active = 0;
    }
}

fn now_plus(seconds: i64) -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64) + seconds
}

#[cfg(test)]
#[path = "agent_host_tests.rs"]
mod agent_host_tests;
