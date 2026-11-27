//! The Files changing panel's working parts: a watcher on the project's folders, the list of what changed (newest
//! first, one row a file), the wording of "how long ago", and the memory of Limoniter's own saves, so a change made
//! by Limoniter can be told from one made by an agent or another program.
//!
//! The watching is `notify`'s: the operating system's own events where they work, and `notify`'s polling (a look
//! every few seconds) on network drives, `\\wsl$` paths and WSL's `/mnt` drives, where they don't. The events are
//! gathered on a thread of their own, and a burst of them for one file (an editor writing in pieces, or saving by
//! writing a temporary file and renaming it into place) is held until the file has been quiet a moment, then shown
//! as one change.
//!
//! For the AI agent panel (a later piece of 0.18): call [`claim`] with the file or folder an agent is about to
//! change and the agent's name, and the changes that follow are labelled with that name instead of "Elsewhere".

use crate::explorer_kept::Hide;
use crate::workspace::{self, Ignores};
use eframe::egui::{self, RichText, Ui};
use notify::{EventKind, RecursiveMode, Watcher as _};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// The most rows kept: the newest.
pub const MOST_ROWS: usize = 500;
/// How long a file has to be quiet before its change is shown.
const QUIET: Duration = Duration::from_millis(300);
/// How often the polling watcher looks, where the system can't say.
const POLL: Duration = Duration::from_secs(3);
/// The most folders watched in one project, so a huge tree can't use up the system's watches.
const MOST_FOLDERS: usize = 8000;
/// How long Limoniter's own save is remembered, and how long an agent's claim lasts after its last change.
const OWN_LIFE: Duration = Duration::from_secs(60);
const CLAIM_LIFE: Duration = Duration::from_secs(120);

// ---------------------------------------------------------------- who changed it

/// Who made a change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Who {
    /// Saved by Limoniter.
    You,
    /// An agent, by name ("Claude Code", "Codex", or a session's name).
    Agent(String),
    /// Anything else: another program, or a tool run in a terminal.
    Elsewhere,
}

impl Who {
    pub fn label(&self) -> String {
        match self {
            Who::You => crate::i18n::tr("You").to_string(),
            Who::Agent(name) => name.clone(),
            Who::Elsewhere => crate::i18n::tr("Elsewhere").to_string(),
        }
    }
}

/// What's known of a file when it's written: its size and when it was last changed.
type Stamp = (u64, Option<SystemTime>);

struct Claim {
    key: PathBuf,
    who: Who,
    at: Instant,
    /// For Limoniter's own saves: the file as it was left, so a later change by someone else isn't taken as ours.
    stamp: Option<Stamp>,
}

static CLAIMS: Mutex<Vec<Claim>> = Mutex::new(Vec::new());

/// A path written the same way whichever way it was given: its folder made absolute and resolved.
fn key(path: &Path) -> PathBuf {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    match (path.parent().and_then(|p| p.canonicalize().ok()), path.file_name()) {
        (Some(parent), Some(name)) => parent.join(name),
        _ => path,
    }
}

fn stamp_of(path: &Path) -> Option<Stamp> {
    std::fs::metadata(path).ok().map(|m| (m.len(), m.modified().ok()))
}

/// Limoniter has just written this file for the person (a save, a save as, autosave): the change that follows is
/// theirs, labelled You. Call it after the write.
pub fn note_own_write(path: &Path) {
    let key = key(path);
    let stamp = stamp_of(&key);
    if let Ok(mut claims) = CLAIMS.lock() {
        claims.retain(|c| !(c.key == key && c.who == Who::You));
        claims.push(Claim { key, who: Who::You, at: Instant::now(), stamp });
    }
}

/// An agent (or anything else with a name) is about to change this file, or the files in this folder: the changes
/// that follow are labelled with `who`. It holds while the changes keep coming, and ends two minutes after the last.
pub fn claim(path: &Path, who: &str) {
    let key = key(path);
    if let Ok(mut claims) = CLAIMS.lock() {
        claims.retain(|c| !(c.key == key && c.who != Who::You));
        claims.push(Claim { key, who: Who::Agent(who.to_string()), at: Instant::now(), stamp: None });
    }
}

/// Who made the change that's just been seen in this file.
fn who_changed(path: &Path) -> Who {
    let key = key(path);
    let now = Instant::now();
    let Ok(mut claims) = CLAIMS.lock() else { return Who::Elsewhere };
    claims.retain(|c| now.duration_since(c.at) < if c.who == Who::You { OWN_LIFE } else { CLAIM_LIFE });
    // The nearest claim: the file's own, before its folders'.
    let mut best: Option<usize> = None;
    for (n, c) in claims.iter().enumerate() {
        if key.starts_with(&c.key) && best.is_none_or(|b| c.key.components().count() > claims[b].key.components().count() || (c.key == claims[b].key && c.who != Who::You)) {
            // Our own save only counts while the file is as we left it.
            if c.who == Who::You && c.stamp != stamp_of(&key) {
                continue;
            }
            best = Some(n);
        }
    }
    match best {
        Some(n) => {
            if claims[n].who != Who::You {
                claims[n].at = now;
            }
            claims[n].who.clone()
        }
        None => Who::Elsewhere,
    }
}

// ---------------------------------------------------------------- the list

/// What happened to a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum What {
    New,
    Changed,
    Deleted,
    /// Moved to the name the row shows.
    Renamed,
    /// Moved away from this name: not a row, but the old row goes.
    MovedAway,
}

/// One change, as the watching thread reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub path: PathBuf,
    pub what: What,
    pub when: SystemTime,
    pub who: Who,
}

