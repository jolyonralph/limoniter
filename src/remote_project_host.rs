//! The window's side of remote projects (see `remote_project`): opening a folder on a server as a project, the
//! work with its server done in the background one piece at a time, each save sent back, the server's changes
//! fetched now and then, and the window that shows what a sync would do, conflicts and all, to be agreed to.

use super::*;
use crate::compare_sync::{Act, SyncPlan, SyncReport};
use crate::diff_engine::{Job as Work, LocalTree, SftpTree, Tree};
use crate::remote::{self, KnownHosts, Refused, Secret, Site};
use crate::remote_project::{self, MirrorState, Origin, Places, PushError};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

/// Noted when the status bar's words about the server are clicked.
pub(super) const STATUS_CLICKED: &str = "remote-project-status-clicked";

/// How often the server is looked at for changes, while the project is open.
const FETCH_EVERY: Duration = Duration::from_secs(120);

/// Something asked of the server for the project. They're done one at a time, in order, so what's known of the
/// mirror is never changed by two at once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Op {
    /// Compare with the server and fetch what's new there; with `ask`, then show what's left to decide.
    Refresh { ask: bool },
    /// Do what's ticked in a plan the person has looked at.
    Carry(SyncPlan),
    /// Send a file that has been saved.
    Push { path: String, force: bool },
    /// Fetch a file left on the server, and open it.
    Fetch(String),
    Rename { from: String, to: String },
    Remove(String),
    MakeFolder(String),
}

/// How an `Op` came out.
pub(super) enum Done {
    Refreshed { plan: SyncPlan, report: SyncReport, ask: bool },
    Carried(SyncReport),
    Pushed,
    Fetched(String),
    Changed,
    ChangedOnServer(String),
    Refused(Refused),
    Failed(String),
}

/// The window showing what a sync would do.
pub(super) struct SyncWindow {
    pub plan: SyncPlan,
}

/// A folder on a server open as the project.
pub(super) struct RemoteProject {
    pub origin: Origin,
    pub places: Places,
    pub state: MirrorState,
    pub queue: VecDeque<Op>,
    running: Option<(Op, Work<(Done, MirrorState)>)>,
    last_fetch: Option<Instant>,
    /// What the last comparison left to decide: changes here not sent, conflicts, deletions.
    pub waiting: Option<SyncPlan>,
    pub window: Option<SyncWindow>,
    /// Why the server couldn't be reached or signed in to, the last time, for the status bar.
    pub stalled: Option<String>,
    /// The server refused the sign-in (or its key isn't trusted): nothing more is tried until the person
    /// connects again, or asks.
    pub refused: bool,
}

impl RemoteProject {
    fn new(origin: Origin, places: Places) -> Self {
        let state = MirrorState::load(&places, &origin);
        Self { origin, places, state, queue: VecDeque::new(), running: None, last_fetch: None, waiting: None, window: None, stalled: None, refused: false }
    }

    /// Asks for something, once: the same thing waiting already isn't asked for again.
    pub fn ask(&mut self, op: Op) {
        if !self.queue.contains(&op) {
            self.queue.push_back(op);
        }
    }

    #[cfg(test)]
    pub fn busy(&self) -> bool {
        self.running.is_some() || !self.queue.is_empty()
    }

    /// A file's path in the mirror, from the top, if it's in it.
    pub fn relative(&self, path: &Path) -> Option<String> {
        remote_project::relative(&self.places.files, path)
    }
}

