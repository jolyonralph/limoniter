//! Drawing an agent session: the bar of what it's using, the subagents, the transcript and the box to type in. It
//! changes nothing outside the session it's given; what needs the window (opening a file, showing a review) is
//! handed back as [`Act`]s.

use crate::agent_session::{Answer, Decided, Item, Limit, Mode, Permission, PermissionKind, Session, TaskState, ToolKind};
use crate::theme;
use eframe::egui::{self, Color32, Key, Modifiers, RichText, Ui};
use std::collections::HashMap;
use std::path::PathBuf;

/// What the window is asked to do.
pub enum Act {
    OpenFile(PathBuf),
    /// Bring this request's review tab to the front.
    ShowReview(String),
}

/// A session and what's remembered of how it's drawn.
pub struct Tab {
    pub session: Session,
    /// The number it's known by in egui's ids.
    pub number: u64,
    /// Markdown laid out for a message: by its id, with the text it was made from.
    markdown: HashMap<String, (usize, Vec<crate::preview::Block>)>,
    pub stick: bool,
}

impl Tab {
    pub fn new(session: Session, number: u64) -> Self {
        Tab { session, number, markdown: HashMap::new(), stick: true }
    }
}

/// A count of tokens in few characters: 950, 12.3k, 1.2M.
pub fn compact(n: u64) -> String {
    if n < 1000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    }
}

/// When a limit resets, for people: the time today, or the day and time.
pub fn reset_text(resets: i64, now: i64) -> String {
    use chrono::{Local, TimeZone};
    let Some(when) = Local.timestamp_opt(resets, 0).single() else { return String::new() };
    if resets - now < 20 * 3600 { when.format("%H:%M").to_string() } else { when.format("%a %H:%M").to_string() }
}

fn limit_text(name: &str, limit: &Limit, now: i64) -> String {
    match limit.resets {
        Some(resets) if resets > now => crate::i18n::tf!("{} {}%, resets {}", name, limit.percent.round() as i64, reset_text(resets, now)),
        _ => crate::i18n::tf!("{} {}%", name, limit.percent.round() as i64),
    }
}

fn now_seconds() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// The words for what's chosen as the model.
fn model_name(session: &Session) -> String {
    let chosen = session.chosen_model.as_deref();
    if let Some(found) = session.models.iter().find(|m| Some(m.value.as_str()) == chosen || (chosen.is_none() && m.value == "default")) {
        return found.name.clone();
    }
    chosen.or(session.model.as_deref()).unwrap_or("").to_string()
}