/// Adds a change to the list: its file's row, if it has one, is replaced and the file goes to the top; the list
/// is cut to the newest `MOST_ROWS`.
pub fn merge(rows: &mut Vec<Change>, change: Change) {
    let before = rows.iter().position(|r| r.path == change.path).map(|n| rows.remove(n));
    if change.what == What::MovedAway {
        return;
    }
    let mut change = change;
    // New, then changed again, is still a new file; deleted, then made again, is a changed one.
    match (before.map(|b| b.what), change.what) {
        (Some(What::New), What::Changed) => change.what = What::New,
        (Some(What::Deleted), What::New) => change.what = What::Changed,
        _ => {}
    }
    rows.insert(0, change);
    rows.truncate(MOST_ROWS);
}

// ---------------------------------------------------------------- how long ago

/// "34 seconds ago", "2 minutes ago", "at 14:05", "yesterday" or "on 12 Nov": how long ago a change was, from its
/// age and the local times of the change and of now (for what day it was).
pub fn ago_text(age: Duration, then: chrono::NaiveDateTime, now: chrono::NaiveDateTime) -> String {
    let seconds = age.as_secs();
    if seconds < 5 {
        crate::i18n::tr("just now").to_string()
    } else if seconds < 60 {
        crate::i18n::tf!("{seconds} seconds ago")
    } else if seconds < 120 {
        crate::i18n::tr("1 minute ago").to_string()
    } else if seconds < 3600 {
        let minutes = seconds / 60;
        crate::i18n::tf!("{minutes} minutes ago")
    } else if then.date() == now.date() {
        let time = then.format("%H:%M").to_string();
        crate::i18n::tf!("at {time}")
    } else if now.date().pred_opt() == Some(then.date()) {
        crate::i18n::tr("yesterday").to_string()
    } else {
        let date = then.format("%d %b").to_string();
        crate::i18n::tf!("on {date}")
    }
}

fn ago_now(when: SystemTime, now: SystemTime) -> String {
    let local = |t: SystemTime| chrono::DateTime::<chrono::Local>::from(t).naive_local();
    ago_text(now.duration_since(when).unwrap_or_default(), local(when), local(now))
}

/// A row's second line: "changed 34 seconds ago", "new, 2 minutes ago".
pub fn said(what: What, when: &str) -> String {
    match what {
        What::New => crate::i18n::tf!("new, {when}"),
        What::Deleted => crate::i18n::tf!("deleted {when}"),
        What::Renamed => crate::i18n::tf!("renamed, {when}"),
        What::Changed | What::MovedAway => crate::i18n::tf!("changed {when}"),
    }
}

// ---------------------------------------------------------------- gathering events

/// What an event said of a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Raw {
    Created,
    Modified,
    Removed,
    RenamedFrom,
    /// Renamed to this name; true when it was renamed from a temporary name (a save by renaming into place).
    RenamedTo(bool),
}

#[derive(Default)]
struct Pending {
    last: Option<Instant>,
    created: bool,
    removed: bool,
    modified: bool,
    renamed_to: bool,
    moved_away: bool,
}

/// Holds events for each file until it's been quiet, then says what became of it.
pub struct Coalescer {
    pending: HashMap<PathBuf, Pending>,
    quiet: Duration,
}

impl Coalescer {
    pub fn new(quiet: Duration) -> Self {
        Coalescer { pending: HashMap::new(), quiet }
    }

    pub fn push(&mut self, path: PathBuf, raw: Raw, now: Instant) {
        let p = self.pending.entry(path).or_default();
        p.last = Some(now);
        match raw {
            Raw::Created => p.created = true,
            Raw::Modified => p.modified = true,
            Raw::Removed => p.removed = true,
            Raw::RenamedFrom => {
                p.removed = true;
                p.moved_away = true;
            }
            Raw::RenamedTo(from_temporary) => {
                if from_temporary {
                    p.modified = true;
                } else {
                    p.renamed_to = true;
                }
            }
        }
    }