/// Does one piece of work on a connection of its own.
fn perform(op: Op, origin: &Origin, places: &Places, mut state: MirrorState, keep: &[String], secret: &Secret, known: &KnownHosts, progress: &crate::diff_engine::Progress, cancel: &AtomicBool) -> (Done, MirrorState) {
    progress.set_phase(crate::i18n::tf!("Connecting to {}", origin.site.host));
    let connection = match remote::connect(&origin.site, secret, known) {
        Ok(connection) => connection,
        Err(refused) => return (Done::Refused(refused), state),
    };
    let remote: Arc<dyn Tree> = match SftpTree::with(Box::new(connection), &origin.site, &origin.folder) {
        Ok(tree) => Arc::new(tree),
        Err(problem) => return (Done::Failed(problem), state),
    };
    if let Err(e) = std::fs::create_dir_all(&places.files) {
        return (Done::Failed(e.to_string()), state);
    }
    let local: Arc<dyn Tree> = Arc::new(LocalTree { root: places.files.clone() });
    let done = match op {
        Op::Refresh { ask } => match remote_project::refresh(local.clone(), remote.clone(), &places.files, &mut state, progress, cancel) {
            Ok(plan) => {
                let mut part = remote_project::fetch_part(&plan);
                part.items.retain(|item| !keep.contains(&item.path));
                let report = remote_project::carry_out(&part, &*local, &*remote, &mut state, &places.backups, progress, cancel);
                Done::Refreshed { plan: remote_project::without(&plan, &part), report, ask }
            }
            Err(problem) => Done::Failed(problem),
        },
        Op::Carry(plan) => Done::Carried(remote_project::carry_out(&plan, &*local, &*remote, &mut state, &places.backups, progress, cancel)),
        Op::Push { path, force } => match remote_project::push(&path, &*local, &*remote, &mut state, force, cancel) {
            Ok(()) => Done::Pushed,
            Err(PushError::ChangedOnServer) => Done::ChangedOnServer(path),
            Err(PushError::Failed(problem)) => Done::Failed(crate::i18n::tf!("Couldn't send {path} to {}.\n\n{problem}\n\nYour changes are saved on this computer.", origin.site.host)),
        },
        Op::Fetch(path) => match remote_project::fetch(&path, &*local, &*remote, &mut state, cancel) {
            Ok(()) => Done::Fetched(path),
            Err(problem) => Done::Failed(problem),
        },
        Op::Rename { from, to } => remote_project::rename(&from, &to, &*remote, &mut state).map_or_else(Done::Failed, |()| Done::Changed),
        Op::Remove(path) => remote_project::remove(&path, &*remote, &mut state).map_or_else(Done::Failed, |()| Done::Changed),
        Op::MakeFolder(path) => remote.make_folder(&path).map_or_else(Done::Failed, |()| Done::Changed),
    };
    (done, state)
}

impl Limoniter {
    /// Where remote projects' mirrors are kept: beside the copies of remote files.
    fn remote_projects_base(&self) -> PathBuf {
        self.remote_root.as_ref().and_then(|copies| copies.parent()).map_or_else(|| std::env::temp_dir().join("limoniter-remote-projects"), |data| data.join("remote-projects"))
    }

    /// File, Open a remote folder: the remote box, to choose a folder rather than a file.
    pub(super) fn open_remote_folder_box(&mut self) {
        self.open_remote_box(None);
        if let Dialog::Remote(state) = &mut self.dialog {
            state.project = true;
            // The project open already, to connect to again.
            if let Some(project) = &self.remote_project {
                state.address = project.origin.url();
                state.key_file = project.origin.site.key_file.as_ref().map_or_else(String::new, |k| k.display().to_string());
            }
        }
    }

    /// Opens a folder on a server as the project: its mirror here opened as a folder project, remembering where it
    /// came from, and the server's files fetched into it.
    pub(super) fn open_remote_project(&mut self, site: Site, folder: String) {
        let origin = Origin { site, folder };
        if let Some(project) = &mut self.remote_project
            && project.origin == origin
        {
            // The same one, connected to again.
            project.refused = false;
            project.ask(Op::Refresh { ask: false });
            return;
        }
        let places = remote_project::places(&self.remote_projects_base(), &origin);
        if let Err(e) = std::fs::create_dir_all(&places.files) {
            return self.message("Open a remote folder", crate::i18n::tf!("Couldn't make a folder for its files at {}.\n\n{e}", places.files.display()));
        }
        self.open_folder(&places.files);
        let Some(project) = self.project.as_mut().filter(|p| p.root.as_deref().is_some_and(|root| same_path(root, &places.files))) else { return };
        if project.remote.as_ref() != Some(&origin) {
            project.remote = Some(origin.clone());
            project.name = crate::i18n::tf!("{} on {}", origin.name(), origin.site.host);
            self.save_project();
        }
        if self.remote_project.as_ref().is_none_or(|p| p.origin != origin) {
            self.start_remote_project();
        }
    }

