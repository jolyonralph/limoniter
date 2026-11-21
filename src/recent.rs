//! Recent projects and recent files, kept apart. File, Open recent (and the start page) has two tabs: Projects,
//! shown first each time it opens, and Files, where the files are grouped by the project they were last opened or
//! saved in, with those worked on outside any project in a group of their own. Picking a file from a project's
//! group opens that project first, if it isn't open, so the file comes back with its project around it.
//!
//! Which project each recent file was in is kept in the settings file beside the recent files themselves, as
//! `recent_file_project=FILE<U+0001>PROJECT` lines. A settings file from before has none, and every file in it is
//! then simply on its own.

use super::*;
use std::collections::HashMap;

/// Which of the two lists is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Projects,
    Files,
}

/// The settings key for which project a recent file was in.
const SETTING: &str = "recent_file_project";

/// The character between a file and its project in the setting: one no path has in it.
const BETWEEN: char = '\u{1}';

/// How many recent files the menu shows.
pub(super) const MENU_FILES: usize = 15;

/// Which project each recent file was last opened or saved in, if it was in one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileProjects(Vec<(PathBuf, PathBuf)>);

impl FileProjects {
    /// The project `file` was last worked on in, if any.
    pub fn project_of(&self, file: &Path) -> Option<&Path> {
        self.0.iter().find(|(known, _)| same_path(known, file)).map(|(_, project)| project.as_path())
    }

    /// Notes that `file` was just opened or saved in `project`, or outside any project when that's None.
    pub fn note(&mut self, file: &Path, project: Option<&Path>) {
        self.0.retain(|(known, _)| !same_path(known, file));
        if let Some(project) = project {
            self.0.insert(0, (file.to_path_buf(), project.to_path_buf()));
        }
    }

    /// Settings lines for the files still on the recent list, in its order.
    pub fn to_settings(&self, recent: &[PathBuf]) -> String {
        recent.iter().filter_map(|file| self.project_of(file).map(|project| format!("{SETTING}={}{BETWEEN}{}\n", file.display(), project.display()))).collect()
    }

    /// Read back from the settings file. A line that doesn't split in two is left out.
    pub fn from_settings(settings: &str) -> Self {
        let pairs = settings.lines().filter_map(|line| line.split_once('=')).filter(|(key, _)| *key == SETTING).filter_map(|(_, value)| value.split_once(BETWEEN));
        Self(pairs.filter(|(file, project)| !file.is_empty() && !project.is_empty()).map(|(file, project)| (PathBuf::from(file), PathBuf::from(project))).collect())
    }
}

/// The recent files of one project, or of none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Group {
    /// The project's file, or None for the files worked on outside any project.
    pub project: Option<PathBuf>,
    /// Newest first.
    pub files: Vec<PathBuf>,
}

/// The recent files (newest first) grouped by project. The group with the newest file comes first, and each
/// group's files keep their order; the files outside any project make a group like any other.
pub(super) fn grouped(recent: &[PathBuf], projects: &FileProjects) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for file in recent {
        let project = projects.project_of(file).map(Path::to_path_buf);
        match groups.iter_mut().find(|group| match (&group.project, &project) {
            (Some(a), Some(b)) => same_path(a, b),
            (None, None) => true,
            _ => false,
        }) {
            Some(group) => group.files.push(file.clone()),
            None => groups.push(Group { project, files: vec![file.clone()] }),
        }
    }
    groups
}

/// `place` cut short from its start to at most `most` characters, as its end says most.
pub(super) fn shortened(place: &str, most: usize) -> String {
    let count = place.chars().count();
    if count <= most {
        return place.to_string();
    }
    let rest: String = place.chars().skip(count + 1 - most).collect();
    format!("\u{2026}{rest}")
}

/// What the recent lists remember between frames.
#[derive(Default)]
pub(super) struct RecentLists {
    /// The tab the menu shows, while it's open.
    menu_tab: Tab,
    /// The pass the menu was last drawn in, to tell when it's opened afresh.
    menu_drawn: Option<u64>,
    /// The tab the menu opens on: Projects, unless a picture asks for Files.
    pub opens_on: Tab,
    /// Each recent project's name and, for a folder opened as a project, the folder, read from its file once.
    about: HashMap<PathBuf, Option<(String, Option<PathBuf>)>>,
}