/// The top of a session: what it runs on, and what it has used.
fn top(ui: &mut Ui, tab: &mut Tab) {
    let session = &mut tab.session;
    ui.horizontal_wrapped(|ui| {
        if !session.models.is_empty() {
            let mut picked = None;
            egui::ComboBox::from_id_salt(("agent-model", tab.number)).selected_text(model_name(session)).width(130.0).show_ui(ui, |ui| {
                for model in &session.models {
                    let chosen = session.chosen_model.as_deref().unwrap_or("default") == model.value;
                    if ui.selectable_label(chosen, &model.name).clicked() {
                        picked = Some(model.value.clone());
                    }
                }
            });
            if let Some(value) = picked {
                session.set_model(Some(&value));
            }
        } else if let Some(model) = &session.model {
            ui.label(RichText::new(model).color(theme::text_dim()));
        }
        let efforts = session.efforts();
        let mut picked = None;
        egui::ComboBox::from_id_salt(("agent-effort", tab.number)).selected_text({ let effort = session.effort.as_deref().unwrap_or_else(|| crate::i18n::tr("Default")); crate::i18n::tf!("Effort: {}", effort) }).width(100.0).show_ui(ui, |ui| {
            for effort in &efforts {
                if ui.selectable_label(session.effort.as_deref() == Some(effort.as_str()), effort).clicked() {
                    picked = Some(effort.clone());
                }
            }
        });
        if let Some(effort) = picked {
            session.set_effort(&effort);
        }
        let mut mode = None;
        egui::ComboBox::from_id_salt(("agent-mode", tab.number)).selected_text(crate::i18n::tr(session.mode.label())).width(140.0).show_ui(ui, |ui| {
            for candidate in Mode::ALL {
                let response = ui.selectable_label(session.mode == candidate, crate::i18n::tr(candidate.label()));
                if response.on_hover_text(crate::i18n::tr(candidate.explain())).clicked() {
                    mode = Some(candidate);
                }
            }
        });
        if let Some(mode) = mode {
            session.set_mode(mode);
        }
    });
    if session.mode == Mode::Bypass {
        ui.label(RichText::new(crate::i18n::tr("Nothing is asked: it may change or delete anything here.")).color(theme::rainbow_mistake()).size(11.5));
    }
    // What it has used.
    ui.horizontal_wrapped(|ui| {
        let small = |text: String| RichText::new(text).color(theme::text_dim()).size(11.5);
        if session.tokens.input + session.tokens.output > 0 {
            ui.label(small(crate::i18n::tf!("{} in, {} out", compact(session.tokens.input), compact(session.tokens.output))));
        }
        if let Some(cost) = session.cost {
            ui.label(small(format!("${cost:.3}")));
        }
        if let Some(context) = session.context {
            let bar = egui::ProgressBar::new(context.percent() / 100.0).desired_width(90.0).text(RichText::new(crate::i18n::tf!("Context {}%", context.percent().round() as i64)).size(10.5));
            ui.add(bar).on_hover_text(crate::i18n::tf!("{} of {} tokens of context used", compact(context.used), compact(context.max)));
        }
    });
    if session.limits.any() {
        let now = now_seconds();
        ui.horizontal_wrapped(|ui| {
            if let Some(limit) = &session.limits.five_hour {
                ui.label(RichText::new(limit_text(crate::i18n::tr("5-hour"), limit, now)).color(theme::text_dim()).size(11.5));
            }
            if let Some(limit) = &session.limits.week {
                ui.label(RichText::new(limit_text(crate::i18n::tr("Week"), limit, now)).color(theme::text_dim()).size(11.5));
            }
        });
    }
    let running = session.running_tasks().len();
    if !session.tasks.is_empty() {
        let title = crate::i18n::tf!("Subagents and tasks ({} running)", running);
        egui::CollapsingHeader::new(RichText::new(title).size(12.0)).id_salt(("agent-tasks", tab.number)).default_open(true).show(ui, |ui| {
            let mut stop = None;
            for task in &session.tasks {
                ui.horizontal_wrapped(|ui| {
                    let mark = match task.state {
                        TaskState::Running => "\u{25B6}",
                        TaskState::Done => "\u{2713}",
                        TaskState::Failed => "\u{2717}",
                        TaskState::Stopped => "\u{25A0}",
                    };
                    ui.label(RichText::new(mark).color(theme::text_dim()));
                    ui.label(RichText::new(&task.description).color(theme::text()).size(12.0));
                    let mut detail = Vec::new();
                    if task.state == TaskState::Running
                        && let Some(tool) = &task.last_tool
                    {
                        detail.push(crate::i18n::tf!("now: {}", tool));
                    }
                    detail.push(crate::i18n::tf!("{} tokens, {} tool uses", compact(task.tokens), task.tool_uses));
                    ui.label(RichText::new(detail.join(" \u{B7} ")).color(theme::text_faint()).size(11.0));
                    if task.state == TaskState::Running && ui.small_button(crate::i18n::tr("Stop")).clicked() {
                        stop = Some(task.id.clone());
                    }
                });
            }
            if let Some(id) = stop {
                session.stop_task(&id);
            }
        });
    }
}

fn result_tail(text: &str) -> String {
    let line = text.lines().map(str::trim).rfind(|line| !line.is_empty()).unwrap_or("");
    if line.chars().count() > 70 { format!("{}\u{2026}", line.chars().take(70).collect::<String>()) } else { line.to_string() }
}

