//! What a folder project keeps about its Explorer: the folders and kinds of file hidden from it (and so from Go to
//! file and searching too), how lately a file has to have been edited to stand out, and when Limoniter last saved
//! each of the folder's files. Also the build and dependency folders a folder is offered to have hidden when it's
//! opened, and how lately each file was edited: the later of Limoniter's save and its last change in Git, or the
//! disk's date for a file Git doesn't know, so a fresh clone doesn't make every file look new.

use eframe::egui::Color32;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Folders that are made by building or by fetching what a project depends on, rather than written by anyone.
pub const BUILD_FOLDERS: [&str; 7] = ["node_modules", "target", "build", "dist", "obj", ".venv", "__pycache__"];
/// How lately a file has to have been edited to stand out, until another number of days is chosen.
pub const RECENT_DAYS: u32 = 7;
/// The most days that can be chosen; Limoniter's save times older than this aren't kept.
pub const MOST_RECENT_DAYS: u32 = 365;
const DAY: u64 = 24 * 60 * 60;

/// Something hidden from a folder project's Explorer, Go to file and searching.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hide {
    /// One folder, by its place in the project, written with / ("build", "src/generated").
    Folder(String),
    /// Every folder of this name, wherever it is ("node_modules", "__pycache__").
    Named(String),
    /// Every file with this ending, without its dot and in small letters ("o" for .o files).
    Kind(String),
}

/// A file's ending, in small letters, without its dot: none for a name without one, or one like .gitignore.
pub fn ending(name: &str) -> Option<String> {
    match name.rsplit_once('.') {
        Some((stem, end)) if !stem.is_empty() && !end.is_empty() => Some(end.to_lowercase()),
        _ => None,
    }
}

impl Hide {
    /// As it's written in the project file, in .gitignore's way: "build/", "**/node_modules/", "*.o".
    pub fn to_text(&self) -> String {
        match self {
            Hide::Folder(folder) => format!("{folder}/"),
            Hide::Named(name) => format!("**/{name}/"),
            Hide::Kind(end) => format!("*.{end}"),
        }
    }

    pub fn from_text(text: &str) -> Option<Hide> {
        let text = text.trim();
        if let Some(end) = text.strip_prefix("*.") {
            return (!end.is_empty() && !end.contains(['/', '\\'])).then(|| Hide::Kind(end.to_lowercase()));
        }
        if let Some(name) = text.strip_prefix("**/").and_then(|t| t.strip_suffix('/')) {
            return (!name.is_empty() && !name.contains('/')).then(|| Hide::Named(name.to_string()));
        }
        let folder = text.strip_suffix('/')?.trim_start_matches('/').replace('\\', "/");
        // Only a place inside the project: never one reached by going up out of it.
        let fine = !folder.is_empty() && folder.split('/').all(|part| !part.is_empty() && part != ".." && part != ".");
        fine.then_some(Hide::Folder(folder))
    }

    /// What's hidden, in words, for the list in the project's settings.
    pub fn describe(&self) -> String {
        match self {
            Hide::Folder(folder) => crate::i18n::tf!("The folder {folder}"),
            Hide::Named(name) => crate::i18n::tf!("Every folder called {name}"),
            Hide::Kind(end) => crate::i18n::tf!(".{end} files"),
        }
    }

    /// Whether this hides something, by its place in the project written with /.
    pub fn hides(&self, relative: &str, is_folder: bool) -> bool {
        match self {
            Hide::Folder(folder) => relative == folder || relative.strip_prefix(folder.as_str()).is_some_and(|rest| rest.starts_with('/')),
            Hide::Named(name) => {
                let mut parts: Vec<&str> = relative.split('/').collect();
                // A file is hidden by the folders it's in, not by its own name.
                if !is_folder {
                    parts.pop();
                }
                parts.contains(&name.as_str())
            }
            Hide::Kind(end) => !is_folder && relative.rsplit('/').next().and_then(ending).as_deref() == Some(end.as_str()),
        }
    }
}

/// Whether any of the rules hides a file or folder in the project at `root`.
pub fn hidden_by(rules: &[Hide], root: &Path, path: &Path, is_folder: bool) -> bool {
    if rules.is_empty() {
        return false;
    }
    let Ok(relative) = path.strip_prefix(root) else { return false };
    let relative = relative.to_string_lossy().replace('\\', "/");
    rules.iter().any(|rule| rule.hides(&relative, is_folder))
}

