//! The window's side of syncing two folders, saved comparisons, rules for comparing files, and applying a patch:
//! the sync's preview of everything it would do, with a second question naming how many files a mirror
//! deletes; saving a comparison by name, in the project or beside the settings, and opening, renaming and
//! deleting them; the rules box, for Settings or one comparison; and the box that checks a patch against a
//! folder or file before applying it.

use super::*;
use crate::compare_rules::{Rule, RuleKind, Rules};
use crate::compare_session::{self, Session};
use crate::compare_sync::{self as sync, Act};
use crate::patch_apply::{self, FileCheck, FilePatch, Fit};

pub(super) fn kind_words(kind: SyncKind) -> &'static str {
    match kind {
        SyncKind::Update => "Update",
        SyncKind::Mirror => "Mirror",
    }
}

pub(super) fn way_words(kind: SyncKind, way: SyncWay) -> &'static str {
    match (kind, way) {
        (SyncKind::Mirror, SyncWay::LeftToRight) => "Make the right like the left",
        (SyncKind::Mirror, SyncWay::RightToLeft) => "Make the left like the right",
        (_, SyncWay::LeftToRight) => "Left to right \u{25B6}",
        (_, SyncWay::RightToLeft) => "\u{25C0} Right to left",
        (_, SyncWay::Both) => "Both ways, newest wins",
    }
}

/// A sync waiting to be agreed to.
pub(crate) struct SyncPreview {
    pub plan: SyncPlan,
    /// Where what's replaced or deleted will be kept.
    pub backups: PathBuf,
    /// The second question a mirror that deletes asks has been reached.
    pub confirming: bool,
    pub left: String,
    pub right: String,
}

fn waking(waker: Option<Context>) -> impl FnOnce() + Send + 'static {
    move || {
        if let Some(ctx) = waker {
            ctx.request_repaint();
        }
    }
}

impl FolderCompare {
    /// Works out what a sync would do, on another thread, to show before it's done.
    pub fn plan_sync(&mut self, backups_under: PathBuf, waker: Option<Context>) {
        let Some(result) = self.result.clone() else {
            self.problem = Some("Compare the folders first.".into());
            return;
        };
        let (kind, way, snapshot) = (self.sync_kind, self.sync_way, self.snapshot.clone());
        self.problem = None;
        self.sync_plan_job = Some(Job::start(
            move |progress, cancel| {
                progress.set_phase("Working out what the sync would do");
                sync::plan_sync(&result, kind, way, snapshot.as_ref(), cancel).map(|plan| (plan, engine::backup_folder(&backups_under)))
            },
            waking(waker),
        ));
    }

    /// Picks up a sync planned, a sync done (comparing again after it), and how both sides were remembered.
    pub fn poll_sync(&mut self, known: crate::remote::KnownHosts, waker: Option<Context>) {
        if let Some(outcome) = self.sync_plan_job.as_ref().and_then(Job::take) {
            self.sync_plan_job = None;
            match outcome {
                Ok((plan, _)) if plan.items.is_empty() && plan.skipped.is_empty() => self.notice = Some("The two sides are already in step, so there's nothing to sync.".into()),
                Ok((plan, backups)) => {
                    let describe = |picker: &SidePicker| picker.side().map_or_else(|_| picker.path.clone(), |side| side.describe());
                    self.sync = Some(SyncPreview { plan, backups, confirming: false, left: describe(&self.pickers[0]), right: describe(&self.pickers[1]) });
                }
                Err(problem) => self.problem = Some(problem),
            }
        }
        if let Some(report) = self.sync_job.as_ref().and_then(Job::take) {
            self.sync_job = None;
            self.notice = Some(report.describe());
            self.snapshot_wanted = true;
            self.start(known, waker.clone());
        }
        if self.snapshot_wanted && self.job.is_none() {
            self.snapshot_wanted = false;
            if let (None, Some(result)) = (&self.problem, self.result.clone()) {
                let previous = self.snapshot.clone();
                self.snapshot_job = Some(Job::start(move |progress, cancel| sync::take_snapshot(&result, previous.as_ref(), progress, cancel), waking(waker)));
            }
        }
        if let Some(snapshot) = self.snapshot_job.as_ref().and_then(Job::take) {
            self.snapshot_job = None;
            self.snapshot = Some(snapshot);
            self.session_dirty = self.session.is_some();
        }
    }

    /// Goes ahead with the sync that was agreed to, on another thread.
    pub fn confirm_sync(&mut self, waker: Option<Context>) {
        let (Some(preview), Some(result)) = (self.sync.take(), self.result.as_ref()) else {
            return;
        };
        let (left, right) = (result.left_tree.clone(), result.right_tree.clone());
        self.sync_job = Some(Job::start(move |progress, cancel| sync::run_sync(&preview.plan, &*left, &*right, &preview.backups, progress, cancel), waking(waker)));
    }

