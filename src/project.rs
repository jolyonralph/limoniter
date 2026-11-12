//! Projects: a named set of files kept in a small file of its own, with backups. Paths are kept relative to that
//! file where they can be, so a project still works when its folder is moved or shared.

use crate::codec;
use crate::theme;
use eframe::egui::{self, RichText, Ui};
use serde_json::{Value, json};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const EXTENSION: &str = "limproj";
/// The most files Add folder will add in one go.
const MOST_FROM_FOLDER: usize = 20_000;
/// Folders that are full of things that aren't really the project's.
const SKIPPED_FOLDERS: [&str; 6] = [".git", ".svn", ".hg", "node_modules", "target", "__pycache__"];
const MANIFEST: &str = "manifest.json";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Project {
    pub path: PathBuf,
    pub name: String,
    pub notes: String,
    pub files: Vec<PathBuf>,
    /// The files that were open when the project was last closed.
    pub open_files: Vec<PathBuf>,
}

fn folder_of(path: &Path) -> PathBuf {
    path.parent().map_or_else(PathBuf::new, Path::to_path_buf)
}

/// A path as it's written in the project file: relative if it's on the same drive, and otherwise whole.
fn stored(base: &Path, path: &Path) -> String {
    relative_to(base, path).unwrap_or_else(|| path.to_path_buf()).to_string_lossy().replace('\\', "/")
}

fn relative_to(base: &Path, path: &Path) -> Option<PathBuf> {
    let (base, path): (Vec<Component>, Vec<Component>) = (base.components().collect(), path.components().collect());
    if base.first()? != path.first()? {
        return None;
    }
    let same = base.iter().zip(&path).take_while(|(a, b)| a == b).count();
    let mut out = PathBuf::new();
    base[same..].iter().for_each(|_| out.push(".."));
    path[same..].iter().for_each(|part| out.push(part));
    Some(out)
}

/// Back to a whole path, tidying away any `..`.
fn resolved(base: &Path, written: &str) -> PathBuf {
    let joined = base.join(written.replace('\\', "/"));
    let mut out = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn same_file(a: &Path, b: &Path) -> bool {
    if cfg!(windows) { a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy()) } else { a == b }
}

impl Project {
    pub fn new(path: &Path) -> Self {
        let name = path.file_stem().map_or_else(|| "Project".to_string(), |stem| stem.to_string_lossy().into_owned());
        Self { path: path.to_path_buf(), name, ..Self::default() }
    }

    pub fn folder(&self) -> PathBuf {
        folder_of(&self.path)
    }

    pub fn to_text(&self) -> String {
        let base = self.folder();
        let list = |paths: &[PathBuf]| paths.iter().map(|path| stored(&base, path)).collect::<Vec<_>>();
        let value = json!({
            "limoniter_project": 1,
            "name": self.name,
            "notes": self.notes,
            "files": list(&self.files),
            "open_files": list(&self.open_files),
        });
        serde_json::to_string_pretty(&value).unwrap_or_default() + "\n"
    }

    pub fn from_text(path: &Path, text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text).map_err(|e| format!("That isn't a project file.\n\n{e}"))?;
        if value.get("limoniter_project").is_none() {
            return Err("That isn't a project file.".to_string());
        }
        let base = folder_of(path);
        let list = |name: &str| -> Vec<PathBuf> { value.get(name).and_then(Value::as_array).map_or_else(Vec::new, |items| items.iter().filter_map(Value::as_str).map(|item| resolved(&base, item)).collect()) };
        let text_of = |name: &str| value.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
        let mut project = Self { path: path.to_path_buf(), name: text_of("name"), notes: text_of("notes"), files: list("files"), open_files: list("open_files") };
        if project.name.is_empty() {
            project.name = Self::new(path).name;
        }
        Ok(project)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        Self::from_text(path, &std::fs::read_to_string(path).map_err(|e| e.to_string())?)
    }

    pub fn save(&self) -> Result<(), String> {
        codec::write_file(&self.path, self.to_text().as_bytes()).map_err(|e| e.to_string())
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.files.iter().any(|file| same_file(file, path))
    }

    /// Adds files that aren't on the list yet, and says how many went on. The list stays in order.
    pub fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> usize {
        let mut added = 0;
        for path in paths {
            if !self.contains(&path) {
                self.files.push(path);
                added += 1;
            }
        }
        self.files.sort_by_key(|path| path.to_string_lossy().to_lowercase());
        added
    }

    pub fn remove(&mut self, path: &Path) {
        self.files.retain(|file| !same_file(file, path));
    }
}

