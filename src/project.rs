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
    /// Build, run and test commands of the project's own, run in its folder.
    pub tasks: Vec<crate::tasks::Task>,
    /// Folder comparisons saved in the project, by name, listed under Comparisons in its panel.
    pub comparisons: Vec<crate::compare_session::Session>,
    /// A folder opened as a project, as VS Code opens one: everything in it is the project, read from the disk as
    /// it is now, so `files` is its index rather than a list kept in the project's file.
    pub root: Option<PathBuf>,
    /// Where a folder project came from, when it's the mirror of a folder on a server: to connect to again.
    pub remote: Option<crate::remote_project::Origin>,
    /// How each of the project's files is debugged (Ctrl+F5), by the file: kept here so it's asked only once.
    pub debug: crate::dap::Kept,
    /// The assembler each processor's source in the project is built with (a processor's id, an assembler's id:
    /// "68000", "gnu-as"), so it's never built with another, whichever is chosen for other source. Where the
    /// program is on this computer comes from the settings, as that differs from one computer to the next.
    pub assemblers: Vec<(String, String)>,
    /// A folder project's Explorer: what's hidden from it (and from Go to file and searching), how lately a file
    /// has to have been edited to stand out, and when Limoniter last saved each file.
    pub explorer: crate::explorer_kept::Kept,
    /// The colours of the project's files, by their places in it, and the rules that colour folders and patterns.
    pub colours: crate::file_colours::Colours,
    /// The database connections kept with the project (without their passwords, which are in the keychain).
    pub databases: Vec<crate::database::Connection>,
    /// How this project saves files by itself, when it says (Off or a way), rather than leaving it to the settings.
    pub autosave: Option<crate::settings::AutosaveChoice>,
    /// What the project says about indenting, search, formatting, language servers and the rest, over the person's
    /// own settings. A folder project keeps it in the folder, in `.limoniter/project.json`, so Git can have it.
    pub prefs: crate::project_prefs::Prefs,
    /// Words the spell checker accepts in this project's code, lower case and in order. A folder project keeps
    /// them in `.limoniter/words.txt` in its folder, so Git has them; any other in its own file.
    pub words: Vec<String>,
    /// What the folder's `.vscode` says, read again whenever the project is opened or its settings are changed,
    /// so a team that keeps those files has one copy. The project's own settings win over it.
    pub vscode: crate::vscode_project::Read,
    /// The `.code-workspace` file the project was opened from, whose settings are read with `.vscode`'s.
    pub workspace_file: Option<PathBuf>,
    /// Whether the extensions `.vscode/extensions.json` recommends have been offered, which is done only once.
    pub vscode_offered: bool,
}

fn folder_of(path: &Path) -> PathBuf {
    path.parent().map_or_else(PathBuf::new, Path::to_path_buf)
}