    /// The comparison as it's to be saved, by a name.
    pub fn to_session(&self, name: &str) -> Result<Session, String> {
        Ok(Session {
            name: name.to_string(),
            left: self.pickers[0].side()?,
            right: self.pickers[1].side()?,
            mode: self.mode,
            filters: self.filters.clone(),
            hide_same: self.hide_same,
            sync_kind: self.sync_kind,
            sync_way: self.sync_way,
            snapshot: self.snapshot.clone(),
        })
    }

    /// A comparison opened from one saved.
    pub fn from_session(session: &Session, in_project: bool) -> Self {
        let mut folders = Self::new(SidePicker::of(&session.left), SidePicker::of(&session.right));
        folders.mode = session.mode;
        folders.filters = session.filters.clone();
        folders.hide_same = session.hide_same;
        folders.sync_kind = session.sync_kind;
        folders.sync_way = session.sync_way;
        folders.snapshot = session.snapshot.clone();
        folders.session = Some((session.name.clone(), in_project));
        folders
    }
}

/// The box showing what a sync will do, with a tick for each. Returns whether to go ahead, or Some(false) to
/// cancel.
pub(super) fn sync_preview_window(ctx: &Context, preview: &mut SyncPreview) -> Option<bool> {
    let mut answer = ctx.input(|i| i.key_pressed(Key::Escape)).then_some(false);
    let mirror = preview.plan.kind == SyncKind::Mirror;
    let title = if mirror { "Mirror" } else { "Update" };
    egui::Window::new(title).collapsible(false).resizable(true).default_width(700.0).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        ui.label(format!("Left: {}\nRight: {}", preview.left, preview.right));
        ui.add_space(4.0);
        let counts = preview.plan.counts();
        if preview.confirming {
            let deletions = preview.plan.deletions();
            let side = if preview.plan.way == SyncWay::LeftToRight { "right" } else { "left" };
            ui.label(RichText::new(format!("This deletes {deletions} file{} and folder{} from the {side}.", if deletions == 1 { "" } else { "s" }, if deletions == 1 { "" } else { "s" })).strong().color(theme::mistake()));
            ui.label(format!("Each file deleted or replaced is kept first in {}.", preview.backups.display()));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button(format!("Delete {deletions} and sync")).clicked() {
                    answer = Some(true);
                }
                if ui.button("Back to the list").clicked() {
                    preview.confirming = false;
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(false);
                }
            });
            return;
        }
        ui.label(RichText::new(counts.describe()).strong());
        if counts.conflicts > 0 {
            ui.label(RichText::new(format!("{} changed on both sides since the last sync. They're left alone unless you tick them.", if counts.conflicts == 1 { "One file was".to_string() } else { format!("{} files were", counts.conflicts) })).color(theme::mistake()));
        }
        ui.horizontal(|ui| {
            if ui.small_button("Tick all").clicked() {
                preview.plan.items.iter_mut().for_each(|item| item.ticked = true);
            }
            if ui.small_button("Untick all").clicked() {
                preview.plan.items.iter_mut().for_each(|item| item.ticked = false);
            }
        });
        let row_height = 20.0;
        egui::ScrollArea::vertical().max_height(340.0).auto_shrink([false, true]).show_rows(ui, row_height, preview.plan.items.len(), |ui, range| {
            for item in &mut preview.plan.items[range] {
                ui.horizontal(|ui| {
                    let arrow = if item.to_right { "\u{25B6} right" } else { "\u{25C0} left" };
                    let words = format!("{:<13} {arrow}   {}{}", item.act.words(), item.path, if matches!(item.act, Act::MakeFolder | Act::RemoveFolder) { "/".to_string() } else { format!("  ({})", short_size(item.size)) });
                    let text = if item.conflict { RichText::new(words).color(theme::mistake()) } else if matches!(item.act, Act::Delete | Act::RemoveFolder) { RichText::new(words).strong() } else { RichText::new(words) };
                    ui.checkbox(&mut item.ticked, text);
                    if let Some(note) = &item.note {
                        ui.label(RichText::new(note).color(theme::text_dim()).size(12.0));
                    }
                });
            }
        });
        for (path, why) in preview.plan.skipped.iter().take(40) {
            ui.label(RichText::new(format!("Left alone    {path}: {why}")).color(theme::text_dim()).size(12.5));
        }
        ui.add_space(6.0);
        if counts.replace + counts.delete > 0 {
            ui.label(format!("Each file replaced or deleted is kept first in {}.", preview.backups.display()));
        }
        if !mirror {
            ui.label(RichText::new("An update deletes nothing.").color(theme::text_dim()));
        }
        ui.horizontal(|ui| {
            let ticked = preview.plan.ticked();
            if ui.add_enabled(ticked > 0, Button::new(format!("Sync {ticked} item{}", if ticked == 1 { "" } else { "s" }))).clicked() {
                answer = sync_pressed(preview);
            }
            if ui.button("Cancel").clicked() {
                answer = Some(false);
            }
        });
    });
    answer
}