    /// Takes up the server of the project just opened, if it came from one.
    pub(super) fn start_remote_project(&mut self) {
        self.remote_project = None;
        let Some((origin, root)) = self.project.as_ref().and_then(|p| Some((p.remote.clone()?, p.root.clone()?))) else { return };
        let mut places = remote_project::places(&self.remote_projects_base(), &origin);
        // Where the project says its files are, should Limoniter's own folders have moved.
        if !same_path(&places.files, &root) {
            places = Places { state: root.with_extension("mirror.json"), backups: root.with_extension("backups"), files: root };
        }
        let mut project = RemoteProject::new(origin, places);
        project.ask(Op::Refresh { ask: false });
        self.remote_project = Some(project);
    }

    /// The way to sign in to the project's server is known, so it can be connected to without asking.
    fn can_sign_in_quietly(&self, site: &Site) -> bool {
        let anonymous = site.scheme.is_ftp() && (site.user.is_empty() || site.user.eq_ignore_ascii_case("anonymous"));
        anonymous || self.remote_secrets.contains_key(&site.key()) || site.key_file.is_some() || remote::stored_password(site).is_some()
    }

    /// Starts what's waiting, takes in what has finished, and asks the server for changes now and then. True
    /// while anything is under way.
    pub(super) fn remote_project_tick(&mut self, ctx: &Context) -> bool {
        let Some(project) = &mut self.remote_project else { return false };
        let finished = project.running.as_ref().and_then(|(_, work)| work.take());
        if let Some((done, state)) = finished {
            let (op, _) = project.running.take().expect("it was running");
            project.state = state;
            let _ = project.state.save(&project.places);
            self.remote_project_done(op, done);
        }
        // Now and then, the server is looked at for changes, if that needs nothing asked. (Whether it does is found
        // only then, as it can mean asking the system's credential store.)
        let due = self.remote_project.as_ref().is_some_and(|p| p.running.is_none() && p.queue.is_empty() && !p.refused && p.last_fetch.is_none_or(|at| at.elapsed() > FETCH_EVERY));
        if due {
            let quietly = self.remote_project.as_ref().is_some_and(|p| self.can_sign_in_quietly(&p.origin.site));
            if let Some(project) = &mut self.remote_project {
                if quietly {
                    project.queue.push_back(Op::Refresh { ask: false });
                } else {
                    project.last_fetch = Some(Instant::now());
                }
            }
        }
        let next = self.remote_project.as_ref().is_some_and(|p| p.running.is_none() && !p.refused && !p.queue.is_empty());
        if next && let Some(secret) = self.remote_project.as_ref().map(|p| self.secret_for(&p.origin.site)) {
            let known = self.known_hosts.clone();
            // Files open with changes not saved yet aren't fetched over; they're left to sync.
            let keep: Vec<String> = self.remote_project.as_ref().map_or_else(Vec::new, |p| self.docs.iter().filter(|doc| doc.is_modified()).filter_map(|doc| p.relative(doc.path.as_deref()?)).collect());
            if let Some(project) = &mut self.remote_project
                && let Some(op) = project.queue.pop_front()
            {
                if matches!(op, Op::Refresh { .. }) {
                    project.last_fetch = Some(Instant::now());
                }
                let (origin, places, state, repaint) = (project.origin.clone(), project.places.clone(), project.state.clone(), ctx.clone());
                let worker_op = op.clone();
                let work = Work::start(move |progress, cancel| perform(worker_op, &origin, &places, state, &keep, &secret, &known, progress, cancel), move || repaint.request_repaint());
                project.running = Some((op, work));
            }
        }
        ctx.request_repaint_after(FETCH_EVERY);
        self.remote_project.as_ref().is_some_and(|p| p.running.is_some())
    }

