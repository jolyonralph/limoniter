//! Git, the window's side: the Source control panel (what's changed, staging, committing, branches, push and
//! pull), the changes since the last commit in the margin, comparing with older versions, a file's history and
//! who changed each line, settling merge conflicts, and the offer to start keeping history with Git. Git itself
//! runs in the background, so a slow network never holds up the window.

use super::*;
use crate::git::{self, Change, Keep, Status};
use std::collections::{HashMap, HashSet};
use std::time::Instant;

/// Something git is doing in the background, and what it says when it's done.
pub(super) struct Job {
    pub title: String,
    result: Arc<Mutex<Option<Result<String, String>>>>,
}

impl Job {
    pub(super) fn start(title: impl Into<String>, work: impl FnOnce() -> Result<String, String> + Send + 'static) -> Job {
        let result = Arc::new(Mutex::new(None));
        let to = result.clone();
        std::thread::spawn(move || {
            let outcome = work();
            *to.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(outcome);
        });
        Job { title: title.into(), result }
    }

    pub(super) fn take(&self) -> Option<Result<String, String>> {
        self.result.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
    }
}

/// A file's history, in its own box: the commits that changed it.
pub(super) struct HistoryBox {
    pub root: PathBuf,
    pub relative: String,
    pub commits: Vec<git::Commit>,
    pub selected: usize,
}

/// Everything about Git, kept by the window.
#[derive(Default)]
pub(super) struct GitState {
    /// The Source control panel is showing.
    pub open: bool,
    /// Whether git is installed, once that's been looked at.
    pub installed: Option<bool>,
    /// The repository the file in front is in, and its status as last read.
    pub repo: Option<PathBuf>,
    pub repo_of: Option<PathBuf>,
    pub status: Option<Status>,
    reading: Option<(PathBuf, Arc<Mutex<Option<Result<Status, String>>>>)>,
    read_at: Option<Instant>,
    pub job: Option<Job>,
    /// What git last said about something done, whether it was a problem, and when.
    pub said: Option<(String, bool, Instant)>,
    pub message: String,
    pub amend: bool,
    pub new_branch: String,
    /// For each document, the commit its file was last read from, its lines then, and its version when marked.
    heads: HashMap<u64, (String, Arc<Vec<Arc<str>>>)>,
    marked: HashMap<u64, (u64, String)>,
    /// Folders never to offer Git for, and those offered this session.
    pub never: Vec<PathBuf>,
    offered: HashSet<PathBuf>,
    /// The folder the offer bar is showing for.
    pub offer: Option<PathBuf>,
    /// Who last changed the line the cursor's on, shown in the status bar while on.
    pub blame_on: bool,
    blame: Option<(u64, String, Vec<git::Blame>)>,
    pub wizard: Option<Box<super::git_wizard::Wizard>>,
    /// What's typed for putting changes aside, and for a new tag.
    pub stash_message: String,
    pub tag_name: String,
    pub tag_message: String,
    pub push_tag: bool,
    /// What the status bar says about the line the cursor's on, while that's on.
    pub blame_line: Option<String>,
}

impl GitState {
    pub(super) fn from_settings(settings: &str) -> Self {
        let never = settings.lines().filter_map(|line| line.strip_prefix("git_never=")).map(PathBuf::from).collect();
        GitState { never, ..GitState::default() }
    }

    pub(super) fn to_settings(&self) -> String {
        self.never.iter().map(|folder| format!("git_never={}\n", folder.display())).collect()
    }

    /// Reads the status again soon.
    pub(super) fn refresh(&mut self) {
        self.read_at = None;
    }
}

impl Limoniter {
    /// Keeps up with Git: which repository the file in front is in, its status, what git has finished doing, and
    /// the margin's marks. True while git is busy.
    pub(super) fn git_tick(&mut self, ctx: &Context) -> bool {
        let installed = *self.git.installed.get_or_insert_with(|| git::program().is_some());
        if !installed {
            return false;
        }
        let path = self.docs.get(self.active).and_then(|doc| doc.path.clone()).filter(|_| !self.no_file());
        if self.git.repo_of != path {
            self.git.repo_of.clone_from(&path);
            let repo = path.as_deref().and_then(git::repository_of);
            if repo != self.git.repo {
                self.git.repo = repo;
                self.git.status = None;
                self.git.refresh();
            }
            self.consider_offering_git();
        }
        let mut busy = false;
        // What git has finished doing.
        if let Some(job) = &self.git.job {
            match job.take() {
                Some(outcome) => {
                    let title = job.title.clone();
                    self.git.job = None;
                    match outcome {
                        Ok(said) => {
                            let said = said.trim();
                            self.git.said = Some((if said.is_empty() { format!("{title}: done") } else { format!("{title}: {}", said.lines().last().unwrap_or(said)) }, false, Instant::now()));
                        }
                        Err(problem) => self.git.said = Some((format!("{title}: {}", explain(&problem)), true, Instant::now())),
                    }
                    // A pull, a switch or a discard can change files that are open.
                    self.after_restore();
                    self.git.refresh();
                    self.git.heads.clear();
                }
                None => busy = true,
            }
        }
        // The status, read again every few seconds while the panel shows, and now and then otherwise.
        if let Some((root, reading)) = &self.git.reading {
            let done = reading.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
            if let Some(result) = done {
                if Some(root) == self.git.repo.as_ref() {
                    self.git.status = result.ok();
                }
                self.git.reading = None;
            } else {
                busy = true;
            }
        } else if let Some(root) = self.git.repo.clone() {
            let every = if self.git.open { Duration::from_secs(3) } else { Duration::from_secs(15) };
            if self.git.read_at.is_none_or(|at| at.elapsed() > every) && self.git.job.is_none() {
                let reading = Arc::new(Mutex::new(None));
                let to = reading.clone();
                let at = root.clone();
                std::thread::spawn(move || {
                    let status = git::status(&at);
                    *to.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(status);
                });
                self.git.reading = Some((root, reading));
                self.git.read_at = Some(Instant::now());
                busy = true;
            }
        }
        self.mark_git_changes();
        self.git.blame_line = self.blame_text();
        if busy {
            ctx.request_repaint_after(Duration::from_millis(200));
        } else if self.git.open || self.git.repo.is_some() {
            ctx.request_repaint_after(Duration::from_secs(3));
        }
        busy
    }