/// Every file in a folder and those under it, leaving out the likes of `.git`. Links aren't followed.
pub fn files_in_folder(folder: &Path) -> (Vec<PathBuf>, bool) {
    let mut found = Vec::new();
    let mut waiting = vec![folder.to_path_buf()];
    while let Some(next) = waiting.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                if !SKIPPED_FOLDERS.iter().any(|skipped| entry.file_name().to_string_lossy().eq_ignore_ascii_case(skipped)) {
                    waiting.push(entry.path());
                }
            } else if kind.is_file() {
                if found.len() >= MOST_FROM_FOLDER {
                    return (found, true);
                }
                found.push(entry.path());
            }
        }
    }
    (found, false)
}

// ---------- Backup and restore ----------

/// How a backup or restore came out.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub done: usize,
    /// Files left as they were, and why.
    pub skipped: Vec<(PathBuf, String)>,
    pub folder: PathBuf,
}

pub type Outcome = Arc<Mutex<Option<Result<Report, String>>>>;

fn backups_root(project: &Project) -> PathBuf {
    project.folder().join(format!("{}-backups", project.path.file_stem().map_or_else(|| "project".to_string(), |stem| stem.to_string_lossy().into_owned())))
}

/// Copies the project's file and all its files into a new folder, and lists what came from where. Each is saved
/// as it is on disk, so unsaved edits in open files aren't in it.
pub fn backup(project: &Project, stamp: &str) -> Result<Report, String> {
    let root = backups_root(project);
    let mut folder = root.join(stamp);
    let mut attempt = 1;
    while folder.exists() {
        attempt += 1;
        folder = root.join(format!("{stamp}-{attempt}"));
    }
    std::fs::create_dir_all(&folder).map_err(|e| format!("Couldn't make {}.\n\n{e}", folder.display()))?;
    let mut report = Report { folder: folder.clone(), ..Report::default() };
    let mut listed = Vec::new();
    for (number, path) in std::iter::once(&project.path).chain(&project.files).enumerate() {
        let name = path.file_name().map_or_else(|| "file".to_string(), |name| name.to_string_lossy().into_owned());
        let copy = format!("{number:05}-{name}");
        match std::fs::copy(path, folder.join(&copy)) {
            Ok(_) => {
                listed.push(json!({ "copy": copy, "original": path.to_string_lossy() }));
                report.done += 1;
            }
            Err(problem) => report.skipped.push((path.clone(), problem.to_string())),
        }
    }
    let manifest = serde_json::to_string_pretty(&json!({ "project": project.name, "files": listed })).unwrap_or_default();
    // A backup without its list couldn't be put back, so it isn't one.
    codec::write_file(&folder.join(MANIFEST), manifest.as_bytes()).map_err(|e| format!("Couldn't finish the backup.\n\n{e}"))?;
    Ok(report)
}

/// One backup that can be put back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backup {
    pub folder: PathBuf,
    pub name: String,
    pub files: Vec<(String, PathBuf)>,
}

fn read_backup(folder: &Path) -> Option<Backup> {
    let text = std::fs::read_to_string(folder.join(MANIFEST)).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let files = value.get("files")?.as_array()?.iter().filter_map(|item| Some((item.get("copy")?.as_str()?.to_string(), PathBuf::from(item.get("original")?.as_str()?)))).collect();
    Some(Backup { folder: folder.to_path_buf(), name: folder.file_name()?.to_string_lossy().into_owned(), files })
}

/// This project's backups, newest first.
pub fn list_backups(project: &Project) -> Vec<Backup> {
    let Ok(entries) = std::fs::read_dir(backups_root(project)) else {
        return Vec::new();
    };
    let mut found: Vec<Backup> = entries.flatten().filter(|entry| entry.path().is_dir()).filter_map(|entry| read_backup(&entry.path())).collect();
    found.sort_by(|a, b| b.name.cmp(&a.name));
    found
}