impl RecentLists {
    /// The tab to show in the menu drawn in pass `pass`: the one it opens on, if it wasn't drawn just before.
    pub fn menu_tab(&mut self, pass: u64) -> Tab {
        if self.menu_drawn.is_none_or(|drawn| drawn + 1 < pass) {
            self.menu_tab = self.opens_on;
        }
        self.menu_drawn = Some(pass);
        self.menu_tab
    }

    pub fn choose_menu_tab(&mut self, tab: Tab) {
        self.menu_tab = tab;
    }

    /// A recent project's name, and its folder if it's a folder opened as a project, read once from its file.
    pub fn project(&mut self, path: &Path) -> Option<(String, Option<PathBuf>)> {
        self.about.entry(path.to_path_buf()).or_insert_with(|| crate::project::Project::load(path).ok().map(|p| (p.name, p.root))).clone()
    }

    /// Reads the project's name and folder again next time, as they may have changed.
    pub fn forget(&mut self, path: &Path) {
        self.about.remove(path);
    }

    /// What a recent project is shown as: its name (a folder's own name, for a folder opened as a project), and
    /// where it is, the folder for a folder and otherwise the project's file.
    pub fn shown_as(&mut self, path: &Path) -> (String, String, bool) {
        let name_of = |path: &Path| path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
        match self.project(path) {
            Some((_, Some(root))) => (name_of(&root), root.display().to_string(), true),
            Some((name, None)) if !name.is_empty() => (name, path.display().to_string(), false),
            _ => (path.file_stem().map_or_else(|| name_of(path), |stem| stem.to_string_lossy().into_owned()), path.display().to_string(), false),
        }
    }

    /// A group's heading: its project's name, or "On their own".
    pub fn group_name(&mut self, group: &Group) -> String {
        match &group.project {
            Some(project) => self.shown_as(project).0,
            None => crate::i18n::tr("On their own").to_string(),
        }
    }
}

impl Limoniter {
    /// The open project, if `file` is one of its files.
    fn project_holding(&self, file: &Path) -> Option<PathBuf> {
        let project = self.project.as_ref()?;
        let inside = project.contains(file) || project.root.as_ref().is_some_and(|root| file.starts_with(root));
        inside.then(|| project.path.clone())
    }

    /// Puts a file at the top of the recent files, noting the project it's being worked on in, if it's one of the
    /// open project's files.
    pub(super) fn note_recent_file(&mut self, file: &Path) {
        panels::note_recent(&mut self.panels.recent, file);
        let project = self.project_holding(file);
        self.recent_in.note(file, project.as_deref());
    }

