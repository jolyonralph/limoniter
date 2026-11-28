//! The window's side of the agents working together: the Claims strip at the top of the AI agents panel, the Tasks
//! view (the board, the build queue, notes and the planner), and keeping the team told which project it's in and
//! which sessions are in it. The rules themselves are in `agent_team.rs`.

use super::*;
use crate::agent_session::{Session, Tool};
use crate::agent_team::{self, PERSON, Status};

/// What's typed in the team's views, and which one is showing.
#[derive(Default)]
pub(super) struct TeamUi {
    /// The Tasks view is showing in place of the transcript.
    pub show_tasks: bool,
    claim_open: bool,
    claim_path: String,
    claim_for: Option<String>,
    new_task: String,
    new_for: Option<String>,
    goal: String,
    planner: Option<String>,
    note_to: Option<String>,
    note_text: String,
    commands: Option<String>,
}

/// The board's words for who has a task: "Codex 1", or nobody.
fn who_text(team: &agent_team::Team, who: &Option<String>) -> String {
    match who {
        Some(key) => team.label(key),
        None => crate::i18n::tr("nobody").to_string(),
    }
}

impl Limoniter {
    /// The name a new session of the tool goes by on the board.
    pub(super) fn team_new_handle(&self, tool: Tool) -> String {
        agent_team::team().new_handle(tool.name())
    }

    /// Where Limoniter's tools are offered to a session: its server's address and key, if it's running and the
    /// person allows it. `host` is the address the session reaches this computer by.
    pub(super) fn team_mcp(&self, host: &str, who: &str) -> Option<crate::agent_codex::Mcp> {
        self.claude_ide.server.as_ref().filter(|_| self.prefs.claude_ide).map(|server| crate::agent_codex::Mcp { url: format!("http://{host}:{}/mcp", server.port), token: server.token.clone(), who: who.to_string() })
    }

    /// Puts a session just started on the team.
    pub(super) fn team_join(&self, session: &mut Session, handle: &str) {
        session.key = handle.to_string();
        agent_team::team().add_member(handle, session.tool.name(), &session.title);
    }

    /// Each time round: the board belongs to the project the window is in, and is read again if its file was
    /// changed by something else; the sessions' titles are kept up to date.
    pub(super) fn team_tick(&mut self) {
        let folder = self.project.as_ref().map(|p| p.folder()).or_else(|| self.agents.tabs.first().map(|t| t.session.folder.clone())).or_else(|| self.agent_folder());
        let mut team = agent_team::team();
        // The tests of the whole program share this one board, so the window's tests don't move it about.
        if !cfg!(test) {
            if let Some(folder) = folder {
                team.set_folder(&folder);
            }
            team.refresh();
        }
        for tab in &self.agents.tabs {
            if !tab.session.key.is_empty() {
                team.set_title(&tab.session.key, &tab.session.title);
            }
        }
    }

    /// The strip at the top of the panel: the Tasks button and who has claimed what.
    pub(super) fn team_strip(&mut self, ui: &mut Ui) {
        let (open, claims, queue) = {
            let team = agent_team::team();
            let claims: Vec<(PathBuf, String, String)> = team.claims.iter().map(|c| (c.path.clone(), team.shown(&c.path), team.label(&c.holder))).collect();
            (team.tasks.iter().filter(|t| t.status != Status::Done).count(), claims, team.queue_rows().len())
        };
        if self.agents.tabs.is_empty() && open == 0 && claims.is_empty() {
            return;
        }
        let small = |text: String| egui::RichText::new(text).color(theme::text_dim()).size(11.5);
        let mut release = None;
        ui.horizontal_wrapped(|ui| {
            let label = if queue > 1 { crate::i18n::tf!("Tasks ({open}, {n} waiting to build)", open = open, n = queue - 1) } else { crate::i18n::tf!("Tasks ({open})", open = open) };
            if ui.selectable_label(self.agents.team_ui.show_tasks, label).on_hover_text(crate::i18n::tr("The task board the agents share: tasks, notes, and builds waiting their turn")).clicked() {
                self.agents.team_ui.show_tasks = !self.agents.team_ui.show_tasks;
            }
            ui.label(small(crate::i18n::tr("Claims:").to_string()));
            if claims.is_empty() {
                ui.label(small(crate::i18n::tr("none").to_string()));
            }
            for (path, shown, who) in &claims {
                ui.label(small(format!("{shown} \u{2192} {who}")));
                if ui.small_button("\u{D7}").on_hover_text(crate::i18n::tr("Let go of this claim")).clicked() {
                    release = Some(path.clone());
                }
            }
            if ui.small_button("+").on_hover_text(crate::i18n::tr("Claim a file or folder for a session")).clicked() {
                self.agents.team_ui.claim_open = !self.agents.team_ui.claim_open;
            }
        });
        if let Some(path) = release {
            let mut team = agent_team::team();
            if let Some(holder) = team.holder_of(&path) {
                team.release_paths(&holder, &[path.display().to_string()]);
            }
        }
        if self.agents.team_ui.claim_open {
            self.team_claim_row(ui);
        }
    }

