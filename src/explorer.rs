//! The Explorer: a folder opened as a project, shown as VS Code shows one. Its tree is read from the disk a folder at
//! a time, as each is opened, and read again when it changes, so files made, renamed or deleted elsewhere show at
//! once. Folders come first; what Git leaves out is shown fainter; .git and its kind aren't shown. The file being
//! edited is found in the tree. New files and folders, and renaming, are typed in the tree itself. Folders and kinds
//! of file can be hidden, kept with the project; files edited lately stand out, with a dot coloured by how lately.

use crate::explorer_kept::{self as kept, Cut, Hide, Kept};
use crate::file_colours::{Colours, Tint};
use crate::git::Change;
use crate::workspace::{self, Ignores};
use eframe::egui::{self, RichText, Sense, Ui, vec2};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// How often an open folder's contents are looked at again.
const LOOK_AGAIN: Duration = Duration::from_millis(1500);

#[derive(Clone, Debug)]
struct Entry {
    name: String,
    path: PathBuf,
    folder: bool,
    ignored: bool,
    /// Hidden in the project: only listed while hidden things are shown, faintly.
    hidden: bool,
    size: u64,
    changed: Option<SystemTime>,
}

/// What the Explorer's lists are sorted by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SortBy {
    #[default]
    Name,
    /// By the ending of each name (.c, .h, .s), then the name.
    Kind,
    /// Largest first.
    Size,
    /// Newest first.
    Date,
}

impl SortBy {
    pub const ALL: [SortBy; 4] = [SortBy::Name, SortBy::Kind, SortBy::Size, SortBy::Date];

    pub fn label(self) -> &'static str {
        match self {
            SortBy::Name => "Name",
            SortBy::Kind => "Type",
            SortBy::Size => "Size, largest first",
            SortBy::Date => "Date changed, newest first",
        }
    }

    fn setting(self) -> &'static str {
        match self {
            SortBy::Name => "name",
            SortBy::Kind => "type",
            SortBy::Size => "size",
            SortBy::Date => "date",
        }
    }
}

/// How the Explorer is laid out: kept in the settings file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Order {
    pub by: SortBy,
    pub folders_first: bool,
    /// Each file's size and when it was changed, at the right of its row.
    pub details: bool,
}

impl Default for Order {
    fn default() -> Self {
        Order { by: SortBy::Name, folders_first: true, details: false }
    }
}

impl Order {
    pub fn to_settings(self) -> String {
        format!("explorer_sort={}\nexplorer_folders_first={}\nexplorer_details={}\n", self.by.setting(), self.folders_first, self.details)
    }

    /// Reads one setting, if it's one of these.
    pub fn read_setting(&mut self, name: &str, value: &str) {
        match name {
            "explorer_sort" => self.by = SortBy::ALL.into_iter().find(|by| by.setting() == value).unwrap_or_default(),
            "explorer_folders_first" => self.folders_first = value != "false",
            "explorer_details" => self.details = value == "true",
            _ => {}
        }
    }
}

/// The ending a name is sorted by for Type: none for a name without one.
fn ending(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, end)) if !stem.is_empty() => end.to_lowercase(),
        _ => String::new(),
    }
}

/// Puts a folder's entries in order.
fn sort_entries(entries: &mut [Entry], order: Order) {
    let by_name = |a: &Entry, b: &Entry| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.name.cmp(&b.name));
    entries.sort_by(|a, b| {
        let folders = if order.folders_first { b.folder.cmp(&a.folder) } else { std::cmp::Ordering::Equal };
        let main = match order.by {
            SortBy::Name => std::cmp::Ordering::Equal,
            SortBy::Kind => ending(&a.name).cmp(&ending(&b.name)),
            SortBy::Size => b.size.cmp(&a.size),
            SortBy::Date => b.changed.cmp(&a.changed),
        };
        folders.then(main).then_with(|| by_name(a, b))
    });
}

/// When a file was changed, briefly: the time if today, the day and month if this year, else the year too.
fn short_date(when: SystemTime) -> String {
    let local = chrono::DateTime::<chrono::Local>::from(when);
    let now = chrono::Local::now();
    if local.date_naive() == now.date_naive() {
        local.format("%H:%M").to_string()
    } else if local.format("%Y").to_string() == now.format("%Y").to_string() {
        local.format("%d %b").to_string()
    } else {
        local.format("%d %b %Y").to_string()
    }
}

/// A size in few letters: 812 B, 25 KB, 1.2 MB.
fn short_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let value = bytes as f64;
    match value {
        _ if value >= KB * KB * KB => format!("{:.1} GB", value / (KB * KB * KB)),
        _ if value >= KB * KB => format!("{:.1} MB", value / (KB * KB)),
        _ if value >= KB => format!("{:.0} KB", value / KB),
        _ => format!("{bytes} B"),
    }
}

struct Listing {
    looked: Instant,
    modified: Option<SystemTime>,
    entries: Vec<Entry>,
}

/// A name being typed in the tree.
#[derive(Clone, Debug, PartialEq)]
pub enum Editing {
    New { folder: PathBuf, is_folder: bool, name: String },
    Rename { path: PathBuf, name: String },
}

/// What the Explorer asks of the editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    Open(PathBuf),
    Create { path: PathBuf, folder: bool },
    Rename { from: PathBuf, to: PathBuf },
    Delete(PathBuf),
    CopyPath(PathBuf),
    CopyRelativePath(PathBuf),
    Reveal(PathBuf),
    Terminal(PathBuf),
    FindIn(PathBuf),
    CompareWith(PathBuf),
    CompareFolder(PathBuf),
    CompareWithHead(PathBuf),
    /// New file or files from one of the project's templates, in this folder.
    NewFromTemplate(PathBuf),
    /// Hide a folder, or a kind of file, in the project.
    Hide(Hide),
    /// Take back whatever hides this.
    ShowAgain { path: PathBuf, folder: bool },
    ShowHidden(bool),
    /// How many days back an edited file stands out; 0 for not at all.
    RecentDays(u32),
    /// The share of the project's files shown, in per cent, the most recently edited first.
    Recency(u32),
    /// The answer to the offer to hide the folder's build and dependency folders: these to hide, or none.
    HideBuildFolders(Vec<String>),
    /// The list of what's hidden, in the project's settings.
    WhatsHidden,
    /// A file given a colour of its own, or (with None) left to the rules.
    ColourFile { path: PathBuf, tint: Option<Tint> },
    /// Every file in a folder coloured by a rule for the folder; or with None, the folder's rule taken away.
    ColourFolder { folder: PathBuf, tint: Option<Tint> },
}

