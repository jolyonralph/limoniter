//! The window's side of file colours: where a file's colour is kept (with the project it's in, or in the settings
//! for a file on its own), the theme each document is drawn in once its tint and its own theme are taken into
//! account, the menu a tab has on a right click, and the rules in the project's settings.

use super::*;
use crate::file_colours::{Colours, Look, Rule, Tint};
use crate::theme::DocumentTheme;
use std::rc::Rc;

/// What's chosen in a tab's right-click menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TabPick {
    Tint(Option<Tint>),
    /// A theme of the file's own, or None for the one the rules give.
    Theme(Option<String>),
    Close,
    /// Every other tab.
    CloseOthers,
    CloseAll,
}

impl Limoniter {
    /// Where a file's colours are kept, and what it's called there: by its place in the project when it's the
    /// open project's (true), or else by its whole path, in the settings.
    fn colour_home(&self, path: &Path) -> (bool, String) {
        if let Some(project) = &self.project {
            let folder = project.folder();
            let inside = if project.root.is_some() { path.starts_with(&folder) } else { project.contains(path) };
            if inside {
                return (true, project::stored(&folder, path));
            }
        }
        (false, crate::file_colours::key_of(path))
    }

    /// The colours a file is kept with, and its name there.
    fn colours_of(&mut self, path: &Path) -> (&mut Colours, String, bool) {
        let (in_project, key) = self.colour_home(path);
        match self.project.as_mut().filter(|_| in_project) {
            Some(project) => (&mut project.colours, key, true),
            None => (&mut self.themes.loose, key, false),
        }
    }

    /// What a file looks like: its tint (its own, or a rule's) and its own theme.
    pub(super) fn file_look(&self, path: &Path) -> Look {
        let (in_project, key) = self.colour_home(path);
        match self.project.as_ref().filter(|_| in_project) {
            Some(project) => project.colours.look(&key),
            None => self.themes.loose.look(&key),
        }
    }

    /// What an open document looks like. One that's never been saved has no colour.
    pub(super) fn doc_look(&self, index: usize) -> Look {
        self.docs.get(index).and_then(|doc| doc.path.as_deref()).map(|path| self.file_look(path)).unwrap_or_default()
    }

    /// The theme chosen for a document alone, by name: for this time only, or kept for its file.
    pub(super) fn own_theme(&self, index: usize) -> Option<String> {
        self.docs.get(index).and_then(|doc| doc.theme_override.clone()).or_else(|| self.doc_look(index).theme)
    }

    /// The theme a document is drawn in: its own if it has one, otherwise the rules', tinted with its colour.
    pub(super) fn document_theme_of(&self, index: usize) -> Rc<DocumentTheme> {
        let Some(doc) = self.docs.get(index) else { return self.themes.documents[0].clone() };
        let look = self.doc_look(index);
        let theme = doc.theme_override.as_deref().or(look.theme.as_deref()).and_then(|name| self.themes.document_named(name)).unwrap_or_else(|| self.themes.document_for(doc));
        match look.tint {
            Some(tint) => Rc::new(crate::file_colours::tinted(&theme, tint)),
            None => theme,
        }
    }

    /// Keeps where it belongs the colour or theme just given a file.
    fn keep_colours(&mut self, in_project: bool) {
        if in_project {
            self.save_project();
        } else {
            self.save_settings_if_changed();
        }
    }

    /// Gives a file a tint of its own, or (with None) leaves it to the rules.
    pub(super) fn set_file_tint(&mut self, path: &Path, tint: Option<Tint>) {
        let (colours, key, in_project) = self.colours_of(path);
        colours.set_tint(&key, tint);
        self.keep_colours(in_project);
    }

    /// Gives a document a theme of its own, kept for its file; or (with None) the theme the rules give it. A
    /// document that's never been saved has it for as long as it's open.
    pub(super) fn set_file_theme(&mut self, index: usize, theme: Option<String>) {
        let Some(path) = self.docs.get(index).and_then(|doc| doc.path.clone()) else {
            if let Some(doc) = self.docs.get_mut(index) {
                doc.theme_override = theme;
            }
            return;
        };
        self.docs[index].theme_override = None;
        let (colours, key, in_project) = self.colours_of(&path);
        colours.set_theme(&key, theme);
        self.keep_colours(in_project);
    }

