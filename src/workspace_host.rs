//! The window's side of a folder opened as a project: opening one, keeping its index of files up to date in the
//! background, what the Explorer asks for (new files and folders, renaming, deleting, paths, a terminal there,
//! searching there), and Go to file.

use super::*;
use std::sync::mpsc::{Receiver, channel};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// How often a folder project's index is made again, as files change outside Limoniter.
const INDEX_AGAIN: Duration = Duration::from_secs(30);
/// How many files Go to file lists.
const MOST_SHOWN: usize = 60;

/// The index being made, and when the last one was.
#[derive(Default)]
pub(super) struct Indexing {
    job: Option<Receiver<(Vec<PathBuf>, bool)>>,
    done: Option<Instant>,
    /// When the files were last changed other than by Limoniter, for the Explorer's recently edited files.
    recent: Option<Receiver<(PathBuf, HashMap<PathBuf, std::time::SystemTime>)>>,
    /// The same for every file, for the Explorer's recency slider.
    every: Option<Receiver<(PathBuf, HashMap<PathBuf, std::time::SystemTime>)>>,
    /// Whether the last index stopped at its most files.
    pub more: bool,
}

/// Go to file: the letters typed, and the one chosen. Typed after @, they find the file's own symbols; after #,
/// the folder's.
#[derive(Default)]
pub(super) struct GoToFile {
    pub query: String,
    chosen: usize,
    focus: bool,
    /// The symbols of the file in front, worked out once.
    file_symbols: Option<Vec<(usize, String)>>,
    /// The chosen file's first lines, shown below the list as the choice moves; read once for each file.
    preview: Option<(PathBuf, Preview)>,
}

/// What Go to file shows of the file chosen.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Preview {
    Lines(Vec<String>),
    /// Not text, or not readable: why.
    Not(String),
}

/// The most of a file read for its preview, and the most lines kept.
const PREVIEW_BYTES: usize = 256 * 1024;
const PREVIEW_LINES: usize = 4000;
/// How many lines the preview shows.
const PREVIEW_ROWS: usize = 12;

/// Reads the start of a file for Go to file's preview.
pub(super) fn preview_of(path: &Path) -> Preview {
    use std::io::Read;
    let mut bytes = Vec::new();
    let read = std::fs::File::open(path).and_then(|file| file.take(PREVIEW_BYTES as u64).read_to_end(&mut bytes));
    if let Err(e) = read {
        return Preview::Not(e.to_string());
    }
    if crate::codec::looks_binary(&bytes) {
        return Preview::Not(crate::i18n::tr("Not text: Enter opens it to see it.").to_string());
    }
    let text = String::from_utf8_lossy(&bytes);
    Preview::Lines(text.lines().take(PREVIEW_LINES).map(|l| l.replace('\t', "    ").chars().take(200).collect()).collect())
}

/// Each file's functions and labels, as its language's grammar finds them, and when the file was last changed.
type SymbolCache = HashMap<PathBuf, (Option<std::time::SystemTime>, Vec<(usize, String)>)>;

/// The folder's symbols: those found, and the job finding them again.
#[derive(Default)]
pub(super) struct Symbols {
    pub found: Arc<SymbolCache>,
    job: Option<Receiver<SymbolCache>>,
}

/// The largest file whose symbols are looked for.
const MOST_SYMBOL_BYTES: u64 = 2 * 1024 * 1024;

/// A file's symbols, read from the disk: its grammar found from its name and first lines.
fn symbols_on_disk(path: &Path) -> Vec<(usize, String)> {
    if std::fs::metadata(path).map_or(true, |m| m.len() > MOST_SYMBOL_BYTES) {
        return Vec::new();
    }
    let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
    // Text only: a zero byte in the first part is a binary file.
    if bytes.iter().take(8000).any(|&b| b == 0) {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<Arc<str>> = text.lines().map(Arc::from).collect();
    let Some(engine) = crate::highlight::engine() else { return Vec::new() };
    let Some(syntax) = engine.detect_with_sample(Some(path), &lines[..lines.len().min(50)]) else { return Vec::new() };
    tidy(engine.list_functions(syntax, &lines, &std::sync::atomic::AtomicBool::new(false)))
}

/// Symbols as they're named: an assembler label without its colon.
pub(super) fn tidy(found: Vec<(usize, String)>) -> Vec<(usize, String)> {
    found.into_iter().map(|(line, name)| (line, name.trim().trim_end_matches(':').trim().to_string())).filter(|(_, name)| !name.is_empty()).collect()
}

/// A name for a folder's project file, the same each time the folder's opened: its name and a hash of its path
/// (FNV-1a, which doesn't change from one version of Rust to another).
fn folder_key(root: &Path) -> String {
    let text = root.to_string_lossy().replace('\\', "/");
    let text = if cfg!(windows) { text.to_lowercase() } else { text };
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    let name: String = root.file_name().map_or_else(|| "folder".to_string(), |n| n.to_string_lossy().into_owned()).chars().map(|c| if c.is_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect();
    format!("{name}-{:08x}", hash as u32)
}

/// What's hidden in a folder project's Explorer, in its settings, each with a button to show it again.
pub(super) fn hidden_list(ui: &mut Ui, hidden: &mut Vec<crate::explorer_kept::Hide>) {
    ui.add_space(4.0);
    ui.label(crate::i18n::tr("Hidden in the Explorer, Go to file and searching:"));
    if hidden.is_empty() {
        ui.label(RichText::new(crate::i18n::tr("Nothing. Right-click a folder or a file in the Explorer to hide it, or its kind of file.")).color(theme::text_faint()).size(12.5));
        return;
    }
    let mut back = None;
    egui::ScrollArea::vertical().id_salt("project-hidden").max_height(140.0).show(ui, |ui| {
        for (at, rule) in hidden.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.small_button(crate::i18n::tr("Show again")).clicked() {
                    back = Some(at);
                }
                ui.label(rule.describe());
            });
        }
    });
    if let Some(at) = back {
        hidden.remove(at);
    }
}