    fn remote_project_done(&mut self, op: Op, done: Done) {
        let Some(project) = &mut self.remote_project else { return };
        let (host, files) = (project.origin.site.host.clone(), project.places.files.clone());
        match done {
            Done::Refreshed { plan, report, ask } => {
                project.stalled = None;
                project.refused = false;
                project.waiting = (!plan.items.is_empty()).then(|| plan.clone());
                let alike = plan.items.is_empty();
                if ask && !alike {
                    project.window = Some(SyncWindow { plan });
                }
                if ask && alike {
                    self.message("Sync with the server", crate::i18n::tf!("The project and {host} are alike: there's nothing to sync."));
                }
                if ask && !report.failed.is_empty() {
                    self.message("Sync with the server", report.describe());
                }
                if report.created + report.replaced + report.folders_made > 0 {
                    self.panels.project_panel.explorer.refresh();
                    self.index_project();
                }
            }
            Done::Carried(report) => {
                project.ask(Op::Refresh { ask: false });
                self.panels.project_panel.explorer.refresh();
                self.index_project();
                self.message("Sync with the server", report.describe());
            }
            Done::Pushed | Done::Changed => {
                project.stalled = None;
                project.refused = false;
                if matches!(op, Op::Rename { .. } | Op::Remove(_) | Op::MakeFolder(_)) {
                    self.panels.project_panel.explorer.refresh();
                }
            }
            Done::Fetched(path) => {
                self.open_path(&remote_project::local_place(&files, &path));
            }
            Done::ChangedOnServer(path) => {
                let question = crate::i18n::tf!("{path} has been changed on {host} since it was last fetched.\n\nReplace it with your version?");
                if ask_yes_no(crate::i18n::tr("Changed on the server"), &question) {
                    if let Some(project) = &mut self.remote_project {
                        project.queue.push_front(Op::Push { path, force: true });
                    }
                } else {
                    self.message("Save", crate::i18n::tf!("Your version is saved on this computer, at {}. Project, Sync with the server shows what's different.", remote_project::local_place(&files, &path).display()));
                }
            }
            Done::Refused(refused) => {
                // Tried again once the person has connected.
                project.queue.push_front(op);
                let site = project.origin.site.clone();
                let text = refused.describe(&site);
                project.stalled = Some(text.clone());
                project.refused = true;
                if let Refused::SignIn(_) = refused {
                    self.remote_secrets.remove(&site.key());
                }
                if !matches!(self.dialog, Dialog::Remote(_)) {
                    self.open_remote_folder_box();
                    if let Dialog::Remote(state) = &mut self.dialog {
                        state.status = text;
                        match refused {
                            Refused::UnknownHost(print) => state.trust = Some(print),
                            Refused::HostKeyChanged { .. } => state.changed_key = true,
                            _ => {}
                        }
                    }
                }
            }
            Done::Failed(problem) => match op {
                // Looking for changes by itself: said in the status bar, and tried again later.
                Op::Refresh { ask: false } => project.stalled = Some(problem),
                // A save that couldn't be sent is offered again by the next sync, as a change here.
                _ => {
                    project.stalled = Some(problem.clone());
                    warn(&problem);
                }
            },
        }
    }

    /// A saved file in the project's mirror is sent to its server.
    pub(super) fn remote_project_saved(&mut self, path: &Path) {
        if let Some(project) = &mut self.remote_project
            && let Some(relative) = project.relative(path)
        {
            project.ask(Op::Push { path: relative, force: false });
        }
    }

    /// A file left on the server, being opened: fetched first, and opened once it's here. True if it is one.
    pub(super) fn remote_project_stand_in(&mut self, path: &Path) -> bool {
        let Some(project) = &mut self.remote_project else { return false };
        let Some(relative) = project.relative(path).filter(|relative| project.state.is_left_there(relative)) else { return false };
        // Something saved over the stand-in is a file of its own now.
        if std::fs::metadata(path).is_ok_and(|meta| meta.len() > 0) {
            return false;
        }
        project.ask(Op::Fetch(relative));
        true
    }