    /// What's in a tab's right-click menu: its colour, and a theme of its own.
    pub(super) fn tab_menu(&self, ui: &mut Ui, index: usize) -> Option<TabPick> {
        let mut pick = None;
        let has_file = self.docs.get(index).is_some_and(|doc| doc.path.is_some());
        let look = self.doc_look(index);
        ui.add_enabled_ui(has_file, |ui| {
            crate::fit::menu_button(ui, crate::i18n::tr("Colour this file"), |ui| {
                if let Some(tint) = crate::file_colours::tint_menu(ui, look.tint) {
                    pick = Some(TabPick::Tint(tint));
                }
            })
            .response
            .on_disabled_hover_text(crate::i18n::tr("Save the file first: its colour is kept with it"));
        });
        crate::fit::menu_button(ui, crate::i18n::tr("Theme for this file"), |ui| {
            let own = self.own_theme(index);
            if ui.radio(own.is_none(), crate::i18n::tr("As the rules say")).clicked() {
                pick = Some(TabPick::Theme(None));
                ui.close();
            }
            ui.separator();
            for theme in &self.themes.documents {
                if ui.radio(own.as_deref() == Some(theme.name.as_str()), theme.name.as_str()).clicked() {
                    pick = Some(TabPick::Theme(Some(theme.name.clone())));
                    ui.close();
                }
            }
        });
        ui.separator();
        for (label, choice) in [("Close", TabPick::Close), ("Close the others", TabPick::CloseOthers), ("Close all", TabPick::CloseAll)] {
            if ui.button(crate::i18n::tr(label)).clicked() {
                pick = Some(choice);
                ui.close();
            }
        }
        pick
    }

    pub(super) fn tab_pick(&mut self, index: usize, pick: TabPick) {
        match pick {
            TabPick::Tint(tint) => {
                if let Some(path) = self.docs.get(index).and_then(|doc| doc.path.clone()) {
                    self.set_file_tint(&path, tint);
                }
            }
            TabPick::Theme(theme) => self.set_file_theme(index, theme),
            TabPick::Close => self.close_tab(index),
            TabPick::CloseOthers => self.close_all(Some(index)),
            TabPick::CloseAll => self.close_all(None),
        }
        self.pending_focus = true;
    }

    /// Colours every file in a folder of the project, by a rule for the folder; or takes the folder's rule away.
    pub(super) fn colour_folder(&mut self, folder: &Path, tint: Option<Tint>) {
        let Some(project) = self.project.as_mut() else { return };
        let place = project::stored(&project.folder(), folder);
        project.colours.colour_folder(&place, tint);
        self.save_project();
    }

    /// A file or folder of the project renamed or moved: its colours go with it.
    pub(super) fn colours_renamed(&mut self, from: &Path, to: &Path) {
        let Some(project) = self.project.as_mut().filter(|p| !p.colours.files.is_empty()) else { return };
        let base = project.folder();
        let before = project.colours.clone();
        project.colours.renamed(&project::stored(&base, from), &project::stored(&base, to));
        if project.colours != before {
            self.save_project();
        }
    }
}

/// The rules that colour a project's files, in its settings: each a pattern and a tint, to change, take away or
/// add to.
pub(super) fn rules_list(ui: &mut Ui, rules: &mut Vec<Rule>) {
    ui.add_space(4.0);
    ui.label(crate::i18n::tr("Colours for files:"));
    ui.label(RichText::new(crate::i18n::tr("A folder ends with / (admin/); a name or a place in the project can have * in it (*.inc, config/production.php). The first that fits colours a file, unless it has a colour of its own from its tab.")).color(theme::text_faint()).size(12.5));
    let mut gone = None;
    egui::ScrollArea::vertical().id_salt("project-colour-rules").max_height(160.0).show(ui, |ui| {
        for (at, rule) in rules.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut rule.pattern).desired_width(200.0).hint_text(crate::i18n::tr("admin/ or *.inc")));
                crate::file_colours::tint_row(ui, &mut rule.tint);
                if ui.small_button(crate::i18n::tr("Take away")).clicked() {
                    gone = Some(at);
                }
            });
        }
    });
    if let Some(at) = gone {
        rules.remove(at);
    }
    if ui.button(crate::i18n::tr("Add a rule")).on_hover_text(crate::i18n::tr("Or right-click a folder in the Explorer, Colour this folder's files")).clicked() {
        rules.push(Rule { pattern: String::new(), tint: Tint::Blue });
    }
}