    fn team_claim_row(&mut self, ui: &mut Ui) {
        let holders = self.team_holders();
        let state = &mut self.agents.team_ui;
        let chosen = state.claim_for.clone().filter(|k| holders.iter().any(|(key, _)| key == k)).or_else(|| holders.first().map(|(k, _)| k.clone()));
        let mut go = false;
        let mut pick = chosen.clone();
        ui.horizontal(|ui| {
            let shown = holders.iter().find(|(k, _)| Some(k) == chosen.as_ref()).map(|(_, l)| l.clone()).unwrap_or_default();
            egui::ComboBox::from_id_salt("team-claim-for").selected_text(shown).width(120.0).show_ui(ui, |ui| {
                for (key, label) in &holders {
                    if ui.selectable_label(Some(key) == chosen.as_ref(), label).clicked() {
                        pick = Some(key.clone());
                    }
                }
            });
            let edit = ui.add(egui::TextEdit::singleline(&mut state.claim_path).hint_text(crate::i18n::tr("src/parser/")).desired_width(120.0));
            go = ui.button(crate::i18n::tr("Claim")).clicked() || (edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        });
        state.claim_for = pick.clone();
        if go && !state.claim_path.trim().is_empty() {
            let result = agent_team::team().claim_paths(pick.as_deref().unwrap_or(PERSON), std::slice::from_ref(&state.claim_path));
            match result {
                Ok(_) => state.claim_path.clear(),
                Err(why) => self.message("AI agents", why),
            }
        }
    }

    /// Who can hold a claim or be given a task: you, and each session.
    fn team_holders(&self) -> Vec<(String, String)> {
        let team = agent_team::team();
        std::iter::once((PERSON.to_string(), crate::i18n::tr("Me").to_string())).chain(team.members.iter().map(|m| (m.key.clone(), team.label(&m.key)))).collect()
    }

    /// A session's tab, by its name on the board.
    fn team_tab(&mut self, key: &str) -> Option<&mut Session> {
        self.agents.tabs.iter_mut().map(|t| &mut t.session).find(|s| s.key == key)
    }

    /// The Tasks view: in place of the transcript.
    pub(super) fn team_tasks_view(&mut self, ui: &mut Ui) {
        let sessions: Vec<(String, String)> = {
            let team = agent_team::team();
            team.members.iter().map(|m| (m.key.clone(), team.label(&m.key))).collect()
        };
        let mut say: Vec<(String, String)> = Vec::new();
        egui::ScrollArea::vertical().id_salt("team-tasks-scroll").auto_shrink([false, false]).show(ui, |ui| {
            let dim = |text: String| egui::RichText::new(text).color(theme::text_dim()).size(12.0);
            ui.add_space(4.0);
            ui.label(egui::RichText::new(crate::i18n::tr("Tasks")).strong());
            self.team_task_rows(ui, &sessions, &mut say);
            // A new task, given to a session or left open.
            ui.horizontal(|ui| {
                let state = &mut self.agents.team_ui;
                let edit = ui.add(egui::TextEdit::singleline(&mut state.new_task).hint_text(crate::i18n::tr("A new task")).desired_width(170.0));
                let shown = state.new_for.as_ref().and_then(|k| sessions.iter().find(|(key, _)| key == k)).map_or_else(|| crate::i18n::tr("nobody").to_string(), |(_, l)| l.clone());
                egui::ComboBox::from_id_salt("team-new-for").selected_text(shown).width(110.0).show_ui(ui, |ui| {
                    if ui.selectable_label(state.new_for.is_none(), crate::i18n::tr("nobody")).clicked() {
                        state.new_for = None;
                    }
                    for (key, label) in &sessions {
                        if ui.selectable_label(state.new_for.as_ref() == Some(key), label).clicked() {
                            state.new_for = Some(key.clone());
                        }
                    }
                });
                if (ui.button(crate::i18n::tr("Add")).clicked() || (edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && !state.new_task.trim().is_empty() {
                    let assignee = state.new_for.clone().filter(|k| sessions.iter().any(|(key, _)| key == k));
                    agent_team::team().add_task(PERSON, &state.new_task, "", assignee);
                    state.new_task.clear();
                }
            });
            ui.add_space(8.0);

            // One session asked to split a goal into tasks.
            ui.label(egui::RichText::new(crate::i18n::tr("Planner")).strong());
            ui.label(dim(crate::i18n::tr("Ask one session to split a goal into tasks on the board. It changes no files while it plans.").to_string()));
            let state = &mut self.agents.team_ui;
            ui.add(egui::TextEdit::multiline(&mut state.goal).hint_text(crate::i18n::tr("The goal, in a few words or lines")).desired_rows(2).desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                let chosen = state.planner.clone().filter(|k| sessions.iter().any(|(key, _)| key == k)).or_else(|| sessions.first().map(|(k, _)| k.clone()));
                let shown = chosen.as_ref().and_then(|k| sessions.iter().find(|(key, _)| key == k)).map(|(_, l)| l.clone()).unwrap_or_default();
                egui::ComboBox::from_id_salt("team-planner").selected_text(shown).width(150.0).show_ui(ui, |ui| {
                    for (key, label) in &sessions {
                        if ui.selectable_label(chosen.as_ref() == Some(key), label).clicked() {
                            state.planner = Some(key.clone());
                        }
                    }
                });
                if ui.add_enabled(chosen.is_some() && !state.goal.trim().is_empty(), egui::Button::new(crate::i18n::tr("Ask to plan"))).clicked()
                    && let Some(key) = chosen
                {
                    say.push((key, agent_team::planner_prompt(&state.goal)));
                    state.goal.clear();
                }
            });
            ui.add_space(8.0);

            self.team_queue_rows(ui);
            self.team_note_rows(ui, &sessions);
            self.team_build_commands(ui);
        });
        for (key, text) in say {
            if let Some(session) = self.team_tab(&key) {
                session.say(&text);
            }
        }
    }

    fn team_task_rows(&mut self, ui: &mut Ui, sessions: &[(String, String)], say: &mut Vec<(String, String)>) {
        let rows: Vec<agent_team::Task> = agent_team::team().tasks.clone();
        if rows.is_empty() {
            ui.label(egui::RichText::new(crate::i18n::tr("No tasks yet. Write one below, or ask a session to plan.")).color(theme::text_dim()).size(12.0));
        }
        let mut delete = None;
        let mut change: Vec<(u32, Status, Option<String>)> = Vec::new();
        for task in &rows {
            let (mark, colour) = match task.status {
                Status::Open => ("\u{25CB}", theme::text_dim()),
                Status::Taken => ("\u{25D0}", theme::git_changed()),
                Status::Done => ("\u{25CF}", theme::rainbow_mistake()),
            };
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(mark).color(colour));
                let title = egui::RichText::new(format!("{}. {}", task.id, task.title));
                ui.label(if task.status == Status::Done { title.strikethrough().color(theme::text_dim()) } else { title });
                let who = who_text(&agent_team::team(), &task.assignee);
                ui.label(egui::RichText::new(format!("\u{2192} {who}")).color(theme::text_dim()).size(11.5));
                // Given to a session, or taken by one that isn't doing it yet: it can be nudged.
                if let (Some(key), true) = (&task.assignee, task.status != Status::Done)
                    && sessions.iter().any(|(k, _)| k == key)
                    && ui.small_button(crate::i18n::tr("Tell it to start")).clicked()
                {
                    say.push((key.clone(), crate::i18n::tf!("Please start on task {id} from the task board: {title}. Claim it with claim_task, claim the files you will change with claim_paths, and finish it with finish_task when it's done.", id = task.id, title = task.title)));
                }
                if task.status == Status::Done {
                    if ui.small_button(crate::i18n::tr("Reopen")).clicked() {
                        change.push((task.id, Status::Open, None));
                    }
                } else if ui.small_button(crate::i18n::tr("Done")).clicked() {
                    change.push((task.id, Status::Done, task.assignee.clone()));
                }
                // Who it's for.
                egui::ComboBox::from_id_salt(("team-assign", task.id)).selected_text(crate::i18n::tr("Give to")).width(70.0).show_ui(ui, |ui| {
                    if ui.selectable_label(task.assignee.is_none(), crate::i18n::tr("nobody")).clicked() {
                        change.push((task.id, Status::Open, None));
                    }
                    for (key, label) in sessions {
                        if ui.selectable_label(task.assignee.as_ref() == Some(key), label).clicked() {
                            change.push((task.id, Status::Open, Some(key.clone())));
                        }
                    }
                });
                if ui.small_button("\u{D7}").on_hover_text(crate::i18n::tr("Remove this task")).clicked() {
                    delete = Some(task.id);
                }
            });
            if !task.details.is_empty() {
                ui.label(egui::RichText::new(&task.details).color(theme::text_dim()).size(11.5));
            }
            if !task.notes.is_empty() {
                ui.label(egui::RichText::new(crate::i18n::tf!("Notes: {}", task.notes)).color(theme::text_dim()).size(11.5));
            }
        }
        if delete.is_some() || !change.is_empty() {
            let mut team = agent_team::team();
            for (id, status, assignee) in change {
                if let Some(task) = team.tasks.iter_mut().find(|t| t.id == id) {
                    task.status = status;
                    task.assignee = assignee;
                }
            }
            if let Some(id) = delete {
                team.tasks.retain(|t| t.id != id);
            }
            team.save();
        }
    }

    fn team_queue_rows(&mut self, ui: &mut Ui) {
        let rows = agent_team::team().queue_rows();
        ui.label(egui::RichText::new(crate::i18n::tr("Builds and tests")).strong());
        if rows.is_empty() {
            ui.label(egui::RichText::new(crate::i18n::tr("Nothing is building. A build or test one session asks for waits here while another's runs.")).color(theme::text_dim()).size(12.0));
        }
        let mut jump = None;
        for row in &rows {
            ui.horizontal_wrapped(|ui| {
                if row.running {
                    ui.label(egui::RichText::new(crate::i18n::tf!("{who} is running {command}", who = row.who, command = row.command)).size(12.0));
                } else {
                    ui.label(egui::RichText::new(crate::i18n::tf!("{who} is waiting to run {command}", who = row.who, command = row.command)).color(theme::text_dim()).size(12.0));
                    if ui.small_button(crate::i18n::tr("Go next")).on_hover_text(crate::i18n::tr("Let this one run as soon as the one running has finished")).clicked() {
                        jump = Some(row.id);
                    }
                }
            });
        }
        if let Some(id) = jump {
            agent_team::team().jump(id);
        }
        ui.add_space(8.0);
    }

    fn team_note_rows(&mut self, ui: &mut Ui, sessions: &[(String, String)]) {
        ui.label(egui::RichText::new(crate::i18n::tr("Notes")).strong());
        let notes: Vec<agent_team::Note> = agent_team::team().notes.iter().rev().take(6).cloned().collect();
        if notes.is_empty() {
            ui.label(egui::RichText::new(crate::i18n::tr("No notes between the agents yet.")).color(theme::text_dim()).size(12.0));
        }
        for note in notes.iter().rev() {
            ui.label(egui::RichText::new(format!("{} \u{2192} {}: {}", note.from, note.to, note.text)).color(theme::text_dim()).size(12.0));
        }
        let state = &mut self.agents.team_ui;
        ui.horizontal(|ui| {
            let shown = state.note_to.clone().unwrap_or_else(|| "all".to_string());
            egui::ComboBox::from_id_salt("team-note-to").selected_text(crate::i18n::tf!("To {}", shown)).width(110.0).show_ui(ui, |ui| {
                if ui.selectable_label(state.note_to.is_none(), crate::i18n::tr("all")).clicked() {
                    state.note_to = None;
                }
                for (key, _) in sessions {
                    if ui.selectable_label(state.note_to.as_ref() == Some(key), key).clicked() {
                        state.note_to = Some(key.clone());
                    }
                }
            });
            let edit = ui.add(egui::TextEdit::singleline(&mut state.note_text).hint_text(crate::i18n::tr("A note")).desired_width(150.0));
            if (ui.button(crate::i18n::tr("Send")).clicked() || (edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && !state.note_text.trim().is_empty() {
                let to = state.note_to.clone().unwrap_or_else(|| "all".to_string());
                let mut team = agent_team::team();
                team.notes.push(agent_team::Note { from: PERSON.to_string(), to, text: state.note_text.trim().to_string(), read_by: Vec::new() });
                team.save();
                state.note_text.clear();
            }
        });
        ui.add_space(8.0);
    }

    /// The list of what counts as a build or a test, one command's first words to a line.
    fn team_build_commands(&mut self, ui: &mut Ui) {
        let state = &mut self.agents.team_ui;
        egui::CollapsingHeader::new(crate::i18n::tr("What counts as a build or test")).id_salt("team-build-commands").show(ui, |ui| {
            let mut shown = state.commands.take().unwrap_or_else(|| agent_team::team().build_commands().join("\n"));
            ui.label(egui::RichText::new(crate::i18n::tr("The first words of a command, one to a line. A command that starts with one waits for the others.")).color(theme::text_dim()).size(11.5));
            ui.add(egui::TextEdit::multiline(&mut shown).desired_rows(5).desired_width(f32::INFINITY));
            let mut reset = false;
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("Save")).clicked() {
                    agent_team::team().set_build_commands(shown.lines().map(str::to_string).collect());
                }
                if ui.button(crate::i18n::tr("Use the usual list")).clicked() {
                    agent_team::team().set_build_commands(agent_team::DEFAULT_BUILD_COMMANDS.iter().map(|s| s.to_string()).collect());
                    reset = true;
                }
            });
            state.commands = if reset { None } else { Some(shown) };
        });
    }

    /// A step before a screenshot: a Claude Code session and a Codex one, each with files claimed, a build waiting
    /// for the other's, and (for "tasks") the Tasks view open.
    pub(super) fn team_screenshot_step(&mut self, tasks: bool) {
        use crate::agent_panel::Tab;
        use crate::agent_session::{Event, Item, Permission, PermissionKind};
        let folder = self.agent_folder().unwrap_or_else(|| PathBuf::from("."));
        let mut claude = Session::without_driver(Tool::Claude, &folder);
        let mut codex = Session::without_driver(Tool::Codex, &folder);
        claude.finished = None;
        codex.finished = None;
        claude.title = "Parser".into();
        codex.title = "Writer".into();
        claude.busy = true;
        codex.busy = true;
        {
            let mut team = agent_team::team();
            team.set_folder(&folder);
            team.tasks.clear();
            team.notes.clear();
            team.claims.clear();
            team.members.clear();
            for (session, tool) in [(&mut claude, Tool::Claude), (&mut codex, Tool::Codex)] {
                session.key = team.new_handle(tool.name());
                team.add_member(&session.key, tool.name(), &session.title);
            }
            let (c, x) = (claude.key.clone(), codex.key.clone());
            let _ = team.claim_paths(&c, &["src/parser/".to_string()]);
            let _ = team.claim_paths(&x, &["src/writer/".to_string(), "docs/".to_string()]);
            let _ = team.claim_paths(PERSON, &["Cargo.toml".to_string()]);
            let first = team.add_task(PERSON, "Lex numbers with an exponent", "in src/parser/lexer.rs", Some(c.clone()));
            let second = team.add_task(PERSON, "Write the output in the new format", "in src/writer/", Some(x.clone()));
            team.add_task(&c, "Update the parser's documentation", "when both are done", None);
            let _ = team.call(&c, "claim_task", &serde_json::json!({ "id": first }));
            let _ = team.call(&x, "claim_task", &serde_json::json!({ "id": second }));
            let _ = team.call(&x, "leave_note", &serde_json::json!({ "to": "Claude Code", "text": "The writer now takes tokens from src/parser/token.rs; please leave its names alone." }));
            let _ = team.call(&c, "leave_note", &serde_json::json!({ "to": "You", "text": "The lexer is done; I'm running the tests." }));
            let _ = team.build_request(&x, "cargo test --release");
            let _ = team.build_request(&c, "cargo build");
            for session in [&mut claude, &mut codex] {
                let key = session.key.clone();
                session.items.extend(team.take_notices(&key).into_iter().map(Item::Notice));
            }
        }
        claude.items.insert(0, Item::You("Make the lexer take numbers with an exponent".to_string()));
        claude.items.push(Item::Permission {
            permission: Permission { id: "p1".into(), tool: "Edit".into(), title: "Edit src/writer/format.rs".to_string(), detail: String::new(), kind: PermissionKind::Other, can_remember: false },
            decided: crate::agent_session::Decided::Refused,
        });
        claude.items.push(Item::Notice(format!("Refused: src/writer/format.rs is claimed by {} (Writer).", codex.key)));
        claude.apply(&Event::Text { id: "m1".into(), text: "The writer's file belongs to Codex, so I've left it a note and will carry on with the lexer.".to_string(), done: true });
        codex.items.insert(0, Item::You("Write the output in the new format".to_string()));
        let number = self.agents.next_number;
        self.agents.next_number += 2;
        self.agents.tabs = vec![Tab::new(claude, number), Tab::new(codex, number + 1)];
        self.agents.active = 0;
        self.agents.team_ui.show_tasks = tasks;
    }
}