    /// The files that have been quiet long enough, and what became of each, by whether it's there now.
    pub fn due(&mut self, now: Instant, exists: impl Fn(&Path) -> bool) -> Vec<(PathBuf, What)> {
        let ready: Vec<PathBuf> = self.pending.iter().filter(|(_, p)| p.last.is_none_or(|t| now.duration_since(t) >= self.quiet)).map(|(path, _)| path.clone()).collect();
        let mut found = Vec::new();
        for path in ready {
            let Some(p) = self.pending.remove(&path) else { continue };
            let there = exists(&path);
            let what = if !there {
                // Made and gone within the burst (a temporary file) was never a change.
                if p.moved_away {
                    Some(What::MovedAway)
                } else if p.created {
                    None
                } else {
                    Some(What::Deleted)
                }
            } else if p.renamed_to {
                Some(What::Renamed)
            } else if p.created && !p.removed {
                Some(What::New)
            } else {
                Some(What::Changed)
            };
            if let Some(what) = what {
                found.push((path, what));
            }
        }
        found.sort_by(|a, b| a.0.cmp(&b.0));
        found
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

/// A name that's only a stand-in while a file is being written: not shown as a change of its own.
pub fn is_temporary(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains(".tmp") || lower.contains(".limoniter-tmp") || lower.ends_with('~') || lower.ends_with(".swp") || lower.ends_with(".swx") || lower.starts_with(".#") || lower.starts_with("~$") || lower == "4913"
}

// ---------------------------------------------------------------- where watching works

/// Whether a place needs polling rather than the system's events: network drives, `\\wsl$` and `\\wsl.localhost`
/// on Windows, and the drives WSL mounts at /mnt. `mounts` is what /proc/mounts says (empty where there isn't one).
pub fn needs_polling(path: &Path, mounts: &str) -> bool {
    let text = path.to_string_lossy();
    if text.starts_with(r"\\") || text.starts_with("//") {
        return true;
    }
    // WSL's own drives: /mnt/c, /mnt/h and so on.
    if let Some(rest) = text.strip_prefix("/mnt/") {
        let drive = rest.split('/').next().unwrap_or("");
        if drive.len() == 1 && drive.chars().all(|c| c.is_ascii_alphabetic()) {
            return true;
        }
    }
    // Whatever's mounted at the longest matching mount point, if it's a kind that sends no events.
    let mut best: Option<(usize, bool)> = None;
    for line in mounts.lines() {
        let mut parts = line.split_whitespace();
        let (Some(_), Some(point), Some(kind)) = (parts.next(), parts.next(), parts.next()) else { continue };
        let point = point.replace("\\040", " ");
        if path.starts_with(&point) && best.is_none_or(|(len, _)| point.len() >= len) {
            let remote = matches!(kind, "nfs" | "nfs4" | "cifs" | "smb3" | "smbfs" | "9p" | "drvfs" | "fuse.sshfs" | "fuse.rclone" | "afs" | "ncpfs");
            best = Some((point.len(), remote));
        }
    }
    best.is_some_and(|(_, remote)| remote)
}

fn mounts() -> String {
    std::fs::read_to_string("/proc/mounts").unwrap_or_default()
}

// ---------------------------------------------------------------- the watcher

/// What to watch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Spec {
    /// A project's folder: watched with everything under it that the Explorer shows.
    pub root: Option<PathBuf>,
    /// Folders watched alone, without those under them.
    pub folders: Vec<PathBuf>,
    /// What the project hides from its Explorer.
    pub hidden: Vec<Hide>,
}

/// Watches folders on a thread of its own, and hands over what changed. Dropping it stops the watching.
pub struct Watcher {
    changes: Receiver<Change>,
    stop: Arc<AtomicBool>,
    /// Set if some folders couldn't be watched (the system's limit on watches, or a folder that went).
    pub problem: Arc<Mutex<Option<String>>>,
    /// Whether it's polling instead of being told.
    pub polling: bool,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Watcher {
    pub fn start(spec: Spec, repaint: Option<egui::Context>) -> Watcher {
        Self::start_with(spec, repaint, QUIET, POLL, false)
    }

    /// `force_poll` polls whatever the place is (for tests of the polling).
    pub fn start_with(spec: Spec, repaint: Option<egui::Context>, quiet: Duration, poll: Duration, force_poll: bool) -> Watcher {
        let (send, changes) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let problem = Arc::new(Mutex::new(None));
        let mounts = mounts();
        let polling = force_poll || spec.root.iter().chain(spec.folders.iter()).any(|p| needs_polling(p, &mounts));
        let (stop2, problem2) = (stop.clone(), problem.clone());
        std::thread::spawn(move || run(spec, polling, quiet, poll, send, stop2, problem2, repaint));
        Watcher { changes, stop, problem, polling }
    }

    /// What's changed since last asked.
    pub fn take(&self) -> Vec<Change> {
        self.changes.try_iter().collect()
    }
}

/// Every folder under `root` that's worth watching, root first: not the ones the Explorer leaves out.
fn folders_under(root: &Path, from: &Path, hidden: &[Hide], ignores: &mut Ignores, found: &mut Vec<PathBuf>) {
    if found.len() >= MOST_FOLDERS {
        return;
    }
    found.push(from.to_path_buf());
    let Ok(entries) = std::fs::read_dir(from) else { return };
    let mut below: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| !workspace::hidden(&e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .filter(|p| !crate::explorer_kept::hidden_by(hidden, root, p, true) && !ignores.ignored(p, true))
        .collect();
    below.sort();
    for folder in below {
        folders_under(root, &folder, hidden, ignores, found);
    }
}

/// Whether a change to this file is left out: in a folder the Explorer leaves out, or a temporary name.
fn left_out(root: Option<&Path>, hidden: &[Hide], ignores: &mut Option<Ignores>, path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if is_temporary(&name) || workspace::hidden(&name) {
        return true;
    }
    let Some(root) = root.filter(|r| path.starts_with(r)) else { return false };
    let Ok(relative) = path.strip_prefix(root) else { return false };
    if relative.components().any(|c| workspace::hidden(&c.as_os_str().to_string_lossy())) || crate::explorer_kept::hidden_by(hidden, root, path, false) {
        return true;
    }
    let Some(ignores) = ignores else { return false };
    // Its folders, one by one from the project's top, then the file.
    let mut folder = root.to_path_buf();
    let parts: Vec<_> = relative.components().collect();
    for part in &parts[..parts.len().saturating_sub(1)] {
        folder.push(part);
        if ignores.ignored(&folder, true) {
            return true;
        }
    }
    ignores.ignored(path, false)
}

#[allow(clippy::too_many_arguments)]
fn run(spec: Spec, polling: bool, quiet: Duration, poll: Duration, send: std::sync::mpsc::Sender<Change>, stop: Arc<AtomicBool>, problem: Arc<Mutex<Option<String>>>, repaint: Option<egui::Context>) {
    let (raw_send, raw_receive) = channel::<notify::Result<notify::Event>>();
    let handler = move |event| {
        let _ = raw_send.send(event);
    };
    let made: notify::Result<Box<dyn notify::Watcher + Send>> = if polling {
        notify::PollWatcher::new(handler, notify::Config::default().with_poll_interval(poll)).map(|w| Box::new(w) as Box<dyn notify::Watcher + Send>)
    } else {
        notify::RecommendedWatcher::new(handler, notify::Config::default()).map(|w| Box::new(w) as Box<dyn notify::Watcher + Send>)
    };
    let mut watcher = match made {
        Ok(w) => w,
        Err(e) => {
            *problem.lock().unwrap() = Some(e.to_string());
            return;
        }
    };
    let root = spec.root.as_deref().map(key);
    let mut ignores = root.as_deref().map(Ignores::new);
    let mut folders = Vec::new();
    if let (Some(root), Some(ignores)) = (&root, &mut ignores) {
        folders_under(root, root, &spec.hidden, ignores, &mut folders);
    }
    folders.extend(spec.folders.iter().map(|f| key(f)));
    let mut watched: HashSet<PathBuf> = HashSet::new();
    let mut failed = None;
    for folder in folders {
        if watched.insert(folder.clone())
            && let Err(e) = watcher.watch(&folder, RecursiveMode::NonRecursive)
        {
            failed = Some(e.to_string());
        }
    }
    *problem.lock().unwrap() = failed;

    let mut coalescer = Coalescer::new(quiet);
    while !stop.load(Ordering::Relaxed) {
        let step = if coalescer.is_empty() { Duration::from_millis(200) } else { Duration::from_millis(40) };
        match raw_receive.recv_timeout(step) {
            Ok(Ok(event)) => {
                let now = Instant::now();
                for (path, raw) in classify(&event) {
                    let path = key(&path);
                    // A new folder is watched too, and what's already in it is counted as new.
                    if path.is_dir() {
                        if matches!(raw, Raw::Created | Raw::RenamedTo(_)) && !watched.contains(&path) && !left_out(root.as_deref(), &spec.hidden, &mut ignores, &path) && root.as_ref().is_some_and(|r| path.starts_with(r)) {
                            let mut inside = Vec::new();
                            let mut quiet_ignores = Ignores::new(root.as_deref().unwrap_or(&path));
                            folders_under(root.as_deref().unwrap_or(&path), &path, &spec.hidden, &mut quiet_ignores, &mut inside);
                            for folder in inside {
                                if watched.insert(folder.clone()) && watcher.watch(&folder, RecursiveMode::NonRecursive).is_ok() {
                                    for file in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
                                        let file = key(&file.path());
                                        if file.is_file() && !left_out(root.as_deref(), &spec.hidden, &mut ignores, &file) {
                                            coalescer.push(file, Raw::Created, now);
                                        }
                                    }
                                }
                            }
                        }
                        continue;
                    }
                    if !left_out(root.as_deref(), &spec.hidden, &mut ignores, &path) {
                        coalescer.push(path, raw, now);
                    }
                }
            }
            Ok(Err(_)) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
        let ready = coalescer.due(Instant::now(), |p| p.exists());
        if !ready.is_empty() {
            let when = SystemTime::now();
            for (path, what) in ready {
                let who = if what == What::MovedAway { Who::Elsewhere } else { who_changed(&path) };
                if send.send(Change { path, what, when, who }).is_err() {
                    return;
                }
            }
            if let Some(ctx) = &repaint {
                ctx.request_repaint();
            }
        }
    }
}

/// What an event says of which files.
fn classify(event: &notify::Event) -> Vec<(PathBuf, Raw)> {
    use notify::event::{ModifyKind, RenameMode};
    let paths = &event.paths;
    match &event.kind {
        EventKind::Create(_) => paths.iter().map(|p| (p.clone(), Raw::Created)).collect(),
        EventKind::Remove(_) => paths.iter().map(|p| (p.clone(), Raw::Removed)).collect(),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if paths.len() == 2 => {
            let temporary = paths[0].file_name().is_some_and(|n| is_temporary(&n.to_string_lossy()));
            vec![(paths[0].clone(), Raw::RenamedFrom), (paths[1].clone(), Raw::RenamedTo(temporary))]
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => paths.iter().map(|p| (p.clone(), Raw::RenamedFrom)).collect(),
        // Where it came from isn't known, so it's a change rather than a rename.
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => paths.iter().map(|p| (p.clone(), Raw::RenamedTo(true))).collect(),
        EventKind::Modify(_) | EventKind::Any | EventKind::Other => paths.iter().map(|p| (p.clone(), Raw::Modified)).collect(),
        EventKind::Access(_) => Vec::new(),
    }
}

// ---------------------------------------------------------------- the panel

/// What the panel asks of the window.
pub enum Act {
    Open(PathBuf),
    Reveal(PathBuf),
}

/// What's watched, worked out from the project and the open files.
pub struct Source {
    pub spec: Spec,
    /// A project on a server, which isn't watched.
    pub on_server: bool,
}

#[derive(Default)]
pub struct Panel {
    pub rows: Vec<Change>,
    watching: Option<(Spec, Watcher)>,
    /// The project's top, to show folders relative to.
    root: Option<PathBuf>,
}

impl Panel {
    /// Starts, restarts or stops the watching so it matches `source`; and gathers what's changed. Rows are kept when
    /// only the folders change, and cleared for another project.
    pub fn update(&mut self, source: &Source, ctx: &egui::Context) {
        if source.on_server || (source.spec.root.is_none() && source.spec.folders.is_empty()) {
            self.watching = None;
        } else if self.watching.as_ref().is_none_or(|(spec, _)| *spec != source.spec) {
            if self.watching.as_ref().is_some_and(|(spec, _)| spec.root != source.spec.root) {
                self.rows.clear();
            }
            self.watching = Some((source.spec.clone(), Watcher::start(source.spec.clone(), Some(ctx.clone()))));
        }
        self.root.clone_from(&source.spec.root);
        if let Some((_, watcher)) = &self.watching {
            for change in watcher.take() {
                merge(&mut self.rows, change);
            }
        }
    }

    /// Stops watching (the panel's shut).
    pub fn stop(&mut self) {
        self.watching = None;
    }

    pub fn draw(&mut self, ui: &mut Ui, on_server: bool, open: &dyn Fn(&Path) -> bool) -> Vec<Act> {
        let mut acts = Vec::new();
        let now = SystemTime::now();
        if on_server {
            ui.label(RichText::new(crate::i18n::tr("This project is on a server, so its files aren't watched here. Open the project's folder on this computer to see its changes.")).color(crate::theme::text_faint()).size(12.0));
            return acts;
        }
        ui.horizontal(|ui| {
            if let Some((_, watcher)) = &self.watching {
                let words = if watcher.polling { crate::i18n::tr("Looking every few seconds") } else { crate::i18n::tr("Watching live") };
                ui.label(RichText::new(words).color(crate::theme::text_faint()).size(11.5));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled(!self.rows.is_empty(), egui::Button::new(crate::i18n::tr("Clear")).small()).on_hover_text(crate::i18n::tr("Empties the list; it fills again as files change")).clicked() {
                    self.rows.clear();
                }
            });
        });
        if let Some(why) = self.watching.as_ref().and_then(|(_, w)| w.problem.lock().ok().and_then(|p| p.clone())) {
            ui.label(RichText::new(crate::i18n::tf!("Some folders couldn't be watched: {why}")).color(crate::theme::problem_mark()).size(11.5));
        }
        if self.rows.is_empty() {
            let words = if self.watching.is_some() { crate::i18n::tr("Nothing has changed yet. Files changed by Limoniter, an agent or another program appear here as it happens.") } else { crate::i18n::tr("Open a folder, a project or a file to watch its folder.") };
            ui.label(RichText::new(words).color(crate::theme::text_faint()).size(12.0));
            return acts;
        }
        // Names that appear more than once get their folder beside them.
        let mut counts: HashMap<String, usize> = HashMap::new();
        for row in &self.rows {
            *counts.entry(name_of(&row.path)).or_default() += 1;
        }
        let mut forget = None;
        let mut next_tick = Duration::from_secs(30);
        egui::ScrollArea::vertical().id_salt("files-changing-scroll").auto_shrink([false, false]).show(ui, |ui| {
            for (n, row) in self.rows.iter().enumerate() {
                let age = now.duration_since(row.when).unwrap_or_default();
                next_tick = next_tick.min(if age < Duration::from_secs(60) { Duration::from_secs(1) } else if age < Duration::from_secs(3600) { Duration::from_secs(5) } else { Duration::from_secs(30) });
                let name = name_of(&row.path);
                let folder = if counts.get(&name).is_some_and(|c| *c > 1) { folder_of(&row.path, self.root.as_deref()) } else { String::new() };
                let deleted = row.what == What::Deleted;
                let response = draw_row(ui, row, &name, &folder, &said(row.what, &ago_now(row.when, now)), open(&row.path));
                let exact = chrono::DateTime::<chrono::Local>::from(row.when).format("%Y-%m-%d %H:%M:%S").to_string();
                let response = response.on_hover_text(format!("{}\n{exact}", row.path.display()));
                if response.clicked() && !deleted {
                    acts.push(Act::Open(row.path.clone()));
                }
                crate::fit::context_menu(&response, |ui| {
                    if ui.add_enabled(!deleted, egui::Button::new(crate::i18n::tr("Open"))).clicked() {
                        acts.push(Act::Open(row.path.clone()));
                        ui.close();
                    }
                    if ui.add_enabled(!deleted, egui::Button::new(crate::platform::SHOW_IN_FILE_MANAGER)).clicked() {
                        acts.push(Act::Reveal(row.path.clone()));
                        ui.close();
                    }
                    if ui.button(crate::i18n::tr("Copy path")).clicked() {
                        ui.ctx().copy_text(row.path.display().to_string());
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(crate::i18n::tr("Forget")).clicked() {
                        forget = Some(n);
                        ui.close();
                    }
                });
            }
        });
        if let Some(n) = forget {
            self.rows.remove(n);
        }
        ui.ctx().request_repaint_after(next_tick);
        acts
    }
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// The folder a file is in, relative to the project's top where it's in it.
fn folder_of(path: &Path, root: Option<&Path>) -> String {
    let parent = path.parent().unwrap_or(path);
    let relative = root.and_then(|r| parent.strip_prefix(r).ok()).filter(|r| !r.as_os_str().is_empty()).unwrap_or(parent);
    relative.display().to_string()
}

/// One row: the name, its folder if given, who did it, and when. Two lines.
fn draw_row(ui: &mut Ui, row: &Change, name: &str, folder: &str, when: &str, is_open: bool) -> egui::Response {
    let height = 38.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 5.0, crate::theme::hover());
    }
    let icon = crate::file_icons::width(13.0) + 4.0;
    crate::file_icons::paint(ui.painter(), egui::Rect::from_min_size(egui::pos2(rect.min.x + 6.0, rect.min.y + 2.0), egui::vec2(icon, 20.0)), name, 13.0);
    let ink = if row.what == What::Deleted { crate::theme::text_faint() } else { crate::theme::text() };
    let who_colour = match row.who {
        Who::You => crate::theme::accent(),
        Who::Agent(_) => crate::theme::tag_colour(6),
        Who::Elsewhere => crate::theme::text_dim(),
    };
    // Who, at the right of the first line.
    let who = row.who.label();
    let who_galley = ui.painter().layout_no_wrap(who, egui::FontId::proportional(11.0), who_colour);
    let who_rect = egui::Rect::from_min_size(egui::pos2(rect.max.x - who_galley.size().x - 12.0, rect.min.y + 3.0), who_galley.size() + egui::vec2(8.0, 2.0));
    ui.painter().rect_stroke(who_rect, 4.0, egui::Stroke::new(1.0, who_colour), egui::StrokeKind::Inside);
    ui.painter().galley(who_rect.min + egui::vec2(4.0, 1.0), who_galley, who_colour);
    // The name, and its folder dimmed.
    let room = (who_rect.min.x - rect.min.x - 8.0 - icon - 8.0).max(20.0);
    let mut job = egui::text::LayoutJob::default();
    job.append(name, 0.0, egui::TextFormat::simple(egui::FontId::proportional(13.0), ink));
    if !folder.is_empty() {
        job.append(&format!("  {folder}"), 0.0, egui::TextFormat::simple(egui::FontId::proportional(11.5), crate::theme::text_faint()));
    }
    job.wrap = egui::text::TextWrapping { max_width: room, max_rows: 1, break_anywhere: true, overflow_character: Some('\u{2026}') };
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(egui::pos2(rect.min.x + 6.0 + icon, rect.min.y + 4.0), galley, ink);
    let sub = if is_open { format!("{when}  \u{2022}") } else { when.to_string() };
    ui.painter().text(egui::pos2(rect.min.x + 6.0 + icon, rect.min.y + 21.0), egui::Align2::LEFT_TOP, sub, egui::FontId::proportional(11.5), crate::theme::text_dim());
    crate::access::button(&response, &format!("{name}, {when}"));
    response
}

// ---------------------------------------------------------------- the window's side

/// The folders to watch: a project's folder with what the Explorer shows, else the folders of the open files.
pub fn source_for(root: Option<&Path>, hidden: &[Hide], on_server: bool, open_files: &[PathBuf]) -> Source {
    let mut folders: Vec<PathBuf> = open_files.iter().filter_map(|p| p.parent()).filter(|p| !p.as_os_str().is_empty()).map(Path::to_path_buf).collect();
    folders.sort();
    folders.dedup();
    if let Some(root) = root {
        // Those inside the project are watched with it.
        folders.retain(|f| !f.starts_with(root));
    }
    Source { spec: Spec { root: root.map(Path::to_path_buf), folders, hidden: hidden.to_vec() }, on_server }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(day: u32, h: u32, m: u32, s: u32) -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, day).unwrap().and_hms_opt(h, m, s).unwrap()
    }

