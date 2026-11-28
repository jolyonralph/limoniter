//! The window's side of a session's own copy of the project (New session box, the bar over the transcript, the box
//! for Review and merge, and the box for asking the other tool to review): the Git and merging are in
//! `agent_worktree.rs`.

use super::*;
use crate::agent_session::{Item, Mode, Tool};
use crate::agent_worktree::{self as wt, Carry, Changed, Comment, Offer, Plan, Worktree};
use super::agent_host::Choice;
use super::compare_host::FileSide;

/// What the box for Review and merge is showing.
enum Stage {
    /// The copy's files, to look at, and the ways to put them back.
    Review,
    /// Merged or committed; the copy can be removed now. Says what was done.
    Done { lines: Vec<String>, committed: bool },
}

struct MergeBox {
    /// The session's tab number.
    tab: u64,
    rows: Vec<(Changed, Plan)>,
    problem: Option<String>,
    stage: Stage,
}

/// The box for asking the other tool to review.
struct AskBox {
    tab: u64,
    /// The tool to ask.
    tool: Tool,
    /// A session already running to ask (its tab number), or a new one.
    existing: Option<u64>,
    prompt: String,
}

/// One review asked for.
pub(super) struct CrossReview {
    /// The tab number of the session that did the work.
    pub author: u64,
    pub reviewer: u64,
    pub reviewer_tool: Tool,
    /// What the comments' paths are from.
    pub root: PathBuf,
    /// The diff that was handed over.
    pub diff: String,
    /// What the reviewer was sent, which marks where its answer starts.
    pub sent: String,
    /// The reviewer's words, when it has finished.
    pub text: Option<String>,
    pub comments: Vec<Comment>,
    pub passed_back: bool,
}

#[derive(Default)]
pub(super) struct WtState {
    /// Whether the project can have a copy made, found when the New session box opens.
    offer: Option<(PathBuf, Offer)>,
    merge: Option<MergeBox>,
    ask: Option<AskBox>,
    pub reviews: Vec<CrossReview>,
    /// The review being looked at (an index into `reviews`).
    shown: Option<usize>,
    /// What reviewers are asked, kept for the session; empty is the usual words.
    pub prompt: String,
    /// Files changing shows this copy's folder instead of the project's.
    pub view: Option<PathBuf>,
}

/// The line in the New session box for a copy of its own.
fn choice_ui(ui: &mut Ui, choice: &mut Choice, offer: &Offer) {
    let reason = wt::why_not(offer);
    let allowed = reason.is_none();
    if !allowed {
        choice.separate = false;
    }
    let response = ui.add_enabled(allowed, egui::Checkbox::new(&mut choice.separate, crate::i18n::tr("Work in a separate copy (Git worktree)")));
    let response = match &reason {
        Some(why) => response.on_disabled_hover_text(why.as_str()),
        None => response.on_hover_text(crate::i18n::tr("The session gets its own folder on its own branch, so it can't disturb what you are doing. Afterwards you review its changes and merge them in.")),
    };
    let _ = response;
    if let Some(why) = &reason {
        ui.label(egui::RichText::new(why.as_str()).color(theme::text_faint()).size(12.0));
    }
    if choice.separate {
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Name for the task:"));
            ui.add(egui::TextEdit::singleline(&mut choice.task).desired_width(220.0).hint_text(crate::i18n::tr("used for the branch name")));
        });
        if let Offer::Dirty(n) = offer {
            ui.label(egui::RichText::new(crate::i18n::tf!("The project has {n} changes that aren't committed yet. What should the copy start with?")).color(theme::text_dim()).size(12.0));
            ui.radio_value(&mut choice.carry, Carry::Carry, crate::i18n::tr("Carry the changes over to the copy"));
            ui.radio_value(&mut choice.carry, Carry::LastCommit, crate::i18n::tr("Start from the last commit"));
        }
    }
}

impl Limoniter {
    /// The box has opened again, so whether a copy can be made is asked afresh.
    pub(super) fn worktree_forget_offer(&mut self) {
        self.agents.wt.offer = None;
    }