#[derive(Default)]
pub struct Explorer {
    root: Option<PathBuf>,
    expanded: HashSet<PathBuf>,
    listings: HashMap<PathBuf, Listing>,
    ignores: Option<Ignores>,
    /// The file last found in the tree for being edited.
    revealed: Option<PathBuf>,
    scroll_to: Option<PathBuf>,
    pub editing: Option<Editing>,
    focus_editing: bool,
    selected: Option<PathBuf>,
    /// What Git says of each changed file, and the folders with changes in them; and what that was worked out from.
    git: HashMap<PathBuf, Change>,
    git_folders: HashSet<PathBuf>,
    git_from: Option<(PathBuf, Vec<crate::git::Entry>)>,
    /// How the lists are sorted, and whether sizes and dates are shown.
    pub order: Order,
    /// What the project hides, and whether it's shown faintly, as the listings were read with.
    hidden: Vec<Hide>,
    show_hidden: bool,
    /// The build and dependency folders found when the folder was opened, offered for hiding.
    offer: Option<(PathBuf, Vec<String>)>,
    /// When each of the folder's files was last changed other than by Limoniter, from the background, for those
    /// changed lately; and how many times that's come in.
    outside: Option<(PathBuf, HashMap<PathBuf, SystemTime>)>,
    outside_count: u64,
    /// The files and folders edited lately, and when; and what that was worked out from, and when.
    recent_files: HashMap<PathBuf, SystemTime>,
    recent_folders: HashMap<PathBuf, SystemTime>,
    recent_from: Option<(u32, usize, Option<u64>, u64, usize)>,
    recent_made: Option<Instant>,
    /// When every file of the folder was last changed other than by Limoniter, for the recency slider, from the
    /// background; and how many times that's come in.
    pub recency: Recency,
}

/// The recency slider's state, shared by the folder Explorer and the Project panel of a `.limproj`: when every file
/// was last edited, what the slider leaves out, and the share while it's being dragged.
#[derive(Default)]
pub struct Recency {
    every: Option<(PathBuf, HashMap<PathBuf, SystemTime>)>,
    every_count: u64,
    /// What the slider leaves out, and what that was worked out from.
    pub cut: Option<Cut>,
    cut_from: Option<(u32, u64, usize, Option<u64>)>,
    /// The share the slider is being dragged to, before it's let go and kept.
    pub slider: Option<u32>,
}

/// How often what counts as recent is worked out again, as time passes.
const RECENT_AGAIN: Duration = Duration::from_secs(60);

/// How a file's Git status is shown: its letter, and its colour.
fn git_mark(change: Change) -> (&'static str, egui::Color32) {
    match change {
        Change::Modified | Change::TypeChanged => ("M", crate::theme::git_changed()),
        Change::Added => ("A", crate::theme::git_added()),
        Change::Untracked => ("U", crate::theme::git_added()),
        Change::Deleted => ("D", crate::theme::git_deleted()),
        Change::Renamed => ("R", crate::theme::git_added()),
        Change::Conflicted => ("!", crate::theme::problem_mark()),
    }
}

/// Where a file or folder dragged onto a row goes: into it if it's a folder, else beside it. None if that's where it
/// is already, or would put a folder inside itself.
pub fn drop_target(dragged: &Path, onto: &Path, onto_folder: bool) -> Option<PathBuf> {
    let folder = if onto_folder { onto.to_path_buf() } else { onto.parent()?.to_path_buf() };
    if dragged.parent() == Some(folder.as_path()) || folder.starts_with(dragged) {
        return None;
    }
    Some(folder.join(dragged.file_name()?))
}

impl Recency {
    /// Whether the dates of a folder's files have been read.
    pub fn has_times(&self, root: &Path) -> bool {
        self.every.as_ref().is_some_and(|(at, _)| at == root)
    }

    /// Forgets the dates read for another folder.
    pub fn keep_for(&mut self, root: &Path) {
        if self.every.as_ref().is_some_and(|(at, _)| at != root) {
            self.every = None;
            self.cut = None;
            self.cut_from = None;
        }
    }

    /// Takes in when every one of a folder's files was last changed other than by Limoniter, for the recency slider.
    pub fn set_every_time(&mut self, root: &Path, times: HashMap<PathBuf, SystemTime>) {
        self.every = Some((root.to_path_buf(), times));
        self.every_count += 1;
    }

    /// Ranks files seen in the folder that the dates read don't have: they were made after the dates were read, so
    /// they're the newest, and are counted at once (the cut is worked out again next time it's drawn) without
    /// reading every file's date again. The dates read are replaced when the next read comes in.
    pub fn rank_new<'a>(&mut self, root: &Path, files: impl IntoIterator<Item = &'a Path>) {
        let Some((_, every)) = self.every.as_mut().filter(|(at, _)| at == root) else { return };
        let now = SystemTime::now();
        let mut added = false;
        for file in files {
            if !every.contains_key(file) {
                every.insert(file.to_path_buf(), now);
                added = true;
            }
        }
        if added {
            self.every_count += 1;
        }
    }

    /// Works out again which files the recency slider leaves out, when the share or what it's worked out from has
    /// changed.
    pub fn update_cut(&mut self, root: &Path, kept: &Kept) {
        let share = self.slider.unwrap_or(kept.recency);
        let from = (share, self.every_count, kept.saved.len(), kept.saved.values().max().copied());
        if share >= 100 {
            self.cut = None;
            self.cut_from = None;
            return;
        }
        if self.cut_from == Some(from) {
            return;
        }
        self.cut_from = Some(from);
        let Some((_, every)) = self.every.as_ref().filter(|(at, _)| at == root) else {
            self.cut = None;
            return;
        };
        let times: Vec<(PathBuf, SystemTime)> = every.iter().filter_map(|(path, &outside)| Some((path.clone(), kept::last_edited(kept.saved_at(path), Some(outside))?))).collect();
        self.cut = Some(Cut::new(root, &times, share));
    }

    /// The slider at the bottom: how much of the project shows, the most recently edited files first.
    pub fn slider(&mut self, ui: &mut Ui, kept: &Kept, acts: &mut Vec<Act>) {
        let mut share = self.slider.unwrap_or(kept.recency);
        let response = ui
            .add(egui::Slider::new(&mut share, 0..=100).suffix("%").text(crate::i18n::tr("Newest")))
            .on_hover_text(crate::i18n::tr("Shows only this share of the project's files: the ones edited most recently"));
        if response.dragged() {
            self.slider = Some(share);
        } else if response.changed() || self.slider.is_some() {
            // Let go, or moved by a key or the wheel: kept with the project.
            self.slider = None;
            if share != kept.recency {
                acts.push(Act::Recency(share));
            }
        }
        if let Some(cut) = self.cut.as_ref().filter(|_| share < 100) {
            ui.label(RichText::new(cut.said()).size(12.0).color(crate::theme::text_faint()));
        } else if share < 100 {
            // The dates are still being read.
            ui.label(RichText::new(crate::i18n::tr("Reading when the files were edited\u{2026}")).size(12.0).color(crate::theme::text_faint()));
        }
    }

    /// Whether the line under the slider will be there, for the room it needs.
    pub fn tall(&self, kept: &Kept) -> bool {
        self.cut.as_ref().is_some_and(|cut| cut.hidden() > 0 || cut.shown == 0) || self.slider.is_some_and(|s| s < 100) || kept.recency < 100
    }

    /// Whether a file or folder is left out of the tree.
    pub fn hides(&self, path: &Path, is_folder: bool) -> bool {
        self.cut.as_ref().is_some_and(|cut| cut.hides(path, is_folder))
    }
}