/// What a folder project keeps about its Explorer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kept {
    /// What's hidden, in the order it was hidden.
    pub hidden: Vec<Hide>,
    /// Hidden things shown faintly in the Explorer, to be found and taken back.
    pub show_hidden: bool,
    /// How many days back a file stands out as recently edited; 0 when that's off.
    pub recent_days: u32,
    /// Whether the folder's build and dependency folders have been offered for hiding, so they're offered once.
    pub offered: bool,
    /// When Limoniter last saved each file, in seconds since 1970, by its whole path.
    pub saved: BTreeMap<PathBuf, u64>,
}

impl Default for Kept {
    fn default() -> Self {
        Kept { hidden: Vec::new(), show_hidden: false, recent_days: RECENT_DAYS, offered: false, saved: BTreeMap::new() }
    }
}

fn seconds(when: SystemTime) -> u64 {
    when.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn at(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
}

impl Kept {
    pub fn is_default(&self) -> bool {
        *self == Kept::default()
    }

    /// For the project file, with the saved files' paths relative to the project's folder. Only what isn't as
    /// it starts is written, so a project that's never had anything hidden has a file as before.
    pub fn to_json(&self, base: &Path) -> Value {
        let mut value = Map::new();
        if !self.hidden.is_empty() {
            value.insert("hidden".into(), json!(self.hidden.iter().map(Hide::to_text).collect::<Vec<_>>()));
        }
        if self.show_hidden {
            value.insert("show_hidden".into(), json!(true));
        }
        if self.recent_days != RECENT_DAYS {
            value.insert("recent_days".into(), json!(self.recent_days));
        }
        if self.offered {
            value.insert("offered".into(), json!(true));
        }
        if !self.saved.is_empty() {
            value.insert("saved".into(), Value::Object(self.saved.iter().map(|(path, when)| (crate::project::stored(base, path), json!(when))).collect()));
        }
        Value::Object(value)
    }

    pub fn from_json(value: Option<&Value>, base: &Path) -> Kept {
        let mut kept = Kept::default();
        let Some(value) = value else { return kept };
        kept.hidden = value.get("hidden").and_then(Value::as_array).map_or_else(Vec::new, |items| items.iter().filter_map(Value::as_str).filter_map(Hide::from_text).collect());
        kept.show_hidden = value.get("show_hidden").and_then(Value::as_bool).unwrap_or(false);
        kept.recent_days = value.get("recent_days").and_then(Value::as_u64).map_or(RECENT_DAYS, |days| days.min(u64::from(MOST_RECENT_DAYS)) as u32);
        kept.offered = value.get("offered").and_then(Value::as_bool).unwrap_or(false);
        kept.saved = value.get("saved").and_then(Value::as_object).map_or_else(BTreeMap::new, |map| map.iter().filter_map(|(path, when)| Some((crate::project::resolved(base, path), when.as_u64()?))).collect());
        kept
    }

    pub fn hides(&self, root: &Path, path: &Path, is_folder: bool) -> bool {
        hidden_by(&self.hidden, root, path, is_folder)
    }

    /// Hides something more, unless it's hidden that way already.
    pub fn hide(&mut self, rule: Hide) {
        if !self.hidden.contains(&rule) {
            self.hidden.push(rule);
        }
    }

    /// Takes back whatever hides a file or folder, and says what that was.
    pub fn show_again(&mut self, root: &Path, path: &Path, is_folder: bool) -> Vec<Hide> {
        let (back, kept): (Vec<Hide>, Vec<Hide>) = std::mem::take(&mut self.hidden).into_iter().partition(|rule| hidden_by(std::slice::from_ref(rule), root, path, is_folder));
        self.hidden = kept;
        back
    }

    /// Notes that Limoniter has just saved a file, and forgets saves too long ago to matter.
    pub fn note_saved(&mut self, path: &Path, when: SystemTime) {
        let now = seconds(when);
        self.saved.retain(|_, then| now.saturating_sub(*then) <= u64::from(MOST_RECENT_DAYS) * DAY);
        self.saved.insert(path.to_path_buf(), now);
    }

    /// When Limoniter last saved a file.
    pub fn saved_at(&self, path: &Path) -> Option<SystemTime> {
        self.saved.get(path).copied().map(at)
    }
}

/// The build and dependency folders in a folder, at its top or one folder down, by name, for offering to hide.
/// Those hidden already, by any rule, aren't offered.
pub fn build_folders(root: &Path, hidden: &[Hide]) -> Vec<String> {
    let mut found = HashSet::new();
    let folders_in = |folder: &Path| -> Vec<PathBuf> { std::fs::read_dir(folder).map(|read| read.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect()).unwrap_or_default() };
    for top in folders_in(root) {
        let name = top.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if BUILD_FOLDERS.contains(&name.as_str()) {
            if !hidden_by(hidden, root, &top, true) {
                found.insert(name);
            }
            continue;
        }
        if crate::workspace::hidden(&name) || name.starts_with('.') {
            continue;
        }
        for inner in folders_in(&top) {
            let name = inner.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if BUILD_FOLDERS.contains(&name.as_str()) && !hidden_by(hidden, root, &inner, true) {
                found.insert(name);
            }
        }
    }
    BUILD_FOLDERS.iter().filter(|name| found.contains(**name)).map(|name| (*name).to_string()).collect()
}

// ---------- How lately each file was edited ----------

/// How lately something was edited, for the colour of its dot: bright for today, cooler as it gets older.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Heat {
    Today,
    ThisWeek,
    ThisMonth,
    /// Longer ago than a month, but within the days chosen.
    Earlier,
}