/// Puts a backup's files back. Each one goes through the safe save, so one that fails leaves the file as it was.
pub fn restore(backup: &Backup, keep: &[PathBuf]) -> Report {
    let mut report = Report { folder: backup.folder.clone(), ..Report::default() };
    for (copy, original) in &backup.files {
        if keep.iter().any(|path| same_file(path, original)) {
            report.skipped.push((original.clone(), "it's open with unsaved changes".to_string()));
            continue;
        }
        // The name comes from a file that could have been edited, so it may only reach into this folder.
        if Path::new(copy).components().count() != 1 || matches!(Path::new(copy).components().next(), Some(Component::ParentDir | Component::RootDir | Component::Prefix(_))) {
            report.skipped.push((original.clone(), "its backup is listed oddly".to_string()));
            continue;
        }
        // Copied rather than read in, because a project can hold files of any size.
        let attempt = (|| -> std::io::Result<()> {
            if let Some(parent) = original.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut temp_name = original.as_os_str().to_owned();
            temp_name.push(".limoniter-tmp");
            let temp = PathBuf::from(temp_name);
            let source = backup.folder.join(copy);
            let copied = std::fs::copy(&source, &temp).and_then(|size| {
                if size == std::fs::metadata(&source)?.len() { std::fs::rename(&temp, original) } else { Err(std::io::Error::other("the copy came out the wrong size")) }
            });
            if copied.is_err() {
                let _ = std::fs::remove_file(&temp);
            }
            copied
        })();
        match attempt {
            Ok(()) => report.done += 1,
            Err(problem) => report.skipped.push((original.clone(), problem.to_string())),
        }
    }
    report
}

/// Runs some file copying on another thread, so the window stays free.
pub fn in_background(work: impl FnOnce() -> Result<Report, String> + Send + 'static) -> Outcome {
    let outcome: Outcome = Arc::new(Mutex::new(None));
    let report = outcome.clone();
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).unwrap_or_else(|_| Err("Copying the files failed unexpectedly.".to_string()));
        *report.lock().unwrap() = Some(result);
    });
    outcome
}

// ---------- The panel ----------

/// What the panel asks the window to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelAction {
    Open(PathBuf),
    Remove(PathBuf),
}

#[derive(Default)]
pub struct PanelState {
    pub filter: String,
}

/// A folder in the panel's tree.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct TreeFolder {
    pub folders: std::collections::BTreeMap<String, TreeFolder>,
    pub files: Vec<(String, PathBuf)>,
}

/// Arranges files by folder, relative to the project's own.
pub fn tree_of(project: &Project, filter: &str) -> TreeFolder {
    let base = project.folder();
    let filter = filter.trim().to_lowercase();
    let mut root = TreeFolder::default();
    for path in &project.files {
        let shown = relative_to(&base, path).unwrap_or_else(|| path.clone());
        let name = shown.file_name().map_or_else(|| shown.to_string_lossy().into_owned(), |name| name.to_string_lossy().into_owned());
        if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
            continue;
        }
        let mut folder = &mut root;
        if let Some(parent) = shown.parent() {
            for part in parent.components() {
                folder = folder.folders.entry(part.as_os_str().to_string_lossy().into_owned()).or_default();
            }
        }
        folder.files.push((name, path.clone()));
    }
    root
}

fn draw_tree(ui: &mut Ui, folder: &TreeFolder, salt: &str, active: Option<&Path>, actions: &mut Vec<PanelAction>, open_all: bool) {
    for (name, inner) in &folder.folders {
        let id = format!("{salt}/{name}");
        egui::CollapsingHeader::new(RichText::new(name).size(12.5)).id_salt(&id).default_open(true).open(open_all.then_some(true)).show(ui, |ui| draw_tree(ui, inner, &id, active, actions, open_all));
    }
    for (name, path) in &folder.files {
        let here = active.is_some_and(|active| same_file(active, path));
        let missing = !path.exists();
        let text = RichText::new(name).size(12.5).color(if missing { theme::text_disabled() } else { theme::text() });
        let button = egui::Button::selectable(here, text).frame_when_inactive(false).wrap_mode(egui::TextWrapMode::Truncate);
        let response = ui.add(button).on_hover_text(if missing { format!("{} (not found)", path.display()) } else { path.display().to_string() });
        if response.clicked() && !missing {
            actions.push(PanelAction::Open(path.clone()));
        }
        response.context_menu(|ui| {
            if ui.button("Take off the project").on_hover_text("The file itself stays").clicked() {
                actions.push(PanelAction::Remove(path.clone()));
                ui.close();
            }
        });
    }
}