    /// The marks in the margin for the file in front: its lines against the last commit's.
    fn mark_git_changes(&mut self) {
        let Some(root) = self.git.repo.clone() else {
            return;
        };
        let Some(head) = self.git.status.as_ref().filter(|s| s.born).map(|s| s.head.clone()) else {
            return;
        };
        let doc = &self.docs[self.active];
        let id = doc.id;
        let version = doc.text.version();
        if doc.hex_mode || doc.text.is_disk_backed() || doc.text.line_count() > 200_000 || self.git.marked.get(&id) == Some(&(version, head.clone())) {
            return;
        }
        let Some(relative) = doc.path.as_deref().and_then(|path| git::relative(&root, path)) else {
            return;
        };
        if self.git.heads.get(&id).is_none_or(|(at, _)| *at != head) {
            // The file as it was at the last commit, read as the document reads it; a new file wasn't there.
            let lines = git::file_at(&root, "HEAD", &relative).map(|bytes| {
                let mut format = doc.text.format.clone();
                codec::decode(&bytes, &mut format).lines.into_iter().map(Arc::from).collect::<Vec<Arc<str>>>()
            });
            self.git.heads.insert(id, (head.clone(), Arc::new(lines.unwrap_or_default())));
        }
        let old = self.git.heads[&id].1.clone();
        let doc = &mut self.docs[self.active];
        let now = doc.text.all_lines();
        let mut marks = git::marks(&old, &now);
        // A file that wasn't in the last commit is new all through.
        if old.is_empty() {
            marks.fill(Some(git::Mark::Added));
        }
        doc.text.git_marks = Some(Arc::new(marks));
        self.git.marked.insert(id, (version, head));
    }

    /// Starts git doing something in the background, unless it's already doing something.
    fn git_job(&mut self, title: &str, work: impl FnOnce() -> Result<String, String> + Send + 'static) {
        if self.git.job.is_some() {
            self.git.said = Some(("Git is still busy with the last thing asked.".into(), true, Instant::now()));
            return;
        }
        self.git.job = Some(Job::start(title, work));
    }