/// A path as it's written in the project file: relative if it's on the same drive, and otherwise whole.
pub(crate) fn stored(base: &Path, path: &Path) -> String {
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
pub(crate) fn resolved(base: &Path, written: &str) -> PathBuf {
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
    /// A folder opened as a project: named after the folder, its project file kept where `path` says.
    pub fn of_folder(path: &Path, root: &Path) -> Self {
        let mut project = Self::new(path);
        project.name = root.file_name().map_or_else(|| root.display().to_string(), |n| n.to_string_lossy().into_owned());
        project.root = Some(root.to_path_buf());
        project.read_words_file();
        project.read_vscode();
        project
    }

    /// A folder project's words, from the file in its folder.
    fn read_words_file(&mut self) {
        if let Some(root) = &self.root {
            self.words = std::fs::read_to_string(crate::code_spelling::words_file(root)).map(|text| crate::code_spelling::read_words(&text)).unwrap_or_default();
        }
    }

    /// Adds a word to the project's list, and keeps it: in the folder's words file for a folder project, and
    /// otherwise the project file (so the caller saves that). Says whether the list changed.
    pub fn add_word(&mut self, word: &str) -> Result<bool, String> {
        let word = word.trim().to_lowercase();
        if word.is_empty() || self.words.contains(&word) {
            return Ok(false);
        }
        self.words.push(word);
        self.words.sort();
        if let Some(root) = &self.root {
            let file = crate::code_spelling::words_file(root);
            std::fs::create_dir_all(file.parent().unwrap_or(root)).map_err(|e| e.to_string())?;
            codec::write_file(&file, crate::code_spelling::write_words(&self.words).as_bytes()).map_err(|e| e.to_string())?;
        }
        Ok(true)
    }

    pub fn new(path: &Path) -> Self {
        let name = path.file_stem().map_or_else(|| "Project".to_string(), |stem| stem.to_string_lossy().into_owned());
        Self { path: path.to_path_buf(), name, ..Self::default() }
    }

    pub fn folder(&self) -> PathBuf {
        self.root.clone().unwrap_or_else(|| folder_of(&self.path))
    }

    pub fn to_text(&self) -> String {
        let base = self.folder();
        let list = |paths: &[PathBuf]| paths.iter().map(|path| stored(&base, path)).collect::<Vec<_>>();
        let files = if self.root.is_some() { Vec::new() } else { list(&self.files) };
        let mut value = json!({
            "limoniter_project": 1,
            "name": self.name,
            "notes": self.notes,
            "files": files,
            "open_files": list(&self.open_files),
            "tasks": self.tasks.iter().map(|t| json!({ "name": t.name, "command": t.command, "kind": t.kind.id() })).collect::<Vec<_>>(),
            "comparisons": crate::compare_session::list_to_json(&self.comparisons, Some(&base)),
        });
        if let Some(root) = &self.root {
            value["root"] = json!(stored(&folder_of(&self.path), root));
        }
        if let Some(remote) = &self.remote {
            value["remote"] = remote.to_json();
        }
        // Left out when there's nothing to keep, so a project that's never been debugged has a file as before.
        if !self.debug.is_empty() {
            value["debug"] = crate::dap::kept_to_json(&self.debug, Some(&base));
        }
        if !self.assemblers.is_empty() {
            value["assemblers"] = Value::Object(self.assemblers.iter().map(|(cpu, id)| (cpu.clone(), json!(id))).collect());
        }
        if self.root.is_none() && !self.words.is_empty() {
            value["words"] = json!(self.words);
        }
        if !self.explorer.is_default() {
            value["explorer"] = self.explorer.to_json(&base);
        }
        if !self.colours.is_empty() {
            value["colours"] = self.colours.to_json();
        }
        if !self.databases.is_empty() {
            value["databases"] = crate::database::list_to_json(&self.databases, Some(&base));
        }
        if let Some(choice) = self.autosave {
            value["autosave"] = json!(choice.id());
        }
        if let Some(file) = &self.workspace_file {
            value["code_workspace"] = json!(stored(&base, file));
        }
        if self.vscode_offered {
            value["vscode_offered"] = json!(true);
        }
        // A folder project's go in the folder instead (see `save`).
        if self.root.is_none() && !self.prefs.is_empty() {
            value["settings"] = self.prefs.to_json();
        }
        serde_json::to_string_pretty(&value).unwrap_or_default() + "\n"
    }

    pub fn from_text(path: &Path, text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text).map_err(|e| crate::i18n::tf!("That isn't a project file.\n\n{e}"))?;
        if value.get("limoniter_project").is_none() {
            return Err(crate::i18n::tr("That isn't a project file.").to_string());
        }
        let root = value.get("root").and_then(Value::as_str).map(|r| resolved(&folder_of(path), r));
        let base = root.clone().unwrap_or_else(|| folder_of(path));
        let list = |name: &str| -> Vec<PathBuf> { value.get(name).and_then(Value::as_array).map_or_else(Vec::new, |items| items.iter().filter_map(Value::as_str).map(|item| resolved(&base, item)).collect()) };
        let text_of = |name: &str| value.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
        let tasks = value.get("tasks").and_then(Value::as_array).map_or_else(Vec::new, |items| {
            items.iter().filter_map(|t| Some(crate::tasks::Task { name: t.get("name")?.as_str()?.to_string(), command: t.get("command")?.as_str()?.to_string(), kind: crate::tasks::Kind::from_id(t.get("kind").and_then(Value::as_str).unwrap_or("")) })).collect()
        });
        let comparisons = crate::compare_session::list_from_json(value.get("comparisons"), Some(&base));
        let remote = value.get("remote").and_then(crate::remote_project::Origin::from_json);
        let debug = crate::dap::kept_from_json(value.get("debug"), Some(&base));
        let assemblers = value.get("assemblers").and_then(Value::as_object).map_or_else(Vec::new, |map| map.iter().filter_map(|(cpu, id)| Some((cpu.clone(), id.as_str()?.to_string()))).collect());
        let explorer = crate::explorer_kept::Kept::from_json(value.get("explorer"), &base);
        let colours = crate::file_colours::Colours::from_json(value.get("colours"));
        let databases = crate::database::list_from_json(value.get("databases"), Some(&base));
        let mut project = Self { path: path.to_path_buf(), name: text_of("name"), notes: text_of("notes"), files: list("files"), open_files: list("open_files"), tasks, comparisons, root, remote, debug, assemblers, explorer, colours, databases, autosave: value.get("autosave").and_then(Value::as_str).and_then(crate::settings::AutosaveChoice::from_id), prefs: crate::project_prefs::Prefs::from_json(value.get("settings")), vscode: Default::default(), workspace_file: value.get("code_workspace").and_then(Value::as_str).map(|w| resolved(&base, w)), vscode_offered: value.get("vscode_offered").and_then(Value::as_bool).unwrap_or(false), words: Vec::new() };
        if project.root.is_some() {
            project.read_words_file();
        } else {
            project.words = crate::code_spelling::read_words(&value.get("words").and_then(Value::as_array).map_or_else(String::new, |items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n")));
        }
        if project.name.is_empty() {
            project.name = Self::new(path).name;
        }
        Ok(project)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let mut project = Self::from_text(path, &std::fs::read_to_string(path).map_err(|e| e.to_string())?)?;
        // A folder's own file says more than the old place (the project file) did, where it exists.
        if let Some(root) = &project.root
            && let Ok(text) = std::fs::read_to_string(crate::project_prefs::file_in(root))
            && let Ok(value) = serde_json::from_str::<Value>(&text)
        {
            project.prefs = crate::project_prefs::Prefs::from_json(Some(&value));
        }
        project.read_vscode();
        Ok(project)
    }

    /// Reads the folder's `.vscode` again, unless the project has taken its values in (Import into Limoniter).
    /// Only a folder project follows one.
    pub fn read_vscode(&mut self) {
        self.vscode = match &self.root {
            Some(root) => crate::vscode_project::read(root, self.workspace_file.as_deref(), self.prefs.follow_vscode != Some(false)),
            None => Default::default(),
        };
    }

    /// The project's settings as they are in effect: its own over what `.vscode` says.
    pub fn eff(&self) -> std::borrow::Cow<'_, crate::project_prefs::Prefs> {
        if self.vscode.prefs == crate::project_prefs::Prefs::default() {
            std::borrow::Cow::Borrowed(&self.prefs)
        } else {
            std::borrow::Cow::Owned(self.prefs.over(&self.vscode.prefs))
        }
    }

    /// How the project saves by itself: its own choice, else what VS Code's `files.autoSave` says.
    pub fn eff_autosave(&self) -> Option<crate::settings::AutosaveChoice> {
        self.autosave.or(self.vscode.autosave)
    }

    pub fn save(&self) -> Result<(), String> {
        codec::write_file(&self.path, self.to_text().as_bytes()).map_err(|e| e.to_string())?;
        if let Some(root) = &self.root {
            self.save_folder_prefs(root)?;
        }
        Ok(())
    }

    /// A folder project's settings go in the folder, so they can be shared. Nothing is written for a project that
    /// has none, and a file that's there is taken away once they've all been put back to the person's own.
    fn save_folder_prefs(&self, root: &Path) -> Result<(), String> {
        let file = crate::project_prefs::file_in(root);
        if self.prefs.is_empty() {
            if file.is_file() {
                let _ = std::fs::remove_file(&file);
            }
            return Ok(());
        }
        let text = serde_json::to_string_pretty(&self.prefs.to_json()).unwrap_or_default() + "\n";
        if std::fs::read_to_string(&file).is_ok_and(|now| now == text) {
            return Ok(());
        }
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        }
        codec::write_file(&file, text.as_bytes()).map_err(|e| e.to_string())
    }

    /// The files search and Go to file look in: the project's, but for those its settings leave out.
    pub fn searchable(&self) -> Vec<PathBuf> {
        let prefs = self.eff();
        if prefs.search_exclude.is_empty() {
            return self.files.clone();
        }
        let base = self.folder();
        self.files.iter().filter(|path| !crate::workspace::excluded(&prefs.search_exclude, &base, path, false)).cloned().collect()
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

    /// The assembler the project builds a processor's source with, by its id, if it says.
    pub fn assembler(&self, cpu: &str) -> Option<&str> {
        self.assemblers.iter().find(|(c, _)| c == cpu).map(|(_, id)| id.as_str())
    }

    /// Says which assembler the project builds a processor's source with, or (with None) leaves it to the settings.
    pub fn set_assembler(&mut self, cpu: &str, id: Option<&str>) {
        self.assemblers.retain(|(c, _)| c != cpu);
        if let Some(id) = id {
            self.assemblers.push((cpu.to_string(), id.to_string()));
        }
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
    std::fs::create_dir_all(&folder).map_err(|e| crate::i18n::tf!("Couldn't make {}.\n\n{e}", folder.display()))?;
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
    codec::write_file(&folder.join(MANIFEST), manifest.as_bytes()).map_err(|e| crate::i18n::tf!("Couldn't finish the backup.\n\n{e}"))?;
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
            report.skipped.push((original.clone(), crate::i18n::tr("it's open with unsaved changes").to_string()));
            continue;
        }
        // The name comes from a file that could have been edited, so it may only reach into this folder.
        if Path::new(copy).components().count() != 1 || matches!(Path::new(copy).components().next(), Some(Component::ParentDir | Component::RootDir | Component::Prefix(_))) {
            report.skipped.push((original.clone(), crate::i18n::tr("its backup is listed oddly").to_string()));
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
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).unwrap_or_else(|_| Err(crate::i18n::tr("Copying the files failed unexpectedly.").to_string()));
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
    /// Compare a file with another, to be chosen.
    CompareWith(PathBuf),
    /// Compare a folder with another, to be chosen.
    CompareFolder(PathBuf),
    /// Compare a folder, or a file, with how it was at the last commit.
    CompareWithHead(PathBuf),
    /// Open a comparison saved in the project, by name.
    OpenComparison(String),
    /// Rename or delete the saved comparisons, starting with one.
    ManageComparisons(String),
    /// Something asked of a folder project's Explorer: making, renaming or deleting files, and the rest.
    Explorer(crate::explorer::Act),
    /// The gear on the panel's title bar: the project's settings.
    Settings,
}

#[derive(Default)]
pub struct PanelState {
    pub filter: String,
    pub explorer: crate::explorer::Explorer,
}

/// A folder in the panel's tree.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct TreeFolder {
    pub folders: std::collections::BTreeMap<String, TreeFolder>,
    pub files: Vec<(String, PathBuf)>,
}