    /// The Explorer has made, renamed or taken away something in the project's mirror: the same is done on the server.
    pub(super) fn remote_project_explorer(&mut self, act: &crate::explorer::Act) {
        use crate::explorer::Act as Did;
        let Some(project) = &mut self.remote_project else { return };
        let op = match act {
            Did::Create { path, folder: true } if path.is_dir() => project.relative(path).map(Op::MakeFolder),
            Did::Create { path, folder: false } if path.is_file() => project.relative(path).map(|path| Op::Push { path, force: false }),
            Did::Rename { from, to } if to.exists() && !from.exists() => project.relative(from).zip(project.relative(to)).map(|(from, to)| Op::Rename { from, to }),
            Did::Delete(path) if !path.exists() => project.relative(path).map(Op::Remove),
            _ => None,
        };
        if let Some(op) = op {
            project.ask(op);
        }
    }

    /// The words to ask with before the Explorer deletes something in a remote project, which goes from the
    /// server too.
    pub(super) fn remote_project_delete_question(&self, path: &Path) -> Option<String> {
        let project = self.remote_project.as_ref()?;
        project.relative(path)?;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Some(if path.is_dir() {
            crate::i18n::tf!("Move the folder {name}, and everything in it, to the Recycle Bin, and delete it from {} too? On the server, that can't be undone.", project.origin.site.host)
        } else {
            crate::i18n::tf!("Move {name} to the Recycle Bin, and delete it from {} too? On the server, that can't be undone.", project.origin.site.host)
        })
    }

    /// Project, Fetch the server's changes; or Sync with the server, which then shows what's left to decide.
    pub(super) fn remote_project_refresh(&mut self, ask: bool) {
        let Some(project) = &mut self.remote_project else {
            return self.message("Sync with the server", crate::i18n::tr("The project open isn't a folder on a server. Open one with File, Open a remote folder.").into());
        };
        project.refused = false;
        project.queue.retain(|op| !matches!(op, Op::Refresh { .. }));
        project.queue.push_back(Op::Refresh { ask });
    }

    /// Words for the status bar: what's being done with the server, or what's waiting to be decided.
    pub(super) fn remote_project_status(&self) -> Option<(String, bool)> {
        let project = self.remote_project.as_ref()?;
        let host = &project.origin.site.host;
        if let Some((op, work)) = &project.running {
            let share = work.progress.share().map_or_else(String::new, |share| format!(" {:.0}%", share * 100.0));
            let words = match op {
                Op::Refresh { .. } | Op::Carry(_) => crate::i18n::tf!("Syncing with {host}{share}"),
                Op::Push { path, .. } => crate::i18n::tf!("Sending {path} to {host}"),
                Op::Fetch(path) => crate::i18n::tf!("Fetching {path} from {host}"),
                Op::Rename { .. } | Op::Remove(_) | Op::MakeFolder(_) => crate::i18n::tf!("Changing {host}"),
            };
            return Some((words, false));
        }
        if project.stalled.is_some() {
            return Some((crate::i18n::tf!("Not connected to {host}"), true));
        }
        let waiting = project.waiting.as_ref().map_or(0, |plan| plan.items.len());
        (waiting > 0).then(|| (if waiting == 1 { crate::i18n::tf!("1 file to sync with {host}") } else { crate::i18n::tf!("{waiting} files to sync with {host}") }, true))
    }

    /// The status bar's words were clicked: connect again, or show what's waiting.
    pub(super) fn remote_project_status_clicked(&mut self) {
        let refused = self.remote_project.as_ref().is_some_and(|p| p.refused);
        if refused {
            self.open_remote_folder_box();
            if let (Some(project), Dialog::Remote(state)) = (&self.remote_project, &mut self.dialog) {
                state.status = project.stalled.clone().unwrap_or_default();
            }
        } else {
            self.remote_project_refresh(true);
        }
    }