pub fn panel(ui: &mut Ui, state: &mut PanelState, project: Option<&Project>, active: Option<&Path>) -> Vec<PanelAction> {
    let mut actions = Vec::new();
    let Some(project) = project else {
        ui.label(RichText::new("No project is open. Tools, Project can make one or open one.").color(theme::text_faint()).size(12.5));
        return actions;
    };
    ui.label(RichText::new(&project.name).color(theme::text()).size(13.0)).on_hover_text(if project.notes.is_empty() { project.path.display().to_string() } else { format!("{}\n\n{}", project.path.display(), project.notes) });
    ui.add(egui::TextEdit::singleline(&mut state.filter).hint_text("Filter").desired_width(f32::INFINITY));
    ui.add_space(2.0);
    if project.files.is_empty() {
        ui.label(RichText::new("No files yet. Add some from Tools, Project.").color(theme::text_faint()).size(12.5));
        return actions;
    }
    let tree = tree_of(project, &state.filter);
    egui::ScrollArea::both().id_salt("project-scroll").auto_shrink([false, false]).show(ui, |ui| draw_tree(ui, &tree, "project", active, &mut actions, !state.filter.trim().is_empty()));
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-project-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_project_survives_its_file_with_relative_paths() {
        let base = PathBuf::from(if cfg!(windows) { r"C:\work\site" } else { "/work/site" });
        let path = base.join("site.limproj");
        let mut project = Project::new(&path);
        assert_eq!(project.name, "site");
        project.notes = "Line one\nline two".into();
        project.add([base.join("index.html"), base.join("css").join("a.css"), base.parent().unwrap().join("shared.txt")]);
        project.open_files = vec![base.join("index.html")];
        let text = project.to_text();
        assert!(text.contains("\"index.html\"") && text.contains("\"css/a.css\"") && text.contains("\"../shared.txt\""), "{text}");
        assert_eq!(Project::from_text(&path, &text).unwrap(), project);
    }

    #[test]
    fn a_moved_project_finds_its_files_in_the_new_place() {
        let (old, new) = (PathBuf::from(if cfg!(windows) { r"C:\a\p.limproj" } else { "/a/p.limproj" }), PathBuf::from(if cfg!(windows) { r"D:\b\p.limproj" } else { "/b/p.limproj" }));
        let mut project = Project::new(&old);
        project.add([old.parent().unwrap().join("x.txt")]);
        let moved = Project::from_text(&new, &project.to_text()).unwrap();
        assert_eq!(moved.files, [new.parent().unwrap().join("x.txt")]);
    }

    #[cfg(windows)]
    #[test]
    fn a_file_on_another_drive_keeps_its_whole_path() {
        let mut project = Project::new(Path::new(r"C:\a\p.limproj"));
        project.add([PathBuf::from(r"D:\b\x.txt")]);
        assert_eq!(Project::from_text(&project.path, &project.to_text()).unwrap().files, [PathBuf::from(r"D:\b\x.txt")]);
    }

    #[test]
    fn other_files_arent_taken_for_projects() {
        let path = Path::new("x.limproj");
        assert!(Project::from_text(path, "plain text").is_err());
        assert!(Project::from_text(path, "{\"a\": 1}").is_err());
        assert_eq!(Project::from_text(path, "{\"limoniter_project\": 1}").unwrap().name, "x", "and a sparse one is fine");
    }

    #[test]
    fn adding_skips_what_is_there_and_removing_takes_it_off() {
        let mut project = Project::new(Path::new("p.limproj"));
        assert_eq!(project.add([PathBuf::from("b.txt"), PathBuf::from("a.txt")]), 2);
        assert_eq!(project.add([PathBuf::from("a.txt"), PathBuf::from("c.txt")]), 1);
        assert_eq!(project.files, [PathBuf::from("a.txt"), PathBuf::from("b.txt"), PathBuf::from("c.txt")]);
        project.remove(Path::new("b.txt"));
        assert!(!project.contains(Path::new("b.txt")));
    }

    #[test]
    fn adding_a_folder_leaves_out_the_clutter() {
        let dir = folder("add");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("Node_Modules")).unwrap();
        for file in ["a.txt", "sub/b.txt", ".git/c", "Node_Modules/d"] {
            std::fs::write(dir.join(file), "x").unwrap();
        }
        let (mut found, cut_short) = files_in_folder(&dir);
        found.sort();
        assert!(!cut_short);
        assert_eq!(found, [dir.join("a.txt"), dir.join("sub").join("b.txt")]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_tree_follows_folders_and_the_filter() {
        let base = PathBuf::from(if cfg!(windows) { r"C:\p" } else { "/p" });
        let mut project = Project::new(&base.join("p.limproj"));
        project.add([base.join("top.txt"), base.join("sub").join("deep").join("x.rs"), base.join("sub").join("y.txt")]);
        let tree = tree_of(&project, "");
        assert_eq!(tree.files.len(), 1);
        assert_eq!(tree.folders["sub"].files[0].0, "y.txt");
        assert_eq!(tree.folders["sub"].folders["deep"].files[0].0, "x.rs");
        let filtered = tree_of(&project, " X.RS ");
        assert!(filtered.files.is_empty());
        assert_eq!(filtered.folders["sub"].folders["deep"].files.len(), 1);
        assert!(filtered.folders["sub"].files.is_empty());
    }

    fn sample_project(dir: &Path) -> Project {
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.txt"), "aaa").unwrap();
        std::fs::write(dir.join("sub").join("a.txt"), "same name").unwrap();
        let mut project = Project::new(&dir.join("p.limproj"));
        project.add([dir.join("a.txt"), dir.join("sub").join("a.txt")]);
        project.save().unwrap();
        project
    }

    #[test]
    fn a_backup_holds_every_file_and_can_be_put_back() {
        let dir = folder("backup");
        let project = sample_project(&dir);
        let report = backup(&project, "2026-11-04-1200").unwrap();
        assert_eq!(report.done, 3, "the project file and both of its files, though two have the same name");
        assert!(report.skipped.is_empty());
        std::fs::write(dir.join("a.txt"), "edited").unwrap();
        std::fs::remove_file(dir.join("sub").join("a.txt")).unwrap();
        let backups = list_backups(&project);
        assert_eq!(backups.len(), 1);
        let back = restore(&backups[0], &[]);
        assert_eq!((back.done, back.skipped.len()), (3, 0));
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"aaa");
        assert_eq!(std::fs::read(dir.join("sub").join("a.txt")).unwrap(), b"same name", "a file gone since comes back");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_backup_says_what_it_missed_and_never_overwrites_another() {
        let dir = folder("missed");
        let mut project = sample_project(&dir);
        project.add([dir.join("gone.txt")]);
        let first = backup(&project, "same").unwrap();
        assert_eq!(first.skipped.len(), 1);
        assert_eq!(first.skipped[0].0, dir.join("gone.txt"));
        let second = backup(&project, "same").unwrap();
        assert_ne!(first.folder, second.folder);
        assert_eq!(list_backups(&project).len(), 2);
        assert_eq!(list_backups(&project)[0].name, "same-2", "newest first");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn restoring_leaves_files_with_unsaved_changes_alone() {
        let dir = folder("keep");
        let project = sample_project(&dir);
        backup(&project, "b").unwrap();
        std::fs::write(dir.join("a.txt"), "edited").unwrap();
        let report = restore(&list_backups(&project)[0], &[dir.join("a.txt")]);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"edited");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_tampered_list_cant_reach_outside_the_backup_for_its_copies() {
        let dir = folder("tamper");
        std::fs::write(dir.join("secret.txt"), "secret").unwrap();
        let backup_folder = dir.join("b");
        std::fs::create_dir_all(&backup_folder).unwrap();
        let backup = Backup { folder: backup_folder, name: "b".into(), files: vec![("../secret.txt".into(), dir.join("target.txt"))] };
        let report = restore(&backup, &[]);
        assert_eq!(report.done, 0);
        assert!(!dir.join("target.txt").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn no_backups_and_broken_ones_are_just_left_out() {
        let dir = folder("none");
        let project = sample_project(&dir);
        assert!(list_backups(&project).is_empty());
        std::fs::create_dir_all(backups_root(&project).join("broken")).unwrap();
        assert!(list_backups(&project).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_panel_draws_with_and_without_a_project() {
        let ctx = egui::Context::default();
        let mut project = Project::new(Path::new("p.limproj"));
        project.add([PathBuf::from("sub/a.txt"), PathBuf::from("b.txt")]);
        let mut state = PanelState::default();
        for chosen in [None, Some(&project)] {
            for _ in 0..2 {
                let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 600.0))), ..Default::default() };
                let mut output = ctx.run_ui(raw, |ui| {
                    assert!(panel(ui, &mut state, chosen, None).is_empty());
                });
                output.textures_delta.clear();
            }
        }
    }
}