/// A tool use: one line, and its result folded under it.
fn tool_line(ui: &mut Ui, id: &str, summary: &str, kind: &ToolKind, result: Option<&crate::agent_session::ToolResult>, acts: &mut Vec<Act>) {
    let mut title = summary.to_string();
    if let (ToolKind::Command(_), Some(result)) = (kind, result) {
        let tail = result_tail(&result.text);
        if !tail.is_empty() {
            title = format!("{title}: {tail}");
        }
    }
    let colour = if result.is_some_and(|r| r.is_error) { theme::rainbow_mistake() } else { theme::text_dim() };
    let text = RichText::new(format!("\u{2022} {title}")).color(colour).size(12.0);
    match result {
        Some(result) if !result.text.trim().is_empty() || matches!(kind, ToolKind::FileChange(_)) => {
            egui::CollapsingHeader::new(text).id_salt(("agent-tool", id)).show(ui, |ui| {
                if let ToolKind::FileChange(path) = kind
                    && ui.small_button(crate::i18n::tr("Open the file")).clicked()
                {
                    acts.push(Act::OpenFile(path.clone()));
                }
                let shown: String = result.text.chars().take(4000).collect();
                ui.label(RichText::new(shown).monospace().size(11.0).color(theme::text_dim()));
            });
        }
        Some(_) => {
            ui.label(text);
        }
        None => {
            ui.horizontal(|ui| {
                ui.label(text);
                ui.spinner();
            });
        }
    }
}

fn permission_box(ui: &mut Ui, permission: &Permission, decided: Decided, reviewing: bool, acts: &mut Vec<Act>, answers: &mut Vec<(String, Answer)>) {
    egui::Frame::new().stroke(egui::Stroke::new(1.0, theme::border())).corner_radius(4.0).inner_margin(6.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(&permission.title).strong().color(theme::strong_text()).size(12.5));
        if !permission.detail.is_empty() {
            let shown: String = permission.detail.chars().take(1500).collect();
            ui.label(RichText::new(shown).monospace().size(11.0).color(theme::text_dim()));
        }
        match decided {
            Decided::Allowed { for_session } => {
                let words = if for_session { crate::i18n::tr("Allowed for this session") } else { crate::i18n::tr("Allowed") };
                ui.label(RichText::new(words).color(theme::text_faint()).size(11.5));
            }
            Decided::Refused => {
                ui.label(RichText::new(crate::i18n::tr("Refused")).color(theme::text_faint()).size(11.5));
            }
            Decided::Waiting if reviewing && matches!(permission.kind, PermissionKind::Edit { .. }) => {
                ui.label(RichText::new(crate::i18n::tr("The change is open in a tab for you to accept or reject. You can edit it there first.")).color(theme::text_dim()).size(11.5));
                ui.horizontal(|ui| {
                    if ui.button(crate::i18n::tr("Show the change")).clicked() {
                        acts.push(Act::ShowReview(permission.id.clone()));
                    }
                    if ui.button(crate::i18n::tr("Refuse")).clicked() {
                        answers.push((permission.id.clone(), Answer::Refuse(crate::i18n::tr("The person refused this change.").to_string())));
                    }
                });
            }
            Decided::Waiting => {
                ui.horizontal_wrapped(|ui| {
                    if ui.button(crate::i18n::tr("Allow once")).clicked() {
                        answers.push((permission.id.clone(), Answer::Allow { text: None, for_session: false }));
                    }
                    if permission.can_remember && ui.button(crate::i18n::tr("Allow for this session")).clicked() {
                        answers.push((permission.id.clone(), Answer::Allow { text: None, for_session: true }));
                    }
                    if ui.button(crate::i18n::tr("Refuse")).clicked() {
                        answers.push((permission.id.clone(), Answer::Refuse(crate::i18n::tr("The person refused this.").to_string())));
                    }
                });
            }
        }
    });
}