/// The rules as they're kept: tidied, and without any left empty.
pub(super) fn tidy_rules(rules: &[Rule]) -> Vec<Rule> {
    rules.iter().map(|rule| Rule::new(&rule.pattern, rule.tint)).filter(|rule| !rule.pattern.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::super::tests::{app_with, draw};
    use super::*;

    fn folder(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-colours-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_lone_file_s_colour_and_theme_are_kept_in_the_settings() {
        let dir = folder("lone");
        let file = dir.join("notes.php");
        std::fs::write(&file, "<?php echo 1;\n").unwrap();
        let (mut app, ctx) = app_with("");
        assert!(app.open_path(&file));
        app.set_file_tint(&file, Some(Tint::Amber));
        app.set_file_theme(app.active, Some("Nord".into()));
        let settings = app.current_settings();
        assert!(settings.contains("file_colour="), "{settings}");
        let (mut again, _) = app_with(&settings);
        assert!(again.open_path(&file));
        assert_eq!(again.doc_look(again.active), Look { tint: Some(Tint::Amber), theme: Some("Nord".into()) });
        // Drawn in its own theme, with the tint behind the text and the syntax colours as Nord's.
        draw(&mut again, &ctx, Vec::new());
        let nord = again.themes.document_named("Nord").unwrap();
        assert_eq!(theme::syntax_scheme(), nord.syntax);
        assert_eq!(theme::editor_text(), nord.editor_text);
        assert_eq!(theme::editor_back(), crate::file_colours::tinted(&nord, Tint::Amber).editor_back);
        assert_ne!(theme::editor_back(), nord.editor_back);
        // Taken away again, nothing's left in the settings.
        again.set_file_tint(&file, None);
        again.set_file_theme(again.active, None);
        assert!(!again.current_settings().contains("file_colour="));
        assert_eq!(again.own_theme(again.active), None);
    }

    #[test]
    fn a_project_keeps_its_files_colours_and_rules_by_their_places_in_it() {
        let dir = folder("project");
        std::fs::create_dir_all(dir.join("admin")).unwrap();
        std::fs::create_dir_all(dir.join("config")).unwrap();
        for name in ["index.php", "admin/users.php", "config/production.php", "config/local.php"] {
            std::fs::write(dir.join(name), "<?php\n").unwrap();
        }
        // The folder's own project file, so nothing is written beside the settings.
        let own = dir.join(format!("{}.{}", dir.file_name().unwrap().to_string_lossy(), project::EXTENSION));
        Project::of_folder(&own, &dir).save().unwrap();
        let (mut app, ctx) = app_with("");
        app.open_folder(&dir);
        let project_file = app.project.as_ref().unwrap().path.clone();
        assert_eq!(project_file, own);
        assert!(app.open_path(&dir.join("index.php")));
        app.set_file_tint(&dir.join("index.php"), Some(Tint::Green));
        app.colour_folder(&dir.join("admin"), Some(Tint::Rose));
        app.project.as_mut().unwrap().colours.rules.push(Rule::new("config/production.php", Tint::Red));
        app.save_project();
        assert!(app.themes.loose.is_empty(), "a project's file is coloured in the project, not in the settings");
        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&project_file).unwrap()).unwrap();
        let kept = value["colours"].to_string();
        assert!(kept.contains("\"index.php\"") && kept.contains("\"admin/\"") && !kept.contains(&*dir.file_name().unwrap().to_string_lossy()), "by places in the project: {kept}");
        let loaded = Project::load(&project_file).unwrap();
        assert_eq!(loaded.colours.look("index.php").tint, Some(Tint::Green));
        assert_eq!(loaded.colours.look("admin/users.php").tint, Some(Tint::Rose));
        assert_eq!(loaded.colours.look("config/production.php").tint, Some(Tint::Red));
        assert_eq!(loaded.colours.look("config/local.php").tint, None);
        // Open again, the files have their colours, and the rules colour files opened later.
        let (mut again, _) = app_with("");
        again.open_folder(&dir);
        assert!(again.open_path(&dir.join("admin/users.php")));
        assert_eq!(again.doc_look(again.active).tint, Some(Tint::Rose));
        assert_eq!(again.file_look(&dir.join("index.php")).tint, Some(Tint::Green));
        assert_eq!(again.file_look(&dir.join("config/production.php")).tint, Some(Tint::Red));
        draw(&mut again, &ctx, Vec::new());
        assert_eq!(theme::editor_back(), crate::file_colours::tinted(&again.themes.document_for(&again.docs[again.active]), Tint::Rose).editor_back);
        // A renamed folder takes its files' colours with it.
        again.set_file_tint(&dir.join("admin/users.php"), Some(Tint::Violet));
        again.colours_renamed(&dir.join("admin"), &dir.join("staff"));
        assert_eq!(again.project.as_ref().unwrap().colours.look("staff/users.php").tint, Some(Tint::Violet));
    }

    #[test]
    fn the_tab_menu_colours_the_file_and_gives_it_a_theme() {
        let dir = folder("tab");
        let file = dir.join("a.php");
        std::fs::write(&file, "<?php\n").unwrap();
        let (mut app, ctx) = app_with("");
        assert!(app.open_path(&file));
        app.tab_pick(app.active, TabPick::Tint(Some(Tint::Blue)));
        assert_eq!(app.doc_look(app.active).tint, Some(Tint::Blue));
        app.tab_pick(app.active, TabPick::Theme(Some("Dracula".into())));
        assert_eq!(app.own_theme(app.active).as_deref(), Some("Dracula"));
        assert_eq!(app.document_theme_of(app.active).name, "Dracula");
        // The menu draws, with its two sub-menus, and a new document can't be coloured until it's saved.
        let _ = ctx.run_ui(Default::default(), |ui| {
            let _ = app.tab_menu(ui, app.active);
        });
        app.new_document();
        app.tab_pick(app.active, TabPick::Theme(Some("Nord".into())));
        assert_eq!(app.docs[app.active].theme_override.as_deref(), Some("Nord"), "for as long as it's open");
        assert_eq!(app.doc_look(app.active), Look::default());
        draw(&mut app, &ctx, Vec::new());
    }

    #[test]
    fn rules_from_the_settings_box_are_tidied() {
        let rules = vec![Rule { pattern: " .\\admin\\ ".into(), tint: Tint::Rose }, Rule { pattern: "  ".into(), tint: Tint::Blue }];
        assert_eq!(tidy_rules(&rules), [Rule::new("admin/", Tint::Rose)]);
    }
}