    /// The Source control panel.
    pub(super) fn git_panel(&mut self, ui: &mut Ui) {
        if !self.git.open {
            return;
        }
        let mut open = true;
        let mut act: Vec<PanelAct> = Vec::new();
        Panel::right("git-panel").default_size(320.0).size_range(220.0..=700.0).resizable(true).frame(egui::Frame::new().fill(theme::window()).inner_margin(egui::Margin::symmetric(6, 4))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Source control").color(theme::text()).size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::access::called(ui.add(Button::new(RichText::new("\u{D7}").color(theme::text_dim())).frame(false)), "Close").on_hover_text("Close").clicked() {
                        open = false;
                    }
                    if ui.add(Button::new(RichText::new("?").color(theme::text_dim())).frame(false)).on_hover_text("The Git wizard: setting up, and what it all means").clicked() {
                        act.push(PanelAct::Wizard);
                    }
                });
            });
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.git_panel_body(ui, &mut act));
        });
        self.git.open = open;
        for action in act {
            self.git_panel_act(action);
        }
    }

    fn git_panel_body(&mut self, ui: &mut Ui, act: &mut Vec<PanelAct>) {
        let faint = |text: &str| RichText::new(text.to_string()).color(theme::text_dim());
        if self.git.installed == Some(false) {
            ui.label("Git isn't installed on this computer yet. It's free, and the wizard shows how to get it.");
            if ui.button("Set up Git\u{2026}").clicked() {
                act.push(PanelAct::Wizard);
            }
            return;
        }
        let Some(root) = self.git.repo.clone() else {
            ui.add(egui::Label::new(faint("This file's folder isn't keeping its history with Git. With Git, every version you choose to keep is saved, and you can always go back, compare, or put the whole project online.")).wrap());
            ui.add_space(4.0);
            if ui.button("Start keeping history\u{2026}").clicked() {
                act.push(PanelAct::Wizard);
            }
            return;
        };
        let Some(status) = self.git.status.clone() else {
            ui.label(faint("Reading what's changed\u{2026}"));
            return;
        };
        // Where it is: the folder, the branch, and how it stands with what's online.
        ui.label(RichText::new(root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).strong()).on_hover_text(root.display().to_string());
        ui.horizontal_wrapped(|ui| {
            let branch = status.branch.clone().unwrap_or_else(|| "(no branch)".into());
            egui::ComboBox::from_id_salt("git-branch").selected_text(format!("\u{2387} {branch}")).width(150.0).show_ui(ui, |ui| {
                if let Some(root) = &self.git.repo {
                    for each in git::branches(root).unwrap_or_default() {
                        let label = if each.remote { format!("{} (online)", each.name) } else { each.name.clone() };
                        if ui.selectable_label(each.current, label).clicked() && !each.current {
                            let name = if each.remote { each.name.split_once('/').map_or(each.name.clone(), |(_, rest)| rest.to_string()) } else { each.name.clone() };
                            act.push(PanelAct::Switch(name));
                        }
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.git.new_branch).hint_text("new-branch").desired_width(110.0));
                    if ui.add_enabled(!self.git.new_branch.trim().is_empty(), Button::new("New branch")).clicked() {
                        act.push(PanelAct::NewBranch(self.git.new_branch.trim().replace(' ', "-")));
                    }
                });
            });
            let busy = self.git.job.is_some();
            if status.upstream.is_some() || !git::remotes(&root).is_empty() {
                let pull = if status.behind > 0 { format!("Pull \u{2193}{}", status.behind) } else { "Pull".into() };
                let push = if status.ahead > 0 { format!("Push \u{2191}{}", status.ahead) } else { "Push".into() };
                if ui.add_enabled(!busy, Button::new(pull).small()).on_hover_text("Brings in what's been pushed from elsewhere").clicked() {
                    act.push(PanelAct::Pull);
                }
                if ui.add_enabled(!busy && status.born, Button::new(push).small()).on_hover_text("Sends your commits online").clicked() {
                    act.push(PanelAct::Push);
                }
                if ui.add_enabled(!busy, Button::new("Fetch").small()).on_hover_text("Looks for what's new online, changing nothing here").clicked() {
                    act.push(PanelAct::Fetch);
                }
            } else if ui.small_button("Put it online\u{2026}").on_hover_text("Connects this to a repository on GitHub or elsewhere").clicked() {
                act.push(PanelAct::Wizard);
            }
        });
        // Something partway through comes first: it's what needs doing.
        match status.underway {
            Some(git::Underway::Rebase) => {
                egui::Frame::new().fill(theme::selected()).inner_margin(egui::Margin::symmetric(6, 4)).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new("A pull is partway through: some of your commits clash with ones from online. Settle each conflict below, press + beside it, then Carry on.").strong()).wrap());
                    ui.horizontal(|ui| {
                        if ui.button("Carry on").on_hover_text("Each of your commits keeps its own message").clicked() {
                            act.push(PanelAct::Continue);
                        }
                        if ui.button("Abort, back to how it was").clicked() {
                            act.push(PanelAct::Abort(git::Underway::Rebase));
                        }
                    });
                });
            }
            Some(git::Underway::Merge) => {
                egui::Frame::new().fill(theme::selected()).inner_margin(egui::Margin::symmetric(6, 4)).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new("A merge is partway through. Settle each conflict below, press + beside it, then Commit.").strong()).wrap());
                    if ui.button("Abort the merge").clicked() {
                        act.push(PanelAct::Abort(git::Underway::Merge));
                    }
                });
                if self.git.message.is_empty() {
                    self.git.message = git::merge_message(&root).unwrap_or_default();
                }
            }
            None if status.ahead > 0 && status.behind > 0 => {
                ui.add(egui::Label::new(RichText::new(format!("Your {} commit(s) and {} from online have both moved on from the same place.", status.ahead, status.behind)).color(theme::text_dim())).wrap());
                if ui.add_enabled(self.git.job.is_none(), Button::new("Pull, putting mine after theirs")).on_hover_text("A rebase: your commits are made again on top of the ones from online, so there's no merge. If they clash, it stops for you to settle").clicked() {
                    act.push(PanelAct::PullRebase);
                }
            }
            None => {}
        }
        if let Some(job) = &self.git.job {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("{}\u{2026}", job.title));
            });
        } else if let Some((said, problem, when)) = &self.git.said
            && when.elapsed() < Duration::from_secs(20)
        {
            let colour = if *problem { theme::rainbow_mistake() } else { theme::text_dim() };
            ui.add(egui::Label::new(RichText::new(said).color(colour).size(12.0)).wrap());
        }
        ui.add_space(4.0);
        // Committing.
        let staged = status.staged().count();
        let changed = status.unstaged().count();
        ui.add(egui::TextEdit::multiline(&mut self.git.message).hint_text("What this commit does, in a line; more below if needed").desired_rows(2).desired_width(f32::INFINITY));
        ui.horizontal_wrapped(|ui| {
            let message = !self.git.message.trim().is_empty();
            let label = if staged > 0 { format!("Commit {staged} file(s)") } else if changed > 0 { "Commit all changes".to_string() } else { "Commit".to_string() };
            let ready = message && (staged > 0 || changed > 0 || self.git.amend) && self.git.job.is_none();
            if ui.add_enabled(ready, Button::new(label)).on_hover_text(if message { "Keeps this version, with your message" } else { "Write what it does first" }).clicked() {
                act.push(PanelAct::Commit);
            }
            ui.checkbox(&mut self.git.amend, "Add to the last commit").on_hover_text("Puts these changes, and the message, into the last commit instead of a new one. Best only before pushing it");
            if (staged > 0 || changed > 0) && ui.add_enabled(self.assist_job.is_none(), Button::new("Write it for me").small()).on_hover_text(format!("{} writes a message from the changes, for you to read over. It's in the Assistant's history", self.assistant.tool.name())).clicked() {
                act.push(PanelAct::WriteMessage);
            }
        });
        ui.add_space(4.0);
        let open_folder = root.clone();
        let row = |ui: &mut Ui, entry: &git::Entry, staged_list: bool, act: &mut Vec<PanelAct>| {
            ui.horizontal(|ui| {
                let change = if staged_list { entry.staged } else { entry.unstaged };
                let letter = match change {
                    Some(Change::Added) => "A",
                    Some(Change::Deleted) => "D",
                    Some(Change::Renamed) => "R",
                    Some(Change::Untracked) => "U",
                    Some(Change::Conflicted) => "!",
                    _ => "M",
                };
                ui.label(RichText::new(letter).monospace().color(theme::text_dim()));
                let name = entry.path.rsplit('/').next().unwrap_or(&entry.path);
                let hover = format!("{} ({})", entry.path, change.map_or("", Change::word));
                if crate::access::clickable(ui.add(egui::Label::new(name).sense(egui::Sense::click()).truncate())).on_hover_text(hover).clicked() {
                    act.push(PanelAct::Open(open_folder.join(&entry.path)));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if staged_list {
                        if ui.small_button("\u{2212}").on_hover_text("Leave it out of the next commit").clicked() {
                            act.push(PanelAct::Unstage(entry.path.clone()));
                        }
                    } else {
                        if ui.small_button("+").on_hover_text("Put it in the next commit").clicked() {
                            act.push(PanelAct::Stage(entry.path.clone()));
                        }
                        if change != Some(Change::Untracked) && ui.small_button("\u{21B6}").on_hover_text("Throw away its changes, back to the last commit").clicked() {
                            act.push(PanelAct::Discard(entry.path.clone()));
                        }
                    }
                });
            });
        };
        let conflicted: Vec<git::Entry> = status.conflicted().cloned().collect();
        if !conflicted.is_empty() {
            ui.label(RichText::new(format!("Conflicts ({})", conflicted.len())).color(theme::rainbow_mistake()).strong());
            ui.label(faint("Open each one and choose what to keep, then press + to mark it settled."));
            for entry in &conflicted {
                row(ui, entry, false, act);
            }
            ui.add_space(4.0);
        }
        let staged_entries: Vec<git::Entry> = status.staged().cloned().collect();
        if !staged_entries.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Ready to commit ({})", staged_entries.len())).strong());
                if ui.small_button("Leave all out").clicked() {
                    act.push(PanelAct::UnstageAll);
                }
            });
            for entry in &staged_entries {
                row(ui, entry, true, act);
            }
            ui.add_space(4.0);
        }
        let changed_entries: Vec<git::Entry> = status.unstaged().cloned().collect();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Changes ({})", changed_entries.len())).strong());
            if !changed_entries.is_empty() && ui.small_button("Put all in").clicked() {
                act.push(PanelAct::StageAll);
            }
        });
        if changed_entries.is_empty() && staged_entries.is_empty() && conflicted.is_empty() {
            ui.label(faint("Nothing's changed since the last commit."));
        }
        for entry in &changed_entries {
            row(ui, entry, false, act);
        }
        ui.add_space(6.0);
        // Changes put aside, to come back to.
        let stashes = git::stashes(&root);
        egui::CollapsingHeader::new(format!("Put aside ({})", stashes.len())).id_salt("git-stashes").show(ui, |ui| {
            ui.add(egui::Label::new(faint("Puts every change aside, leaving the files as at the last commit, to bring back later: to switch branch, or pull, in the middle of something.")).wrap());
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.git.stash_message).hint_text("What they are (optional)").desired_width(170.0));
                if ui.add_enabled(!status.entries.is_empty(), Button::new("Put aside")).clicked() {
                    act.push(PanelAct::Stash);
                }
            });
            for stash in &stashes {
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(&stash.message).truncate()).on_hover_text(&stash.name);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Throw away").clicked() {
                            act.push(PanelAct::DropStash(stash.name.clone()));
                        }
                        if ui.small_button("Bring back").clicked() {
                            act.push(PanelAct::Unstash(stash.name.clone()));
                        }
                    });
                });
            }
        });
        // Tags, as releases are named.
        egui::CollapsingHeader::new("Tags").id_salt("git-tags").show(ui, |ui| {
            ui.add(egui::Label::new(faint("A tag names this commit for good, as a release is: v1.2.0, say. Pushed, it's online too.")).wrap());
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.git.tag_name).hint_text("v1.0.0").desired_width(80.0));
                ui.add(egui::TextEdit::singleline(&mut self.git.tag_message).hint_text("What it is (optional)").desired_width(140.0));
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.git.push_tag, "And push it");
                let name = self.git.tag_name.trim().to_string();
                if ui.add_enabled(!name.is_empty() && !name.contains(' ') && status.born && self.git.job.is_none(), Button::new("Tag this commit")).clicked() {
                    act.push(PanelAct::Tag(name));
                }
            });
            for tag in git::tags(&root).into_iter().take(8) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&tag.name).strong());
                    ui.label(faint(&format!("{} {}", tag.when, tag.subject)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Push").on_hover_text("Sends this tag online").clicked() {
                            act.push(PanelAct::PushTag(tag.name.clone()));
                        }
                    });
                });
            }
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("History\u{2026}").on_hover_text("The commits that changed the file in front").clicked() {
                act.push(PanelAct::History);
            }
            if ui.small_button("Compare with last commit").clicked() {
                act.push(PanelAct::CompareHead);
            }
        });
    }

    fn git_panel_act(&mut self, action: PanelAct) {
        let Some(root) = self.git.repo.clone() else {
            if matches!(action, PanelAct::Wizard) {
                self.open_git_wizard();
            }
            return;
        };
        let born = self.git.status.as_ref().is_none_or(|s| s.born);
        let say = |state: &mut GitState, result: Result<(), String>| {
            if let Err(problem) = result {
                state.said = Some((explain(&problem), true, Instant::now()));
            }
            state.refresh();
        };
        match action {
            PanelAct::Wizard => self.open_git_wizard(),
            PanelAct::Open(path) => {
                if path.is_file() {
                    self.open_path(&path);
                }
            }
            PanelAct::Stage(path) => say(&mut self.git, git::stage(&root, &[path])),
            PanelAct::Unstage(path) => say(&mut self.git, git::unstage(&root, &[path], born)),
            PanelAct::StageAll => {
                let paths: Vec<String> = self.git.status.as_ref().map(|s| s.unstaged().map(|e| e.path.clone()).collect()).unwrap_or_default();
                say(&mut self.git, git::stage(&root, &paths));
            }
            PanelAct::UnstageAll => {
                let paths: Vec<String> = self.git.status.as_ref().map(|s| s.staged().map(|e| e.path.clone()).collect()).unwrap_or_default();
                say(&mut self.git, git::unstage(&root, &paths, born));
            }
            PanelAct::Discard(path) => {
                if ask_yes_no("Throw away changes", &format!("Throw away the changes to {path} since the last commit?\n\nThis can't be undone.")) {
                    say(&mut self.git, git::discard(&root, std::slice::from_ref(&path)));
                    self.after_restore();
                }
            }
            PanelAct::Commit => self.git_commit(&root),
            PanelAct::Push => {
                let status = self.git.status.clone().unwrap_or_default();
                self.git_job("Push", move || git::push(&root, &status));
            }
            PanelAct::Pull => {
                if self.docs.iter().any(|doc| doc.is_modified() && doc.path.as_deref().is_some_and(|p| p.starts_with(&root))) {
                    self.git.said = Some(("Save your open files first: a pull can change them.".into(), true, Instant::now()));
                    return;
                }
                self.git_job("Pull", move || git::pull(&root));
            }
            PanelAct::Fetch => self.git_job("Fetch", move || git::fetch(&root)),
            PanelAct::Switch(branch) => {
                if self.docs.iter().any(|doc| doc.is_modified() && doc.path.as_deref().is_some_and(|p| p.starts_with(&root))) {
                    self.git.said = Some(("Save your open files first: switching branch changes them.".into(), true, Instant::now()));
                    return;
                }
                let title = format!("Switch to {branch}");
                self.git_job(&title, move || git::switch(&root, &branch).map(|()| String::new()));
            }
            PanelAct::NewBranch(name) => {
                self.git.new_branch.clear();
                say(&mut self.git, git::create_branch(&root, &name));
            }
            PanelAct::History => self.git_history(),
            PanelAct::CompareHead => self.compare_with_commit("HEAD"),
            PanelAct::PullRebase => {
                if self.docs.iter().any(|doc| doc.is_modified() && doc.path.as_deref().is_some_and(|p| p.starts_with(&root))) {
                    self.git.said = Some(("Save your open files first: a pull can change them.".into(), true, Instant::now()));
                    return;
                }
                self.git_job("Pull, mine after theirs", move || git::pull_rebase(&root));
            }
            PanelAct::Continue => self.git_job("Carry on", move || git::rebase_continue(&root)),
            PanelAct::Abort(what) => {
                if ask_yes_no("Abort", "Put everything back as it was before it began?\n\nWhat's been settled so far is let go.") {
                    say(&mut self.git, git::abort(&root, what));
                    self.after_restore();
                }
            }
            PanelAct::Stash => {
                let message = std::mem::take(&mut self.git.stash_message);
                if self.docs.iter().any(|doc| doc.is_modified() && doc.path.as_deref().is_some_and(|p| p.starts_with(&root))) {
                    self.git.said = Some(("Save your open files first: their changes are what's put aside.".into(), true, Instant::now()));
                    return;
                }
                say(&mut self.git, git::stash(&root, &message));
                self.after_restore();
            }
            PanelAct::Unstash(name) => {
                say(&mut self.git, git::unstash(&root, &name, false));
                self.after_restore();
            }
            PanelAct::DropStash(name) => {
                if ask_yes_no("Throw away", "Throw away these changes that were put aside?\n\nThis can't be undone.") {
                    say(&mut self.git, git::drop_stash(&root, &name));
                }
            }
            PanelAct::Tag(name) => {
                let message = std::mem::take(&mut self.git.tag_message);
                self.git.tag_name.clear();
                match git::create_tag(&root, &name, &message) {
                    Ok(()) if self.git.push_tag => self.git_job(&format!("Push {name}"), move || git::push_tag(&root, &name)),
                    Ok(()) => self.git.said = Some((format!("Tagged this commit {name}"), false, Instant::now())),
                    Err(problem) => self.git.said = Some((explain(&problem), true, Instant::now())),
                }
            }
            PanelAct::PushTag(name) => self.git_job(&format!("Push {name}"), move || git::push_tag(&root, &name)),
            PanelAct::WriteMessage => {
                if let Err(problem) = self.assist_commit_message(&root) {
                    self.git.said = Some((problem, true, Instant::now()));
                } else {
                    self.git.said = Some((format!("{} is writing the message\u{2026}", self.assistant.tool.name()), false, Instant::now()));
                }
            }
        }
    }

    /// Commits: what's staged, or if nothing is, every change to the files git already has.
    pub(super) fn git_commit(&mut self, root: &Path) {
        // Unsaved edits aren't in the files git sees.
        let unsaved: Vec<String> = self.docs.iter().filter(|doc| doc.is_modified() && doc.path.as_deref().is_some_and(|p| p.starts_with(root))).map(|doc| doc.name()).collect();
        if !unsaved.is_empty() && !ask_yes_no("Commit", &format!("{} have unsaved edits, which won't be in the commit.\n\nCommit anyway?", unsaved.join(", "))) {
            return;
        }
        let Some(status) = self.git.status.clone() else {
            return;
        };
        let message = self.git.message.trim().to_string();
        let amend = self.git.amend;
        let everything: Vec<String> = if status.staged().count() == 0 { status.unstaged().filter(|e| e.unstaged != Some(Change::Untracked)).map(|e| e.path.clone()).collect() } else { Vec::new() };
        if status.staged().count() == 0 && everything.is_empty() && !amend {
            self.git.said = Some(("Only new files have changed: press + beside each to put it in the commit.".into(), true, Instant::now()));
            return;
        }
        let root = root.to_path_buf();
        self.git.message.clear();
        self.git.amend = false;
        self.git_job("Commit", move || {
            if !everything.is_empty() {
                git::stage(&root, &everything)?;
            }
            git::commit(&root, &message, amend).map(|said| said.lines().next().unwrap_or_default().to_string())
        });
    }

    /// Compares the file in front, as it is now, with a version of it from a commit.
    pub(super) fn compare_with_commit(&mut self, revision: &str) {
        let Some((root, relative)) = self.git_file() else {
            return self.message("Compare with a commit", "This file isn't in a Git repository.".into());
        };
        let Some(bytes) = git::file_at(&root, revision, &relative) else {
            return self.message("Compare with a commit", "This file wasn't in that commit.".into());
        };
        let mut format = self.docs[self.active].text.format.clone();
        let lines: Vec<Arc<str>> = codec::decode(&bytes, &mut format).lines.into_iter().map(Arc::from).collect();
        let short = if revision == "HEAD" { "the last commit".to_string() } else { format!("commit {}", &revision[..revision.len().min(8)]) };
        let name = format!("{} at {short}", self.docs[self.active].name());
        self.show_comparison(Ok((name, crate::big_jobs::Side::Lines(LineStore::from_vec(lines)))));
    }

    /// The repository and the file in front's place in it.
    fn git_file(&self) -> Option<(PathBuf, String)> {
        let path = self.docs.get(self.active)?.path.clone()?;
        let root = git::repository_of(&path)?;
        let relative = git::relative(&root, &path)?;
        Some((root, relative))
    }

    /// The history of the file in front.
    pub(super) fn git_history(&mut self) {
        let Some((root, relative)) = self.git_file() else {
            return self.message("History", "This file isn't in a Git repository.".into());
        };
        match git::log(&root, Some(&relative), 500) {
            Ok(commits) if commits.is_empty() => self.message("History", "This file hasn't been committed yet.".into()),
            Ok(commits) => self.dialog = Dialog::GitHistory(Box::new(HistoryBox { root, relative, commits, selected: 0 })),
            Err(problem) => self.message("History", explain(&problem)),
        }
    }

    /// The box listing a file's history: each commit can be compared with the file now, or opened as it was.
    pub(super) fn git_history_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::GitHistory(state) = &mut self.dialog else {
            return false;
        };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let (mut compare, mut open) = (false, false);
        egui::Window::new(format!("History of {}", state.relative)).collapsible(false).resizable(true).default_size([560.0, 420.0]).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
                for (n, commit) in state.commits.iter().enumerate() {
                    let label = format!("{}  {}  {}", commit.when, commit.author, commit.subject);
                    let response = ui.selectable_label(n == state.selected, label).on_hover_text(&commit.hash);
                    if response.clicked() {
                        state.selected = n;
                    }
                    if response.double_clicked() {
                        compare = true;
                    }
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                compare |= ui.button("Compare with now").clicked();
                open |= ui.button("Open as it was").on_hover_text("A read-only copy, to look at or copy from").clicked();
                close |= ui.button("Close").clicked();
            });
        });
        if compare || open {
            let Dialog::GitHistory(state) = std::mem::replace(&mut self.dialog, Dialog::None) else {
                return true;
            };
            let commit = state.commits[state.selected].clone();
            if compare {
                self.compare_with_commit(&commit.hash);
            } else if let Some(bytes) = git::file_at(&state.root, &commit.hash, &state.relative) {
                let mut format = self.docs[self.active].text.format.clone();
                let lines: Vec<String> = codec::decode(&bytes, &mut format).lines;
                let name = format!("{} at {}", state.relative.rsplit('/').next().unwrap_or(&state.relative), &commit.hash[..8]);
                let mut doc = Document::untitled(name, &PRESETS[0]);
                doc.text.format = format;
                doc.text.set_content(lines);
                doc.text.mark_saved();
                doc.set_read_only(true);
                self.docs.push(doc);
                self.active = self.docs.len() - 1;
            }
            return true;
        }
        if close {
            self.close_dialog();
        }
        true
    }

    /// The stretch of changes the cursor is in, against a version of the file: the version's lines, and the stretch.
    fn hunk_at_cursor(&self, revision: &str) -> Option<(PathBuf, String, Vec<String>, git::Hunk)> {
        let (root, relative) = self.git_file()?;
        let doc = &self.docs[self.active];
        let mut format = doc.text.format.clone();
        let old: Vec<String> = git::file_at(&root, revision, &relative).map(|bytes| codec::decode(&bytes, &mut format).lines).unwrap_or_default();
        let now = doc.text.all_lines();
        let caret = doc.text.caret_line;
        let hunk = git::hunks(&old, &now).into_iter().find(|h| (h.new_start..h.new_start + h.new_lines.len().max(1)).contains(&caret))?;
        Some((root, relative, old, hunk))
    }

    /// Puts just the change the cursor is in into the next commit.
    pub(super) fn stage_change_at_cursor(&mut self) {
        let Some((root, relative, staged, hunk)) = self.hunk_at_cursor("") else {
            return self.message("Put this change in the next commit", "The cursor isn't in a change that isn't in the next commit already.".into());
        };
        let doc = &self.docs[self.active];
        // The staged version is made as text, so only a file in UTF-8 (or plain ASCII) can be staged a part at a time.
        if !matches!(doc.text.format.charset.id.as_str(), "utf-8" | "us-ascii") {
            return self.message("Put this change in the next commit", "Only a UTF-8 file can be committed a change at a time. Press + beside it in Source control to put the whole file in.".into());
        }
        // The staged version keeps the line endings it has, whatever the editor puts at the end of a new line.
        let staged_bytes = git::file_at(&root, "", &relative).unwrap_or_default();
        let ending = if staged_bytes.windows(2).any(|pair| pair == b"\r\n") {
            "\r\n"
        } else if staged_bytes.is_empty() {
            match doc.text.insert_eol() {
                codec::Eol::CrLf => "\r\n",
                _ => "\n",
            }
        } else {
            "\n"
        };
        let ends = staged_bytes.is_empty() || staged_bytes.ends_with(b"\n");
        match git::stage_hunk(&root, &relative, &staged, &hunk, ending, ends) {
            Ok(()) => self.git.said = Some((format!("Put the change at line {} in the next commit", hunk.new_start + 1), false, Instant::now())),
            Err(problem) => self.message("Put this change in the next commit", explain(&problem)),
        }
        self.git.refresh();
    }

    /// Puts the lines the cursor is in back as they were at the last commit: an edit, which Undo takes back.
    pub(super) fn undo_change_at_cursor(&mut self) {
        let Some((_, _, _, hunk)) = self.hunk_at_cursor("HEAD") else {
            return self.message("Undo this change", "The cursor isn't in a change since the last commit.".into());
        };
        let text = &mut self.doc().text;
        let at = hunk.new_start;
        let old: Vec<Arc<str>> = hunk.old_lines.into_iter().map(Arc::from).collect();
        text.splice_lines(at, at + hunk.new_lines.len(), old);
        text.move_caret(at, 0, false);
        text.reveal = true;
        self.pending_focus = true;
    }

    /// Who last changed the line the cursor is on, for the status bar, while that's switched on.
    pub(super) fn blame_text(&mut self) -> Option<String> {
        if !self.git.blame_on {
            return None;
        }
        let (root, relative) = self.git_file()?;
        let doc = &self.docs[self.active];
        let head = self.git.status.as_ref().map(|s| s.head.clone()).unwrap_or_default();
        if self.git.blame.as_ref().is_none_or(|(id, at, _)| *id != doc.id || *at != head) {
            let lines = git::blame(&root, &relative).unwrap_or_default();
            self.git.blame = Some((doc.id, head, lines));
        }
        let line = self.docs[self.active].text.caret_line;
        if self.docs[self.active].text.git_marks.as_ref().and_then(|m| m.get(line).copied().flatten()).is_some_and(|m| m != git::Mark::DeletedAbove) {
            return Some("This line: not committed yet".into());
        }
        let (_, _, lines) = self.git.blame.as_ref()?;
        let blame = lines.get(line)?;
        Some(format!("{}, {}: {}", blame.author, blame.when, blame.subject))
    }

    /// The conflicts in the file in front, if git stopped a merge in it.
    pub(super) fn conflict_bar(&mut self, ui: &mut Ui) {
        let doc = &self.docs[self.active];
        if doc.hex_mode || doc.text.is_disk_backed() || doc.text.line_count() > 200_000 {
            return;
        }
        let in_conflict = self.git_file().is_some_and(|(_, relative)| self.git.status.as_ref().and_then(|s| s.of(&relative)).is_some_and(|e| e.unstaged == Some(Change::Conflicted)));
        if !in_conflict {
            return;
        }
        let lines = doc.text.all_lines();
        let found = git::conflicts(&lines);
        let caret = doc.text.caret_line;
        let here = found.iter().position(|c| c.start <= caret && caret <= c.end);
        let mut chosen: Option<Keep> = None;
        let (mut next, mut settled, mut ask) = (false, false, false);
        Panel::top("conflict-bar").frame(egui::Frame::new().fill(theme::selected()).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if found.is_empty() {
                    ui.label(RichText::new("All the conflicts here are settled.").strong());
                    settled = ui.button("Save, and mark it settled").clicked();
                    return;
                }
                ui.label(RichText::new(format!("{} conflict(s): both sides changed the same lines.", found.len())).strong());
                if here.is_some() {
                    for (keep, label, tip) in [(Keep::Mine, "Keep mine", "The lines above the ======="), (Keep::Theirs, "Keep theirs", "The lines coming in, below it"), (Keep::Both, "Keep both", "Mine, then theirs")] {
                        if ui.button(label).on_hover_text(tip).clicked() {
                            chosen = Some(keep);
                        }
                    }
                    ask = ui.button("Ask the assistant").on_hover_text("Claude Code or Codex works out what each side meant, and suggests lines that do both, for you to look at first").clicked();
                } else {
                    ui.label(RichText::new("Put the cursor in one to choose what to keep.").color(theme::text_dim()));
                }
                next = ui.button("Next conflict").clicked();
            });
        });
        if let (true, Some(at)) = (ask, here) {
            let conflict = found[at].clone();
            if let Err(problem) = self.assist_conflict(conflict.start..conflict.end + 1) {
                self.message("Ask the assistant", problem);
            }
        } else if let (Some(keep), Some(at)) = (chosen, here) {
            let conflict = found[at].clone();
            let kept = git::settle(&lines, &conflict, keep);
            let text = &mut self.doc().text;
            text.replace_lines(conflict.start, conflict.end, kept);
            text.move_caret(conflict.start, 0, false);
            text.reveal = true;
            self.pending_focus = true;
        } else if next && !found.is_empty() {
            let target = found.iter().find(|c| c.start > caret).unwrap_or(&found[0]).start;
            let text = &mut self.doc().text;
            text.move_caret(target, 0, false);
            text.reveal = true;
            self.pending_focus = true;
        } else if settled
            && self.save(self.active)
            && let Some((root, relative)) = self.git_file()
        {
            let result = git::stage(&root, &[relative]);
            if let Err(problem) = result {
                self.message("Mark settled", explain(&problem));
            }
            self.git.refresh();
        }
    }

    /// Offers Git for a file's folder, once a session, unless asked never to: only for a folder that looks like a
    /// project, with a few files in it.
    fn consider_offering_git(&mut self) {
        self.git.offer = None;
        if self.git.repo.is_some() || self.git.installed == Some(false) {
            return;
        }
        let Some(folder) = self.git.repo_of.as_deref().and_then(Path::parent).map(Path::to_path_buf) else {
            return;
        };
        if self.git.offered.contains(&folder) || self.git.never.iter().any(|never| folder.starts_with(never)) {
            return;
        }
        let files = std::fs::read_dir(&folder).map(|entries| entries.flatten().filter(|e| e.path().is_file()).take(5).count()).unwrap_or(0);
        let home_or_root = folder.parent().is_none() || std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).is_some_and(|home| folder == Path::new(&home));
        if files >= 3 && !home_or_root {
            self.git.offer = Some(folder);
        }
    }

    /// The bar offering to keep a folder's history with Git.
    pub(super) fn git_offer_bar(&mut self, ui: &mut Ui) {
        let Some(folder) = self.git.offer.clone() else {
            return;
        };
        let (mut yes, mut later, mut never) = (false, false, false);
        Panel::top("git-offer").frame(egui::Frame::new().fill(theme::selected()).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                ui.label(RichText::new(format!("Keep the history of \u{201C}{name}\u{201D} with Git?")).strong()).on_hover_text("Git keeps every version you choose to, so you can always go back or compare. It's free, it stays on your computer unless you put it online, and the wizard explains each step.");
                yes = ui.button("Show me how\u{2026}").clicked();
                later = ui.button("Not now").clicked();
                never = ui.button("Never for this folder").clicked();
            });
        });
        if yes || later || never {
            self.git.offered.insert(folder.clone());
            self.git.offer = None;
        }
        if never {
            self.git.never.push(folder);
            self.save_settings_if_changed();
        }
        if yes {
            self.open_git_wizard();
        }
    }
}