    fn say(age: u64, then: chrono::NaiveDateTime, now: chrono::NaiveDateTime) -> String {
        ago_text(Duration::from_secs(age), then, now)
    }

    #[test]
    fn how_long_ago_is_worded_at_its_boundaries() {
        let now = at(27, 15, 0, 0);
        assert_eq!(say(0, now, now), "just now");
        assert_eq!(say(4, now, now), "just now");
        assert_eq!(say(5, now, now), "5 seconds ago");
        assert_eq!(say(34, now, now), "34 seconds ago");
        assert_eq!(say(59, now, now), "59 seconds ago");
        assert_eq!(say(60, now, now), "1 minute ago");
        assert_eq!(say(119, now, now), "1 minute ago");
        assert_eq!(say(120, now, now), "2 minutes ago");
        assert_eq!(say(3599, now, now), "59 minutes ago");
        // An hour or more ago, the same day: the time. Another day: yesterday, then the date.
        assert_eq!(say(3600, at(27, 14, 0, 0), now), "at 14:00");
        assert_eq!(say(15 * 3600 + 5, at(26, 23, 59, 55), at(27, 15, 0, 0)), "yesterday");
        assert_eq!(say(2 * 86400, at(25, 15, 0, 0), now), "on 25 Nov");
        // Over an hour old and from before midnight is yesterday's.
        assert_eq!(say(3700, at(26, 23, 0, 0), at(27, 0, 2, 0)), "yesterday");
    }