/// Arranges files by folder, relative to the project's own.
/// Files the recency slider hides are left out, and so are the folders with nothing shown in them.
pub fn tree_of(project: &Project, filter: &str, recency: &crate::explorer::Recency) -> TreeFolder {
    let base = project.folder();
    let filter = filter.trim().to_lowercase();
    let mut root = TreeFolder::default();
    for path in &project.files {
        let shown = relative_to(&base, path).unwrap_or_else(|| path.clone());
        let name = shown.file_name().map_or_else(|| shown.to_string_lossy().into_owned(), |name| name.to_string_lossy().into_owned());
        if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
            continue;
        }
        if recency.hides(path, false) {
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

/// What marks a file in the project's tree: being the one in front, and its colour, found by its place in the project.
struct Marks<'a> {
    active: Option<&'a Path>,
    base: &'a Path,
    colours: &'a crate::file_colours::Colours,
    /// What the recency slider always shows, for the Always show item's tick.
    always: &'a [String],
}

/// Always show, in a right-click on a file or folder of the tree: ticked when it's on the list, so the same item
/// takes it off.
fn always_item(ui: &mut Ui, marks: &Marks<'_>, path: &Path, is_folder: bool, actions: &mut Vec<PanelAction>) {
    let rule = crate::explorer_kept::always_rule(&stored(marks.base, path), is_folder);
    let mut on = marks.always.contains(&rule);
    if ui.checkbox(&mut on, crate::i18n::tr("Always show")).on_hover_text(crate::i18n::tr("Shown whatever the Newest slider says, without taking a place among the newest. Kept with the project, in Project settings, Files")).clicked() {
        actions.push(PanelAction::Explorer(crate::explorer::Act::AlwaysShow(rule)));
        ui.close();
    }
}

fn draw_tree(ui: &mut Ui, folder: &TreeFolder, salt: &str, place: &Path, marks: &Marks<'_>, actions: &mut Vec<PanelAction>, open_all: bool) {
    let active = marks.active;
    for (name, inner) in &folder.folders {
        let id = format!("{salt}/{name}");
        let here = place.join(name);
        let shown = egui::CollapsingHeader::new(RichText::new(name).size(12.5)).id_salt(&id).default_open(true).open(open_all.then_some(true)).show(ui, |ui| draw_tree(ui, inner, &id, &here, marks, actions, open_all));
        crate::fit::context_menu(&shown.header_response, |ui| {
            if ui.button(crate::i18n::tr("Compare folders\u{2026}")).on_hover_text(crate::i18n::tr("Compare this folder with another, side by side")).clicked() {
                actions.push(PanelAction::CompareFolder(here.clone()));
                ui.close();
            }
            if ui.button(crate::i18n::tr("Compare with the last commit")).on_hover_text(crate::i18n::tr("Compare this folder with how it was at the last Git commit")).clicked() {
                actions.push(PanelAction::CompareWithHead(here.clone()));
                ui.close();
            }
            always_item(ui, marks, &here, true, actions);
        });
    }
    for (name, path) in &folder.files {
        let here = active.is_some_and(|active| same_file(active, path));
        let missing = !path.exists();
        let text = RichText::new(name).size(12.5).color(if missing { theme::text_disabled() } else { theme::text() });
        let button = egui::Button::selectable(here, text).frame_when_inactive(false).wrap_mode(egui::TextWrapMode::Truncate);
        let response = ui.add(button).on_hover_text(if missing { format!("{} (not found)", path.display()) } else { path.display().to_string() });
        // The file's colour, as a bar at the left of its row.
        if let Some(tint) = marks.colours.look(&stored(marks.base, path)).tint {
            ui.painter().rect_filled(egui::Rect::from_min_size(response.rect.min + egui::vec2(0.0, 2.0), egui::vec2(3.0, response.rect.height() - 4.0)), 1.0, tint.mark());
        }
        if response.clicked() && !missing {
            actions.push(PanelAction::Open(path.clone()));
        }
        crate::fit::context_menu(&response, |ui| {
            if !missing && ui.button(crate::i18n::tr("Compare with\u{2026}")).on_hover_text(crate::i18n::tr("Compare this file with another, side by side")).clicked() {
                actions.push(PanelAction::CompareWith(path.clone()));
                ui.close();
            }
            if !missing && ui.button(crate::i18n::tr("Compare with the last commit")).clicked() {
                actions.push(PanelAction::CompareWithHead(path.clone()));
                ui.close();
            }
            if ui.button(crate::i18n::tr("Take off the project")).on_hover_text(crate::i18n::tr("The file itself stays")).clicked() {
                actions.push(PanelAction::Remove(path.clone()));
                ui.close();
            }
            always_item(ui, marks, path, false, actions);
        });
    }
}

pub fn panel(ui: &mut Ui, state: &mut PanelState, project: Option<&Project>, active: Option<&Path>) -> Vec<PanelAction> {
    let mut actions = Vec::new();
    let Some(project) = project else {
        ui.label(RichText::new(crate::i18n::tr("No project is open. File, Open folder opens a folder as one; Project can make one or open one.")).color(theme::text_faint()).size(12.5));
        return actions;
    };
    // A folder opened as a project: the Explorer, or the files a few letters find while some are typed.
    if let Some(root) = &project.root {
        ui.add(egui::TextEdit::singleline(&mut state.filter).hint_text(crate::i18n::tr("Find a file by its name (Ctrl+E)")).desired_width(f32::INFINITY));
        ui.add_space(2.0);
        comparisons(ui, project, &mut actions);
        let acts = if state.filter.trim().is_empty() { state.explorer.draw(ui, root, active, &project.explorer, &project.colours) } else { crate::explorer::filtered(ui, root, &project.files, &state.filter, active, &project.colours) };
        for act in acts {
            actions.push(match act {
                crate::explorer::Act::Open(path) => PanelAction::Open(path),
                crate::explorer::Act::CompareWith(path) => PanelAction::CompareWith(path),
                crate::explorer::Act::CompareFolder(path) => PanelAction::CompareFolder(path),
                crate::explorer::Act::CompareWithHead(path) => PanelAction::CompareWithHead(path),
                other => PanelAction::Explorer(other),
            });
        }
        return actions;
    }
    ui.label(RichText::new(&project.name).color(theme::text()).size(13.0)).on_hover_text(if project.notes.is_empty() { project.path.display().to_string() } else { format!("{}\n\n{}", project.path.display(), project.notes) });
    ui.add(egui::TextEdit::singleline(&mut state.filter).hint_text(crate::i18n::tr("Filter")).desired_width(f32::INFINITY));
    ui.add_space(2.0);
    comparisons(ui, project, &mut actions);
    if project.files.is_empty() {
        ui.label(RichText::new(crate::i18n::tr("No files yet. Add some from Tools, Project.")).color(theme::text_faint()).size(12.5));
        return actions;
    }
    let base = project.folder();
    // The recency slider, at the bottom: only the most recently edited share of the files shows.
    state.explorer.recency.keep_for(&base);
    state.explorer.recency.update_cut(&base, &project.explorer);
    let tree = tree_of(project, &state.filter, &state.explorer.recency);
    let room = if state.explorer.recency.tall(&project.explorer) { 52.0 } else { 30.0 };
    egui::ScrollArea::both().id_salt("project-scroll").auto_shrink([false, false]).max_height((ui.available_height() - room).max(40.0)).show(ui, |ui| draw_tree(ui, &tree, "project", &base, &Marks { active, base: &base, colours: &project.colours, always: &project.explorer.always }, &mut actions, !state.filter.trim().is_empty()));
    let mut acts = Vec::new();
    state.explorer.recency.slider(ui, &project.explorer, &mut acts);
    actions.extend(acts.into_iter().map(PanelAction::Explorer));
    actions
}

/// The comparisons saved in the project.
fn comparisons(ui: &mut Ui, project: &Project, actions: &mut Vec<PanelAction>) {
    if !project.comparisons.is_empty() {
        egui::CollapsingHeader::new(RichText::new(crate::i18n::tr("Comparisons")).size(12.5)).id_salt("project-comparisons").default_open(true).show(ui, |ui| {
            for session in &project.comparisons {
                let response = ui.add(egui::Button::selectable(false, RichText::new(&session.name).size(12.5).color(theme::text())).frame_when_inactive(false).wrap_mode(egui::TextWrapMode::Truncate));
                let response = response.on_hover_text(crate::i18n::tf!("{}\n\u{2194} {}", session.left.describe(), session.right.describe()));
                if response.clicked() {
                    actions.push(PanelAction::OpenComparison(session.name.clone()));
                }
                crate::fit::context_menu(&response, |ui| {
                    if ui.button(crate::i18n::tr("Open")).clicked() {
                        actions.push(PanelAction::OpenComparison(session.name.clone()));
                        ui.close();
                    }
                    if ui.button(crate::i18n::tr("Rename or delete\u{2026}")).clicked() {
                        actions.push(PanelAction::ManageComparisons(session.name.clone()));
                        ui.close();
                    }
                });
            }
        });
    }
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
    fn a_projects_spelling_words_are_kept_in_its_file_or_in_a_folders_words_file() {
        let dir = folder("words");
        // An ordinary project has them in its own file, and says nothing when there are none.
        let mut project = Project::new(&dir.join("demo.limproj"));
        assert!(!project.to_text().contains("words"));
        assert_eq!(project.add_word(" Frobnicate "), Ok(true));
        assert_eq!(project.add_word("frobnicate"), Ok(false), "a word is only kept once, in any case");
        assert_eq!(project.add_word(""), Ok(false));
        assert_eq!(Project::from_text(&project.path, &project.to_text()).unwrap().words, ["frobnicate"]);
        assert!(!dir.join(".limoniter").exists(), "nothing goes in a folder for a project that isn't one");
        // A folder project writes one word a line, to go into Git, and reads them again when it's opened.
        let root = dir.join("code");
        std::fs::create_dir_all(&root).unwrap();
        let mut folder_project = Project::of_folder(&dir.join("kept.limproj"), &root);
        folder_project.add_word("widget").unwrap();
        folder_project.add_word("Acme").unwrap();
        assert_eq!(std::fs::read_to_string(root.join(".limoniter").join("words.txt")).unwrap(), "acme\nwidget\n");
        assert!(!folder_project.to_text().contains("\"words\""));
        assert_eq!(Project::of_folder(&dir.join("kept.limproj"), &root).words, ["acme", "widget"]);
        assert_eq!(Project::from_text(&folder_project.path, &folder_project.to_text()).unwrap().words, ["acme", "widget"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_keeps_the_assembler_each_processor_s_source_is_built_with() {
        let dir = folder("assemblers");
        let mut project = Project::new(&dir.join("demo.limproj"));
        assert!(!project.to_text().contains("assemblers"), "nothing said, nothing written");
        project.set_assembler("68000", Some("gnu-as"));
        project.set_assembler("z80", Some("limoniter"));
        project.set_assembler("z80", None);
        let loaded = Project::from_text(&project.path, &project.to_text()).unwrap();
        assert_eq!(loaded.assembler("68000"), Some("gnu-as"));
        assert_eq!(loaded.assembler("z80"), None);
        assert_eq!(loaded, project);
    }

    #[test]
    fn a_moved_project_finds_its_files_in_the_new_place() {
        let (old, new) = (PathBuf::from(if cfg!(windows) { r"C:\a\p.limproj" } else { "/a/p.limproj" }), PathBuf::from(if cfg!(windows) { r"D:\b\p.limproj" } else { "/b/p.limproj" }));
        let mut project = Project::new(&old);
        project.add([old.parent().unwrap().join("x.txt")]);
        let moved = Project::from_text(&new, &project.to_text()).unwrap();
        assert_eq!(moved.files, [new.parent().unwrap().join("x.txt")]);
    }

    /// How a C file in the project is debugged: its program and folder in the project, a debugger of its own.
    fn debugged_here(base: &Path) -> (PathBuf, crate::dap::Config) {
        let config = crate::dap::Config { kind: crate::dap::Kind::Gdb, adapter: "gdb -i dap".into(), program: base.join("build").join("life").display().to_string(), arguments: "--size 40 \"two words\"".into(), folder: base.join("data").display().to_string(), emulator: None, env: Vec::new(), stop_on_entry: false };
        (base.join("src").join("life.c"), config)
    }

    #[test]
    fn debug_settings_are_kept_in_the_project_relative_to_it() {
        let base = PathBuf::from(if cfg!(windows) { r"C:\work\life" } else { "/work/life" });
        let path = base.join("life.limproj");
        let mut project = Project::new(&path);
        let (file, config) = debugged_here(&base);
        project.debug.insert(file.clone(), config.clone());
        let text = project.to_text();
        assert!(text.contains("\"src/life.c\"") && text.contains("\"build/life\"") && text.contains("\"data\""), "kept relative: {text}");
        assert!(!text.contains("work"), "with no whole path in it: {text}");
        assert_eq!(Project::from_text(&path, &text).unwrap(), project);
        // Moved to another folder (and drive), the file, its program and its folder are found in the new place.
        let new_base = PathBuf::from(if cfg!(windows) { r"D:\elsewhere\life2" } else { "/elsewhere/life2" });
        let moved = Project::from_text(&new_base.join("life.limproj"), &text).unwrap();
        let (new_file, new_config) = debugged_here(&new_base);
        assert_eq!(moved.debug.get(&new_file), Some(&new_config));
        assert_eq!(moved.debug.len(), 1);
    }

    #[test]
    fn a_project_from_before_debug_settings_still_opens() {
        let base = PathBuf::from(if cfg!(windows) { r"C:\old" } else { "/old" });
        let path = base.join("old.limproj");
        let old = "{\n  \"limoniter_project\": 1,\n  \"name\": \"old\",\n  \"notes\": \"\",\n  \"files\": [\"a.c\"],\n  \"open_files\": [],\n  \"tasks\": [],\n  \"comparisons\": []\n}\n";
        let project = Project::from_text(&path, old).unwrap();
        assert!(project.debug.is_empty());
        assert_eq!(project.files, [base.join("a.c")]);
        assert!(!project.to_text().contains("debug"), "and one that's never been debugged is written as before");
        // An entry this version can't read is left out, and the rest kept.
        let odd = old.replace("\"comparisons\": []", "\"comparisons\": [], \"debug\": [{\"file\": \"a.c\", \"debugger\": \"future\"}, {\"file\": \"b.c\", \"debugger\": \"lldb-dap\", \"program\": \"b\"}, 7]");
        let read = Project::from_text(&path, &odd).unwrap();
        assert_eq!(read.debug.len(), 1);
        assert_eq!(read.debug[&base.join("b.c")].program, base.join("b").display().to_string());
        assert_eq!(read.debug[&base.join("b.c")].kind, crate::dap::Kind::Lldb);
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
        let tree = tree_of(&project, "", &Default::default());
        assert_eq!(tree.files.len(), 1);
        assert_eq!(tree.folders["sub"].files[0].0, "y.txt");
        assert_eq!(tree.folders["sub"].folders["deep"].files[0].0, "x.rs");
        let filtered = tree_of(&project, " X.RS ", &Default::default());
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

    #[test]
    fn the_recency_slider_thins_a_limproj_panel_s_tree_and_its_folders() {
        use std::time::{Duration, SystemTime};
        let base = PathBuf::from(if cfg!(windows) { r"C:\work\site" } else { "/work/site" });
        let mut project = Project::new(&base.join("site.limproj"));
        project.add([base.join("new.txt"), base.join("css/a.css"), base.join("css/b.css"), base.join("old/z.txt")]);
        let at = |secs: u64| SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
        let mut recency = crate::explorer::Recency::default();
        recency.set_every_time(&base, [(base.join("new.txt"), at(90)), (base.join("css/b.css"), at(50)), (base.join("css/a.css"), at(50)), (base.join("old/z.txt"), at(1))].into());
        assert_eq!(tree_of(&project, "", &recency).files.len(), 1, "all shown at 100%");
        project.explorer.recency = 50;
        recency.update_cut(&base, &project.explorer);
        let tree = tree_of(&project, "", &recency);
        assert_eq!(tree.files.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["new.txt"]);
        assert_eq!(tree.folders.keys().collect::<Vec<_>>(), ["css"], "old has nothing shown, so it's gone");
        assert_eq!(tree.folders["css"].files.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["a.css"], "the tie goes to the name");
        assert_eq!(recency.cut.as_ref().unwrap().said(), "2 files shown, 2 hidden");
        // Kept with the .limproj.
        let read = Project::from_text(&project.path, &project.to_text()).unwrap();
        assert_eq!(read.explorer.recency, 50);
    }
}
