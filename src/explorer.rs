//! The Explorer: a folder opened as a project, shown as VS Code shows one. Its tree is read from the disk a folder at
//! a time, as each is opened, and read again when it changes, so files made, renamed or deleted elsewhere show at
//! once. Folders come first; what Git leaves out is shown fainter; .git and its kind aren't shown. The file being
//! edited is found in the tree. New files and folders, and renaming, are typed in the tree itself.

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
}

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

impl Explorer {
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
                    let ignored = self.ignores.as_mut().is_some_and(|i| i.ignored(&path, folder));
                    let meta = std::fs::metadata(&path).ok();
                    let size = meta.as_ref().filter(|_| !folder).map_or(0, std::fs::Metadata::len);
                    let changed = meta.and_then(|m| m.modified().ok());
                    entries.push(Entry { name, path, folder, ignored, size, changed });
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

    pub fn draw(&mut self, ui: &mut Ui, root: &Path, active: Option<&Path>) -> Vec<Act> {
        let mut acts = Vec::new();
        if self.root.as_deref() != Some(root) {
            *self = Explorer { root: Some(root.to_path_buf()), ignores: Some(Ignores::new(root)), order: self.order, ..Explorer::default() };
            self.expanded.insert(root.to_path_buf());
        }
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
                })
                .response
                .on_hover_text(crate::i18n::tr("Sort by, and sizes and dates"));
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
        egui::ScrollArea::both().id_salt("explorer-tree").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            self.folder_rows(ui, root, root, 0, active, &mut acts);
        });
        acts
    }

    fn folder_rows(&mut self, ui: &mut Ui, root: &Path, folder: &Path, depth: usize, active: Option<&Path>, acts: &mut Vec<Act>) {
        // A new file or folder being named goes at the top of its folder.
        if let Some(Editing::New { folder: at, is_folder, .. }) = &self.editing
            && at == folder
        {
            let is_folder = *is_folder;
            self.editing_row(ui, depth, if is_folder { "\u{25B8} " } else { "" }, acts);
        }
        for entry in self.listing(folder) {
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
                _ if entry.ignored => crate::theme::text_faint(),
                Some(change) => git_mark(change).1,
                None => crate::theme::text(),
            };
            // A row the panel's width, as VS Code's are: the file being edited in the selection's colour.
            let font = egui::FontId::proportional(12.5);
            let galley = ui.painter().layout_no_wrap(format!("{chevron}{}", entry.name), font, colour);
            // A file's badge goes where a folder's arrow is.
            let icon = if entry.folder { 0.0 } else { crate::file_icons::width(12.5) + 3.0 };
            let width = (depth as f32 * 12.0 + icon + galley.size().x + 8.0).max(ui.available_width());
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
            if let Some(fill) = fill {
                ui.painter().rect_filled(rect, 2.0, fill);
            }
            let galley_width = galley.size().x + icon;
            if !entry.folder {
                let at = egui::Rect::from_min_size(rect.min + vec2(4.0 + depth as f32 * 12.0, 0.0), vec2(icon, rect.height()));
                crate::file_icons::paint(ui.painter(), at, &entry.name, 12.5);
            }
            ui.painter().galley(rect.min + vec2(4.0 + depth as f32 * 12.0 + icon, 2.0), galley, colour);
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
            let response = response.on_hover_text(entry.path.display().to_string());
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
            crate::fit::context_menu(&response, |ui| self.menu(ui, root, &entry, acts));
            if open {
                self.folder_rows(ui, root, &entry.path, depth + 1, active, acts);
            }
        }
    }

    fn menu(&mut self, ui: &mut Ui, root: &Path, entry: &Entry, acts: &mut Vec<Act>) {
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
        if ui.button(crate::i18n::tr("Rename\u{2026}")).clicked() {
            self.start_rename(&path);
            ui.close();
        }
        item(ui, "Move to the Recycle Bin", Act::Delete(path));
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
pub fn filtered(ui: &mut Ui, root: &Path, files: &[PathBuf], filter: &str, active: Option<&Path>) -> Vec<Act> {
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
        let entry = |name: &str, folder: bool, size: u64, changed: u64| Entry { name: name.into(), path: PathBuf::from(name), folder, ignored: false, size, changed: at(changed) };
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
}