impl Explorer {
    /// The names listed in a folder, for tests.
    #[cfg(test)]
    pub fn names_for_test(&mut self, folder: &Path) -> Vec<String> {
        self.listing(folder).into_iter().map(|e| e.name).collect()
    }

    fn listing(&mut self, folder: &Path) -> Vec<Entry> {
        let modified = std::fs::metadata(folder).and_then(|m| m.modified()).ok();
        let fresh = self.listings.get(folder).is_some_and(|l| l.looked.elapsed() < LOOK_AGAIN || l.modified == modified);
        if !fresh {
            let mut entries = Vec::new();
            if let Ok(read) = std::fs::read_dir(folder) {
                for entry in read.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if workspace::hidden(&name) {
                        continue;
                    }
                    let path = entry.path();
                    // A link to a folder is shown as one, and opened as one.
                    let folder = path.is_dir();
                    let hidden = self.root.as_deref().is_some_and(|root| kept::hidden_by(&self.hidden, root, &path, folder));
                    if hidden && !self.show_hidden {
                        continue;
                    }
                    let ignored = self.ignores.as_mut().is_some_and(|i| i.ignored(&path, folder));
                    let meta = std::fs::metadata(&path).ok();
                    let size = meta.as_ref().filter(|_| !folder).map_or(0, std::fs::Metadata::len);
                    let changed = meta.and_then(|m| m.modified().ok());
                    entries.push(Entry { name, path, folder, ignored, hidden, size, changed });
                }
            }
            sort_entries(&mut entries, self.order);
            self.listings.insert(folder.to_path_buf(), Listing { looked: Instant::now(), modified, entries });
        } else if let Some(l) = self.listings.get_mut(folder) {
            l.looked = Instant::now();
        }
        self.listings.get(folder).map(|l| l.entries.clone()).unwrap_or_default()
    }

    /// Reads everything again: after a change made here, or Refresh.
    pub fn refresh(&mut self) {
        self.listings.clear();
        if let Some(root) = &self.root {
            self.ignores = Some(Ignores::new(root));
        }
    }

    /// Starts typing a new file's or folder's name in a folder.
    pub fn start_new(&mut self, folder: PathBuf, is_folder: bool) {
        self.expanded.insert(folder.clone());
        self.editing = Some(Editing::New { folder, is_folder, name: String::new() });
        self.focus_editing = true;
    }

    fn start_rename(&mut self, path: &Path) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.editing = Some(Editing::Rename { path: path.to_path_buf(), name });
        self.focus_editing = true;
    }

    /// Opens the folders down to a file, and scrolls to it.
    fn reveal(&mut self, root: &Path, file: &Path) {
        let Ok(relative) = file.strip_prefix(root) else { return };
        let mut folder = root.to_path_buf();
        for part in relative.parent().into_iter().flat_map(Path::components) {
            folder.push(part);
            self.expanded.insert(folder.clone());
        }
        self.scroll_to = Some(file.to_path_buf());
        self.selected = Some(file.to_path_buf());
    }

    /// Offers to hide the build and dependency folders found in a folder just opened, in a bar at the top.
    pub fn offer_to_hide(&mut self, root: &Path, names: Vec<String>) {
        self.offer = (!names.is_empty()).then(|| (root.to_path_buf(), names));
    }

    /// The build and dependency folders being offered for hiding in a folder, if they are.
    #[cfg(test)]
    pub fn offering(&self, root: &Path) -> Option<&[String]> {
        self.offer.as_ref().filter(|(at, _)| at == root).map(|(_, names)| names.as_slice())
    }

    /// Takes in when each of a folder's files was last changed other than by Limoniter, worked out in the
    /// background.
    pub fn set_outside_times(&mut self, root: &Path, times: HashMap<PathBuf, SystemTime>) {
        self.outside = Some((root.to_path_buf(), times));
        self.outside_count += 1;
    }

    pub fn set_every_time(&mut self, root: &Path, times: HashMap<PathBuf, SystemTime>) {
        self.recency.set_every_time(root, times);
    }

    /// Works out again which files and folders were edited lately, when what that comes from has changed, or a
    /// minute has gone by.
    fn update_recent(&mut self, root: &Path, kept: &Kept) {
        let from = (kept.recent_days, kept.saved.len(), kept.saved.values().max().copied(), self.outside_count, kept.hidden.len());
        if self.recent_from == Some(from) && self.recent_made.is_some_and(|made| made.elapsed() < RECENT_AGAIN) {
            return;
        }
        self.recent_from = Some(from);
        self.recent_made = Some(Instant::now());
        self.recent_files.clear();
        self.recent_folders.clear();
        let now = SystemTime::now();
        let outside = self.outside.as_ref().filter(|(at, _)| at == root).map(|(_, times)| times);
        let paths: HashSet<&PathBuf> = outside.into_iter().flat_map(HashMap::keys).chain(kept.saved.keys().filter(|path| path.starts_with(root))).collect();
        for path in paths {
            let Some(when) = kept::last_edited(kept.saved_at(path), outside.and_then(|times| times.get(path).copied())) else { continue };
            if !kept::within(when, now, kept.recent_days) || kept.hides(root, path, false) {
                continue;
            }
            self.recent_files.insert(path.clone(), when);
            // The folders it's in get its dot too, so it's found with them folded.
            let mut folder = path.parent();
            while let Some(here) = folder.filter(|f| f.starts_with(root) && *f != root) {
                let latest = self.recent_folders.entry(here.to_path_buf()).or_insert(when);
                *latest = (*latest).max(when);
                folder = here.parent();
            }
        }
    }

    /// The tree of a folder, with the file being edited (if it's in it) found and shown.
    /// Takes in what Git says has changed, in the repository at `repo`, when that's changed.
    pub fn set_git(&mut self, repo: Option<&Path>, entries: &[crate::git::Entry]) {
        let now = repo.map(|r| (r.to_path_buf(), entries.to_vec()));
        if now == self.git_from {
            return;
        }
        self.git.clear();
        self.git_folders.clear();
        if let Some((repo, entries)) = &now {
            for entry in entries {
                let Some(change) = entry.unstaged.or(entry.staged) else { continue };
                let path = repo.join(&entry.path);
                let mut folder = path.parent();
                while let Some(here) = folder.filter(|f| f.starts_with(repo)) {
                    if !self.git_folders.insert(here.to_path_buf()) {
                        break;
                    }
                    folder = here.parent();
                }
                self.git.insert(path, change);
            }
        }
        self.git_from = now;
    }

    pub fn draw(&mut self, ui: &mut Ui, root: &Path, active: Option<&Path>, kept: &Kept, colours: &Colours) -> Vec<Act> {
        let mut acts = Vec::new();
        if self.root.as_deref() != Some(root) {
            // What came in for this folder before it was first drawn stays.
            let offer = self.offer.take().filter(|(at, _)| at == root);
            let outside = self.outside.take().filter(|(at, _)| at == root);
            let recency = std::mem::take(&mut self.recency);
            *self = Explorer { root: Some(root.to_path_buf()), ignores: Some(Ignores::new(root)), order: self.order, offer, outside, outside_count: self.outside_count, recency, ..Explorer::default() };
            self.recency.keep_for(root);
            self.expanded.insert(root.to_path_buf());
        }
        if self.hidden != kept.hidden || self.show_hidden != kept.show_hidden {
            self.hidden.clone_from(&kept.hidden);
            self.show_hidden = kept.show_hidden;
            self.listings.clear();
        }
        self.update_recent(root, kept);
        self.recency.update_cut(root, kept);
        if let Some(active) = active
            && self.revealed.as_deref() != Some(active)
        {
            self.revealed = Some(active.to_path_buf());
            self.reveal(root, active);
        }
        ui.horizontal(|ui| {
            let name = root.file_name().map_or_else(|| root.display().to_string(), |n| n.to_string_lossy().into_owned());
            ui.label(RichText::new(name.to_uppercase()).strong().size(12.0)).on_hover_text(root.display().to_string());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(crate::i18n::tr("\u{229F}")).on_hover_text(crate::i18n::tr("Collapse every folder")).clicked() {
                    self.expanded.clear();
                    self.expanded.insert(root.to_path_buf());
                }
                if ui.small_button(crate::i18n::tr("\u{27F3}")).on_hover_text(crate::i18n::tr("Read the folder again")).clicked() {
                    self.refresh();
                }
                // How the lists are sorted, and whether sizes and dates show.
                let before = self.order;
                crate::fit::menu_button(ui, crate::i18n::tr("\u{21C5}"), |ui| {
                    for by in SortBy::ALL {
                        ui.radio_value(&mut self.order.by, by, crate::i18n::tr(by.label()));
                    }
                    ui.separator();
                    ui.checkbox(&mut self.order.folders_first, crate::i18n::tr("Folders first"));
                    ui.checkbox(&mut self.order.details, crate::i18n::tr("Sizes and dates"));
                    ui.separator();
                    let mut show = kept.show_hidden;
                    ui.add_enabled(!kept.hidden.is_empty(), egui::Checkbox::new(&mut show, crate::i18n::tr("Show hidden items"))).on_hover_text(crate::i18n::tr("What's hidden is shown faintly, to be found and shown again"));
                    if show != kept.show_hidden {
                        acts.push(Act::ShowHidden(show));
                    }
                    if ui.add_enabled(!kept.hidden.is_empty(), egui::Button::new(crate::i18n::tr("What's hidden\u{2026}"))).clicked() {
                        acts.push(Act::WhatsHidden);
                        ui.close();
                    }
                    ui.separator();
                    ui.label(crate::i18n::tr("Files edited lately stand out:"));
                    let mut days = kept.recent_days;
                    for (choice, label) in [(0, "Not at all"), (1, "In the last day"), (7, "In the last week"), (30, "In the last month")] {
                        ui.radio_value(&mut days, choice, crate::i18n::tr(label));
                    }
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::tr("In the last"));
                        ui.add(egui::DragValue::new(&mut days).range(0..=kept::MOST_RECENT_DAYS));
                        ui.label(crate::i18n::tr("days"));
                    });
                    if days != kept.recent_days {
                        acts.push(Act::RecentDays(days));
                    }
                })
                .response
                .on_hover_text(crate::i18n::tr("Sort by, sizes and dates, what's hidden, and files edited lately"));
                if self.order != before {
                    self.listings.clear();
                }
                let target = self.selected.as_ref().map(|s| if s.is_dir() { s.clone() } else { s.parent().map_or_else(|| root.to_path_buf(), Path::to_path_buf) }).filter(|f| f.starts_with(root)).unwrap_or_else(|| root.to_path_buf());
                if ui.small_button(crate::i18n::tr("+\u{1F4C1}")).on_hover_text(crate::i18n::tr("New folder")).clicked() {
                    self.start_new(target.clone(), true);
                }
                if ui.small_button(crate::i18n::tr("+\u{1F4C4}")).on_hover_text(crate::i18n::tr("New file")).clicked() {
                    self.start_new(target, false);
                }
            });
        });
        self.offer_bar(ui, root, &mut acts);
        // The slider at the bottom takes its room first.
        let slider_room = if self.recency.tall(kept) { 52.0 } else { 30.0 };
        egui::ScrollArea::both().id_salt("explorer-tree").auto_shrink([false, false]).max_height((ui.available_height() - slider_room).max(40.0)).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            self.folder_rows(ui, root, root, 0, active, colours, &mut acts);
        });
        self.recency.slider(ui, kept, &mut acts);
        acts
    }

    fn folder_rows(&mut self, ui: &mut Ui, root: &Path, folder: &Path, depth: usize, active: Option<&Path>, colours: &Colours, acts: &mut Vec<Act>) {
        // A new file or folder being named goes at the top of its folder.
        if let Some(Editing::New { folder: at, is_folder, .. }) = &self.editing
            && at == folder
        {
            let is_folder = *is_folder;
            self.editing_row(ui, depth, if is_folder { "\u{25B8} " } else { "" }, acts);
        }
        let entries = self.listing(folder);
        // A file made since the slider's dates were read is the newest: ranked now, and counted.
        self.recency.rank_new(root, entries.iter().filter(|e| !e.folder && !e.hidden).map(|e| e.path.as_path()));
        for entry in entries {
            if self.recency.hides(&entry.path, entry.folder) {
                continue;
            }
            let renaming = matches!(&self.editing, Some(Editing::Rename { path, .. }) if *path == entry.path);
            if renaming {
                self.editing_row(ui, depth, if entry.folder { "\u{25B8} " } else { "" }, acts);
                continue;
            }
            let open = entry.folder && self.expanded.contains(&entry.path);
            let is_active = active.is_some_and(|a| a == entry.path);
            let selected = self.selected.as_deref() == Some(entry.path.as_path());
            let chevron = if entry.folder { if open { "\u{25BE} " } else { "\u{25B8} " } } else { "" };
            let change = self.git.get(&entry.path).copied();
            let colour = match change {
                _ if entry.hidden => crate::theme::text_disabled(),
                _ if entry.ignored => crate::theme::text_faint(),
                Some(change) => git_mark(change).1,
                None => crate::theme::text(),
            };
            // Edited lately: a file's name in bold, and a dot beside it (a folder's too) as hot as how lately.
            let recent = if entry.folder { self.recent_folders.get(&entry.path) } else { self.recent_files.get(&entry.path) }.copied();
            let bold = recent.is_some() && !entry.folder;
            // A row the panel's width, as VS Code's are: the file being edited in the selection's colour.
            let font = match crate::preview::bold_family().filter(|_| bold) {
                Some(family) => egui::FontId::new(12.5, family),
                None => egui::FontId::proportional(12.5),
            };
            let galley = ui.painter().layout_no_wrap(format!("{chevron}{}", entry.name), font, colour);
            // A file's badge goes where a folder's arrow is.
            let icon = if entry.folder { 0.0 } else { crate::file_icons::width(12.5) + 3.0 };
            let dot_room = if recent.is_some() { 12.0 } else { 0.0 };
            let width = (depth as f32 * 12.0 + icon + galley.size().x + dot_room + 8.0).max(ui.available_width());
            let (rect, response) = ui.allocate_exact_size(vec2(width, galley.size().y + 4.0), Sense::click_and_drag());
            // Dragged onto another folder (or a file in one), it's moved there.
            response.dnd_set_drag_payload(entry.path.clone());
            let dropping = response.dnd_hover_payload::<PathBuf>().and_then(|dragged| drop_target(&dragged, &entry.path, entry.folder));
            if let Some(dragged) = response.dnd_release_payload::<PathBuf>()
                && let Some(to) = drop_target(&dragged, &entry.path, entry.folder)
            {
                acts.push(Act::Rename { from: dragged.to_path_buf(), to });
            }
            // The folder a drag would drop into, as the file being edited is.
            let fill = if dropping.is_some() || is_active {
                Some(ui.visuals().selection.bg_fill)
            } else if selected || response.hovered() {
                Some(ui.visuals().widgets.hovered.weak_bg_fill)
            } else {
                None
            };
            // A file's colour (or a folder's, from a rule for it) tints its row, with a bar of it at the left: kept
            // apart from the dot of a file edited lately.
            let place = relative(root, &entry.path);
            let tint = if entry.folder { colours.folder_tint(&place) } else { colours.look(&place).tint };
            if let Some(tint) = tint.filter(|_| fill.is_none()) {
                ui.painter().rect_filled(rect, 2.0, tint.mark().gamma_multiply(0.16));
            }
            if let Some(fill) = fill {
                ui.painter().rect_filled(rect, 2.0, fill);
            }
            if let Some(tint) = tint {
                ui.painter().rect_filled(egui::Rect::from_min_size(rect.min + vec2(0.0, 2.0), vec2(3.0, rect.height() - 4.0)), 1.0, tint.mark());
            }
            let galley_width = galley.size().x + icon;
            if !entry.folder {
                let at = egui::Rect::from_min_size(rect.min + vec2(4.0 + depth as f32 * 12.0, 0.0), vec2(icon, rect.height()));
                crate::file_icons::paint(ui.painter(), at, &entry.name, 12.5);
            }
            let name_at = rect.min + vec2(4.0 + depth as f32 * 12.0 + icon, 2.0);
            // Without a bold font, bold is drawn twice, a little apart.
            if bold && crate::preview::bold_family().is_none() {
                ui.painter().galley(name_at + vec2(0.6, 0.0), galley.clone(), colour);
            }
            let name_width = galley.size().x;
            ui.painter().galley(name_at, galley, colour);
            let now = SystemTime::now();
            if let Some(when) = recent {
                ui.painter().circle_filled(egui::pos2(name_at.x + name_width + 7.0, rect.center().y), 3.0, kept::Heat::of(when, now).colour());
            }
            let galley_width = galley_width + dot_room;
            // Git's letter at the right, or a dot on a folder with changes in it.
            let right = ui.clip_rect().max.x.min(rect.max.x) - 8.0;
            // Each file's size and date before it, faint, where the name leaves room.
            if self.order.details {
                // The date and the size each in a column of its own, lined up at their right.
                let faint = crate::theme::text_faint();
                let small = egui::FontId::proportional(11.0);
                let date = entry.changed.map(short_date).unwrap_or_default();
                let size = if entry.folder { String::new() } else { short_size(entry.size) };
                let date_end = right - 14.0;
                let date_width = ui.painter().layout_no_wrap(date.clone(), small.clone(), faint).size().x;
                let size_end = date_end - 48.0;
                let size_width = ui.painter().layout_no_wrap(size.clone(), small.clone(), faint).size().x;
                let name_end = rect.min.x + 4.0 + depth as f32 * 12.0 + galley_width + 8.0;
                let y = rect.center().y;
                // The date alone where there's only room for it.
                if date_end - date_width > name_end {
                    ui.painter().text(egui::pos2(date_end, y), egui::Align2::RIGHT_CENTER, date, small.clone(), faint);
                }
                if size_end - size_width > name_end {
                    ui.painter().text(egui::pos2(size_end, y), egui::Align2::RIGHT_CENTER, size, small, faint);
                }
            }
            if let Some(change) = change {
                let (letter, tint) = git_mark(change);
                ui.painter().text(egui::pos2(right, rect.center().y), egui::Align2::RIGHT_CENTER, letter, egui::FontId::proportional(11.5), tint);
            } else if entry.folder && self.git_folders.contains(&entry.path) {
                ui.painter().circle_filled(egui::pos2(right - 3.0, rect.center().y), 3.0, crate::theme::git_changed());
            }
            let hover = match recent {
                Some(when) if entry.folder => format!("{}\n{}", entry.path.display(), crate::i18n::tf!("Holds files edited lately. {}", kept::when_said(when, now))),
                Some(when) => format!("{}\n{}", entry.path.display(), kept::when_said(when, now)),
                None if entry.hidden => format!("{}\n{}", entry.path.display(), crate::i18n::tr("Hidden in this project")),
                None => entry.path.display().to_string(),
            };
            let response = response.on_hover_text(hover);
            if self.scroll_to.as_deref() == Some(entry.path.as_path()) {
                response.scroll_to_me(Some(egui::Align::Center));
                self.scroll_to = None;
            }
            if response.clicked() {
                self.selected = Some(entry.path.clone());
                if entry.folder {
                    if open {
                        self.expanded.remove(&entry.path);
                    } else {
                        self.expanded.insert(entry.path.clone());
                    }
                } else {
                    acts.push(Act::Open(entry.path.clone()));
                }
            }
            crate::fit::context_menu(&response, |ui| self.menu(ui, root, &entry, colours, acts));
            if open {
                self.folder_rows(ui, root, &entry.path, depth + 1, active, colours, acts);
            }
        }
    }

    /// The offer to hide a folder's build and dependency folders, made once when it's opened: a bar at the top of
    /// the Explorer, out of the way, that goes when it's answered.
    fn offer_bar(&mut self, ui: &mut Ui, root: &Path, acts: &mut Vec<Act>) {
        let Some((_, names)) = self.offer.as_ref().filter(|(at, _)| at == root) else { return };
        let names = names.clone();
        let mut answered = None;
        egui::Frame::new().fill(ui.visuals().faint_bg_color).corner_radius(3.0).inner_margin(egui::Margin::symmetric(6, 4)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let words = crate::i18n::tf!("Hide the build and dependency folders ({})?", names.join(", "));
            ui.add(egui::Label::new(RichText::new(words).size(12.0)).wrap());
            ui.horizontal(|ui| {
                if ui.small_button(crate::i18n::tr("Hide them")).on_hover_text(crate::i18n::tr("From the Explorer, Go to file and searching; taken back in the project's settings")).clicked() {
                    answered = Some(names.clone());
                }
                if ui.small_button(crate::i18n::tr("Not now")).clicked() {
                    answered = Some(Vec::new());
                }
            });
        });
        if let Some(hide) = answered {
            self.offer = None;
            acts.push(Act::HideBuildFolders(hide));
        }
    }

    fn menu(&mut self, ui: &mut Ui, root: &Path, entry: &Entry, colours: &Colours, acts: &mut Vec<Act>) {
        let path = entry.path.clone();
        let folder = if entry.folder { path.clone() } else { path.parent().map_or_else(|| root.to_path_buf(), Path::to_path_buf) };
        let mut item = |ui: &mut Ui, label: &str, act: Act| {
            if ui.button(crate::i18n::tr(label)).clicked() {
                acts.push(act);
                ui.close();
            }
        };
        if !entry.folder {
            item(ui, "Open", Act::Open(path.clone()));
        }
        if ui.button(crate::i18n::tr("New file\u{2026}")).clicked() {
            self.start_new(folder.clone(), false);
            ui.close();
        }
        item(ui, "New from template\u{2026}", Act::NewFromTemplate(folder.clone()));
        if ui.button(crate::i18n::tr("New folder\u{2026}")).clicked() {
            self.start_new(folder.clone(), true);
            ui.close();
        }
        ui.separator();
        item(ui, "Find in this folder\u{2026}", Act::FindIn(folder.clone()));
        item(ui, "Open a terminal here", Act::Terminal(folder.clone()));
        item(ui, "Show in the file manager", Act::Reveal(path.clone()));
        ui.separator();
        item(ui, "Copy path", Act::CopyPath(path.clone()));
        item(ui, "Copy relative path", Act::CopyRelativePath(path.clone()));
        ui.separator();
        if entry.folder {
            item(ui, "Compare folders\u{2026}", Act::CompareFolder(path.clone()));
        } else {
            item(ui, "Compare with\u{2026}", Act::CompareWith(path.clone()));
        }
        item(ui, "Compare with the last commit", Act::CompareWithHead(path.clone()));
        ui.separator();
        // Hidden from the Explorer, Go to file and searching, kept with the project.
        if entry.hidden {
            item(ui, "Show this again", Act::ShowAgain { path: path.clone(), folder: entry.folder });
        } else if entry.folder {
            item(ui, "Hide this folder", Act::Hide(Hide::Folder(relative(root, &path))));
        } else if let Some(end) = kept::ending(&entry.name) {
            // Already in the words wanted, which are their own translation.
            item(ui, &crate::i18n::tf!("Hide .{end} files"), Act::Hide(Hide::Kind(end)));
        }
        // Coloured, kept with the project: a file alone, or every file in a folder by a rule for it.
        let place = relative(root, &path);
        let mut coloured = None;
        if entry.folder {
            crate::fit::menu_button(ui, crate::i18n::tr("Colour this folder's files"), |ui| {
                if let Some(tint) = crate::file_colours::tint_menu(ui, colours.folder_rule(&place)) {
                    coloured = Some(Act::ColourFolder { folder: path.clone(), tint });
                }
            });
        } else {
            crate::fit::menu_button(ui, crate::i18n::tr("Colour this file"), |ui| {
                if let Some(tint) = crate::file_colours::tint_menu(ui, colours.files.get(&place).and_then(|look| look.tint)) {
                    coloured = Some(Act::ColourFile { path: path.clone(), tint });
                }
            });
        }
        ui.separator();
        if ui.button(crate::i18n::tr("Rename\u{2026}")).clicked() {
            self.start_rename(&path);
            ui.close();
        }
        item(ui, "Move to the Recycle Bin", Act::Delete(path));
        acts.extend(coloured);
    }

    /// The box a name is typed in: Enter makes it, Escape leaves it.
    fn editing_row(&mut self, ui: &mut Ui, depth: usize, prefix: &str, acts: &mut Vec<Act>) {
        let Some(editing) = &mut self.editing else { return };
        let name = match editing {
            Editing::New { name, .. } | Editing::Rename { name, .. } => name,
        };
        let mut done = None;
        ui.horizontal(|ui| {
            ui.add_space(depth as f32 * 12.0);
            ui.label(RichText::new(prefix).size(12.5));
            let field = ui.add(egui::TextEdit::singleline(name).desired_width(ui.available_width().max(80.0)).hint_text(crate::i18n::tr("Name")));
            if std::mem::take(&mut self.focus_editing) {
                field.request_focus();
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = Some(false);
            } else if field.lost_focus() {
                done = Some(ui.input(|i| i.key_pressed(egui::Key::Enter)) && !name.trim().is_empty());
            }
        });
        if let Some(keep) = done {
            let editing = self.editing.take();
            if keep {
                match editing {
                    Some(Editing::New { folder, is_folder, name }) => acts.push(Act::Create { path: folder.join(name.trim()), folder: is_folder }),
                    Some(Editing::Rename { path, name }) => {
                        let to = path.with_file_name(name.trim());
                        if to != path {
                            acts.push(Act::Rename { from: path, to });
                        }
                    }
                    None => {}
                }
            }
        }
    }
}