/// What the Source control panel asks for.
enum PanelAct {
    Wizard,
    Open(PathBuf),
    Stage(String),
    Unstage(String),
    StageAll,
    UnstageAll,
    Discard(String),
    Commit,
    Push,
    Pull,
    Fetch,
    Switch(String),
    NewBranch(String),
    History,
    CompareHead,
    PullRebase,
    Continue,
    Abort(git::Underway),
    Stash,
    Unstash(String),
    DropStash(String),
    Tag(String),
    PushTag(String),
    WriteMessage,
}

/// What git said, with a word about what to do for the problems people meet most.
pub(super) fn explain(said: &str) -> String {
    let lower = said.to_lowercase();
    let hint = if lower.contains("please tell me who you are") || lower.contains("user.email") {
        Some("Git needs your name and email for commits: the wizard (? in the panel) sets them.")
    } else if lower.contains("authentication failed") || lower.contains("could not read username") || lower.contains("terminal prompts disabled") {
        Some("Git couldn't sign in. On Windows and Mac, Git's own sign-in window should appear; if it doesn't, the wizard shows how to sign in to GitHub.")
    } else if lower.contains("non-fast-forward") || lower.contains("fetch first") || lower.contains("rejected") {
        Some("There are commits online that aren't here yet: Pull first, then Push.")
    } else if lower.contains("not possible to fast-forward") || lower.contains("diverging") {
        Some("Your commits and the ones online have both moved on. The simplest way through is at a command line: git pull --rebase.")
    } else if lower.contains("would be overwritten") {
        Some("Commit or throw away your changes to those files first.")
    } else if lower.contains("does not appear to be a git repository") || lower.contains("no such remote") {
        Some("This repository isn't connected to one online yet: Put it online, in the panel, does that.")
    } else {
        None
    };
    match hint {
        Some(hint) => format!("{}\n\n{hint}", said.trim()),
        None => said.trim().to_string(),
    }
}