/// What pressing Sync in the preview does: a sync that deletes asks again first, naming how many; any other goes
/// ahead.
pub(super) fn sync_pressed(preview: &mut SyncPreview) -> Option<bool> {
    if preview.plan.deletions() > 0 && !preview.confirming {
        preview.confirming = true;
        return None;
    }
    Some(true)
}

// ---------- Saved comparisons ----------

/// The box for saving a comparison by name, or for opening, renaming and deleting those saved.
pub(crate) struct SessionBox {
    /// The compare tab being saved, or None to look after those saved.
    pub saving: Option<u64>,
    pub name: String,
    pub in_project: bool,
    /// The saved comparison chosen, by name, and whether it's in the project.
    pub selected: Option<(String, bool)>,
    pub rename: String,
    pub confirm_delete: bool,
    pub problem: String,
}

/// The box of rules for comparing files: for Settings, or for one comparison.
pub(crate) struct RulesBox {
    pub rules: Rules,
    pub selected: usize,
    /// The compare tab whose own rules these are, or None for those in Settings.
    pub tab: Option<u64>,
    pub problem: String,
}

/// The box that checks a patch, then applies it.
pub(crate) struct PatchBox {
    pub patch_file: PathBuf,
    pub patch: Vec<FilePatch>,
    /// The folder or file it's applied to.
    pub target: String,
    pub checks: Option<Result<Vec<FileCheck>, String>>,
    /// The compare tab to compare again once it's applied.
    pub compare_tab: Option<u64>,
}

impl Limoniter {
    /// Opens a pair from a folder comparison as its rule says: bytes as bytes, text with what's left out.
    pub(in crate::app) fn open_ruled_pair(&mut self, left: FileSide, right: FileSide, rule: Option<Rule>) {
        let Some(rule) = rule else {
            return self.open_file_compare_sides(left, right);
        };
        if rule.kind == RuleKind::Binary {
            let read = |side: &FileSide| -> Result<Vec<u8>, String> {
                match side {
                    FileSide::Path(path) => std::fs::read(path).map_err(|e| format!("{} couldn't be read: {e}", path.display())),
                    FileSide::Bytes { bytes, .. } => Ok(bytes.clone()),
                    _ => Err("That side can't be read as bytes.".into()),
                }
            };
            match (read(&left), read(&right)) {
                (Ok(a), Ok(b)) => {
                    let names = [self.side_name(&left), self.side_name(&right)];
                    let title = format!("{} \u{2194} {}", names[0], names[1]);
                    self.add_compare_tab(title, CompareTab::Bytes(Box::new(ByteCompare::new(names, [a, b]))));
                }
                (Err(problem), _) | (_, Err(problem)) => self.message("Compare", problem),
            }
            return;
        }
        let before = self.docs.len();
        self.open_file_compare_sides(left, right);
        if rule.loosens_text() && self.docs.len() > before {
            let ignore = rule.ignore_regex().ok().flatten();
            if let Some(CompareTab::Files(files)) = self.docs[self.active].compare.as_deref_mut() {
                files.diff.options = rule.text;
                files.diff.ignore_lines = ignore;
                files.diff.worked_out = None;
            }
        }
    }

    /// Every saved comparison: those in the open project first, then those beside the settings.
    pub(in crate::app) fn all_sessions(&self) -> Vec<(Session, bool)> {
        let in_project = self.project.iter().flat_map(|project| project.comparisons.iter().cloned().map(|session| (session, true)));
        in_project.chain(self.compare_sessions.iter().cloned().map(|session| (session, false))).collect()
    }

    fn save_sessions_file(&mut self) -> Result<(), String> {
        match &self.compare_sessions_file {
            Some(file) => compare_session::save(file, &self.compare_sessions).map_err(|e| format!("The saved comparisons couldn't be kept in {}.\n\n{e}", file.display())),
            None => Ok(()),
        }
    }

    /// Keeps a comparison, in place of one of the same name.
    pub(in crate::app) fn put_session(&mut self, session: Session, in_project: bool) -> Result<(), String> {
        let place = |list: &mut Vec<Session>, session: Session| match list.iter().position(|kept| kept.name.eq_ignore_ascii_case(&session.name)) {
            Some(at) => list[at] = session,
            None => list.push(session),
        };
        if in_project {
            let project = self.project.as_mut().ok_or("No project is open to keep it in.")?;
            place(&mut project.comparisons, session);
            return project.save().map_err(|e| format!("Couldn't save {}.\n\n{e}", project.path.display()));
        }
        place(&mut self.compare_sessions, session);
        self.save_sessions_file()
    }

    fn remove_session(&mut self, name: &str, in_project: bool) -> Result<(), String> {
        if in_project {
            let project = self.project.as_mut().ok_or("No project is open.")?;
            project.comparisons.retain(|kept| kept.name != name);
            return project.save().map_err(|e| format!("Couldn't save {}.\n\n{e}", project.path.display()));
        }
        self.compare_sessions.retain(|kept| kept.name != name);
        self.save_sessions_file()
    }