    /// Whether a session in `folder` may have a copy; asked once for each time the box opens.
    fn worktree_offer(&mut self, folder: &Path) -> Offer {
        if let Some((seen, offer)) = &self.agents.wt.offer
            && seen == folder
        {
            return offer.clone();
        }
        let offer = wt::offer(folder);
        self.agents.wt.offer = Some((folder.to_path_buf(), offer.clone()));
        offer
    }

    /// The copy's part of the New session box; true if Start has to wait for a choice.
    pub(super) fn worktree_choice(&mut self, ui: &mut Ui, folder: &Path, choice: &mut Choice) -> bool {
        let offer = self.worktree_offer(folder);
        choice_ui(ui, choice, &offer);
        choice.separate && matches!(offer, Offer::Dirty(_)) && choice.carry == Carry::Ask
    }

    /// The folder a new session works in: the project's, or a copy made for it. A session taken up again has no new copy.
    pub(super) fn worktree_for_start(&mut self, folder: &Path, choice: &Choice, resume: bool) -> Result<(PathBuf, Option<Worktree>), String> {
        if !choice.separate || resume {
            return Ok((folder.to_path_buf(), None));
        }
        let name = if choice.task.trim().is_empty() { chrono::Local::now().format("session-%Y%m%d-%H%M%S").to_string() } else { choice.task.clone() };
        let made = wt::create(folder, &name, choice.carry)?;
        Ok((made.folder.clone(), Some(made)))
    }

    /// The session tab with this number.
    fn tab_numbered(&self, number: u64) -> Option<usize> {
        self.agents.tabs.iter().position(|t| t.number == number)
    }

    /// A session closed: its copy stays where it is, which the person is told.
    pub(super) fn worktree_left_behind(&mut self, copy: &Worktree) {
        self.message("AI agents", crate::i18n::tf!("The session's separate copy is still at {} on the branch {}. Remove it with git worktree remove when you are done with it.", copy.path.display(), copy.branch));
    }

    // ---------- Review and merge ----------

    pub(super) fn worktree_open_merge_box(&mut self, tab: u64) {
        let Some(copy) = self.tab_numbered(tab).and_then(|i| self.agents.tabs[i].worktree.clone()) else { return };
        let (rows, problem) = match wt::preview(&copy) {
            Ok(rows) => (rows, None),
            Err(why) => (Vec::new(), Some(why)),
        };
        self.agents.wt.merge = Some(MergeBox { tab, rows, problem, stage: Stage::Review });
    }

    /// Puts the copy's changes into the project, and opens each file that clashes in the merge view. What was done.
    pub(super) fn worktree_merge_back(&mut self, tab: u64) -> Result<Vec<String>, String> {
        let copy = self.tab_numbered(tab).and_then(|i| self.agents.tabs[i].worktree.clone()).ok_or_else(|| crate::i18n::tr("This session has no separate copy.").to_string())?;
        let report = wt::merge_back(&copy)?;
        let mut lines = Vec::new();
        let mut note = |list: &[String], text: String| {
            if !list.is_empty() {
                lines.push(text);
            }
        };
        note(&report.taken, crate::i18n::tf!("{} files were taken as the copy has them.", report.taken.len()));
        note(&report.merged, crate::i18n::tf!("{} files were changed in both places and fitted together.", report.merged.len()));
        note(&report.same, crate::i18n::tf!("{} files were already the same.", report.same.len()));
        note(&report.left, crate::i18n::tf!("{} files that aren't text were changed in both places and are left as they are: {}", report.left.len(), report.left.join(", ")));
        for conflict in &report.conflicts {
            let format = crate::codec::TextFormat::default();
            let sides = [
                (crate::i18n::tf!("Original: {}", conflict.path), conflict.base.clone()),
                (crate::i18n::tf!("Mine (the project): {}", conflict.path), conflict.mine.clone()),
                (crate::i18n::tf!("Theirs (the copy): {}", conflict.path), conflict.theirs.clone()),
            ];
            self.open_merge(sides, format, &copy.project.join(&conflict.path), None);
        }
        if !report.conflicts.is_empty() {
            lines.push(crate::i18n::tf!("{} files clash with your changes and are open in the merge view: settle each one and save it.", report.conflicts.len()));
        }
        if lines.is_empty() {
            lines.push(crate::i18n::tr("The copy has no changes to merge.").to_string());
        }
        Ok(lines)
    }