    /// A remote project with its sync window open, for a screenshot: nothing is connected to.
    pub(super) fn remote_sync_screenshot(&mut self) {
        use crate::compare_sync::{SyncItem, SyncKind, SyncWay};
        let origin = Origin { site: Site { scheme: remote::Scheme::Ftpes, host: "nas.example.org".into(), port: 21, user: "ann".into(), key_file: None }, folder: "/home/ann/website".into() };
        let places = remote_project::places(&std::env::temp_dir().join("limoniter-screenshot-projects"), &origin);
        let mut project = RemoteProject::new(origin, places);
        project.refused = true;
        let item = |path: &str, act: Act, to_right: bool, conflict: bool, note: Option<&str>| SyncItem { path: path.into(), act, to_right, ticked: !conflict && note.is_none(), conflict, note: note.map(str::to_string), size: 1_204, modified: Some(1_763_000_000), expected: None };
        let items = vec![
            item("css/site.css", Act::Replace, true, true, Some("Changed here and on the server since the last sync")),
            item("index.html", Act::Replace, true, false, None),
            item("blog/2026-11-19.html", Act::Create, true, false, None),
            item("images/old-logo.png", Act::Create, true, false, Some("Deleted from the server since the last sync")),
        ];
        project.window = Some(SyncWindow { plan: SyncPlan { kind: SyncKind::Update, way: SyncWay::Both, items, skipped: Vec::new() } });
        project.waiting = project.window.as_ref().map(|w| w.plan.clone());
        self.remote_project = Some(project);
    }