/// The transcript.
fn transcript(ui: &mut Ui, tab: &mut Tab, reviewing: &dyn Fn(&str) -> bool, acts: &mut Vec<Act>) {
    let Tab { session, number, markdown, stick, .. } = tab;
    let mut answers = Vec::new();
    egui::ScrollArea::vertical().auto_shrink([false, false]).stick_to_bottom(*stick).id_salt(("agent-transcript", *number)).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        for item in &session.items {
            match item {
                Item::You(text) => {
                    egui::Frame::new().fill(theme::hover()).corner_radius(4.0).inner_margin(6.0).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(text).color(theme::text()));
                    });
                }
                Item::Text { id, text, .. } => {
                    if text.trim().is_empty() {
                        continue;
                    }
                    let cached = markdown.get(id).is_some_and(|(len, _)| *len == text.len());
                    if !cached {
                        markdown.insert(id.clone(), (text.len(), crate::preview::render(crate::preview::Kind::Markdown, text)));
                    }
                    if let Some((_, blocks)) = markdown.get(id) {
                        crate::preview::draw_inline(ui, blocks);
                    }
                }
                Item::Thinking { id, text } => {
                    egui::CollapsingHeader::new(RichText::new(crate::i18n::tr("Thinking")).italics().color(theme::text_faint()).size(12.0)).id_salt(("agent-think", id)).show(ui, |ui| {
                        ui.label(RichText::new(text).italics().color(theme::text_dim()).size(11.5));
                    });
                }
                Item::Tool { id, summary, kind, parent, result, .. } => {
                    // What a subagent does is told in the list of subagents, not here.
                    if parent.is_none() {
                        tool_line(ui, id, summary, kind, result.as_ref(), acts);
                    }
                }
                Item::Permission { permission, decided } => {
                    permission_box(ui, permission, *decided, reviewing(&permission.id), acts, &mut answers);
                }
                Item::Notice(text) => {
                    ui.label(RichText::new(text).italics().color(theme::text_faint()).size(11.5));
                }
                Item::Error(text) => {
                    ui.label(RichText::new(text).color(theme::rainbow_mistake()));
                }
            }
            ui.add_space(3.0);
        }
    });
    for (id, answer) in answers {
        session.answer(&id, answer);
    }
}

/// The `@name` the caret is at the end of, if the draft ends in one.
pub fn mention_at_end(draft: &str) -> Option<&str> {
    let start = draft.rfind('@')?;
    let before_ok = start == 0 || draft[..start].chars().next_back().is_some_and(char::is_whitespace);
    let word = &draft[start + 1..];
    (before_ok && !word.contains(char::is_whitespace)).then_some(word)
}

/// Files that fit what's been typed after `@`: those with all of it in their path, the shortest first.
pub fn matching_files<'a>(files: &'a [String], query: &str) -> Vec<&'a String> {
    let query = query.to_lowercase();
    let mut found: Vec<&String> = files.iter().filter(|f| f.to_lowercase().contains(&query)).collect();
    found.sort_by_key(|f| (!f.to_lowercase().starts_with(&query), f.len()));
    found.truncate(6);
    found
}

/// Replaces the `@word` at the end of the draft with a mention of the file.
pub fn complete_mention(draft: &mut String, file: &str) {
    if let Some(start) = draft.rfind('@') {
        draft.truncate(start);
        draft.push('@');
        draft.push_str(file);
        draft.push(' ');
    }
}