    #[test]
    fn a_row_says_what_happened() {
        assert_eq!(said(What::Changed, "34 seconds ago"), "changed 34 seconds ago");
        assert_eq!(said(What::New, "just now"), "new, just now");
        assert_eq!(said(What::Deleted, "at 14:05"), "deleted at 14:05");
    }

    fn change(path: &str, what: What, secs: u64) -> Change {
        Change { path: PathBuf::from(path), what, when: SystemTime::UNIX_EPOCH + Duration::from_secs(secs), who: Who::Elsewhere }
    }

    #[test]
    fn the_list_is_newest_first_one_row_a_file() {
        let mut rows = Vec::new();
        merge(&mut rows, change("/p/a.php", What::Changed, 1));
        merge(&mut rows, change("/p/b.php", What::New, 2));
        merge(&mut rows, change("/p/a.php", What::Changed, 3));
        let names: Vec<_> = rows.iter().map(|r| (name_of(&r.path), r.what)).collect();
        assert_eq!(names, [("a.php".to_string(), What::Changed), ("b.php".to_string(), What::New)]);
        // New, then changed again, stays new; deleted then made again is changed; a rename drops the old name.
        merge(&mut rows, change("/p/b.php", What::Changed, 4));
        assert_eq!(rows[0].what, What::New);
        merge(&mut rows, change("/p/b.php", What::Deleted, 5));
        merge(&mut rows, change("/p/b.php", What::New, 6));
        assert_eq!(rows[0].what, What::Changed);
        merge(&mut rows, change("/p/a.php", What::MovedAway, 7));
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn the_list_keeps_only_the_newest_rows() {
        let mut rows = Vec::new();
        for n in 0..(MOST_ROWS as u64 + 50) {
            merge(&mut rows, change(&format!("/p/f{n}.txt"), What::Changed, n));
        }
        assert_eq!(rows.len(), MOST_ROWS);
        assert_eq!(rows[0].path, PathBuf::from(format!("/p/f{}.txt", MOST_ROWS + 49)));
        assert!(rows.iter().all(|r| r.path != Path::new("/p/f0.txt")));
    }

    #[test]
    fn a_burst_of_events_is_one_change_once_it_is_quiet() {
        let mut c = Coalescer::new(Duration::from_millis(100));
        let t0 = Instant::now();
        let path = PathBuf::from("/p/a.txt");
        for n in 0..5 {
            c.push(path.clone(), Raw::Modified, t0 + Duration::from_millis(n * 10));
        }
        assert!(c.due(t0 + Duration::from_millis(60), |_| true).is_empty(), "still being written");
        assert_eq!(c.due(t0 + Duration::from_millis(200), |_| true), [(path.clone(), What::Changed)]);
        assert!(c.due(t0 + Duration::from_millis(300), |_| true).is_empty(), "said once");
        // Made and written in pieces: new. Made and gone: nothing. Replaced: changed. Removed: deleted.
        c.push(path.clone(), Raw::Created, t0);
        c.push(path.clone(), Raw::Modified, t0);
        assert_eq!(c.due(t0 + Duration::from_secs(1), |_| true), [(path.clone(), What::New)]);
        c.push(path.clone(), Raw::Created, t0);
        c.push(path.clone(), Raw::Removed, t0);
        assert!(c.due(t0 + Duration::from_secs(1), |_| false).is_empty());
        c.push(path.clone(), Raw::Removed, t0);
        c.push(path.clone(), Raw::Created, t0);
        assert_eq!(c.due(t0 + Duration::from_secs(1), |_| true), [(path.clone(), What::Changed)]);
        c.push(path.clone(), Raw::Removed, t0);
        assert_eq!(c.due(t0 + Duration::from_secs(1), |_| false), [(path, What::Deleted)]);
    }

    #[test]
    fn renames_show_as_the_new_name_and_saves_by_renaming_are_changes() {
        let mut c = Coalescer::new(Duration::ZERO);
        let t = Instant::now();
        c.push("/p/old.txt".into(), Raw::RenamedFrom, t);
        c.push("/p/new.txt".into(), Raw::RenamedTo(false), t);
        c.push("/p/saved.txt.tmp.123".into(), Raw::Created, t);
        c.push("/p/saved.txt".into(), Raw::RenamedTo(true), t);
        let found = c.due(t, |p| p != Path::new("/p/old.txt") && p != Path::new("/p/saved.txt.tmp.123"));
        let expected: Vec<(PathBuf, What)> = vec![("/p/new.txt".into(), What::Renamed), ("/p/old.txt".into(), What::MovedAway), ("/p/saved.txt".into(), What::Changed)];
        assert_eq!(found, expected);
    }

    #[test]
    fn polling_is_for_places_the_system_cannot_watch() {
        assert!(needs_polling(Path::new(r"\\wsl$\Ubuntu\home\a"), ""));
        assert!(needs_polling(Path::new(r"\\wsl.localhost\Ubuntu\home\a"), ""));
        assert!(needs_polling(Path::new(r"\\server\share\x"), ""));
        assert!(needs_polling(Path::new("/mnt/h/Limoniter"), ""));
        assert!(!needs_polling(Path::new("/mnt/wsl/x"), ""));
        assert!(!needs_polling(Path::new("/home/a/p"), "/dev/sda1 / ext4 rw 0 0\n"));
        let mounts = "/dev/sda1 / ext4 rw 0 0\nserver:/x /srv/share nfs4 rw 0 0\n/dev/sdb1 /srv/share/local ext4 rw 0 0\n";
        assert!(needs_polling(Path::new("/srv/share/p"), mounts));
        assert!(!needs_polling(Path::new("/srv/share/local/p"), mounts), "the nearest mount decides");
    }

    // The tests below watch a real folder. Each has its own, and gives up after a while rather than hanging.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-changing-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn watch(dir: &Path) -> Watcher {
        // Polled quickly, so the test doesn't depend on the system's events (or on /tmp being where they work).
        Watcher::start_with(Spec { root: Some(dir.to_path_buf()), ..Spec::default() }, None, Duration::from_millis(150), Duration::from_millis(100), false)
    }