    /// The window that shows what a sync would do, each change ticked or not, conflicts first.
    pub(super) fn remote_sync_window(&mut self, ctx: &Context) {
        let Some(project) = &mut self.remote_project else { return };
        let Some(window) = &mut project.window else { return };
        let host = project.origin.site.host.clone();
        let (mut close, mut go) = (ctx.input(|i| i.key_pressed(Key::Escape)), false);
        let mut open = true;
        egui::Window::new(crate::i18n::tr("Sync with the server")).id(egui::Id::unique("remote-sync")).open(&mut open).collapsible(false).resizable(true).default_size([620.0, 420.0]).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            let conflicts = window.plan.items.iter().filter(|item| item.conflict).count();
            ui.label(crate::i18n::tf!("What would make the project and {host} alike. Nothing is done until you say. Each file replaced or deleted is kept first, in the project's backups."));
            if conflicts > 0 {
                ui.colored_label(theme::rainbow_mistake(), if conflicts == 1 { crate::i18n::tr("1 file has changed on both sides since the last sync. Tick it to choose that side's version.").to_string() } else { crate::i18n::tf!("{conflicts} files have changed on both sides since the last sync. Tick each to choose that side's version.") });
            }
            ui.separator();
            egui::ScrollArea::vertical().max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new("remote-sync-items").num_columns(4).striped(true).spacing([12.0, 4.0]).show(ui, |ui| {
                    for item in window.plan.items.iter_mut() {
                        ui.checkbox(&mut item.ticked, "");
                        ui.label(if item.to_right { crate::i18n::tr("\u{2191} To the server") } else { crate::i18n::tr("\u{2193} From the server") });
                        ui.label(crate::i18n::tr(item.act.words()));
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&item.path).monospace());
                            if let Some(note) = &item.note {
                                let text = RichText::new(note).small();
                                ui.label(if item.conflict { text.color(theme::rainbow_mistake()) } else { text.color(theme::text_dim()) });
                            }
                            // A file changed on both sides can go either way.
                            if item.conflict && item.act == Act::Replace && ui.small_button(crate::i18n::tr("The other way")).on_hover_text(crate::i18n::tr("Keep the other side's version instead")).clicked() {
                                let target = item.expected;
                                item.expected = Some(crate::diff_engine::Meta { is_dir: false, size: item.size, modified: item.modified });
                                item.size = target.map_or(0, |meta| meta.size);
                                item.modified = target.and_then(|meta| meta.modified);
                                item.to_right = !item.to_right;
                            }
                        });
                        ui.end_row();
                    }
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                let ticked = window.plan.ticked();
                if ui.add_enabled(ticked > 0, egui::Button::new(crate::i18n::tf!("Do the {ticked} ticked"))).clicked() {
                    go = true;
                }
                if ui.button(crate::i18n::tr("Close")).clicked() {
                    close = true;
                }
            });
        });
        if go && let Some(window) = project.window.take() {
            let mut plan = window.plan;
            plan.items.retain(|item| item.ticked);
            project.queue.push_back(Op::Carry(plan));
        }
        if close || !open {
            project.window = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{app_with, command, draw};
    use super::*;
    use crate::ftp::ftp_tests::{Offers, PASSWORD, USER, start};

    /// Draws until the work with the server is done.
    fn settle(app: &mut Limoniter, ctx: &Context) {
        let began = Instant::now();
        loop {
            draw(app, ctx, Vec::new());
            if app.remote_project.as_ref().is_none_or(|p| !p.busy()) {
                break;
            }
            assert!(began.elapsed() < Duration::from_secs(60), "the server never answered");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// A folder on an FTP server opened as a project, through the window: mirrored, edited and saved back, the
    /// server's changes fetched, a large file fetched when opened, the Explorer's renames done there too, a file
    /// changed on both sides shown to decide, and the project opened again later from where it's remembered.
    #[test]
    fn a_folder_on_a_server_opens_as_a_project_and_keeps_in_step() {
        let server = start(Offers::default());
        let served = server.root.join("home/ann/site");
        std::fs::create_dir_all(served.join("css")).unwrap();
        std::fs::write(served.join("index.html"), "<p>Hello</p>\n").unwrap();
        std::fs::write(served.join("css/site.css"), "p { color: red }\n").unwrap();
        std::fs::write(served.join("video.bin"), vec![1u8; remote_project::LARGE as usize + 10]).unwrap();
        let (mut app, ctx) = app_with("");
        let data = std::env::temp_dir().join(format!("limoniter-remote-project-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        app.remote_root = Some(data.join("remote-copies"));
        let site = Site { scheme: remote::Scheme::Ftp, host: "127.0.0.1".into(), port: server.port, user: USER.into(), key_file: None };
        app.remote_secrets.insert(site.key(), Secret::Password(PASSWORD.into()));
        app.open_remote_project(site.clone(), "/home/ann/site".into());
        settle(&mut app, &ctx);
        let root = app.project.as_ref().and_then(|p| p.root.clone()).expect("a folder project");
        assert!(root.starts_with(data.join("remote-projects")), "{}", root.display());
        assert_eq!(app.project.as_ref().unwrap().remote.as_ref().map(Origin::url), Some(site.url("/home/ann/site")));
        assert_eq!(std::fs::read_to_string(root.join("css/site.css")).unwrap(), "p { color: red }\n");
        assert_eq!(std::fs::metadata(root.join("video.bin")).unwrap().len(), 0, "a large file waits on the server");

        // Edited and saved: sent at once.
        assert!(app.open_path(&root.join("index.html")));
        app.doc().text.set_content(vec!["<p>Hello again</p>".into(), String::new()]);
        draw(&mut app, &ctx, Vec::new());
        command(&mut app, &ctx, Command::Save);
        settle(&mut app, &ctx);
        assert_eq!(std::fs::read_to_string(served.join("index.html")).unwrap().replace("\r\n", "\n"), "<p>Hello again</p>\n");

        // Changed on the server: fetched when asked.
        std::fs::write(served.join("css/site.css"), "p { color: blue }\n").unwrap();
        let later = std::time::SystemTime::now() + Duration::from_secs(60);
        std::fs::File::options().write(true).open(served.join("css/site.css")).unwrap().set_modified(later).unwrap();
        command(&mut app, &ctx, Command::RemoteProjectFetch);
        settle(&mut app, &ctx);
        assert_eq!(std::fs::read_to_string(root.join("css/site.css")).unwrap(), "p { color: blue }\n");

        // The large file, opened: fetched first.
        assert!(app.open_path(&root.join("video.bin")));
        settle(&mut app, &ctx);
        assert_eq!(std::fs::metadata(root.join("video.bin")).unwrap().len(), remote_project::LARGE + 10);
        assert!(app.docs.iter().any(|doc| doc.path.as_deref().is_some_and(|p| same_path(p, &root.join("video.bin")))), "and opened");

        // Renamed in the Explorer: renamed there too.
        let _ = ctx.clone().run_ui(Default::default(), |ui| app.explorer_act(crate::explorer::Act::Rename { from: root.join("css"), to: root.join("styles") }, ui));
        settle(&mut app, &ctx);
        assert!(served.join("styles/site.css").is_file() && !served.join("css").exists());

        // Changed on both sides: shown, unticked, to decide.
        std::fs::write(root.join("styles/site.css"), "mine\n").unwrap();
        std::fs::write(served.join("styles/site.css"), "theirs, longer\n").unwrap();
        command(&mut app, &ctx, Command::RemoteProjectSync);
        settle(&mut app, &ctx);
        let window = app.remote_project.as_ref().and_then(|p| p.window.as_ref()).expect("the sync window is shown");
        let item = window.plan.items.iter().find(|item| item.path == "styles/site.css").expect("the file changed on both sides");
        assert!(item.conflict && !item.ticked);
        assert_eq!(std::fs::read_to_string(served.join("styles/site.css")).unwrap(), "theirs, longer\n", "nothing done yet");
        // Ticked as it is, the server's copy is replaced with this one.
        let project = app.remote_project.as_mut().unwrap();
        let window = project.window.as_mut().unwrap();
        window.plan.items.iter_mut().for_each(|item| item.ticked = item.path == "styles/site.css");
        let plan = window.plan.clone();
        project.window = None;
        project.ask(Op::Carry(plan.clone()));
        settle(&mut app, &ctx);
        let local_won = plan.items.iter().find(|item| item.path == "styles/site.css").unwrap().to_right;
        let expected = if local_won { "mine\n" } else { "theirs, longer\n" };
        assert_eq!(std::fs::read_to_string(served.join("styles/site.css")).unwrap(), expected);
        assert_eq!(std::fs::read_to_string(root.join("styles/site.css")).unwrap(), expected);

        // Closed and opened again: the project remembers its server.
        app.close_project();
        assert!(app.remote_project.is_none());
        let kept = app.recent_projects[0].clone();
        app.open_project(&kept);
        assert_eq!(app.remote_project.as_ref().map(|p| p.origin.url()), Some(site.url("/home/ann/site")));
        settle(&mut app, &ctx);
        assert!(app.remote_project.as_ref().unwrap().stalled.is_none());
        app.close_project();
        let _ = std::fs::remove_file(&kept);
        let _ = std::fs::remove_dir_all(&data);
    }

    /// A server that refuses the sign-in: the remote box is opened to sign in, and the work waits for it.
    #[test]
    fn a_project_whose_server_refuses_asks_to_sign_in() {
        let server = start(Offers::default());
        std::fs::create_dir_all(server.root.join("home/ann/site")).unwrap();
        let (mut app, ctx) = app_with("");
        let data = std::env::temp_dir().join(format!("limoniter-remote-refused-{}", std::process::id()));
        app.remote_root = Some(data.join("remote-copies"));
        let site = Site { scheme: remote::Scheme::Ftp, host: "127.0.0.1".into(), port: server.port, user: USER.into(), key_file: None };
        app.remote_secrets.insert(site.key(), Secret::Password("wrong".into()));
        app.open_remote_project(site.clone(), "/home/ann/site".into());
        let began = Instant::now();
        while !matches!(app.dialog, Dialog::Remote(_)) {
            draw(&mut app, &ctx, Vec::new());
            assert!(began.elapsed() < Duration::from_secs(30), "the box to sign in never came");
            std::thread::sleep(Duration::from_millis(10));
        }
        let Dialog::Remote(state) = &app.dialog else { unreachable!() };
        assert!(state.project && state.address == site.url("/home/ann/site"), "{}", state.address);
        let project = app.remote_project.as_ref().unwrap();
        assert!(project.refused && project.queue.front() == Some(&Op::Refresh { ask: false }), "the work waits");
        assert!(!app.remote_secrets.contains_key(&site.key()), "the wrong password is forgotten");
        // Signed in properly: connected again, from the box, and the work goes on.
        app.remote_secrets.insert(site.key(), Secret::Password(PASSWORD.into()));
        app.close_dialog();
        app.open_remote_project(site, "/home/ann/site".into());
        settle(&mut app, &ctx);
        assert!(app.remote_project.as_ref().unwrap().stalled.is_none());
        app.close_project();
        let _ = std::fs::remove_dir_all(&data);
    }
}