impl Heat {
    pub fn of(when: SystemTime, now: SystemTime) -> Heat {
        let today = chrono::DateTime::<chrono::Local>::from(now).date_naive();
        let age = now.duration_since(when).unwrap_or_default().as_secs();
        if chrono::DateTime::<chrono::Local>::from(when).date_naive() >= today {
            Heat::Today
        } else if age < 7 * DAY {
            Heat::ThisWeek
        } else if age < 30 * DAY {
            Heat::ThisMonth
        } else {
            Heat::Earlier
        }
    }

    /// From hot to cool: they read on light and dark panels alike.
    pub fn colour(self) -> Color32 {
        match self {
            Heat::Today => Color32::from_rgb(255, 110, 40),
            Heat::ThisWeek => Color32::from_rgb(235, 175, 40),
            Heat::ThisMonth => Color32::from_rgb(70, 170, 170),
            Heat::Earlier => Color32::from_rgb(110, 125, 165),
        }
    }
}

/// Whether something edited then is within the days chosen.
pub fn within(when: SystemTime, now: SystemTime, days: u32) -> bool {
    days > 0 && now.duration_since(when).map_or(true, |age| age.as_secs() < u64::from(days) * DAY)
}

/// What Git says of a repository's files, for working out when each was last edited.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitTimes {
    pub repo: PathBuf,
    /// The files Git keeps, as it names them.
    pub tracked: HashSet<String>,
    /// Those changed since the last commit, whose disk date says more than Git's.
    pub changed: HashSet<String>,
    /// When each file last changed in a commit, in seconds since 1970, over the days looked at.
    pub committed: HashMap<String, u64>,
}

impl GitTimes {
    /// Asks Git, with one look at its history over the whole repository, rather than one for each file.
    pub fn read(repo: &Path, days: u32) -> Result<GitTimes, String> {
        let tracked = crate::git::tracked(repo)?;
        let changed = crate::git::status(repo)?.entries.into_iter().map(|e| e.path).collect();
        let committed = crate::git::last_changes(repo, days)?;
        Ok(GitTimes { repo: repo.to_path_buf(), tracked, changed, committed })
    }
}

/// When a file was last changed other than by Limoniter's saving it: its last change in Git if Git keeps it and it
/// hasn't been changed since (None if that was before the days Git was asked about), or else the disk's date.
pub fn outside_time(git: Option<&GitTimes>, path: &Path, disk: impl FnOnce() -> Option<SystemTime>) -> Option<SystemTime> {
    if let Some(git) = git
        && let Ok(relative) = path.strip_prefix(&git.repo)
    {
        let relative = relative.to_string_lossy().replace('\\', "/");
        if git.tracked.contains(&relative) && !git.changed.contains(&relative) {
            return git.committed.get(&relative).copied().map(at);
        }
    }
    disk()
}