/// The box to type in, with the queue above it and the files that fit an `@` above that.
fn input(ui: &mut Ui, tab: &mut Tab, files: &[String]) {
    let session = &mut tab.session;
    if let Some(why) = &session.finished
        && !why.is_empty()
    {
        ui.label(RichText::new(why).color(theme::rainbow_mistake()).size(11.5));
    }
    let mut unqueue = None;
    for (n, text) in session.queue.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(crate::i18n::tf!("Waiting to be sent: {}", crate::agent_session::title_from(text))).color(theme::text_faint()).size(11.5));
            if ui.small_button("\u{D7}").on_hover_text(crate::i18n::tr("Don't send this")).clicked() {
                unqueue = Some(n);
            }
        });
    }
    if let Some(n) = unqueue {
        session.queue.remove(n);
    }
    if let Some(query) = mention_at_end(&session.draft) {
        let found = matching_files(files, query);
        let mut chosen = None;
        for file in found {
            if ui.selectable_label(false, RichText::new(file).size(11.5)).clicked() {
                chosen = Some(file.clone());
            }
        }
        if let Some(file) = chosen {
            complete_mention(&mut session.draft, &file);
        }
    }
    let id = egui::Id::unique(("agent-input", tab.number));
    let focused = ui.ctx().memory(|m| m.has_focus(id));
    // Enter sends and Shift+Enter makes a new line, so Enter is taken before the box sees it.
    // (A key matches a pattern without Shift even with Shift held, so Shift is asked about first.)
    let send = focused
        && ui.input_mut(|i| {
            let shifted = i.events.iter().any(|e| matches!(e, egui::Event::Key { key: Key::Enter, pressed: true, modifiers, .. } if modifiers.shift));
            !shifted && !i.modifiers.shift && i.consume_key(Modifiers::NONE, Key::Enter)
        });
    let ended = session.finished.is_some();
    ui.add_enabled_ui(!ended, |ui| {
        ui.add(egui::TextEdit::multiline(&mut session.draft).id(id).desired_rows(3).desired_width(f32::INFINITY).hint_text(crate::i18n::tr("Ask for something. Enter sends, Shift+Enter starts a new line, @ names a file.")));
    });
    ui.horizontal(|ui| {
        let can_send = !session.draft.trim().is_empty() && !ended;
        let label = if session.busy { crate::i18n::tr("Queue") } else { crate::i18n::tr("Send") };
        if (ui.add_enabled(can_send, egui::Button::new(label)).clicked() || (send && can_send)) && can_send {
            let text = std::mem::take(&mut session.draft);
            session.say(&text);
            tab.stick = true;
        }
        if session.busy && ui.button(crate::i18n::tr("Stop")).clicked() {
            session.stop();
        }
        if session.busy {
            ui.spinner();
        }
    });
}

/// Draws a whole session. `reviewing` says whether a request has its review open.
pub fn draw(ui: &mut Ui, tab: &mut Tab, files: &[String], reviewing: &dyn Fn(&str) -> bool) -> Vec<Act> {
    let mut acts = Vec::new();
    top(ui, tab);
    ui.separator();
    let height = ui.available_height();
    // The box at the bottom keeps its room; the transcript has the rest.
    let input_height = 110.0 + 20.0 * tab.session.queue.len() as f32;
    egui::Frame::new().show(ui, |ui| {
        ui.set_height((height - input_height).max(60.0));
        transcript(ui, tab, reviewing, &mut acts);
    });
    ui.separator();
    input(ui, tab, files);
    acts
}

