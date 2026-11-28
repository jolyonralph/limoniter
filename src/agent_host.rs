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
    pub tool: Tool,
    pub mode: Mode,
    /// A model's `value`, "default" for the tool's own choice.
    pub model: String,
    /// An effort, or empty for the tool's own.
    pub effort: String,
}

impl Default for Choice {
    fn default() -> Self {
        Choice { tool: Tool::Claude, mode: Mode::Ask, model: "default".into(), effort: String::new() }
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
    /// The same for Codex.
    codex_models: Vec<ModelChoice>,
    codex_probing: Option<Receiver<Result<Vec<ModelChoice>, String>>>,
    codex_probed: bool,
    /// What Codex was last told the person has open, and what that was worked out from.
    codex_context_key: Option<(u64, u64, usize, usize, usize, usize, usize)>,
    /// Codex's logs are followed for the project in this folder.
    codex_watching: Option<PathBuf>,
    reviews: Vec<OpenReview>,
    /// Past sessions of the project, listed when asked for.
    past: Option<(PathBuf, Vec<Past>)>,
    showing_past: bool,
    files: FileList,
    scanning: Option<PathBuf>,
    /// The program that is Claude Code, and what goes before its own arguments. Tests put a stand-in here.
    pub program: (String, Vec<String>),
    /// The same for Codex.
    pub codex_program: (String, Vec<String>),
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
            codex_models: Vec::new(),
            codex_probing: None,
            codex_probed: false,
            codex_context_key: None,
            codex_watching: None,
            reviews: Vec::new(),
            past: None,
            showing_past: false,
            files: Arc::new(Mutex::new(None)),
            scanning: None,
            program: ("claude".to_string(), Vec::new()),
            codex_program: ("codex".to_string(), Vec::new()),
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
        let wake = ctx.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || wake.request_repaint());
        let model = Some(choice.model.clone()).filter(|m| m != "default");
        let effort = Some(choice.effort.clone()).filter(|e| !e.is_empty());
        let (handle, known) = match choice.tool {
            Tool::Claude => {
                let (program, prefix) = self.agents.program.clone();
                // Tried with Claude Code 2.1.332 and Limoniter's own IDE server: a `claude -p` session didn't connect to it,
                // with `--ide`, the port in CLAUDE_CODE_SSE_PORT and a matching lock file, so no port is passed (see NOTES).
                let ide_port = None;
                let options = crate::agent_claude::Options { program, prefix, folder: folder.to_path_buf(), mode: choice.mode, model, effort, resume: resume.map(|p| p.id.clone()), ide_port, wake };
                (crate::agent_claude::start(options)?, self.agents.models.clone())
            }
            Tool::Codex => {
                let (program, prefix) = self.agents.codex_program.clone();
                // Limoniter's tools go to this one thread as its own settings: the person's config.toml isn't touched.
                let mcp = self.claude_ide.server.as_ref().filter(|_| self.prefs.claude_ide).map(|server| crate::agent_codex::Mcp { url: format!("http://127.0.0.1:{}/mcp", server.port), token: server.token.clone() });
                let options = crate::agent_codex::Options { program, prefix, folder: folder.to_path_buf(), mode: choice.mode, model, effort, resume: resume.map(|p| p.id.clone()), mcp, wake };
                (crate::agent_codex::start(options)?, self.agents.codex_models.clone())
            }
        };
        let mut session = Session::new(choice.tool, folder, handle);
        session.mode = choice.mode;
        session.chosen_model = Some(choice.model.clone());
        session.effort = Some(choice.effort.clone()).filter(|e| !e.is_empty());
        session.models = known;
        if let Some(past) = resume {
            session.items = match past.tool {
                Tool::Claude => agent_logs::transcript(&past.path, folder),
                Tool::Codex => crate::agent_codex_logs::transcript(&past.path, folder),
            };
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
        if let Some(probing) = &self.agents.codex_probing {
            match probing.try_recv() {
                Ok(result) => {
                    self.agents.codex_probing = None;
                    if let Ok(models) = result
                        && !models.is_empty()
                    {
                        self.agents.codex_models = models;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.agents.codex_probing = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        self.codex_context_tick();
        let mut asked = Vec::new();
        let mut known = None;
        let mut known_codex = None;
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
                    Event::Ready { models, .. } if !models.is_empty() => {
                        if tab.session.tool == Tool::Codex { known_codex = Some(models) } else { known = Some(models) }
                    }
                    _ => {}
                }
            }
        }
        if let Some(models) = known {
            self.agents.models = models;
        }
        if let Some(models) = known_codex {
            self.agents.codex_models = models;
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

    /// Keeps Codex told what the person has open (the file in front, what's selected, the other tabs), as its own VS
    /// Code extension does, while a session of Codex is open; and follows the logs of a Codex run elsewhere.
    fn codex_context_tick(&mut self) {
        let folder = self.agent_folder();
        if self.agents.codex_watching != folder
            && let Some(folder) = &folder
            && crate::agent_codex_logs::codex_home().is_some_and(|home| home.join("sessions").is_dir())
        {
            crate::agent_codex_logs::watch(folder);
            self.agents.codex_watching = Some(folder.clone());
        }
        let Some(folder) = folder else { return };
        if !self.agents.tabs.iter().any(|t| t.session.tool == Tool::Codex && t.session.finished.is_none()) {
            self.agents.codex_context_key = None;
            return;
        }
        let key = self.docs.get(self.active).map(|doc| (doc.id, doc.text.version(), doc.text.caret_line, doc.text.caret_col, doc.text.anchor_line, doc.text.anchor_col, self.docs.len()));
        if key.is_some() && key == self.agents.codex_context_key {
            return;
        }
        self.agents.codex_context_key = key;
        let selection = self.claude_current_selection();
        let file = selection.as_ref().map(|s| PathBuf::from(&s.path));
        let tabs: Vec<(String, PathBuf)> = self.docs.iter().filter_map(|doc| doc.path.clone().map(|path| (doc.name(), path))).collect();
        let text = crate::agent_codex::ide_context_text(&folder, file.as_deref(), selection.as_ref().map_or("", |s| s.text.as_str()), &tabs);
        crate::agent_codex::set_ide_context(text);
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
            let mut listed = crate::claude_ide::config_dir().map(|config| agent_logs::list(&config, folder)).unwrap_or_default();
            listed.extend(crate::agent_codex_logs::codex_home().map(|home| crate::agent_codex_logs::list(&home, folder)).unwrap_or_default());
            listed.sort_by_key(|past| std::cmp::Reverse(past.when));
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
                ui.label(egui::RichText::new(format!("{}  {}  {}", when.format("%d %b %H:%M"), session.tool.name(), crate::agent_session::title_from(&session.first_message))).color(theme::text_faint()).size(11.0));
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
        crate::panels::dock_right(ui, "agents-panel", "AI agents", 400.0, &mut shown, |outer| {
            // Drawn in a box as wide as the panel and no wider: a long line or a row of choices wider than the
            // panel would otherwise make the panel grow to fit it, again and again as the transcript streams in.
            let room = outer.available_rect_before_wrap();
            let mut inside = outer.new_child(egui::UiBuilder::new().max_rect(room));
            inside.set_clip_rect(room.intersect(outer.clip_rect()));
            inside.set_max_width(room.width());
            let ui = &mut inside;
            'draw: {
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
                    break 'draw;
                }
                let active = self.agents.active.min(self.agents.tabs.len() - 1);
                let State { tabs, reviews, .. } = &mut self.agents;
                let reviewing = |id: &str| reviews.iter().any(|r| r.request == id);
                acts = agent_panel::draw(ui, &mut tabs[active], &files, &reviewing);
            }
            outer.allocate_rect(room, egui::Sense::hover());
        });
        self.panels.agents = shown;
        if new {
            self.agent_new_session_box();
        }
        if let Some(past) = resume {
            self.agents.showing_past = false;
            let choice = self.agents.last.clone();
            if let Err(why) = self.agent_start(&ctx, &folder, &Choice { tool: past.tool, model: if choice.tool == past.tool { choice.model.clone() } else { "default".into() }, effort: if choice.tool == past.tool { choice.effort.clone() } else { String::new() }, ..choice }, Some(&past)) {
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

    /// Whether a tool's program can be run: its own, or the stand-in a test gave.
    fn agent_installed(&self, tool: Tool) -> bool {
        let (program, prefix) = match tool {
            Tool::Claude => &self.agents.program,
            Tool::Codex => &self.agents.codex_program,
        };
        !prefix.is_empty() || assistant::installed(program)
    }

    /// Asks the tool which models it has, once, on a thread; nothing is sent to the service.
    fn agent_probe(&mut self, ctx: &Context, tool: Tool, folder: &Path) {
        let (tx, rx) = std::sync::mpsc::channel();
        let (program, prefix) = match tool {
            Tool::Claude => self.agents.program.clone(),
            Tool::Codex => self.agents.codex_program.clone(),
        };
        let (folder, wake) = (folder.to_path_buf(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(match tool {
                Tool::Claude => crate::agent_claude::probe_models(&program, &prefix, &folder),
                Tool::Codex => crate::agent_codex::probe_models(&program, &prefix, &folder),
            });
            wake.request_repaint();
        });
        match tool {
            Tool::Claude => {
                self.agents.probed = true;
                self.agents.probing = Some(rx);
            }
            Tool::Codex => {
                self.agents.codex_probed = true;
                self.agents.codex_probing = Some(rx);
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
        let tool = choice.tool;
        let installed = self.agent_installed(tool);
        let installed_both = [self.agent_installed(Tool::Claude), self.agent_installed(Tool::Codex)];
        let (known, asking, probed) = match tool {
            Tool::Claude => (&self.agents.models, self.agents.probing.is_some(), self.agents.probed),
            Tool::Codex => (&self.agents.codex_models, self.agents.codex_probing.is_some(), self.agents.codex_probed),
        };
        let probe_now = known.is_empty() && !asking && !probed && installed;
        let models = if known.is_empty() { vec![ModelChoice { value: "default".into(), name: crate::i18n::tr("Default").to_string(), efforts: Vec::new() }] } else { known.clone() };
        if probe_now {
            self.agent_probe(ctx, tool, &folder);
        }
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
                for (n, candidate) in [Tool::Claude, Tool::Codex].into_iter().enumerate() {
                    let response = ui.add_enabled(installed_both[n], egui::RadioButton::new(choice.tool == candidate, candidate.name()));
                    let response = if installed_both[n] { response } else { response.on_disabled_hover_text(crate::i18n::tf!("{} isn't installed here, or isn't on the PATH.", candidate.name())) };
                    if response.clicked() && choice.tool != candidate {
                        // What was chosen for one tool means nothing to the other.
                        choice.tool = candidate;
                        choice.model = "default".into();
                        choice.effort = String::new();
                    }
                }
            });
            if !installed {
                let how = match tool {
                    Tool::Claude => crate::i18n::tr("Claude Code isn't installed here, or isn't on the PATH. Install it from claude.com/claude-code, then try again."),
                    Tool::Codex => crate::i18n::tr("Codex isn't installed here, or isn't on the PATH. Install it with npm i -g @openai/codex and sign in with codex, then try again."),
                };
                ui.label(egui::RichText::new(how).color(theme::rainbow_mistake()));
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
            ui.label(if choice.tool == Tool::Codex { crate::i18n::tr("What Codex may do without asking:") } else { crate::i18n::tr("What it may do without asking:") });
            for mode in choice.tool.modes() {
                ui.radio_value(&mut choice.mode, mode, crate::i18n::tr(mode.label_for(choice.tool)));
            }
            let warning = choice.mode == Mode::Bypass;
            let colour = if warning { theme::rainbow_mistake() } else { theme::text_dim() };
            ui.label(egui::RichText::new(crate::i18n::tr(choice.mode.explain_for(choice.tool))).color(colour).size(12.0));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(installed_both[usize::from(choice.tool == Tool::Codex)], egui::Button::new(crate::i18n::tr("Start"))).clicked() {
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

    /// A step before a screenshot: the box for a new session, with a tool chosen ("codex") and the models its own
    /// program would have given (the program isn't started).
    pub(super) fn agents_dialog_screenshot_step(&mut self, value: &str) {
        self.agents.probed = true;
        self.agents.codex_probed = true;
        let efforts: Vec<String> = ["low", "medium", "high", "xhigh", "max", "ultra"].iter().map(|e| e.to_string()).collect();
        self.agents.codex_models = vec![
            ModelChoice { value: "default".into(), name: "Default (GPT-6.2-Sol)".into(), efforts: efforts.clone() },
            ModelChoice { value: "gpt-6.2-sol".into(), name: "GPT-6.2-Sol".into(), efforts },
            ModelChoice { value: "gpt-5.6-luna".into(), name: "GPT-5.6-Luna".into(), efforts: vec!["low".into(), "medium".into()] },
        ];
        self.agent_new_session_box();
        if let Some(choice) = self.agents.dialog.as_mut() {
            choice.tool = if value.contains("codex") { Tool::Codex } else { Tool::Claude };
            choice.mode = Mode::AcceptEdits;
        }
    }

    /// A step before a screenshot: a session with a conversation in it, as it would look part way through.
    /// "top" has no request waiting but the subagents and the bar of what's used in view; the default has a
    /// command waiting to be allowed.
    pub(super) fn agents_screenshot_step(&mut self, value: &str) {
        use crate::agent_session::{Context as Fill, Limit, Limits, Permission, Task, TaskState, Tokens};
        self.panels.agents = true;
        let folder = self.agent_folder().unwrap_or_else(|| PathBuf::from("."));
        // "codex" is the same conversation as Codex has it (its own mode names, models and efforts).
        let codex = value.contains("codex");
        let mut session = Session::without_driver(if codex { Tool::Codex } else { Tool::Claude }, &folder);
        session.finished = None;
        session.title = "Fix the parser's last line".into();
        let events = [
            Event::Ready { models: vec![ModelChoice { value: "default".into(), name: if codex { "Default (GPT-6.2-Sol)" } else { "Default (recommended)" }.into(), efforts: Vec::new() }], mode: if codex { Mode::AcceptEdits } else { Mode::Ask } },
            Event::Started { session_id: "s".into(), model: if codex { "gpt-6.2-sol" } else { "claude-opus-5-5" }.into(), mode: if codex { Mode::AcceptEdits } else { Mode::Ask } },
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
        if !value.contains("top") {
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