/// When a file was last edited: the later of when Limoniter saved it and when it was last changed otherwise.
pub fn last_edited(saved: Option<SystemTime>, outside: Option<SystemTime>) -> Option<SystemTime> {
    saved.max(outside)
}

/// When each of a folder's files was last changed other than by Limoniter, for those within the days chosen.
/// Run in the background: it asks Git once, and reads the disk's date of the files Git doesn't vouch for.
pub fn outside_times(root: &Path, files: &[PathBuf], days: u32) -> HashMap<PathBuf, SystemTime> {
    let now = SystemTime::now();
    let git = crate::git::repository_of(root).filter(|_| crate::git::program().is_some()).and_then(|repo| GitTimes::read(&repo, days).ok());
    files.iter().filter_map(|path| outside_time(git.as_ref(), path, || std::fs::metadata(path).and_then(|m| m.modified()).ok()).filter(|&when| within(when, now, days)).map(|when| (path.clone(), when))).collect()
}

/// When a file was edited, for hovering over its name: "Last edited today at 14:02", or on a day.
pub fn when_said(when: SystemTime, now: SystemTime) -> String {
    let local = chrono::DateTime::<chrono::Local>::from(when);
    if Heat::of(when, now) == Heat::Today {
        crate::i18n::tf!("Last edited today at {}", local.format("%H:%M"))
    } else {
        let days = now.duration_since(when).unwrap_or_default().as_secs().div_ceil(DAY).max(1);
        crate::i18n::tf!("Last edited {} ({days} days ago)", local.format("%d %b %Y, %H:%M"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_hide_folders_named_folders_and_kinds_of_file() {
        let folder = Hide::Folder("src/gen".into());
        assert!(folder.hides("src/gen", true) && folder.hides("src/gen/a.c", false) && folder.hides("src/gen/deep/b.c", false));
        assert!(!folder.hides("src/generated", true) && !folder.hides("gen", true) && !folder.hides("src/a.c", false));
        let named = Hide::Named("node_modules".into());
        assert!(named.hides("node_modules", true) && named.hides("web/node_modules/x/index.js", false));
        assert!(!named.hides("node_modules", false), "a file of that name isn't a folder");
        let kind = Hide::Kind("o".into());
        assert!(kind.hides("main.o", false) && kind.hides("obj/deep/LIB.O", false));
        assert!(!kind.hides("main.c", false) && !kind.hides("thing.o", true) && !kind.hides(".o", false));
        // Written in the project file as .gitignore writes them, and read back the same.
        for rule in [folder, named, kind] {
            assert_eq!(Hide::from_text(&rule.to_text()), Some(rule.clone()), "{}", rule.to_text());
        }
        assert_eq!(Hide::from_text("../outside/"), None, "never a place outside the project");
        assert_eq!(Hide::from_text("nonsense"), None);
        assert_eq!(ending("a.tar.GZ").as_deref(), Some("gz"));
        assert_eq!(ending(".gitignore"), None);
    }

    #[test]
    fn what_hides_something_can_be_taken_back() {
        let root = Path::new("/w");
        let mut kept = Kept::default();
        kept.hide(Hide::Folder("build".into()));
        kept.hide(Hide::Kind("o".into()));
        kept.hide(Hide::Kind("o".into()));
        assert_eq!(kept.hidden.len(), 2, "hidden once");
        assert!(kept.hides(root, Path::new("/w/build/x.txt"), false) && kept.hides(root, Path::new("/w/src/a.o"), false));
        assert!(!kept.hides(root, Path::new("/w/src/a.c"), false) && !kept.hides(Path::new("/other"), Path::new("/w/build"), true));
        assert_eq!(kept.show_again(root, Path::new("/w/src/a.o"), false), [Hide::Kind("o".into())]);
        assert_eq!(kept.hidden, [Hide::Folder("build".into())]);
    }

    #[test]
    fn kept_settings_go_in_the_project_file_only_when_set() {
        let base = PathBuf::from(if cfg!(windows) { r"C:\work\game" } else { "/work/game" });
        let kept = Kept::default();
        assert!(kept.is_default());
        assert_eq!(kept.to_json(&base), json!({}), "nothing written as it starts");
        let mut kept = Kept { show_hidden: true, recent_days: 30, offered: true, ..Kept::default() };
        kept.hide(Hide::Named("target".into()));
        kept.hide(Hide::Kind("o".into()));
        kept.note_saved(&base.join("src").join("main.c"), at(2_000_000_000));
        let value = kept.to_json(&base);
        assert_eq!(value["hidden"], json!(["**/target/", "*.o"]));
        assert_eq!(value["saved"], json!({ "src/main.c": 2_000_000_000u64 }), "relative to the project");
        assert_eq!(Kept::from_json(Some(&value), &base), kept);
        // Moved elsewhere, the saved files are found in the new place.
        let moved = PathBuf::from(if cfg!(windows) { r"D:\game" } else { "/game" });
        assert_eq!(Kept::from_json(Some(&value), &moved).saved_at(&moved.join("src").join("main.c")), Some(at(2_000_000_000)));
        // Saves too long ago to matter are forgotten.
        kept.note_saved(&base.join("b.c"), at(2_000_000_000 + u64::from(MOST_RECENT_DAYS + 1) * DAY));
        assert_eq!(kept.saved.len(), 1);
    }

    #[test]
    fn build_folders_are_found_at_the_top_and_one_down() {
        let root = std::env::temp_dir().join(format!("limoniter-build-folders-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for folder in ["target/release", "web/node_modules/x", "src/__pycache__", "src/deep/dist", ".git/objects", "docs"] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        std::fs::write(root.join("build"), "a file, not a folder").unwrap();
        assert_eq!(build_folders(&root, &[]), ["node_modules", "target", "__pycache__"]);
        assert_eq!(build_folders(&root, &[Hide::Named("target".into())]), ["node_modules", "__pycache__"], "not offered again once hidden");
        assert_eq!(build_folders(&root, &[Hide::Folder("web".into()), Hide::Folder("src/__pycache__".into())]), ["target"], "nor when hidden another way");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_file_was_last_edited_when_limoniter_saved_it_or_git_or_the_disk_says() {
        let now = SystemTime::now();
        let ago = |days: u64| now - Duration::from_secs(days * DAY);
        let repo = PathBuf::from("/w");
        let git = GitTimes {
            repo: repo.clone(),
            tracked: ["old.c", "fresh.c", "edited.c"].map(String::from).into(),
            changed: ["edited.c"].map(String::from).into(),
            committed: [("fresh.c".to_string(), seconds(ago(2)))].into(),
        };
        let disk_now = || Some(now);
        // A fresh clone: every file's date on the disk is now, but Git says when each really changed.
        assert_eq!(outside_time(Some(&git), &repo.join("old.c"), disk_now), None, "last changed before the days asked about");
        assert_eq!(outside_time(Some(&git), &repo.join("fresh.c"), disk_now), Some(at(seconds(ago(2)))));
        // Changed since its last commit, or not in Git at all: the disk's date.
        assert_eq!(outside_time(Some(&git), &repo.join("edited.c"), disk_now), Some(now));
        assert_eq!(outside_time(Some(&git), &repo.join("new.c"), disk_now), Some(now));
        assert_eq!(outside_time(None, &repo.join("old.c"), || Some(ago(40))), Some(ago(40)), "no Git: the disk's date");
        // Limoniter's save counts when it's later than Git's change, and Git's when that is.
        assert_eq!(last_edited(Some(ago(1)), Some(ago(2))), Some(ago(1)));
        assert_eq!(last_edited(Some(ago(5)), Some(ago(2))), Some(ago(2)));
        assert_eq!(last_edited(Some(ago(5)), None), Some(ago(5)));
        assert!(within(ago(6), now, 7) && !within(ago(8), now, 7) && !within(now, now, 0), "0 days is off");
        // The dot's colour: hot today, cooler as it gets older.
        assert_eq!(Heat::of(now, now), Heat::Today);
        assert_eq!(Heat::of(ago(3), now), Heat::ThisWeek);
        assert_eq!(Heat::of(ago(20), now), Heat::ThisMonth);
        assert_eq!(Heat::of(ago(45), now), Heat::Earlier);
        assert!(when_said(now, now).contains("today"));
        assert!(when_said(ago(3), now).contains("3 days ago"), "{}", when_said(ago(3), now));
    }
}