/// A file's place in the folder, written with /.
pub fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

/// The files a few letters find, shown in place of the tree while the Explorer's filter is typed in.
pub fn filtered(ui: &mut Ui, root: &Path, files: &[PathBuf], filter: &str, active: Option<&Path>, colours: &Colours) -> Vec<Act> {
    let mut acts = Vec::new();
    let found = workspace::find(filter, files.iter().map(|p| (p.as_path(), relative(root, p))), 300);
    if found.is_empty() {
        ui.label(RichText::new(crate::i18n::tr("No file's name has those letters in that order.")).color(crate::theme::text_faint()).size(12.5));
        return acts;
    }
    egui::ScrollArea::both().id_salt("explorer-found").auto_shrink([false, false]).show(ui, |ui| {
        for (path, shown, places) in found {
            let mut job = egui::text::LayoutJob::default();
            for (i, c) in shown.chars().enumerate() {
                let format = egui::TextFormat { color: if places.contains(&i) { crate::theme::accent() } else { crate::theme::text() }, font_id: egui::FontId::proportional(12.5), ..Default::default() };
                job.append(&c.to_string(), 0.0, format);
            }
            let here = active == Some(path);
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::click());
            if here || response.hovered() {
                ui.painter().rect_filled(rect, 2.0, ui.visuals().selection.bg_fill.gamma_multiply(if here { 1.0 } else { 0.5 }));
            }
            let galley = ui.painter().layout_job(job);
            ui.painter().galley(rect.min + vec2(4.0, 1.0), galley, crate::theme::text());
            if let Some(tint) = colours.look(&relative(root, path)).tint {
                crate::file_colours::swatch(ui, egui::Rect::from_center_size(egui::pos2(rect.max.x - 9.0, rect.center().y), vec2(8.0, 8.0)), tint);
            }
            if response.clicked() {
                acts.push(Act::Open(path.to_path_buf()));
            }
        }
    });
    acts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lists_are_sorted_as_chosen() {
        let at = |seconds: u64| Some(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds));
        let entry = |name: &str, folder: bool, size: u64, changed: u64| Entry { name: name.into(), path: PathBuf::from(name), folder, ignored: false, hidden: false, size, changed: at(changed) };
        let all = vec![entry("b.c", false, 300, 2), entry("src", true, 0, 1), entry("A.h", false, 10, 3), entry("c.c", false, 20, 4)];
        let sorted = |order: Order| {
            let mut entries = all.clone();
            sort_entries(&mut entries, order);
            entries.iter().map(|e| e.name.clone()).collect::<Vec<_>>()
        };
        assert_eq!(sorted(Order::default()), ["src", "A.h", "b.c", "c.c"]);
        assert_eq!(sorted(Order { by: SortBy::Kind, ..Order::default() }), ["src", "b.c", "c.c", "A.h"]);
        assert_eq!(sorted(Order { by: SortBy::Size, ..Order::default() }), ["src", "b.c", "c.c", "A.h"]);
        assert_eq!(sorted(Order { by: SortBy::Date, folders_first: false, details: true }), ["c.c", "A.h", "b.c", "src"]);
        // Kept in the settings file.
        let order = Order { by: SortBy::Date, folders_first: false, details: true };
        let mut read = Order::default();
        for line in order.to_settings().lines() {
            let (name, value) = line.split_once('=').unwrap();
            read.read_setting(name, value);
        }
        assert_eq!(read, order);
        assert_eq!(short_size(812), "812 B");
        assert_eq!(short_size(25 * 1024 + 300), "25 KB");
    }

    #[test]
    fn a_dragged_file_goes_into_a_folder_or_beside_a_file() {
        let p = Path::new;
        assert_eq!(drop_target(p("/w/src/a.s"), p("/w/lib"), true), Some(PathBuf::from("/w/lib/a.s")));
        assert_eq!(drop_target(p("/w/src/a.s"), p("/w/lib/b.s"), false), Some(PathBuf::from("/w/lib/a.s")), "onto a file: into its folder");
        assert_eq!(drop_target(p("/w/src/a.s"), p("/w/src/b.s"), false), None, "where it is already");
        assert_eq!(drop_target(p("/w/src"), p("/w/src/inner"), true), None, "not into itself");
    }

    #[test]
    fn hidden_things_leave_the_tree_or_show_faintly() {
        let root = std::env::temp_dir().join(format!("limoniter-explorer-hidden-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for file in ["src/main.c", "src/main.o", "build/out.bin", "notes.txt"] {
            std::fs::create_dir_all(root.join(file).parent().unwrap()).unwrap();
            std::fs::write(root.join(file), "x").unwrap();
        }
        let mut explorer = Explorer { root: Some(root.clone()), ..Explorer::default() };
        let names = |explorer: &mut Explorer, folder: &Path| explorer.listing(folder).iter().map(|e| (e.name.clone(), e.hidden)).collect::<Vec<_>>();
        assert_eq!(names(&mut explorer, &root), [("build".into(), false), ("src".into(), false), ("notes.txt".into(), false)]);
        explorer.hidden = vec![Hide::Folder("build".into()), Hide::Kind("o".into())];
        explorer.refresh();
        assert_eq!(names(&mut explorer, &root), [("src".into(), false), ("notes.txt".into(), false)]);
        assert_eq!(names(&mut explorer, &root.join("src")), [("main.c".into(), false)]);
        // Shown faintly, to be found and taken back.
        explorer.show_hidden = true;
        explorer.refresh();
        assert_eq!(names(&mut explorer, &root), [("build".into(), true), ("src".into(), false), ("notes.txt".into(), false)]);
        assert_eq!(names(&mut explorer, &root.join("build")), [("out.bin".into(), true)], "what's in a hidden folder is hidden too");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn files_edited_lately_and_the_folders_holding_them_stand_out() {
        let root = PathBuf::from("/w");
        let now = SystemTime::now();
        let ago = |hours: u64| now - Duration::from_secs(hours * 3600);
        let mut explorer = Explorer::default();
        // From Git and the disk: one changed two days ago, one long ago.
        explorer.set_outside_times(&root, [(root.join("src/gfx/a.s"), ago(48)), (root.join("old.s"), ago(24 * 40))].into());
        let mut kept = Kept::default();
        // Saved here: one an hour ago that Git last changed long ago, one only saved here, and a hidden one.
        kept.note_saved(&root.join("old.s"), ago(1));
        kept.note_saved(&root.join("lib/b.s"), ago(24 * 3));
        kept.note_saved(&root.join("lib/b.o"), ago(1));
        kept.note_saved(Path::new("/elsewhere/c.s"), ago(1));
        kept.hide(Hide::Kind("o".into()));
        explorer.update_recent(&root, &kept);
        let mut files: Vec<_> = explorer.recent_files.keys().map(|p| relative(&root, p)).collect();
        files.sort();
        assert_eq!(files, ["lib/b.s", "old.s", "src/gfx/a.s"]);
        let secs = |when: SystemTime| when.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();
        assert_eq!(secs(explorer.recent_files[&root.join("old.s")]), secs(ago(1)), "Limoniter's save is later than Git's change");
        assert_eq!(explorer.recent_folders.get(&root.join("src")), Some(&ago(48)), "folded, the folder has the dot");
        assert!(explorer.recent_folders.contains_key(&root.join("src/gfx")) && !explorer.recent_folders.contains_key(&root));
        // Two days back, only today's and yesterday's; and none when it's off.
        kept.recent_days = 2;
        explorer.update_recent(&root, &kept);
        assert_eq!(explorer.recent_files.len(), 1);
        kept.recent_days = 0;
        explorer.update_recent(&root, &kept);
        assert!(explorer.recent_files.is_empty() && explorer.recent_folders.is_empty());
    }

    #[test]
    fn git_marks_files_and_the_folders_above_them() {
        let mut explorer = Explorer::default();
        let entry = |path: &str, unstaged| crate::git::Entry { path: path.into(), from: None, staged: None, unstaged };
        explorer.set_git(Some(Path::new("/w")), &[entry("src/gfx/a.s", Some(Change::Modified)), entry("new.txt", Some(Change::Untracked))]);
        assert_eq!(explorer.git.get(Path::new("/w/src/gfx/a.s")), Some(&Change::Modified));
        assert!(explorer.git_folders.contains(Path::new("/w/src")) && explorer.git_folders.contains(Path::new("/w/src/gfx")));
        assert_eq!(git_mark(Change::Untracked).0, "U");
        explorer.set_git(None, &[]);
        assert!(explorer.git.is_empty() && explorer.git_folders.is_empty());
    }

    #[test]
    fn the_slider_leaves_out_files_in_the_tree_and_keeps_their_order() {
        let root = std::env::temp_dir().join(format!("limoniter-explorer-slider-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for file in ["b.txt", "a.txt", "old/x.txt", "new/y.txt"] {
            std::fs::create_dir_all(root.join(file).parent().unwrap()).unwrap();
            std::fs::write(root.join(file), "x").unwrap();
        }
        let at = |secs: u64| SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
        let mut explorer = Explorer { root: Some(root.clone()), ..Explorer::default() };
        explorer.recency.set_every_time(&root, [(root.join("b.txt"), at(50)), (root.join("a.txt"), at(50)), (root.join("old/x.txt"), at(5)), (root.join("new/y.txt"), at(90))].into());
        let mut kept = Kept::default();
        explorer.recency.update_cut(&root, &kept);
        assert!(explorer.recency.cut.is_none(), "nothing is hidden at 100%");
        kept.recency = 50;
        explorer.recency.update_cut(&root, &kept);
        let shown = |explorer: &mut Explorer, folder: &Path| {
            let cut = explorer.recency.cut.clone();
            explorer.listing(folder).iter().filter(|e| !cut.as_ref().is_some_and(|c| c.hides(&e.path, e.folder))).map(|e| e.name.clone()).collect::<Vec<_>>()
        };
        // The newest two: new/y.txt, then the tie at 50 goes to a.txt. A to Z still, and the empty folder is gone.
        assert_eq!(shown(&mut explorer, &root), ["new", "a.txt"]);
        // A later save here brings a file up: b.txt, saved just now, beats a.txt.
        kept.note_saved(&root.join("b.txt"), SystemTime::now());
        explorer.recency.update_cut(&root, &kept);
        assert_eq!(shown(&mut explorer, &root), ["new", "b.txt"]);
        // Being dragged, the slider's share is the one used.
        explorer.recency.slider = Some(0);
        explorer.recency.update_cut(&root, &kept);
        assert!(shown(&mut explorer, &root).is_empty());
        assert_eq!(explorer.recency.cut.as_ref().unwrap().said(), "No files shown, 4 hidden");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A file made after the dates were read is ranked as the newest as soon as it's seen, and the counts under the
    /// slider follow, with no second read of every file.
    #[test]
    fn a_new_file_is_ranked_newest_at_once_and_counted() {
        let root = std::env::temp_dir().join(format!("limoniter-explorer-newfile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for file in ["a.txt", "b.txt", "c.txt", "d.txt"] {
            std::fs::write(root.join(file), "x").unwrap();
        }
        let at = |secs: u64| SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
        let mut explorer = Explorer { root: Some(root.clone()), ..Explorer::default() };
        explorer.recency.set_every_time(&root, [(root.join("a.txt"), at(40)), (root.join("b.txt"), at(30)), (root.join("c.txt"), at(20)), (root.join("d.txt"), at(10))].into());
        let kept = Kept { recency: 50, ..Kept::default() };
        explorer.recency.update_cut(&root, &kept);
        assert_eq!(explorer.recency.cut.as_ref().unwrap().said(), "2 files shown, 2 hidden");
        // Made now, after the read.
        std::fs::write(root.join("new.txt"), "x").unwrap();
        let entries = explorer.listing(&root);
        explorer.recency.rank_new(&root, entries.iter().filter(|e| !e.folder).map(|e| e.path.as_path()));
        explorer.recency.update_cut(&root, &kept);
        let cut = explorer.recency.cut.clone().unwrap();
        assert!(!cut.hides(&root.join("new.txt"), false), "the newest is shown");
        assert!(!cut.hides(&root.join("a.txt"), false), "a.txt is still the second newest of five (half of five is two)");
        assert!(cut.hides(&root.join("b.txt"), false));
        assert_eq!(cut.said(), "2 files shown, 3 hidden", "the counts follow");
        // Seen again, it isn't ranked a second time.
        let count = explorer.recency.every_count;
        explorer.recency.rank_new(&root, entries.iter().map(|e| e.path.as_path()));
        assert_eq!(explorer.recency.every_count, count);
        let _ = std::fs::remove_dir_all(&root);
    }
}