impl Limoniter {
    pub(super) fn choose_folder_to_open(&mut self) {
        if let Some(folder) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Open a folder")).pick_folder() {
            self.open_folder(&folder);
        }
    }

    /// Opens a folder as a project. A project file of the folder's own (NAME.limproj at its top) is used if
    /// there is one; otherwise one is kept beside Limoniter's settings, so nothing is written into the folder.
    pub(super) fn open_folder(&mut self, root: &Path) {
        let root = std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf());
        let name = root.file_name().map_or_else(|| "folder".to_string(), |n| n.to_string_lossy().into_owned());
        let own = root.join(format!("{name}.{}", project::EXTENSION));
        if own.is_file() {
            return self.open_project(&own);
        }
        let Some(kept) = settings_folder().map(|f| f.join("projects").join(format!("{}.{}", folder_key(&root), project::EXTENSION))) else {
            return self.message("Open folder", crate::i18n::tr("There's nowhere to keep the folder's project: Limoniter's settings folder couldn't be found.").into());
        };
        if kept.is_file() {
            return self.open_project(&kept);
        }
        let _ = std::fs::create_dir_all(kept.parent().unwrap_or(&root));
        self.close_project();
        self.note_recent_project(&kept);
        self.project = Some(Project::of_folder(&kept, &root));
        self.panels.project = true;
        self.save_project();
        self.offer_to_hide_build_folders();
        self.index_project();
    }

    /// When a folder is opened, its build and dependency folders are offered for hiding, in a bar in the
    /// Explorer: once for each folder, whatever the answer.
    pub(super) fn offer_to_hide_build_folders(&mut self) {
        let Some(project) = self.project.as_ref().filter(|p| p.root.is_some() && !p.explorer.offered) else { return };
        let root = project.folder();
        let names = crate::explorer_kept::build_folders(&root, &project.explorer.hidden);
        self.panels.project_panel.explorer.offer_to_hide(&root, names);
    }

    /// Keeps when Limoniter saved a file of a folder project, so the Explorer can show it was edited lately.
    pub(super) fn note_saved_in_project(&mut self, path: &Path) {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let Some(project) = self.project.as_mut().filter(|p| p.root.as_deref().is_some_and(|root| path.starts_with(root))) else { return };
        project.explorer.note_saved(&path, std::time::SystemTime::now());
        self.save_project();
    }

    /// Opens the project's settings: its name and notes, and for a folder, what's hidden in its Explorer.
    pub(super) fn open_project_settings(&mut self) {
        if let Some(project) = &self.project {
            self.dialog = Dialog::ProjectSettings { name: project.name.clone(), notes: project.notes.clone(), hidden: project.root.is_some().then(|| project.explorer.hidden.clone()), colours: project.colours.rules.clone() };
        }
    }

    /// Something about the Explorer kept with the project changed there: hiding, showing again, or how lately.
    fn explorer_setting(&mut self, act: crate::explorer::Act) {
        use crate::explorer::Act;
        let Some(project) = self.project.as_mut() else { return };
        let root = project.folder();
        let kept = &mut project.explorer;
        let mut index = true;
        match act {
            Act::Hide(rule) => kept.hide(rule),
            Act::ShowAgain { path, folder } => {
                kept.show_again(&root, &path, folder);
            }
            Act::ShowHidden(show) => {
                kept.show_hidden = show;
                index = false;
            }
            Act::RecentDays(days) => {
                kept.recent_days = days;
                index = false;
            }
            Act::Recency(share) => {
                kept.recency = share.min(100);
                index = false;
            }
            Act::HideBuildFolders(names) => {
                kept.offered = true;
                for name in names {
                    kept.hide(crate::explorer_kept::Hide::Named(name));
                }
            }
            _ => return,
        }
        self.save_project();
        // Go to file and searching leave out what's hidden as soon as it is.
        if index {
            self.index_project();
        } else {
            self.find_recent();
        }
    }

    /// Starts making the folder project's index of files, in the background.
    pub(super) fn index_project(&mut self) {
        let Some((root, hidden)) = self.project.as_ref().and_then(|p| Some((p.root.clone()?, p.explorer.hidden.clone()))) else { return };
        let (send, receive) = channel();
        std::thread::spawn(move || {
            let _ = send.send(crate::workspace::index(&root, &hidden));
        });
        self.indexing.job = Some(receive);
    }

    /// Starts working out, in the background, when the folder's files were last changed other than by Limoniter:
    /// one look at Git's history, and the disk's dates of the files Git doesn't vouch for.
    fn find_recent(&mut self) {
        self.find_every_time();
        let Some(project) = self.project.as_ref().filter(|p| p.root.is_some() && p.explorer.recent_days > 0) else { return };
        let (root, files, days) = (project.folder(), project.files.clone(), project.explorer.recent_days);
        let (send, receive) = channel();
        std::thread::spawn(move || {
            let times = crate::explorer_kept::outside_times(&root, &files, days);
            let _ = send.send((root, times));
        });
        self.indexing.recent = Some(receive);
    }

    /// Starts reading, in the background, when each of the folder's files was last edited, for the recency slider.
    /// It's only done while the slider is below 100%, as it reads the date of every file.
    fn find_every_time(&mut self) {
        let Some(project) = self.project.as_ref().filter(|p| p.explorer.recency < 100) else { return };
        if self.indexing.every.is_some() {
            return;
        }
        let (root, files) = (project.folder(), project.files.clone());
        let (send, receive) = channel();
        std::thread::spawn(move || {
            let times = crate::explorer_kept::every_time(&root, &files);
            let _ = send.send((root, times));
        });
        self.indexing.every = Some(receive);
    }

    /// Starts finding the folder's symbols again, in the background: only files changed since are read.
    fn find_symbols(&mut self) {
        let Some(files) = self.project.as_ref().filter(|p| p.root.is_some()).map(|p| p.files.clone()) else { return };
        let before = self.symbols.found.clone();
        let (send, receive) = channel();
        std::thread::spawn(move || {
            crate::highlight::load_now();
            let mut found = SymbolCache::new();
            for path in files {
                let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
                let symbols = match before.get(&path) {
                    Some((when, symbols)) if *when == modified => symbols.clone(),
                    _ => symbols_on_disk(&path),
                };
                if !symbols.is_empty() {
                    found.insert(path, (modified, symbols));
                }
            }
            let _ = send.send(found);
        });
        self.symbols.job = Some(receive);
    }

    /// Takes in an index that's ready, and makes it again now and then, as files change outside.
    pub(super) fn project_index_tick(&mut self, ctx: &Context) {
        // A `.limproj`'s files aren't indexed, so its dates are read once the slider is below 100%.
        if let Some(project) = self.project.as_ref().filter(|p| p.root.is_none() && p.explorer.recency < 100)
            && self.indexing.every.is_none()
            && !self.panels.project_panel.explorer.recency.has_times(&project.folder())
        {
            self.find_every_time();
        }
        if let Some(job) = &self.symbols.job
            && let Ok(found) = job.try_recv()
        {
            self.symbols = Symbols { found: Arc::new(found), job: None };
        }
        if let Some(job) = &self.indexing.recent
            && let Ok((root, times)) = job.try_recv()
        {
            self.indexing.recent = None;
            self.panels.project_panel.explorer.set_outside_times(&root, times);
        }
        if let Some(job) = &self.indexing.every
            && let Ok((root, times)) = job.try_recv()
        {
            self.indexing.every = None;
            self.panels.project_panel.explorer.set_every_time(&root, times);
        }
        if let Some(job) = &self.indexing.job {
            if let Ok((files, more)) = job.try_recv() {
                if let Some(project) = self.project.as_mut().filter(|p| p.root.is_some()) {
                    project.files = files;
                }
                self.indexing = Indexing { job: None, done: Some(Instant::now()), more, recent: self.indexing.recent.take(), every: self.indexing.every.take() };
                if self.symbols.job.is_none() {
                    self.find_symbols();
                }
                if self.indexing.recent.is_none() {
                    self.find_recent();
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(200));
            }
            return;
        }
        if self.project.as_ref().is_some_and(|p| p.root.is_some()) && self.indexing.done.is_none_or(|d| d.elapsed() > INDEX_AGAIN) {
            self.index_project();
        }
    }

    /// What the Explorer asked for.
    pub(super) fn explorer_act(&mut self, act: crate::explorer::Act, ui: &Ui) {
        use crate::explorer::Act;
        let root = self.project.as_ref().map(Project::folder).unwrap_or_default();
        // In a folder on a server, the same is done there afterwards.
        let done = act.clone();
        let changed = match act {
            Act::Open(path) => {
                self.open_path(&path);
                false
            }
            Act::NewFromTemplate(folder) => {
                self.open_template_box(Some(folder), 0);
                false
            }
            Act::Create { path, folder } => {
                if path.exists() {
                    self.message("New", crate::i18n::tf!("There's already something called {} there.", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                    return;
                }
                // A folder with a template chosen for it makes the file from that.
                if !folder && self.made_from_folder_template(&path) {
                    self.panels.project_panel.explorer.refresh();
                    return;
                }
                let made = if folder {
                    std::fs::create_dir_all(&path)
                } else {
                    path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::File::create_new(&path).map(|_| ()))
                };
                match made {
                    Ok(()) if !folder => {
                        self.open_path(&path);
                    }
                    Ok(()) => {}
                    Err(e) => self.message("New", crate::i18n::tf!("Couldn't make {}.\n\n{e}", path.display())),
                }
                true
            }
            Act::Rename { from, to } => {
                if to.exists() {
                    self.message("Rename", crate::i18n::tf!("There's already something called {} there.", to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
                    return;
                }
                match std::fs::rename(&from, &to) {
                    Ok(()) => {
                        self.colours_renamed(&from, &to);
                        // Open files go with it: the file itself, or everything in a folder renamed.
                        for doc in &mut self.docs {
                            if let Some(path) = &doc.path
                                && let Ok(rest) = path.strip_prefix(&from)
                            {
                                doc.path = Some(if rest.as_os_str().is_empty() { to.clone() } else { to.join(rest) });
                            }
                        }
                    }
                    Err(e) => self.message("Rename", crate::i18n::tf!("Couldn't rename {}.\n\n{e}", from.display())),
                }
                true
            }
            Act::Delete(path) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let question = self.remote_project_delete_question(&path).unwrap_or_else(|| if path.is_dir() { crate::i18n::tf!("Move the folder {name}, and everything in it, to the Recycle Bin?") } else { crate::i18n::tf!("Move {name} to the Recycle Bin?") });
                if !ask_yes_no(crate::i18n::tr("Move to the Recycle Bin"), &question) {
                    return;
                }
                if let Err(problem) = crate::trash::move_to_bin(&path) {
                    self.message("Move to the Recycle Bin", problem);
                }
                true
            }
            Act::CopyPath(path) => {
                ui.ctx().copy_text(path.display().to_string());
                false
            }
            Act::CopyRelativePath(path) => {
                ui.ctx().copy_text(crate::explorer::relative(&root, &path));
                false
            }
            Act::Reveal(path) => {
                if let Err(problem) = crate::platform::reveal(&path) {
                    self.message("Show in the file manager", problem);
                }
                false
            }
            Act::Terminal(folder) => {
                self.new_terminal(Some(folder));
                false
            }
            Act::FindIn(folder) => {
                self.open_find(FindTab::InFiles);
                self.find.scope = FileScope::Folder;
                self.find.folder = folder.display().to_string();
                self.find.recursive = true;
                false
            }
            Act::CompareWith(_) | Act::CompareFolder(_) | Act::CompareWithHead(_) => false,
            Act::WhatsHidden => {
                self.open_project_settings();
                false
            }
            Act::ColourFile { path, tint } => {
                self.set_file_tint(&path, tint);
                false
            }
            Act::ColourFolder { folder, tint } => {
                self.colour_folder(&folder, tint);
                false
            }
            Act::Hide(_) | Act::ShowAgain { .. } | Act::ShowHidden(_) | Act::RecentDays(_) | Act::Recency(_) | Act::HideBuildFolders(_) => {
                self.explorer_setting(act);
                false
            }
        };
        if changed {
            self.remote_project_explorer(&done);
            self.panels.project_panel.explorer.refresh();
            self.index_project();
        }
    }

    pub(super) fn open_go_to_file(&mut self) {
        self.go_to_file = Some(GoToFile { focus: true, ..GoToFile::default() });
    }

    /// Go to file, finding the symbols of the file in front (Ctrl+Shift+O) or of the folder (Ctrl+T).
    pub(super) fn open_go_to_symbol(&mut self, in_project: bool) {
        self.go_to_file = Some(GoToFile { query: if in_project { "#" } else { "@" }.into(), focus: true, ..GoToFile::default() });
    }

    /// The functions and labels of the file in front, as its grammar finds them.
    fn symbols_in_front(&mut self) -> Vec<(usize, String)> {
        let doc = &mut self.docs[self.active];
        if doc.hex_mode || doc.text.is_disk_backed() {
            return Vec::new();
        }
        let Some(syntax) = doc.syntax_index() else { return Vec::new() };
        let lines: Vec<Arc<str>> = (0..doc.text.line_count()).map(|i| doc.text.line(i)).collect();
        tidy(crate::highlight::engine().map(|e| e.list_functions(syntax, &lines, &std::sync::atomic::AtomicBool::new(false))).unwrap_or_default())
    }

    /// The files Go to file looks among, as shown: a folder project's index, or else the open files and those
    /// opened lately.
    fn go_to_file_candidates(&self) -> (PathBuf, Vec<PathBuf>) {
        if let Some(project) = &self.project
            && !project.files.is_empty()
        {
            return (project.folder(), project.files.clone());
        }
        let mut files: Vec<PathBuf> = self.docs.iter().filter_map(|d| d.path.clone()).collect();
        for path in &self.panels.recent {
            if !files.contains(path) {
                files.push(path.clone());
            }
        }
        (PathBuf::new(), files)
    }

    /// Go to file (Ctrl+E): a few letters of a file's name, its folders' too, find it; Enter opens it.
    pub(super) fn go_to_file_window(&mut self, ctx: &Context) {
        let Some(mut state) = self.go_to_file.take() else { return };
        let (root, files) = self.go_to_file_candidates();
        let shown = |p: &Path| if root.as_os_str().is_empty() { p.display().to_string() } else { crate::explorer::relative(&root, p) };
        // Each find: the file, the line to go to (for a symbol), what's shown, and the letters matched in it.
        let found: Vec<(PathBuf, Option<usize>, String, Vec<usize>)> = if let Some(letters) = state.query.strip_prefix('@') {
            if state.file_symbols.is_none() {
                state.file_symbols = Some(self.symbols_in_front());
            }
            let here = self.docs[self.active].path.clone().unwrap_or_default();
            let symbols = state.file_symbols.clone().unwrap_or_default();
            let mut scored: Vec<(i64, usize, String, Vec<usize>)> = symbols.into_iter().filter_map(|(line, name)| crate::workspace::score(letters, &name).map(|(s, m)| (s, line, name, m))).collect();
            if letters.trim().is_empty() {
                scored.sort_by_key(|(_, line, ..)| *line);
            } else {
                scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            }
            scored.into_iter().take(MOST_SHOWN * 4).map(|(_, line, name, m)| (here.clone(), Some(line), name, m)).collect()
        } else if let Some(letters) = state.query.strip_prefix('#') {
            let mut scored: Vec<(i64, PathBuf, usize, String, Vec<usize>)> = Vec::new();
            if !letters.trim().is_empty() {
                for (path, (_, symbols)) in self.symbols.found.iter() {
                    for (line, name) in symbols {
                        if let Some((s, m)) = crate::workspace::score(letters, name) {
                            scored.push((s, path.clone(), *line, name.clone(), m));
                        }
                    }
                }
            }
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.3.cmp(&b.3)));
            scored.into_iter().take(MOST_SHOWN).map(|(_, path, line, name, m)| {
                let place = format!("    {}:{}", shown(&path), line + 1);
                (path, Some(line), format!("{name}{place}"), m)
            }).collect()
        } else {
            crate::workspace::find(&state.query, files.iter().map(|p| (p.as_path(), shown(p))), MOST_SHOWN).into_iter().map(|(p, s, m)| (p.to_path_buf(), None, s, m)).collect()
        };
        // Each file's colour, shown as a small square at the end of its row.
        let tints: Vec<Option<crate::file_colours::Tint>> = found.iter().map(|(path, line, ..)| if line.is_none() { self.file_look(path).tint } else { None }).collect();
        let (mut open, mut close, mut beside) = (None, false, false);
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                state.chosen = (state.chosen + 1).min(found.len().saturating_sub(1));
            }
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                state.chosen = state.chosen.saturating_sub(1);
            }
            if i.consume_key(egui::Modifiers::NONE, Key::Escape) {
                close = true;
            }
            if i.consume_key(egui::Modifiers::NONE, Key::Enter) {
                open = found.get(state.chosen).map(|(p, line, ..)| (p.clone(), *line));
            }
            // Ctrl+Enter: beside the file in front.
            if i.consume_key(egui::Modifiers::COMMAND, Key::Enter) {
                open = found.get(state.chosen).map(|(p, line, ..)| (p.clone(), *line));
                beside = true;
            }
        });
        crate::fit::Dialog::new(crate::i18n::tr("Go to file")).title_bar(false).resizable(false).anchor(egui::Align2::CENTER_TOP, [0.0, 40.0]).show(ctx, |ui| {
            ui.set_width(620.0);
            let field = ui.add(egui::TextEdit::singleline(&mut state.query).hint_text(crate::i18n::tr("A few letters of the file's name; @ for this file's functions and labels, # for the folder's")).desired_width(f32::INFINITY));
            if std::mem::take(&mut state.focus) {
                field.request_focus();
            }
            if field.changed() {
                state.chosen = 0;
            }
            if state.query.starts_with('#') && self.symbols.found.is_empty() {
                let words = if self.project.as_ref().is_some_and(|p| p.root.is_some()) { crate::i18n::tr("The folder's symbols are still being found.") } else { crate::i18n::tr("Open a folder (File, Open folder) to find symbols in all its files; @ finds those of the file in front.") };
                ui.label(RichText::new(words).color(theme::text_faint()).size(12.5));
            }
            if self.indexing.more {
                ui.label(RichText::new(crate::i18n::tf!("The folder has more than {} files: only that many are looked among.", crate::workspace::MOST_FILES)).color(theme::text_faint()).size(12.5));
            }
            if files.is_empty() {
                ui.label(RichText::new(crate::i18n::tr("Open a folder (File, Open folder) to find its files here; until then, the files open and those opened lately are.")).color(theme::text_faint()).size(12.5));
            }
            egui::ScrollArea::vertical().max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
                for (n, (path, line, text, places)) in found.iter().enumerate() {
                    let mut job = egui::text::LayoutJob::default();
                    for (i, c) in text.chars().enumerate() {
                        let colour = if places.contains(&i) { theme::accent() } else { theme::text() };
                        job.append(&c.to_string(), 0.0, egui::TextFormat { color: colour, font_id: egui::FontId::proportional(13.0), ..Default::default() });
                    }
                    let galley = ui.painter().layout_job(job);
                    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), galley.size().y + 6.0), egui::Sense::click());
                    // A file has its badge before it; a symbol has none.
                    let icon = if line.is_none() { crate::file_icons::width(13.0) + 4.0 } else { 0.0 };
                    if n == state.chosen {
                        ui.painter().rect_filled(rect, 3.0, ui.visuals().selection.bg_fill);
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, 3.0, ui.visuals().widgets.hovered.weak_bg_fill);
                    }
                    if line.is_none() {
                        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        crate::file_icons::paint(ui.painter(), egui::Rect::from_min_size(rect.min + egui::vec2(5.0, 0.0), egui::vec2(icon, rect.height())), &name, 13.0);
                    }
                    ui.painter().galley(rect.min + egui::vec2(6.0 + icon, 3.0), galley, theme::text());
                    if let Some(tint) = tints[n] {
                        let square = egui::Rect::from_center_size(egui::pos2(rect.max.x - 10.0, rect.center().y), egui::vec2(9.0, 9.0));
                        crate::file_colours::swatch(ui, square, tint);
                    }
                    if n == state.chosen {
                        response.scroll_to_me(None);
                    }
                    if response.clicked() {
                        open = Some((path.clone(), *line));
                    }
                }
            });
            // The file chosen, as the choice moves through the list: its first lines, or those around a symbol.
            if let Some((path, line, ..)) = found.get(state.chosen)
                && path.is_file()
            {
                if state.preview.as_ref().is_none_or(|(p, _)| p != path) {
                    state.preview = Some((path.clone(), preview_of(path)));
                }
                ui.separator();
                let mono = egui::FontId::monospace(12.0);
                match state.preview.as_ref().map(|(_, p)| p) {
                    Some(Preview::Lines(lines)) => {
                        let first = line.map_or(0, |l| l.saturating_sub(PREVIEW_ROWS / 3));
                        ui.spacing_mut().item_spacing.y = 0.0;
                        // A line each, each cut at the box's edge.
                        for (number, text) in lines.iter().enumerate().skip(first).take(PREVIEW_ROWS) {
                            let mut job = egui::text::LayoutJob::default();
                            job.append(&format!("{:>5}  ", number + 1), 0.0, egui::TextFormat::simple(mono.clone(), theme::text_faint()));
                            let mut format = egui::TextFormat::simple(mono.clone(), theme::text());
                            if *line == Some(number) {
                                format.background = ui.visuals().selection.bg_fill;
                            }
                            job.append(text, 0.0, format);
                            ui.add(egui::Label::new(job).wrap_mode(egui::TextWrapMode::Truncate));
                        }
                    }
                    Some(Preview::Not(why)) => {
                        ui.label(RichText::new(why).color(theme::text_faint()).size(12.5));
                    }
                    None => {}
                }
            }
        });
        if let Some((path, line)) = open {
            let front = (!self.no_file()).then(|| self.docs[self.active].id);
            match line {
                Some(line) => self.go_to_place(&path.display().to_string(), line + 1),
                None => {
                    self.open_path(&path);
                }
            }
            // Opened beside: on the right, with the one that was in front on the left.
            if beside
                && let Some(front) = front
                && self.docs[self.active].id != front
            {
                self.show_beside(front, false);
            }
            self.pending_focus = true;
            return;
        }
        if close {
            self.pending_focus = true;
            return;
        }
        self.go_to_file = Some(state);
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    /// Takes in the folder project's index as soon as it's made, after something's changed.
    fn index_now(app: &mut Limoniter, ctx: &Context) {
        app.index_project();
        for _ in 0..500 {
            app.project_index_tick(ctx);
            if app.indexing.job.is_none() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the index never came");
    }

    #[test]
    fn hidden_folders_and_kinds_of_file_leave_go_to_file_and_find_in_files_and_stay_with_the_project() {
        let (mut app, ctx) = app_with("");
        let root = std::env::temp_dir().join(format!("limoniter-folder-hidden-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for file in ["src/main.s", "src/main.o", "build/out.s", "web/node_modules/x/index.js", "notes.txt"] {
            std::fs::create_dir_all(root.join(file).parent().unwrap()).unwrap();
            std::fs::write(root.join(file), "needle\n").unwrap();
        }
        let root = std::path::absolute(&root).unwrap();
        app.open_folder(&root);
        // Its build and dependency folders are offered for hiding, once.
        assert_eq!(app.panels.project_panel.explorer.offering(&root), Some(["node_modules".to_string(), "build".to_string()].as_slice()));
        index_now(&mut app, &ctx);
        assert_eq!(app.project.as_ref().unwrap().files.len(), 5);
        let ui_ctx = ctx.clone();
        let act = |app: &mut Limoniter, act: crate::explorer::Act| {
            let _ = ui_ctx.run_ui(Default::default(), |ui| app.explorer_act(act.clone(), ui));
        };
        act(&mut app, crate::explorer::Act::HideBuildFolders(vec!["node_modules".into()]));
        act(&mut app, crate::explorer::Act::Hide(crate::explorer_kept::Hide::Folder("build".into())));
        act(&mut app, crate::explorer::Act::Hide(crate::explorer_kept::Hide::Kind("o".into())));
        index_now(&mut app, &ctx);
        let names = |app: &Limoniter| app.project.as_ref().unwrap().files.iter().map(|f| crate::explorer::relative(&root, f)).collect::<Vec<_>>();
        assert_eq!(names(&app), ["notes.txt", "src/main.s"]);
        // Go to file looks only among those.
        let (_, candidates) = app.go_to_file_candidates();
        assert_eq!(candidates.len(), 2);
        // And so does Find in files, in the project's files.
        app.find.find = "needle".into();
        app.find.scope = FileScope::Project;
        app.start_find_in_files(false);
        let job = app.job.clone().unwrap();
        while !job.finished.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut found: Vec<String> = job.hits.lock().unwrap().iter().map(|hit| crate::explorer::relative(&root, &hit.path)).collect();
        found.sort();
        assert_eq!(found, ["notes.txt", "src/main.s"]);
        // Kept with the project: opened again, the same things are hidden, and the offer isn't made again.
        app.close_project();
        let kept = app.recent_projects[0].clone();
        assert!(std::fs::read_to_string(&kept).unwrap().contains("**/node_modules/"));
        app.panels.project_panel.explorer = crate::explorer::Explorer::default();
        app.open_folder(&root);
        assert_eq!(app.project.as_ref().unwrap().explorer.hidden.len(), 3);
        assert_eq!(app.panels.project_panel.explorer.offering(&root), None, "asked once for each folder");
        // Taken back from the Explorer, or in the project's settings, they're found again.
        act(&mut app, crate::explorer::Act::ShowAgain { path: root.join("build/out.s"), folder: false });
        assert_eq!(app.project.as_ref().unwrap().explorer.hidden.len(), 2, "what hid it, its folder, is taken back");
        app.open_project_settings();
        let Dialog::ProjectSettings { hidden: Some(hidden), .. } = &mut app.dialog else { panic!("a folder project's settings list what's hidden") };
        assert_eq!(hidden.iter().map(crate::explorer_kept::Hide::describe).collect::<Vec<_>>(), ["Every folder called node_modules", ".o files"]);
        hidden.retain(|rule| *rule != crate::explorer_kept::Hide::Kind("o".into()));
        // As OK does.
        if let (Some(project), Dialog::ProjectSettings { hidden: Some(hidden), .. }) = (&mut app.project, &app.dialog) {
            project.explorer.hidden.clone_from(hidden);
        }
        app.dialog = Dialog::None;
        index_now(&mut app, &ctx);
        assert_eq!(names(&app), ["build/out.s", "notes.txt", "src/main.o", "src/main.s"]);
        // Saving a file in the folder keeps when, with the project, for the Explorer's recently edited files.
        assert!(app.open_path(&root.join("notes.txt")));
        let index = app.active;
        assert!(app.write_to(index, &root.join("notes.txt")));
        let saved = app.project.as_ref().unwrap().explorer.saved_at(&root.join("notes.txt"));
        assert!(saved.is_some_and(|when| when.elapsed().unwrap_or_default() < Duration::from_secs(60)));
        let kept = app.project.as_ref().unwrap().path.clone();
        assert_eq!(Project::load(&kept).unwrap().explorer.saved_at(&root.join("notes.txt")), saved);
        app.close_project();
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(kept);
    }

    #[test]
    fn a_folder_opens_as_a_project_with_its_files_found_and_kept_in_step() {
        let (mut app, ctx) = app_with("");
        let root = std::env::temp_dir().join(format!("limoniter-folder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (path, text) in [("src/main.s", "start:\tmove.l\td0,d1\n\tlea\tdata,a0\n\tmoveq\t#1,d2\n\tjsr\tutil\n\trts\n"), ("src/lib/util.s", "util:\tmove.l\t(a0)+,d0\n\tmove.w\td0,$dff180\n\tlea\t$dff000,a6\n\tmoveq\t#0,d1\n\trts\n"), (".gitignore", "*.o\n"), ("src/main.o", ""), ("README.md", "# hi\n")] {
            let full = root.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, text).unwrap();
        }
        app.open_folder(&root);
        let project = app.project.as_ref().expect("a project");
        assert_eq!(project.root.as_deref(), Some(std::path::absolute(&root).unwrap().as_path()));
        assert!(!project.path.starts_with(&root), "nothing is written into the folder");
        // The index comes in from the background, without what .gitignore leaves out.
        for _ in 0..200 {
            app.project_index_tick(&ctx);
            if !app.project.as_ref().unwrap().files.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let names: Vec<String> = app.project.as_ref().unwrap().files.iter().map(|f| crate::explorer::relative(&root, f)).collect();
        assert_eq!(names, [".gitignore", "README.md", "src/lib/util.s", "src/main.s"]);
        // The folder's symbols are found too: Go to symbol (#) finds a label in another file, and goes to its line.
        for _ in 0..300 {
            app.project_index_tick(&ctx);
            if !app.symbols.found.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let util = std::path::absolute(root.join("src/lib/util.s")).unwrap();
        assert!(app.symbols.found.get(&util).is_some_and(|(_, s)| s.iter().any(|(_, n)| n == "util")), "{:?}", app.symbols.found);
        app.open_go_to_symbol(true);
        app.go_to_file.as_mut().unwrap().query = "#util".into();
        let _ = ctx.run_ui(Default::default(), |ui| {
            ui.input_mut(|i| i.events.push(egui::Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }));
            app.go_to_file_window(ui.ctx());
        });
        assert_eq!(app.docs[app.active].path.as_deref(), Some(util.as_path()), "Enter goes to the label's file");
        assert!(app.go_to_file.is_none());
        // From main.s, which doesn't INCLUDE it: peeked at and gone to by name.
        let main = std::path::absolute(root.join("src/main.s")).unwrap();
        assert!(app.open_path(&main));
        app.doc().text.move_caret(3, 8, false);
        app.peek_definition();
        assert!(app.peek.as_ref().is_some_and(|p| p.file == util && p.line == 0), "peeked: {:?} {:?}", app.peek.as_ref().map(|p| (&p.file, p.line)), match &app.dialog { Dialog::Message { body, .. } => body.clone(), _ => String::new() });
        app.peek = None;
        app.doc().text.move_caret(3, 8, false);
        app.go_to_definition();
        assert_eq!(app.docs[app.active].path.as_deref(), Some(util.as_path()), "F12 goes there");
        // A new file made in the Explorer is made, opened, and found.
        let ui_ctx = ctx.clone();
        let _ = ui_ctx.run_ui(Default::default(), |ui| app.explorer_act(crate::explorer::Act::Create { path: root.join("src/new.s"), folder: false }, ui));
        assert!(root.join("src/new.s").is_file());
        assert_eq!(app.docs[app.active].path.as_deref(), Some(root.join("src/new.s").as_path()));
        // Renamed, the open file goes with it.
        let _ = ui_ctx.run_ui(Default::default(), |ui| app.explorer_act(crate::explorer::Act::Rename { from: root.join("src"), to: root.join("code") }, ui));
        assert_eq!(app.docs[app.active].path.as_deref(), Some(root.join("code/new.s").as_path()));
        // Opened again, it's the same project, with its tabs.
        app.close_project();
        let kept = app.recent_projects[0].clone();
        app.open_folder(&root);
        assert_eq!(app.project.as_ref().unwrap().path, kept);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(kept);
    }
}