    fn rename_session(&mut self, name: &str, in_project: bool, new_name: &str) -> Result<(), String> {
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return Err("Give it a name.".into());
        }
        let taken = self.all_sessions().iter().any(|(kept, place)| *place == in_project && kept.name.eq_ignore_ascii_case(new_name) && kept.name != name);
        if taken {
            return Err(format!("There's already a comparison called {new_name}."));
        }
        let mut session = self.all_sessions().into_iter().find(|(kept, place)| *place == in_project && kept.name == name).map(|(kept, _)| kept).ok_or("It isn't there any more.")?;
        self.remove_session(name, in_project)?;
        session.name = new_name.to_string();
        self.put_session(session, in_project)?;
        // An open tab of it keeps up.
        for doc in &mut self.docs {
            if let Some(CompareTab::Folders(folders)) = doc.compare.as_deref_mut()
                && folders.session.as_ref().is_some_and(|(kept, place)| kept == name && *place == in_project)
            {
                folders.session = Some((new_name.to_string(), in_project));
                doc.untitled = format!("{new_name} (comparison)");
            }
        }
        Ok(())
    }

    /// Stores what a compare tab that's a saved comparison now is, its last sync included.
    pub(in crate::app) fn store_compare_session(&mut self, doc_id: u64) {
        let found = self.with_compare_tab(doc_id, |_, tab| match tab {
            CompareTab::Folders(folders) => folders.session.clone().map(|(name, in_project)| folders.to_session(&name).map(|session| (session, in_project))),
            _ => None,
        });
        let outcome = match found.flatten() {
            Some(Ok((session, in_project))) => self.put_session(session, in_project),
            Some(Err(problem)) => Err(problem),
            None => return,
        };
        if let Err(problem) = outcome {
            self.message("Save the comparison", problem);
        }
    }

    /// Saves the comparison in a tab: as it was saved before, or by a name asked for.
    pub(in crate::app) fn save_compare_session(&mut self, doc_id: u64) {
        let linked = self.with_compare_tab(doc_id, |_, tab| match tab {
            CompareTab::Folders(folders) => Some((folders.session.clone(), folders.to_session("check").map(|_| ()), folders.pickers.clone())),
            _ => None,
        });
        let Some(Some((session, valid, pickers))) = linked else { return };
        if let Err(problem) = valid {
            return self.message("Save the comparison", problem);
        }
        if let Some((name, _)) = session {
            self.store_compare_session(doc_id);
            self.with_compare_tab(doc_id, |_, tab| {
                if let CompareTab::Folders(folders) = tab {
                    folders.notice = Some(format!("Saved as {name}."));
                }
            });
            return;
        }
        let short = |picker: &SidePicker| short_name(Path::new(picker.path.trim_end_matches(['/', '\\'])));
        let suggested = format!("{} and {}", short(&pickers[0]), short(&pickers[1]));
        let in_project = self.project.is_some();
        let taken: Vec<Session> = self.all_sessions().into_iter().filter(|(_, place)| *place == in_project).map(|(session, _)| session).collect();
        self.dialog = Dialog::CompareSession(Box::new(SessionBox { saving: Some(doc_id), name: compare_session::free_name(&suggested, &taken), in_project, selected: None, rename: String::new(), confirm_delete: false, problem: String::new() }));
    }

    /// Saves a tab's comparison by a new name.
    pub(in crate::app) fn save_compare_session_as(&mut self, doc_id: u64, name: &str, in_project: bool) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Give it a name.".into());
        }
        if self.all_sessions().iter().any(|(kept, place)| *place == in_project && kept.name.eq_ignore_ascii_case(name)) {
            return Err(format!("There's already a comparison called {name}. Choose another name."));
        }
        let session = self.with_compare_tab(doc_id, |_, tab| match tab {
            CompareTab::Folders(folders) => folders.to_session(name),
            _ => Err("That tab isn't a folder comparison.".into()),
        });
        let session = session.ok_or("That tab has been closed.")??;
        self.put_session(session, in_project)?;
        self.with_compare_tab(doc_id, |_, tab| {
            if let CompareTab::Folders(folders) = tab {
                folders.session = Some((name.to_string(), in_project));
                folders.notice = Some(format!("Saved as {name}{}.", if in_project { ", in the project" } else { "" }));
            }
        });
        if let Some(doc) = self.docs.iter_mut().find(|doc| doc.id == doc_id) {
            doc.untitled = format!("{name} (comparison)");
        }
        Ok(())
    }

    /// Opens a saved comparison by name, and compares at once. One in the project is looked for first when
    /// `project_first`, otherwise one beside the settings.
    pub(in crate::app) fn open_compare_session(&mut self, name: &str, project_first: bool) {
        let mut all = self.all_sessions();
        if !project_first {
            all.sort_by_key(|(_, in_project)| *in_project);
        }
        let Some((session, in_project)) = all.into_iter().find(|(session, _)| session.name == name) else {
            return self.message("Open a saved comparison", format!("There's no comparison called {name}."));
        };
        let mut folders = FolderCompare::from_session(&session, in_project);
        folders.start(self.known_hosts.clone(), self.waker.clone());
        self.add_compare_tab(format!("{} (comparison)", session.name), CompareTab::Folders(Box::new(folders)));
    }

    pub(in crate::app) fn open_sessions_box(&mut self, selected: Option<String>) {
        let all = self.all_sessions();
        let selected = selected.and_then(|name| all.iter().find(|(session, _)| session.name == name).map(|(session, in_project)| (session.name.clone(), *in_project))).or_else(|| all.first().map(|(session, in_project)| (session.name.clone(), *in_project)));
        let rename = selected.as_ref().map(|(name, _)| name.clone()).unwrap_or_default();
        self.dialog = Dialog::CompareSession(Box::new(SessionBox { saving: None, name: String::new(), in_project: false, selected, rename, confirm_delete: false, problem: String::new() }));
    }

    pub(in crate::app) fn open_rules_box(&mut self, tab: Option<u64>) {
        let rules = match tab {
            Some(doc_id) => self.with_compare_tab(doc_id, |_, tab| if let CompareTab::Folders(folders) = tab { Some(folders.filters.rules.clone()) } else { None }).flatten().unwrap_or_else(|| self.compare_rules.clone()),
            None => self.compare_rules.clone(),
        };
        self.dialog = Dialog::CompareRules(Box::new(RulesBox { rules, selected: 0, tab, problem: String::new() }));
    }

    /// Asks for a patch and opens the box that checks it against a folder or file before applying it: the folder
    /// given, or the file in front for a patch of one file, or the project's folder.
    pub(in crate::app) fn apply_patch_to(&mut self, folder: Option<PathBuf>, compare_tab: Option<u64>) {
        let Some(patch_file) = rfd::FileDialog::new().set_title("Apply a patch").add_filter("Patch", &["patch", "diff"]).add_filter("Any file", &["*"]).pick_file() else {
            return;
        };
        self.open_patch_box(&patch_file, folder, compare_tab);
    }

    /// The box for a patch already chosen.
    pub(in crate::app) fn open_patch_box(&mut self, patch_file: &Path, folder: Option<PathBuf>, compare_tab: Option<u64>) {
        let parsed = std::fs::read(patch_file).map_err(|e| format!("{} couldn't be read: {e}", patch_file.display())).and_then(|bytes| patch_apply::parse(&String::from_utf8_lossy(&bytes)));
        let patch = match parsed {
            Ok(patch) => patch,
            Err(problem) => return self.message("Apply a patch", problem),
        };
        let front = self.docs.get(self.active).filter(|doc| doc.compare.is_none()).and_then(|doc| doc.path.clone());
        let target = folder
            .or_else(|| front.filter(|_| patch.len() == 1))
            .or_else(|| self.project.as_ref().map(Project::folder))
            .or_else(|| patch_file.parent().map(Path::to_path_buf))
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let mut state = PatchBox { patch_file: patch_file.to_path_buf(), patch, target, checks: None, compare_tab };
        check_patch(&mut state);
        self.dialog = Dialog::ApplyPatch(Box::new(state));
    }

    /// Draws the boxes for saved comparisons, rules and patches, if one is open.
    pub(in crate::app) fn compare_dialogs(&mut self, ctx: &Context) -> bool {
        match &self.dialog {
            Dialog::CompareSession(_) => self.session_dialog(ctx),
            Dialog::CompareRules(_) => self.rules_dialog(ctx),
            Dialog::ApplyPatch(_) => self.patch_dialog(ctx),
            _ => return false,
        }
        true
    }

    fn session_dialog(&mut self, ctx: &Context) {
        let all = self.all_sessions();
        let project_name = self.project.as_ref().map(|project| project.name.clone());
        let Dialog::CompareSession(state) = &mut self.dialog else { return };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        enum Do {
            Save(u64, String, bool),
            Open(String, bool),
            Rename(String, bool, String),
            Delete(String, bool),
        }
        let mut wanted = None;
        if let Some(doc_id) = state.saving {
            egui::Window::new("Save this comparison").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label("Both sides, how they're compared, the filters and rules, and the sync, kept by name to open again.");
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Name");
                    let edit = ui.add(egui::TextEdit::singleline(&mut state.name).desired_width(300.0));
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        wanted = Some(Do::Save(doc_id, state.name.clone(), state.in_project));
                    }
                });
                match &project_name {
                    Some(project) => {
                        ui.checkbox(&mut state.in_project, format!("Keep it in the project {project}")).on_hover_text("It's listed under Comparisons in the Project panel, and goes wherever the project goes");
                    }
                    None => {
                        ui.label(RichText::new("It's kept beside Limoniter's settings. Tools, Saved comparisons lists it.").color(theme::text_dim()).size(12.5));
                    }
                }
                if !state.problem.is_empty() {
                    ui.label(RichText::new(&state.problem).color(theme::mistake()));
                }
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        wanted = Some(Do::Save(doc_id, state.name.clone(), state.in_project));
                    }
                    close |= ui.button("Cancel").clicked();
                });
            });
        } else {
            egui::Window::new("Saved comparisons").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                if all.is_empty() {
                    ui.label("None yet. In a folder comparison, Save this comparison keeps one by name.");
                }
                egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                    for (session, in_project) in &all {
                        let chosen = state.selected.as_ref().is_some_and(|(name, place)| name == &session.name && place == in_project);
                        let words = format!("{}{}", session.name, if *in_project { "   (in the project)" } else { "" });
                        let response = ui.selectable_label(chosen, words).on_hover_text(format!("{}\n\u{2194} {}", session.left.describe(), session.right.describe()));
                        if response.clicked() {
                            state.selected = Some((session.name.clone(), *in_project));
                            state.rename = session.name.clone();
                            state.confirm_delete = false;
                        }
                        if response.double_clicked() {
                            wanted = Some(Do::Open(session.name.clone(), *in_project));
                        }
                    }
                });
                ui.add_space(6.0);
                if let Some((name, in_project)) = state.selected.clone() {
                    ui.horizontal(|ui| {
                        if ui.button("Open").clicked() {
                            wanted = Some(Do::Open(name.clone(), in_project));
                        }
                        ui.add(egui::TextEdit::singleline(&mut state.rename).desired_width(200.0));
                        if ui.add_enabled(state.rename.trim() != name, Button::new("Rename")).clicked() {
                            wanted = Some(Do::Rename(name.clone(), in_project, state.rename.clone()));
                        }
                        if state.confirm_delete {
                            if ui.button(RichText::new(format!("Delete {name}")).color(theme::mistake())).clicked() {
                                wanted = Some(Do::Delete(name.clone(), in_project));
                            }
                        } else if ui.button("Delete\u{2026}").on_hover_text("Forgets the saved comparison. No files are touched").clicked() {
                            state.confirm_delete = true;
                        }
                    });
                }
                if !state.problem.is_empty() {
                    ui.label(RichText::new(&state.problem).color(theme::mistake()));
                }
                ui.add_space(4.0);
                close |= ui.button("Close").clicked();
            });
        }
        let outcome = match wanted {
            None => Ok(false),
            Some(Do::Save(doc_id, name, in_project)) => self.save_compare_session_as(doc_id, &name, in_project).map(|()| true),
            Some(Do::Open(name, in_project)) => {
                self.dialog = Dialog::None;
                self.open_compare_session(&name, in_project);
                return;
            }
            Some(Do::Rename(name, in_project, new_name)) => self.rename_session(&name, in_project, &new_name).map(|()| {
                if let Dialog::CompareSession(state) = &mut self.dialog {
                    state.selected = Some((new_name.trim().to_string(), in_project));
                    state.problem.clear();
                }
                false
            }),
            Some(Do::Delete(name, in_project)) => self.remove_session(&name, in_project).map(|()| {
                for doc in &mut self.docs {
                    if let Some(CompareTab::Folders(folders)) = doc.compare.as_deref_mut()
                        && folders.session.as_ref().is_some_and(|(kept, place)| *kept == name && *place == in_project)
                    {
                        folders.session = None;
                    }
                }
                if let Dialog::CompareSession(state) = &mut self.dialog {
                    state.selected = None;
                    state.confirm_delete = false;
                    state.problem.clear();
                }
                false
            }),
        };
        match outcome {
            Ok(done) => close |= done,
            Err(problem) => {
                if let Dialog::CompareSession(state) = &mut self.dialog {
                    state.problem = problem;
                }
            }
        }
        if close {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
    }

    fn rules_dialog(&mut self, ctx: &Context) {
        let usual = self.compare_rules.clone();
        let Dialog::CompareRules(state) = &mut self.dialog else { return };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut keep = false;
        let title = if state.tab.is_some() { "Rules for this comparison" } else { "Rules for comparing files" };
        egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label("How files are compared, by name. The first rule that matches a file decides; one no rule matches is compared as usual.");
            ui.add_space(6.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(250.0);
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for (index, rule) in state.rules.list.iter().enumerate() {
                            let patterns = if rule.patterns.trim().is_empty() { "(no files)" } else { rule.patterns.as_str() };
                            if ui.selectable_label(state.selected == index, format!("{}\n{}", rule.describe(), patterns)).clicked() {
                                state.selected = index;
                            }
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Add").clicked() {
                            let at = (state.selected + 1).min(state.rules.list.len());
                            state.rules.list.insert(at, Rule::new(RuleKind::Text, "*.txt"));
                            state.selected = at;
                        }
                        if ui.add_enabled(!state.rules.list.is_empty(), Button::new("Remove")).clicked() {
                            state.rules.list.remove(state.selected);
                            state.selected = state.selected.min(state.rules.list.len().saturating_sub(1));
                        }
                        if ui.add_enabled(state.selected > 0, Button::new("Up")).clicked() {
                            state.rules.list.swap(state.selected, state.selected - 1);
                            state.selected -= 1;
                        }
                        if ui.add_enabled(state.selected + 1 < state.rules.list.len(), Button::new("Down")).clicked() {
                            state.rules.list.swap(state.selected, state.selected + 1);
                            state.selected += 1;
                        }
                        if ui.button("Put back the usual rules").clicked() {
                            state.rules = Rules::usual();
                            state.selected = 0;
                        }
                        if state.tab.is_some() && ui.button("Use those in Settings").clicked() {
                            state.rules = usual.clone();
                            state.selected = 0;
                        }
                    });
                });
                ui.add_space(16.0);
                let Some(rule) = state.rules.list.get_mut(state.selected) else {
                    ui.label(RichText::new("There are no rules, so every file is compared as usual.").color(theme::text_dim()));
                    return;
                };
                ui.vertical(|ui| {
                    ui.set_width(380.0);
                    egui::Grid::new("compare-rule").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Files");
                        ui.add(egui::TextEdit::singleline(&mut rule.patterns).hint_text("*.log; build/; docs/*.txt").desired_width(260.0)).on_hover_text("Separated by ; or commas. One ending / is for folders only; one with a / inside is matched against the path from the top");
                        ui.end_row();
                        ui.label("Compared");
                        egui::ComboBox::from_id_salt("compare-rule-kind").width(260.0).selected_text(rule.kind.words()).show_ui(ui, |ui| {
                            for kind in RuleKind::ALL {
                                ui.selectable_value(&mut rule.kind, kind, kind.words());
                            }
                        });
                        ui.end_row();
                    });
                    if rule.kind == RuleKind::Text {
                        ui.add_space(4.0);
                        ui.checkbox(&mut rule.text.ignore_whitespace, "Ignore spaces and tabs");
                        ui.checkbox(&mut rule.text.ignore_case, "Ignore case");
                        ui.checkbox(&mut rule.text.ignore_line_endings, "Ignore line endings");
                        ui.label("Lines matching this count as the same as each other:");
                        ui.add(egui::TextEdit::singleline(&mut rule.ignore_lines).hint_text(r"^// Built on \d+").font(egui::TextStyle::Monospace).desired_width(360.0)).on_hover_text("A regular expression, such as one for a line with a date or time in it. Leave empty for none");
                    }
                    let about = match rule.kind {
                        RuleKind::Text => "Compared line by line, with what's ticked left out. Opened side by side with the same left out.",
                        RuleKind::Binary => "Compared byte by byte, and opened to show the bytes that differ.",
                        RuleKind::Picture => "Two files showing the same pixels are the same, whatever their bytes. Opened side by side as pictures.",
                        RuleKind::SizeAndTime => "Never read: the same size and modified time are the same file, even when comparing by contents.",
                        RuleKind::Skip => "Left out of every comparison, and so never synced.",
                    };
                    ui.add_space(4.0);
                    ui.label(RichText::new(about).color(theme::text_dim()).size(12.5));
                });
            });
            if !state.problem.is_empty() {
                ui.label(RichText::new(&state.problem).color(theme::mistake()));
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                keep = ui.button("OK").clicked();
                close |= ui.button("Cancel").clicked();
            });
        });
        if keep {
            if let Some(problem) = state.rules.problem() {
                state.problem = problem;
                return;
            }
            let Dialog::CompareRules(state) = std::mem::replace(&mut self.dialog, Dialog::None) else { return };
            match state.tab {
                Some(doc_id) => {
                    let (known, waker) = (self.known_hosts.clone(), self.waker.clone());
                    self.with_compare_tab(doc_id, |_, tab| {
                        if let CompareTab::Folders(folders) = tab {
                            folders.filters.rules = state.rules;
                            folders.session_dirty = folders.session.is_some();
                            if folders.result.is_some() {
                                folders.start(known, waker);
                            }
                        }
                    });
                    self.store_compare_session_if_dirty(doc_id);
                }
                None => self.compare_rules = state.rules,
            }
            self.pending_focus = true;
        } else if close {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
    }

    fn store_compare_session_if_dirty(&mut self, doc_id: u64) {
        let dirty = self.with_compare_tab(doc_id, |_, tab| if let CompareTab::Folders(folders) = tab { std::mem::take(&mut folders.session_dirty) } else { false });
        if dirty == Some(true) {
            self.store_compare_session(doc_id);
        }
    }

    fn patch_dialog(&mut self, ctx: &Context) {
        let Dialog::ApplyPatch(state) = &mut self.dialog else { return };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut apply = false;
        let mut browse: Option<bool> = None;
        egui::Window::new("Apply a patch").collapsible(false).resizable(true).default_width(620.0).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(format!("The patch: {}", state.patch_file.display()));
            ui.horizontal(|ui| {
                ui.label("Apply it to");
                let edit = ui.add(egui::TextEdit::singleline(&mut state.target).desired_width(300.0).hint_text("A folder, or one file"));
                if edit.lost_focus() {
                    check_patch(state);
                }
                if ui.button("Folder\u{2026}").clicked() {
                    browse = Some(true);
                }
                if ui.button("File\u{2026}").clicked() {
                    browse = Some(false);
                }
            });
            ui.add_space(4.0);
            match &state.checks {
                None => {
                    ui.label(RichText::new("Choose where it's to be applied.").color(theme::text_dim()));
                }
                Some(Err(problem)) => {
                    ui.label(RichText::new(problem).color(theme::mistake()));
                }
                Some(Ok(checks)) => {
                    egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
                        for check in checks {
                            let colour = if check.ready() { theme::text() } else { theme::mistake() };
                            ui.label(RichText::new(format!("{}    {}", check.name, check.describe())).color(colour));
                            let shown: Vec<String> = check
                                .fits
                                .iter()
                                .enumerate()
                                .map(|(n, fit)| match fit {
                                    Fit::Exact => format!("change {} fits", n + 1),
                                    Fit::Moved(by) => format!("change {} fits {} line{} {}", n + 1, by.unsigned_abs(), if by.unsigned_abs() == 1 { "" } else { "s" }, if *by > 0 { "further down" } else { "further up" }),
                                    Fit::Fails => format!("change {} can't be found", n + 1),
                                })
                                .collect();
                            if !shown.is_empty() {
                                ui.label(RichText::new(format!("      {}", shown.join(", "))).color(theme::text_dim()).size(12.0));
                            }
                        }
                    });
                    let ready = checks.iter().filter(|check| check.ready()).count();
                    if ready < checks.len() {
                        ui.label(RichText::new(format!("{} file{} will be left as {} are, because not every change in {} can be found.", checks.len() - ready, if checks.len() - ready == 1 { "" } else { "s" }, if checks.len() - ready == 1 { "it" } else { "they" }, if checks.len() - ready == 1 { "it" } else { "them" })).color(theme::text_dim()));
                    }
                    ui.label(RichText::new("Each file is kept as it was first, in the compare backups folder.").color(theme::text_dim()));
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let ready = state.checks.as_ref().and_then(|checks| checks.as_ref().ok()).map_or(0, |checks| checks.iter().filter(|check| check.ready()).count());
                if ui.add_enabled(ready > 0, Button::new(format!("Apply to {ready} file{}", if ready == 1 { "" } else { "s" }))).clicked() {
                    apply = true;
                }
                close |= ui.button("Cancel").clicked();
            });
        });
        if let Some(folder) = browse {
            let picked = if folder { rfd::FileDialog::new().set_title("The folder to apply it to").pick_folder() } else { rfd::FileDialog::new().set_title("The file to apply it to").pick_file() };
            if let (Some(path), Dialog::ApplyPatch(state)) = (picked, &mut self.dialog) {
                state.target = path.display().to_string();
                check_patch(state);
            }
            return;
        }
        if apply {
            let Dialog::ApplyPatch(state) = std::mem::replace(&mut self.dialog, Dialog::None) else { return };
            let report = self.carry_out_patch(&state);
            self.message("Apply a patch", report);
            return;
        }
        if close {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
    }

    /// Applies a patch that's been checked, and says what it did. A file open here with changes not saved is left
    /// alone; one open and saved is read again.
    pub(in crate::app) fn carry_out_patch(&mut self, state: &PatchBox) -> String {
        let Some(Ok(checks)) = &state.checks else {
            return "Choose where it's to be applied first.".into();
        };
        let mut allowed = Vec::new();
        let mut held = Vec::new();
        for check in checks {
            match self.doc_for_path(&check.target) {
                Some(index) if self.docs[index].is_modified() => held.push((check.name.clone(), "It's open here with changes not saved yet.".to_string())),
                _ => allowed.push(check.clone()),
            }
        }
        let backups = engine::backup_folder(&compare_backups_root().join("Patches"));
        let mut report = patch_apply::apply(&allowed, &backups);
        report.left.extend(held);
        for check in &allowed {
            if let Some(index) = self.doc_for_path(&check.target)
                && check.target.is_file()
                && let Ok(doc) = Document::open(&check.target, self.encoding_prefs.get(&check.target))
            {
                let id = self.docs[index].id;
                self.docs[index] = doc;
                self.docs[index].id = id;
            }
        }
        if let Some(doc_id) = state.compare_tab {
            let (known, waker) = (self.known_hosts.clone(), self.waker.clone());
            self.with_compare_tab(doc_id, |_, tab| {
                if let CompareTab::Folders(folders) = tab {
                    folders.start(known, waker);
                }
            });
        }
        report.describe()
    }
}

/// Checks the patch against where it's to go, writing nothing.
pub(super) fn check_patch(state: &mut PatchBox) {
    let target = state.target.trim();
    state.checks = if target.is_empty() {
        None
    } else if !Path::new(target).exists() {
        Some(Err(format!("{target} isn't there.")))
    } else {
        Some(patch_apply::check(&state.patch, Path::new(target)))
    };
}

#[cfg(test)]
#[path = "sync_host_tests.rs"]
mod sync_host_tests;