    /// Waits for a change matching, and gives it.
    fn wait_for(watcher: &Watcher, found: &mut Vec<Change>, wanted: impl Fn(&Change) -> bool) -> Option<Change> {
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(10) {
            found.extend(watcher.take());
            if let Some(change) = found.iter().find(|c| wanted(c)) {
                return Some(change.clone());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }

    #[test]
    fn writes_creations_and_deletions_in_a_folder_are_seen() {
        let dir = scratch("real");
        std::fs::write(dir.join("old.txt"), "one").unwrap();
        let watcher = watch(&dir);
        std::thread::sleep(Duration::from_millis(600));
        let mut found = Vec::new();
        std::fs::write(dir.join("new.txt"), "a").unwrap();
        let change = wait_for(&watcher, &mut found, |c| c.path == dir.join("new.txt")).expect("a new file is seen");
        assert_eq!((change.what, change.who), (What::New, Who::Elsewhere));
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(dir.join("old.txt"), "changed and longer").unwrap();
        let change = wait_for(&watcher, &mut found, |c| c.path == dir.join("old.txt")).expect("a change is seen");
        assert_eq!(change.what, What::Changed);
        std::fs::remove_file(dir.join("old.txt")).unwrap();
        wait_for(&watcher, &mut found, |c| c.path == dir.join("old.txt") && c.what == What::Deleted).expect("a deletion is seen");
        // A file made in a new folder is seen too.
        std::fs::create_dir(dir.join("sub")).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        std::fs::write(dir.join("sub").join("deep.txt"), "x").unwrap();
        wait_for(&watcher, &mut found, |c| c.path == dir.join("sub").join("deep.txt")).expect("a file in a new folder is seen");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn polling_sees_changes_too() {
        let dir = scratch("polled");
        let watcher = Watcher::start_with(Spec { root: Some(dir.clone()), ..Spec::default() }, None, Duration::from_millis(150), Duration::from_millis(100), true);
        assert!(watcher.polling);
        std::thread::sleep(Duration::from_millis(600));
        let mut found = Vec::new();
        std::fs::write(dir.join("p.txt"), "a").unwrap();
        assert_eq!(wait_for(&watcher, &mut found, |c| c.path == dir.join("p.txt")).expect("seen").what, What::New);
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::remove_file(dir.join("p.txt")).unwrap();
        wait_for(&watcher, &mut found, |c| c.path == dir.join("p.txt") && c.what == What::Deleted).expect("a deletion is seen");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_written_in_pieces_is_one_change() {
        let dir = scratch("pieces");
        let watcher = watch(&dir);
        std::thread::sleep(Duration::from_millis(600));
        let path = dir.join("big.txt");
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&path).unwrap();
            for n in 0..8 {
                writeln!(file, "piece {n}").unwrap();
                file.flush().unwrap();
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        let mut found = Vec::new();
        wait_for(&watcher, &mut found, |c| c.path == path).expect("seen");
        std::thread::sleep(Duration::from_millis(800));
        found.extend(watcher.take());
        assert_eq!(found.iter().filter(|c| c.path == path).count(), 1, "{found:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saved_by_renaming_into_place_is_a_change_to_the_file_and_the_temporary_is_not_shown() {
        let dir = scratch("rename");
        std::fs::write(dir.join("a.txt"), "before").unwrap();
        let watcher = watch(&dir);
        std::thread::sleep(Duration::from_millis(1200));
        crate::codec::write_file(&dir.join("a.txt"), b"after, and longer").unwrap();
        let mut found = Vec::new();
        let change = wait_for(&watcher, &mut found, |c| c.path == dir.join("a.txt")).expect("seen");
        assert_eq!(change.what, What::Changed);
        std::thread::sleep(Duration::from_millis(600));
        found.extend(watcher.take());
        assert!(found.iter().all(|c| !c.path.to_string_lossy().contains("limoniter-tmp")), "{found:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn limoniters_own_save_is_you_and_a_claim_names_the_agent() {
        let dir = scratch("who");
        let watcher = watch(&dir);
        std::thread::sleep(Duration::from_millis(600));
        let own = dir.join("mine.txt");
        crate::codec::write_file(&own, b"saved by Limoniter").unwrap();
        note_own_write(&own);
        let theirs = dir.join("agent.txt");
        claim(&theirs, "Claude Code");
        std::fs::write(&theirs, "written by an agent").unwrap();
        let other = dir.join("other.txt");
        std::fs::write(&other, "from elsewhere").unwrap();
        let mut found = Vec::new();
        for (path, who) in [(&own, Who::You), (&theirs, Who::Agent("Claude Code".into())), (&other, Who::Elsewhere)] {
            let change = wait_for(&watcher, &mut found, |c| &c.path == path).expect("seen");
            assert_eq!(change.who, who, "{}", path.display());
        }
        // A save by Limoniter that someone then changes again is no longer ours.
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::write(&own, "changed behind Limoniter's back, longer than before").unwrap();
        found.clear();
        let change = wait_for(&watcher, &mut found, |c| c.path == own && c.what == What::Changed).expect("seen");
        assert_eq!(change.who, Who::Elsewhere);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_claim_on_a_folder_covers_the_files_in_it() {
        let dir = scratch("claim-folder");
        claim(&dir, "Codex");
        assert_eq!(who_changed(&dir.join("x").join("y.rs")), Who::Agent("Codex".into()));
        assert_eq!(who_changed(&dir.with_file_name("elsewhere-file")), Who::Elsewhere);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_the_explorer_leaves_out_is_left_out() {
        let dir = scratch("ignored");
        std::fs::create_dir_all(dir.join("build")).unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("cache")).unwrap();
        std::fs::write(dir.join(".gitignore"), "build/\n*.log\n").unwrap();
        let hidden = vec![Hide::Folder("cache".into())];
        let watcher = Watcher::start_with(Spec { root: Some(dir.clone()), folders: Vec::new(), hidden }, None, Duration::from_millis(150), Duration::from_millis(100), false);
        std::thread::sleep(Duration::from_millis(600));
        for left_out in ["build/o.txt", ".git/HEAD", "cache/c.txt", "src/debug.log"] {
            std::fs::write(dir.join(left_out), "x").unwrap();
        }
        std::fs::write(dir.join("src").join("shown.rs"), "x").unwrap();
        let mut found = Vec::new();
        wait_for(&watcher, &mut found, |c| c.path == dir.join("src").join("shown.rs")).expect("the shown file is seen");
        std::thread::sleep(Duration::from_millis(600));
        found.extend(watcher.take());
        let names: Vec<String> = found.iter().map(|c| c.path.strip_prefix(&dir).unwrap().display().to_string()).collect();
        assert_eq!(names, [Path::new("src").join("shown.rs").display().to_string()], "{names:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_projects_folder_is_watched_and_open_files_outside_it_are_too() {
        let source = source_for(Some(Path::new("/p")), &[], false, &["/p/src/a.rs".into(), "/q/b.rs".into(), "/q/c.rs".into()]);
        assert_eq!(source.spec.root, Some(PathBuf::from("/p")));
        assert_eq!(source.spec.folders, [PathBuf::from("/q")]);
        let none = source_for(None, &[], false, &["/q/b.rs".into(), "/q/c.rs".into(), "/r/d.rs".into()]);
        assert_eq!(none.spec.folders, [PathBuf::from("/q"), PathBuf::from("/r")]);
    }
}