    /// Removes a session's copy, closes its session, and says if that failed.
    pub(super) fn worktree_remove(&mut self, tab: u64, delete_branch: bool) -> Result<(), String> {
        let index = self.tab_numbered(tab).ok_or_else(|| crate::i18n::tr("This session has no separate copy.").to_string())?;
        let copy = self.agents.tabs[index].worktree.clone().ok_or_else(|| crate::i18n::tr("This session has no separate copy.").to_string())?;
        // The session works in the folder that's going, so it goes first.
        self.agents.tabs[index].worktree = None;
        self.agent_close(index);
        if self.agents.wt.view.as_deref() == Some(copy.path.as_path()) {
            self.agents.wt.view = None;
        }
        wt::remove(&copy, delete_branch)
    }

    /// The box for Review and merge.
    pub(super) fn worktree_dialogs(&mut self, ctx: &Context) {
        self.worktree_merge_dialog(ctx);
        self.worktree_ask_dialog(ctx);
        self.worktree_review_dialog(ctx);
    }

    fn worktree_merge_dialog(&mut self, ctx: &Context) {
        let Some(mut state) = self.agents.wt.merge.take() else { return };
        let Some(copy) = self.tab_numbered(state.tab).and_then(|i| self.agents.tabs[i].worktree.clone()) else { return };
        let mut open = true;
        let mut look = None;
        let mut action = None;
        crate::fit::Dialog::new(crate::i18n::tr("Review and merge")).open(&mut open).collapsible(false).resizable(true).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(560.0);
            ui.label(crate::i18n::tf!("The copy is on the branch {}, started from {}.", copy.branch, copy.base.chars().take(8).collect::<String>()));
            ui.add_space(4.0);
            if let Some(why) = &state.problem {
                ui.label(egui::RichText::new(why.as_str()).color(theme::rainbow_mistake()));
            }
            match &state.stage {
                Stage::Review => {
                    if state.rows.is_empty() && state.problem.is_none() {
                        ui.label(crate::i18n::tr("The copy has no changes."));
                    } else if !state.rows.is_empty() {
                        ui.label(egui::RichText::new(crate::i18n::tr("Click a file to see its changes beside the project's file.")).color(theme::text_dim()).size(12.0));
                    }
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for (change, plan) in &state.rows {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(crate::i18n::tr(change.what.word())).color(theme::text_dim()));
                                if ui.link(&change.path).clicked() {
                                    look = Some(change.clone());
                                }
                                let (text, clash) = match plan {
                                    Plan::Nothing => (crate::i18n::tr("already the same in the project"), false),
                                    Plan::Write(_) | Plan::Remove => (crate::i18n::tr("goes in as it is"), false),
                                    Plan::Merged(_) => (crate::i18n::tr("fits with your changes"), false),
                                    Plan::Conflict { .. } => (crate::i18n::tr("clashes with your changes: opens in the merge view"), true),
                                    Plan::Unmergeable => (crate::i18n::tr("not text, changed in both places: left alone"), true),
                                };
                                ui.label(egui::RichText::new(text).size(12.0).color(if clash { theme::rainbow_mistake() } else { theme::text_faint() }));
                            });
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button(crate::i18n::tr("Merge into the project")).on_hover_text(crate::i18n::tr("Puts the changes into your files, merging with anything you changed since. Nothing is committed.")).clicked() {
                            action = Some("merge");
                        }
                        if ui.add_enabled(!state.rows.is_empty(), egui::Button::new(crate::i18n::tr("Commit on its branch and leave it"))).clicked() {
                            action = Some("commit");
                        }
                        if ui.button(crate::i18n::tr("Close")).clicked() {
                            action = Some("close");
                        }
                    });
                }
                Stage::Done { lines, committed } => {
                    for line in lines {
                        ui.label(line.as_str());
                    }
                    ui.add_space(8.0);
                    ui.label(if *committed { crate::i18n::tr("Remove the copy's folder now? The branch stays, with the commit on it.") } else { crate::i18n::tr("Remove the copy and its branch now? Its session is closed with it.") });
                    ui.horizontal(|ui| {
                        if ui.button(crate::i18n::tr("Remove it")).clicked() {
                            action = Some(if *committed { "remove-keep" } else { "remove" });
                        }
                        if ui.button(crate::i18n::tr("Keep it")).clicked() {
                            action = Some("close");
                        }
                    });
                }
            }
        });
        if let Some(change) = look {
            let mine = copy.project.join(&change.path);
            let side = |path: PathBuf, name: &str| if path.is_file() { FileSide::Path(path) } else { FileSide::Bytes { name: name.to_string(), bytes: Vec::new() } };
            let left = side(mine, &crate::i18n::tf!("{} (not in the project)", change.path));
            let right = side(copy.path.join(&change.path), &crate::i18n::tf!("{} (deleted in the copy)", change.path));
            self.open_file_compare_sides(left, right);
        }
        match action {
            Some("merge") => match self.worktree_merge_back(state.tab) {
                Ok(lines) => state.stage = Stage::Done { lines, committed: false },
                Err(why) => state.problem = Some(why),
            },
            Some("commit") => {
                let message = crate::i18n::tf!("Changes made in the session: {}", self.tab_numbered(state.tab).map_or(String::new(), |i| self.agents.tabs[i].session.title.clone()));
                match wt::commit(&copy, &message) {
                    Ok(()) => state.stage = Stage::Done { lines: vec![crate::i18n::tf!("Committed on the branch {}. The copy is still at {}.", copy.branch, copy.path.display())], committed: true },
                    Err(why) => state.problem = Some(why),
                }
            }
            Some(removing @ ("remove" | "remove-keep")) => {
                if let Err(why) = self.worktree_remove(state.tab, removing == "remove") {
                    self.message("AI agents", why);
                }
                return;
            }
            Some("close") => return,
            _ => {}
        }
        if open {
            self.agents.wt.merge = Some(state);
        }
    }

    // ---------- The bar over a session's transcript ----------

    /// The bar for the session in front: its copy, and the reviews asked of the other tool.
    pub(super) fn worktree_bar(&mut self, ui: &mut Ui) {
        let Some(tab) = self.agents.tabs.get(self.agents.active.min(self.agents.tabs.len().saturating_sub(1))) else { return };
        let (number, copy, tool, idle, said) = (tab.number, tab.worktree.clone(), tab.session.tool, !tab.session.busy && tab.session.finished.is_none(), tab.session.items.iter().any(|i| matches!(i, Item::You(_))));
        let mut merge = false;
        let mut ask = false;
        let mut view = None;
        let mut show = None;
        if let Some(copy) = &copy {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(crate::i18n::tf!("Separate copy on {}", copy.branch)).color(theme::text_dim()).size(12.0)).on_hover_text(copy.path.display().to_string());
                if ui.small_button(crate::i18n::tr("Review and merge")).clicked() {
                    merge = true;
                }
                if ui.small_button(crate::i18n::tr("Ask the other to review")).clicked() {
                    ask = true;
                }
                let mut shown = self.agents.wt.view.as_deref() == Some(copy.path.as_path());
                if ui.checkbox(&mut shown, crate::i18n::tr("Show it in Files changing")).on_hover_text(crate::i18n::tr("Files changing lists what changes in the copy, instead of in the project.")).changed() {
                    view = Some(shown.then(|| copy.path.clone()));
                }
            });
        } else if idle && said && wt::offer(&self.agent_folder().unwrap_or_default()) != Offer::NotGit {
            ui.horizontal_wrapped(|ui| {
                if ui.small_button(crate::i18n::tr("Ask the other to review")).on_hover_text(crate::i18n::tr("Hands the changes in the project to the other tool to review.")).clicked() {
                    ask = true;
                }
            });
        }
        for (n, review) in self.agents.wt.reviews.iter().enumerate().filter(|(_, r)| r.author == number) {
            ui.horizontal_wrapped(|ui| {
                let name = review.reviewer_tool.name();
                match &review.text {
                    None => ui.label(egui::RichText::new(crate::i18n::tf!("{name} is reviewing...")).color(theme::text_dim()).size(12.0)),
                    Some(_) => ui.label(egui::RichText::new(crate::i18n::tf!("{name} reviewed it: {} comments", review.comments.len())).color(theme::text_dim()).size(12.0)),
                };
                if review.text.is_some() && ui.small_button(crate::i18n::tr("Show the review")).clicked() {
                    show = Some(n);
                }
            });
        }
        if merge {
            self.worktree_open_merge_box(number);
        }
        if ask {
            let other = self.other_tool(tool);
            self.agents.wt.ask = Some(AskBox { tab: number, tool: other, existing: None, prompt: if self.agents.wt.prompt.is_empty() { wt::default_review_prompt() } else { self.agents.wt.prompt.clone() } });
        }
        if let Some(view) = view {
            self.agents.wt.view = view;
        }
        if show.is_some() {
            self.agents.wt.shown = show;
        }
    }

    // ---------- Asking the other tool to review ----------

    /// The tool to ask: the other one if it's here, else the same.
    fn other_tool(&self, tool: Tool) -> Tool {
        let other = if tool == Tool::Claude { Tool::Codex } else { Tool::Claude };
        if self.agent_installed(other) { other } else { tool }
    }

    fn worktree_ask_dialog(&mut self, ctx: &Context) {
        let Some(mut ask) = self.agents.wt.ask.take() else { return };
        let author = self.tab_numbered(ask.tab).map(|i| self.agents.tabs[i].session.tool);
        let Some(author) = author else { return };
        let candidates: Vec<(u64, String)> = self.agents.tabs.iter().filter(|t| t.number != ask.tab && t.session.tool == ask.tool && t.session.finished.is_none()).map(|t| (t.number, t.session.title.clone())).collect();
        let mut open = true;
        let mut send = false;
        let mut cancel = false;
        crate::fit::Dialog::new(crate::i18n::tr("Ask the other to review")).open(&mut open).collapsible(false).resizable(true).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.label(crate::i18n::tf!("{} will be given the changes and asked to review them. It won't be asked to change anything.", ask.tool.name()));
            if ask.tool == author {
                ui.label(egui::RichText::new(crate::i18n::tr("Only one of Claude Code and Codex is installed, so the same one reviews.")).color(theme::text_faint()).size(12.0));
            }
            ui.add_space(4.0);
            ui.radio_value(&mut ask.existing, None, crate::i18n::tf!("A new {} session", ask.tool.name()));
            for (number, title) in &candidates {
                ui.radio_value(&mut ask.existing, Some(*number), crate::i18n::tf!("The session: {}", title));
            }
            ui.add_space(4.0);
            ui.label(crate::i18n::tr("What it is asked:"));
            ui.add(egui::TextEdit::multiline(&mut ask.prompt).desired_rows(5).desired_width(f32::INFINITY));
            if ui.small_button(crate::i18n::tr("Use the usual words")).clicked() {
                ask.prompt = wt::default_review_prompt();
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("Ask")).clicked() {
                    send = true;
                }
                if ui.button(crate::i18n::tr("Cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        if send {
            self.agents.wt.prompt = if ask.prompt.trim() == wt::default_review_prompt() { String::new() } else { ask.prompt.clone() };
            if let Err(why) = self.worktree_ask(ctx, ask.tab, ask.tool, ask.existing, &ask.prompt) {
                self.message("AI agents", why);
            }
        } else if open && !cancel {
            self.agents.wt.ask = Some(ask);
        }
    }

    /// Hands a session's changes to a session of `tool` (a new one, or the one numbered `existing`) to review.
    pub(super) fn worktree_ask(&mut self, ctx: &Context, author: u64, tool: Tool, existing: Option<u64>, prompt: &str) -> Result<(), String> {
        let index = self.tab_numbered(author).ok_or_else(|| crate::i18n::tr("That session has gone.").to_string())?;
        let (copy, folder) = (self.agents.tabs[index].worktree.clone(), self.agents.tabs[index].session.folder.clone());
        let (diff, root) = match &copy {
            Some(copy) => (wt::diff_text(copy)?, copy.path.clone()),
            None => (wt::diff_of_project(&folder)?, crate::git::repository_of(&folder).unwrap_or_else(|| folder.clone())),
        };
        if diff.trim().is_empty() {
            return Err(crate::i18n::tr("There are no changes to review yet.").to_string());
        }
        let message = wt::review_message(prompt, &diff);
        let reviewer = match existing.and_then(|n| self.tab_numbered(n)) {
            Some(i) => i,
            None => {
                // A reviewer only reads, so it is given the mode that changes least.
                let mode = if tool == Tool::Codex { Mode::Plan } else { Mode::Ask };
                let (last, active) = (self.agents.last.clone(), self.agents.active);
                let choice = Choice { tool, mode, ..Choice::default() };
                let started = self.agent_start(ctx, &folder, &choice, None);
                self.agents.last = last;
                started?;
                self.agents.active = active.min(self.agents.tabs.len() - 1);
                self.agents.tabs.len() - 1
            }
        };
        let number = self.agents.tabs[reviewer].number;
        self.agents.tabs[reviewer].session.say(&message);
        self.agents.wt.reviews.push(CrossReview { author, reviewer: number, reviewer_tool: tool, root, diff, sent: message, text: None, comments: Vec::new(), passed_back: false });
        Ok(())
    }

    /// Takes in a reviewer's answer once it has finished.
    pub(super) fn worktree_tick(&mut self) {
        for n in 0..self.agents.wt.reviews.len() {
            if self.agents.wt.reviews[n].text.is_some() {
                continue;
            }
            let Some(index) = self.tab_numbered(self.agents.wt.reviews[n].reviewer) else {
                continue;
            };
            let session = &self.agents.tabs[index].session;
            if session.busy || !session.queue.is_empty() {
                continue;
            }
            let sent = &self.agents.wt.reviews[n].sent;
            let Some(at) = session.items.iter().rposition(|i| matches!(i, Item::You(t) if t == sent)) else { continue };
            let said: Vec<&str> = session.items[at + 1..].iter().filter_map(|i| if let Item::Text { text, .. } = i { Some(text.as_str()) } else { None }).collect();
            if said.is_empty() {
                continue;
            }
            let text = said.join("\n\n");
            let review = &mut self.agents.wt.reviews[n];
            review.comments = wt::parse_comments(&text);
            review.text = Some(text);
        }
    }

    /// Passes a review back to the session that was reviewed, as its next message.
    pub(super) fn worktree_send_back(&mut self, n: usize) {
        let Some(review) = self.agents.wt.reviews.get(n) else { return };
        let Some(text) = review.text.clone() else { return };
        let Some(index) = self.tab_numbered(review.author) else { return };
        let message = wt::send_back_message(review.reviewer_tool.name(), &text);
        self.agents.tabs[index].session.say(&message);
        self.agents.wt.reviews[n].passed_back = true;
    }

    /// The box showing a review: the comments, each a click to its line, and the diff they are about.
    fn worktree_review_dialog(&mut self, ctx: &Context) {
        let Some(n) = self.agents.wt.shown else { return };
        let Some(review) = self.agents.wt.reviews.get(n) else {
            self.agents.wt.shown = None;
            return;
        };
        let (name, text, root, diff, passed) = (review.reviewer_tool.name(), review.text.clone().unwrap_or_default(), review.root.clone(), review.diff.clone(), review.passed_back);
        let comments = review.comments.clone();
        let mut open = true;
        let mut go = None;
        let mut back = false;
        crate::fit::Dialog::new(crate::i18n::tf!("Review by {name}")).open(&mut open).collapsible(false).resizable(true).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(640.0);
            if comments.is_empty() {
                ui.label(crate::i18n::tr("It made no comments about particular lines."));
            } else {
                ui.label(crate::i18n::tr("Click a comment to go to its line."));
                egui::ScrollArea::vertical().id_salt("review-comments").max_height(160.0).show(ui, |ui| {
                    for comment in &comments {
                        ui.horizontal_wrapped(|ui| {
                            if ui.link(format!("{}:{}", comment.path, comment.line)).clicked() {
                                go = Some(comment.clone());
                            }
                            ui.label(comment.text.as_str());
                        });
                    }
                });
            }
            egui::CollapsingHeader::new(crate::i18n::tr("What it said")).id_salt("review-said").show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt("review-said-text").max_height(140.0).show(ui, |ui| {
                    ui.label(text.as_str());
                });
            });
            egui::CollapsingHeader::new(crate::i18n::tr("The changes it looked at")).id_salt("review-diff").show(ui, |ui| {
                egui::ScrollArea::both().id_salt("review-diff-text").max_height(200.0).show(ui, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(diff.as_str()).monospace().size(12.0)).extend());
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(!passed, egui::Button::new(crate::i18n::tr("Send the review back"))).on_hover_text(crate::i18n::tr("Gives it to the session that made the changes, as its next message.")).clicked() {
                    back = true;
                }
                if passed {
                    ui.label(egui::RichText::new(crate::i18n::tr("Sent.")).color(theme::text_dim()));
                }
            });
        });
        if let Some(comment) = go {
            self.go_to_place(&root.join(&comment.path).display().to_string(), comment.line);
        }
        if back {
            self.worktree_send_back(n);
        }
        if !open {
            self.agents.wt.shown = None;
        }
    }

    /// A step before a screenshot: a session in a copy, with its bar, and a review of it shown.
    pub(super) fn worktree_screenshot_step(&mut self, value: &str) {
        let folder = self.agent_folder().unwrap_or_else(|| PathBuf::from("."));
        self.agents_screenshot_step("top");
        let copy = Worktree { project: folder.clone(), path: folder.join("../app.limoniter/fix-parser"), branch: "limoniter/fix-parser".into(), base: "1f2e3d4c5b6a".into(), folder };
        self.agents.tabs[0].worktree = Some(copy);
        let review = CrossReview {
            author: self.agents.tabs[0].number,
            reviewer: 0,
            reviewer_tool: Tool::Codex,
            root: PathBuf::from("."),
            diff: "diff --git a/src/parser.rs b/src/parser.rs\n--- a/src/parser.rs\n+++ b/src/parser.rs\n@@ -10,3 +10,4 @@\n     let mut lines = text.lines();\n-    lines.next_back();\n+    let last = lines.next_back();\n".into(),
            sent: String::new(),
            text: Some("src/parser.rs:11: `last` is never used, so the final line is still dropped.\nsrc/parser_tests.rs:4: This test doesn't cover a file with no final newline.\n\nNot ready yet.".into()),
            comments: Vec::new(),
            passed_back: false,
        };
        let mut review = review;
        review.comments = wt::parse_comments(review.text.as_deref().unwrap_or(""));
        self.agents.wt.reviews = vec![review];
        if value.contains("review") {
            self.agents.wt.shown = Some(0);
        } else if value.contains("ask") {
            self.agents.wt.ask = Some(AskBox { tab: self.agents.tabs[0].number, tool: Tool::Codex, existing: None, prompt: wt::default_review_prompt() });
        }
    }
}

#[cfg(test)]
#[path = "agent_worktree_host_tests.rs"]
mod agent_worktree_host_tests;