    /// Opens a recent file, with the project it was in opened first if that's not the one open.
    pub(super) fn open_recent_file(&mut self, file: PathBuf, project: Option<PathBuf>) {
        if let Some(project) = project
            && !self.project.as_ref().is_some_and(|open| same_path(&open.path, &project))
        {
            self.open_project(&project);
        }
        self.open_request = Some(file);
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{app_with, draw};
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn the_files_are_grouped_by_project_newest_group_first() {
        let recent = paths(&["/g/main.s", "/n/notes.md", "/g/gfx.s", "/w/site.html", "/n/list.txt", "/g/sound.s"]);
        let mut projects = FileProjects::default();
        for file in ["/g/main.s", "/g/gfx.s", "/g/sound.s"] {
            projects.note(Path::new(file), Some(Path::new("/g/game.limproj")));
        }
        projects.note(Path::new("/w/site.html"), Some(Path::new("/w/web.limproj")));
        let groups = grouped(&recent, &projects);
        let shown: Vec<(Option<PathBuf>, Vec<PathBuf>)> = groups.into_iter().map(|g| (g.project, g.files)).collect();
        assert_eq!(
            shown,
            [
                (Some(PathBuf::from("/g/game.limproj")), paths(&["/g/main.s", "/g/gfx.s", "/g/sound.s"])),
                (None, paths(&["/n/notes.md", "/n/list.txt"])),
                (Some(PathBuf::from("/w/web.limproj")), paths(&["/w/site.html"])),
            ]
        );
        assert!(grouped(&[], &projects).is_empty());
    }

    #[test]
    fn a_file_belongs_to_the_project_it_was_last_in_and_case_doesnt_matter() {
        let mut projects = FileProjects::default();
        projects.note(Path::new(r"C:\Work\a.s"), Some(Path::new(r"C:\Work\a.limproj")));
        assert_eq!(projects.project_of(Path::new(r"c:\work\A.s")), Some(Path::new(r"C:\Work\a.limproj")));
        projects.note(Path::new(r"C:\Work\a.s"), Some(Path::new(r"C:\Other\b.limproj")));
        assert_eq!(projects.project_of(Path::new(r"C:\Work\a.s")), Some(Path::new(r"C:\Other\b.limproj")));
        projects.note(Path::new(r"C:\Work\a.s"), None);
        assert_eq!(projects.project_of(Path::new(r"C:\Work\a.s")), None, "worked on outside a project, it's on its own");
    }

    #[test]
    fn the_projects_go_through_the_settings_and_old_settings_have_none() {
        let recent = paths(&["/g/main.s", "/n/notes=1.md", "/gone.txt"]);
        let mut projects = FileProjects::default();
        projects.note(Path::new("/g/main.s"), Some(Path::new("/g/game=1.limproj")));
        projects.note(Path::new("/gone.txt"), Some(Path::new("/g/game=1.limproj")));
        // Only the files still on the list are kept.
        let saved = projects.to_settings(&recent[..2]);
        assert_eq!(saved, "recent_file_project=/g/main.s\u{1}/g/game=1.limproj\n");
        assert_eq!(FileProjects::from_settings(&saved).project_of(Path::new("/g/main.s")), Some(Path::new("/g/game=1.limproj")));
        // A settings file from before the projects were kept reads as every file on its own.
        let (old, _) = app_with("recent_file=/g/main.s\nrecent_file=/n/notes.md\nrecent_project=/g/game.limproj\n");
        assert_eq!(old.panels.recent, paths(&["/g/main.s", "/n/notes.md"]));
        assert_eq!(old.recent_in, FileProjects::default());
        assert_eq!(grouped(&old.panels.recent, &old.recent_in), [Group { project: None, files: paths(&["/g/main.s", "/n/notes.md"]) }]);
        // And the whole of the settings carries them, each in its group.
        let (mut app, _) = app_with("");
        app.panels.recent = recent.clone();
        app.recent_in = projects;
        let (again, _) = app_with(&app.current_settings());
        assert_eq!(again.panels.recent, recent);
        assert_eq!(again.recent_in.project_of(Path::new("/g/main.s")), Some(Path::new("/g/game=1.limproj")));
        assert_eq!(again.recent_in.project_of(Path::new("/n/notes=1.md")), None);
        assert_eq!(FileProjects::from_settings("recent_file_project=no split\nrecent_file_project=\u{1}/p\n"), FileProjects::default(), "broken lines are left out");
    }

    fn folder(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-recent-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_file_opened_in_a_project_is_noted_with_it_and_one_outside_on_its_own() {
        let (mut app, _ctx) = app_with("");
        let dir = folder("note");
        let inside = dir.join("game.s");
        let outside = dir.join("notes.md");
        std::fs::write(&inside, "start: rts\n").unwrap();
        std::fs::write(&outside, "notes\n").unwrap();
        app.new_project(dir.join("game"));
        let project = dir.join("game.limproj");
        app.project.as_mut().unwrap().add([inside.clone()]);
        assert!(app.open_path(&inside));
        assert!(app.open_path(&outside), "open while the project is, but not one of its files");
        assert_eq!(app.recent_in.project_of(&inside), Some(project.as_path()));
        assert_eq!(app.recent_in.project_of(&outside), None);
        // Saved after the project's closed, it's been worked on on its own.
        app.close_project();
        let index = app.docs.iter().position(|d| d.path.as_deref() == Some(inside.as_path())).unwrap();
        app.active = index;
        assert!(app.write_to(index, &inside));
        assert_eq!(app.recent_in.project_of(&inside), None);
        drop(app);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_file_picked_from_a_projects_group_opens_the_project_and_then_the_file() {
        let (mut app, ctx) = app_with("");
        let dir = folder("open");
        let file = dir.join("main.s");
        let other = dir.join("other.s");
        std::fs::write(&file, "start: rts\n").unwrap();
        std::fs::write(&other, "loop: bra loop\n").unwrap();
        app.new_project(dir.join("game"));
        let project = dir.join("game.limproj");
        app.project.as_mut().unwrap().add([file.clone(), other.clone()]);
        app.save_project();
        app.close_project();
        app.recent_in.note(&file, Some(&project));
        app.open_recent_file(file.clone(), Some(project.clone()));
        assert_eq!(app.project.as_ref().map(|p| p.path.clone()), Some(project.clone()), "the project opened");
        draw(&mut app, &ctx, Vec::new());
        assert_eq!(app.docs[app.active].path.as_deref(), Some(file.as_path()), "then the file");
        // With the project open already, it isn't opened again (which would close and reopen its files).
        let tabs = app.docs.len();
        app.open_recent_file(other.clone(), Some(project.clone()));
        draw(&mut app, &ctx, Vec::new());
        assert_eq!(app.docs[app.active].path.as_deref(), Some(other.as_path()));
        assert_eq!(app.docs.len(), tabs + 1);
        // A file on its own opens without touching the project.
        app.open_recent_file(file.clone(), None);
        assert_eq!(app.project.as_ref().map(|p| p.path.clone()), Some(project));
        drop(app);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_long_folder_is_cut_short_from_its_start() {
        assert_eq!(shortened("/work/game", 40), "/work/game");
        assert_eq!(shortened("/home/me/work/game", 10), "\u{2026}work/game");
        assert_eq!(shortened("/home/me/work/game", 10).chars().count(), 10);
    }

    /// Every node's name.
    fn names(output: &egui::FullOutput) -> Vec<String> {
        crate::access::tests::nodes(output).into_iter().filter_map(|(_, node)| node.label().or(node.value()).map(str::to_string)).collect()
    }

    /// Which of the `wanted` names are shown, from the top of the window down, each once.
    fn top_down(output: &egui::FullOutput, wanted: &[&str]) -> Vec<String> {
        let mut found: Vec<(f64, String)> = crate::access::tests::nodes(output)
            .into_iter()
            .filter_map(|(_, node)| Some((node.bounds()?.y0, node.label().or(node.value())?.to_string())))
            .filter_map(|(y, name)| wanted.iter().find(|w| name == **w || name.starts_with(&format!("{w} "))).map(|w| (y, w.to_string())))
            .collect();
        found.sort_by(|a, b| a.0.total_cmp(&b.0));
        found.dedup_by(|a, b| a.1 == b.1);
        found.into_iter().map(|(_, name)| name).collect()
    }

    #[test]
    fn file_open_recent_has_the_two_tabs_and_stays_open_to_change_them() {
        let (mut app, ctx) = app_with("");
        ctx.enable_accesskit();
        let dir = folder("menu");
        let root = dir.join("game");
        std::fs::create_dir_all(&root).unwrap();
        let kept = dir.join("game-folder.limproj");
        crate::project::Project::of_folder(&kept, &root).save().unwrap();
        app.recent_projects = vec![kept.clone()];
        app.panels.recent = vec![root.join("main.s"), dir.join("notes.md")];
        app.recent_in.note(&root.join("main.s"), Some(&kept));
        let wanted = vec!["File".to_string(), "Open recent".to_string()];
        ctx.data_mut(|data| data.insert_temp(crate::fit::opened_menus_key(), wanted));
        for _ in 0..6 {
            draw(&mut app, &ctx, Vec::new());
        }
        ctx.data_mut(|data| data.remove::<Vec<String>>(crate::fit::opened_menus_key()));
        let output = draw(&mut app, &ctx, Vec::new());
        let shown = names(&output);
        assert!(shown.iter().any(|name| name.starts_with("game")), "the folder project, by its folder's name: {shown:?}");
        assert!(!shown.iter().any(|name| name.starts_with("main.s")), "the files are on the other tab: {shown:?}");
        // A click on Files changes the list, and the menu stays open.
        let nodes = crate::access::tests::nodes(&output);
        let files = nodes.iter().rev().find(|(_, node)| node.label() == Some("Files") && node.role() != egui::accesskit::Role::Label).and_then(|(_, node)| node.bounds()).expect("a Files tab");
        let at = egui::pos2(((files.x0 + files.x1) / 2.0) as f32, ((files.y0 + files.y1) / 2.0) as f32);
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
            draw(&mut app, &ctx, events);
        }
        let output = draw(&mut app, &ctx, Vec::new());
        assert_eq!(top_down(&output, &["game", "main.s", "On their own", "notes.md"]), ["game", "main.s", "On their own", "notes.md"], "{:?}", names(&output));
        assert_eq!(app.recent_lists.menu_tab, Tab::Files);
        drop(app);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_menu_opens_on_projects_each_time() {
        let mut lists = RecentLists::default();
        assert_eq!(lists.menu_tab(10), Tab::Projects);
        lists.choose_menu_tab(Tab::Files);
        assert_eq!(lists.menu_tab(11), Tab::Files, "kept while it stays open");
        assert_eq!(lists.menu_tab(12), Tab::Files);
        assert_eq!(lists.menu_tab(20), Tab::Projects, "closed and opened again, it's back on Projects");
    }
}