/// The colour of a session's tab: the agent's own colour while it works, quiet otherwise.
pub fn tab_colour(tab: &Tab) -> Color32 {
    if !tab.session.waiting().is_empty() { theme::accent() } else if tab.session.busy { theme::text() } else { theme::text_dim() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_counts_are_short() {
        assert_eq!(compact(950), "950");
        assert_eq!(compact(12_345), "12.3k");
        assert_eq!(compact(1_250_000), "1.2M");
    }

    #[test]
    fn a_mention_is_found_at_the_end_of_the_draft() {
        assert_eq!(mention_at_end("look at @src/ma"), Some("src/ma"));
        assert_eq!(mention_at_end("@"), Some(""));
        assert_eq!(mention_at_end("mail me@home"), None, "an @ inside a word isn't a mention");
        assert_eq!(mention_at_end("look at @src/main.rs and"), None, "already finished");
        assert_eq!(mention_at_end("nothing"), None);
    }

    #[test]
    fn files_that_fit_are_found_shortest_and_best_first() {
        let files: Vec<String> = ["src/main.rs", "src/domain/mod.rs", "README.md", "docs/main.md"].iter().map(|s| s.to_string()).collect();
        let found = matching_files(&files, "main");
        assert_eq!(found, vec!["src/main.rs", "docs/main.md", "src/domain/mod.rs"], "\"domain\" has main in it too, but is longest");
        assert_eq!(matching_files(&files, "READ"), vec!["README.md"]);
        assert!(matching_files(&files, "zzz").is_empty());
    }

    #[test]
    fn completing_a_mention_replaces_what_was_typed() {
        let mut draft = "look at @src/ma".to_string();
        complete_mention(&mut draft, "src/main.rs");
        assert_eq!(draft, "look at @src/main.rs ");
    }

    #[test]
    fn a_reset_today_shows_the_time_and_a_later_one_the_day() {
        let now = 1_800_000_000;
        assert_eq!(reset_text(now + 3600, now).len(), 5);
        assert_eq!(reset_text(now + 3 * 86400, now).len(), 9);
    }

    fn tab() -> (Tab, std::sync::mpsc::Receiver<crate::agent_session::Command>) {
        let (commands, command_rx) = std::sync::mpsc::channel();
        let (_events_tx, events) = std::sync::mpsc::channel();
        let session = Session::new(crate::agent_session::Tool::Claude, std::path::Path::new("/p"), crate::agent_session::Handle { commands, events });
        (Tab::new(session, 7), command_rx)
    }

    fn frame(ctx: &egui::Context, tab: &mut Tab, events: Vec<egui::Event>) {
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(420.0, 800.0))), events, ..Default::default() };
        let _ = ctx.run_ui(input, |ui| {
            draw(ui, tab, &[], &|_| false);
        });
    }

    fn enter(shift: bool) -> egui::Event {
        egui::Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: if shift { Modifiers::SHIFT } else { Modifiers::NONE } }
    }

    #[test]
    fn enter_sends_what_was_typed_and_shift_enter_starts_a_new_line() {
        let ctx = egui::Context::default();
        let (mut tab, commands) = tab();
        frame(&ctx, &mut tab, Vec::new());
        ctx.memory_mut(|m| m.request_focus(egui::Id::unique(("agent-input", 7u64))));
        frame(&ctx, &mut tab, Vec::new());
        frame(&ctx, &mut tab, vec![egui::Event::Text("first".into())]);
        frame(&ctx, &mut tab, vec![enter(true)]);
        frame(&ctx, &mut tab, vec![egui::Event::Text("second".into())]);
        assert_eq!(tab.session.draft, "first\nsecond", "Shift+Enter made a new line and sent nothing");
        assert!(commands.try_recv().is_err());
        frame(&ctx, &mut tab, vec![enter(false)]);
        assert_eq!(commands.try_recv(), Ok(crate::agent_session::Command::Prompt("first\nsecond".into())));
        assert!(tab.session.draft.is_empty(), "the box is cleared");
        assert!(matches!(&tab.session.items[..], [Item::You(text)] if text == "first\nsecond"));
        // While it works, the next message is queued, not sent.
        frame(&ctx, &mut tab, vec![egui::Event::Text("third".into())]);
        frame(&ctx, &mut tab, vec![enter(false)]);
        assert_eq!(tab.session.queue, vec!["third".to_string()]);
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn an_empty_box_sends_nothing() {
        let ctx = egui::Context::default();
        let (mut tab, commands) = tab();
        frame(&ctx, &mut tab, Vec::new());
        ctx.memory_mut(|m| m.request_focus(egui::Id::unique(("agent-input", 7u64))));
        frame(&ctx, &mut tab, Vec::new());
        frame(&ctx, &mut tab, vec![enter(false)]);
        assert!(commands.try_recv().is_err());
        assert!(tab.session.items.is_empty());
    }
}
