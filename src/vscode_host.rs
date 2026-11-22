//! The window's side of reading a folder's `.vscode`: the chooser of debug setups from `launch.json`, the bar that
//! offers the extensions `extensions.json` recommends (once for each project), and opening a `.code-workspace`
//! file as a project. What the files say is `crate::vscode_project`; this only shows it and acts on it.

use super::*;
use crate::vscode_project as vs;

/// What's being shown of it.
#[derive(Default)]
pub(super) struct State {
    /// The debug setup picked in the chooser, while it's open.
    pub chooser: Option<usize>,
    /// The recommended extensions being offered, by id.
    pub offer: Option<Vec<String>>,
}

/// What the chooser says about a setup, under its name.
fn describe(launch: &vs::Launch) -> String {
    let program = if launch.program.is_empty() { crate::i18n::tr("waits for the browser").to_string() } else { launch.program.clone() };
    format!("{} \u{2013} {program}", launch.type_name)
}

impl Limoniter {
    /// Opens the chooser of the debug setups `launch.json` has.
    pub(super) fn choose_vscode_launch(&mut self) {
        if self.project.as_ref().is_some_and(|p| !p.vscode.launches.is_empty()) {
            self.vscode.chooser = Some(0);
        }
    }

    /// Whether the open project has debug setups from `launch.json`.
    pub(super) fn has_vscode_launches(&self) -> bool {
        self.project.as_ref().is_some_and(|p| !p.vscode.launches.is_empty())
    }

    /// Starts debugging with a setup of `launch.json`, filled in for the project's folder and the file in front.
    pub(super) fn start_vscode_launch(&mut self, index: usize) {
        let Some(project) = &self.project else { return };
        let Some(launch) = project.vscode.launches.get(index).cloned() else { return };
        let file = if self.no_file() { None } else { self.docs.get(self.active).and_then(|d| d.path.clone()) };
        if launch.needs_file() && file.is_none() {
            return self.message("Debug", crate::i18n::tr("This setup debugs the file in front, so save it first, or open one.").into());
        }
        let folder = project.folder();
        if let Some(path) = &file
            && self.docs[self.active].is_modified()
            && !self.save(self.active)
        {
            let _ = path;
            return;
        }
        let config = launch.to_config(&folder, file.as_deref());
        self.launch_debugger(config);
    }

    /// The chooser: the setups listed with what each runs, and Debug to start the one picked.
    pub(super) fn vscode_launch_window(&mut self, ctx: &Context) {
        let Some(mut chosen) = self.vscode.chooser else { return };
        let Some(project) = &self.project else {
            self.vscode.chooser = None;
            return;
        };
        let launches = project.vscode.launches.clone();
        let (mut go, mut close) = (false, ctx.input(|i| i.key_pressed(egui::Key::Escape)));
        crate::fit::Dialog::new(crate::i18n::tr("Debug with a VS Code setup")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(crate::i18n::tr("The setups in this folder's .vscode/launch.json:"));
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                for (place, launch) in launches.iter().enumerate() {
                    if ui.radio(chosen == place, &launch.name).on_hover_text(describe(launch)).clicked() {
                        chosen = place;
                    }
                    ui.label(RichText::new(describe(launch)).color(theme::text_faint()).size(12.0));
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                go = ui.button(crate::i18n::tr("Debug")).clicked();
                close |= ui.button(crate::i18n::tr("Cancel")).clicked();
            });
        });
        self.vscode.chooser = Some(chosen);
        if go {
            self.vscode.chooser = None;
            self.start_vscode_launch(chosen);
        } else if close {
            self.vscode.chooser = None;
        }
    }

    /// Once for each project, the extensions its `.vscode/extensions.json` recommends are offered in a bar.
    fn consider_vscode_offer(&mut self) {
        if self.vscode.offer.is_some() {
            return;
        }
        let Some(project) = self.project.as_mut().filter(|p| p.root.is_some() && !p.vscode_offered && !p.vscode.recommendations.is_empty()) else { return };
        project.vscode_offered = true;
        self.vscode.offer = Some(project.vscode.recommendations.clone());
        self.save_project();
    }

    /// The language servers among the recommendations that Limoniter knows and that aren't installed yet, with
    /// the command that installs each.
    fn servers_to_install(ids: &[String]) -> Vec<(String, String)> {
        let mut found: Vec<(String, String)> = Vec::new();
        for (_, program) in ids.iter().filter_map(|id| vs::server_for(id)) {
            if !crate::lsp::on_path(program)
                && let Some((command, _)) = crate::lsp::install_command(program)
                && !found.iter().any(|(p, _)| p == program)
            {
                found.push((program.to_string(), command));
            }
        }
        found
    }

    /// The recommendations that are left for Open VSX: those no language server here stands for, and that VS Code
    /// alone can use are left out.
    fn extensions_to_look_for(ids: &[String]) -> Vec<String> {
        ids.iter().filter(|id| vs::server_for(id).is_none() && !vs::only_for_vs_code(id)).cloned().collect()
    }

    /// The bar offering what the project recommends.
    pub(super) fn vscode_offer_bar(&mut self, ui: &mut Ui) {
        self.consider_vscode_offer();
        let Some(ids) = self.vscode.offer.clone() else { return };
        let servers = Self::servers_to_install(&ids);
        let others = Self::extensions_to_look_for(&ids);
        let (mut install, mut look, mut later) = (false, false, false);
        Panel::top("vscode-offer").frame(egui::Frame::new().fill(theme::selected()).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let names = ids.join(", ");
                ui.label(RichText::new(crate::i18n::tf!("This project recommends VS Code extensions: {names}.")).strong()).on_hover_text(crate::i18n::tr("From its .vscode/extensions.json. Limoniter has language servers for some, and can install themes, grammars and snippets from Open VSX."));
                if !servers.is_empty() {
                    let programs = servers.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>().join(", ");
                    install = ui.button(crate::i18n::tf!("Install {programs} in a terminal", programs = programs)).on_hover_text(servers.iter().map(|(_, c)| c.as_str()).collect::<Vec<_>>().join("\n")).clicked();
                }
                if !others.is_empty() {
                    look = ui.button(crate::i18n::tr("Look for them on Open VSX\u{2026}")).clicked();
                }
                later = ui.button(crate::i18n::tr("Not now")).clicked();
            });
        });
        if install {
            self.new_terminal(None);
            if let Some(terminal) = self.terminal.as_mut() {
                for (_, command) in &servers {
                    terminal.send(format!("{command}\r").as_bytes());
                }
            }
        }
        if look {
            self.open_extensions();
            if let Dialog::Extensions(state) = &mut self.dialog {
                state.query = others[0].clone();
                state.status = crate::i18n::tf!("The project recommends: {}", others.join(", "));
            }
        }
        if install || look || later {
            self.vscode.offer = None;
        }
    }

    /// Opens a `.code-workspace` file. A project here has one folder, so the first of the workspace's folders is
    /// opened as the project, with the workspace's own settings, and the others are named in a message.
    pub(super) fn open_code_workspace(&mut self, file: &Path) -> bool {
        let text = match std::fs::read_to_string(file) {
            Ok(text) => text,
            Err(e) => {
                self.message("Open", crate::i18n::tf!("Couldn't open {}.\n\n{e}", file.display()));
                return true;
            }
        };
        let folders = vs::workspace_folders(&text, file);
        let Some((_, first)) = folders.first().cloned() else {
            self.message("Open", crate::i18n::tf!("{} names no folder that exists, so there's nothing to open.", file.display()));
            return true;
        };
        self.open_folder(&first);
        let file = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
        if let Some(project) = self.project.as_mut().filter(|p| p.root.as_deref() == Some(first.as_path())) {
            project.workspace_file = Some(file);
            project.read_vscode();
            self.save_project();
            self.apply_tab_size();
        }
        if folders.len() > 1 {
            let others = folders[1..].iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(", ");
            self.message("Open", crate::i18n::tf!("A project here has one folder, so only {} was opened, with the workspace's settings. These weren't opened: {others}.", folders[0].0));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{app_with, command, draw};
    use super::*;
    use super::super::project_settings_host::Draft;

    fn folder(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-vscode-host-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".vscode")).unwrap();
        std::path::absolute(&dir).unwrap()
    }

    fn write(root: &Path, name: &str, text: &str) {
        std::fs::write(root.join(".vscode").join(name), text).unwrap();
    }

    /// Opens a folder as a project and gives back the kept project file, to take away afterwards.
    fn open(app: &mut Limoniter, root: &Path) -> PathBuf {
        app.open_folder(root);
        app.project.as_ref().unwrap().path.clone()
    }

    fn change(app: &mut Limoniter, edit: impl FnOnce(&mut Draft)) {
        app.open_project_settings();
        let Dialog::ProjectSettings(mut draft) = std::mem::replace(&mut app.dialog, Dialog::None) else { panic!("the box opened") };
        edit(&mut draft);
        app.apply_project_settings(*draft);
    }

    const SETTINGS: &str = r#"{
  // as VS Code writes it
  "editor.tabSize": 2,
  "editor.insertSpaces": false,
  "files.trimTrailingWhitespace": true,
  "files.exclude": { "**/dist": true },
  "files.autoSave": "onWindowChange",
  "cSpell.words": ["limoniter"],
}
"#;

    #[test]
    fn the_projects_own_settings_win_and_vscode_fills_in_what_they_leave_unsaid() {
        let (mut app, _ctx) = app_with("");
        let root = folder("precedence");
        write(&root, "settings.json", SETTINGS);
        let kept = open(&mut app, &root);
        // Only VS Code says: its values are in effect, and the person's settings stand under them.
        assert_eq!(app.tab_size_now(), 2);
        assert_eq!(app.autosave_mode(), Some(crate::settings::AutosaveWhen::OnWindowChange));
        assert!(app.project.as_ref().unwrap().prefs.is_empty(), "nothing of VS Code's is written into the project");
        // The project says something of its own: that wins, and the rest still comes from VS Code.
        change(&mut app, |draft| {
            draft.prefs.tab_width = Some(6);
            draft.autosave = Some(crate::settings::AutosaveChoice::Off);
        });
        assert_eq!(app.tab_size_now(), 6);
        assert_eq!(app.autosave_mode(), None);
        let project = app.project.as_ref().unwrap();
        assert_eq!(project.eff().spaces, Some(false));
        assert_eq!(project.eff().trim_trailing, Some(true));
        assert_eq!(project.eff().search_exclude, vec!["**/dist".to_string()]);
        // An .editorconfig-style say from the project's own file is not touched by reading .vscode.
        assert_eq!(project.prefs.spaces, None);
        // A change made to the file is followed when the project is opened again.
        write(&root, "settings.json", r#"{ "editor.tabSize": 4 }"#);
        app.close_project();
        app.open_project(&kept);
        assert_eq!(app.project.as_ref().unwrap().eff().tab_width, Some(6));
        assert_eq!(app.project.as_ref().unwrap().vscode.prefs.tab_width, Some(4));
        app.close_project();
        let _ = std::fs::remove_file(kept);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn files_excluded_in_vscode_are_left_out_of_search_and_the_index() {
        let (mut app, _ctx) = app_with("");
        let root = folder("exclude");
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::write(root.join("dist").join("a.js"), "x").unwrap();
        std::fs::write(root.join("main.js"), "x").unwrap();
        write(&root, "settings.json", SETTINGS);
        let kept = open(&mut app, &root);
        let found = app.project.as_ref().unwrap().eff().index_options();
        let (files, _) = crate::workspace::index_with(&root, &[], &found);
        let names: Vec<String> = files.iter().map(|f| crate::explorer::relative(&root, f)).collect();
        assert!(names.contains(&"main.js".to_string()), "{names:?}");
        assert!(!names.iter().any(|n| n.contains("dist")), "{names:?}");
        app.close_project();
        let _ = std::fs::remove_file(kept);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn import_copies_the_settings_tasks_and_words_in_and_stops_reading_them() {
        let (mut app, _ctx) = app_with("");
        let root = folder("import");
        write(&root, "settings.json", SETTINGS);
        write(&root, "tasks.json", r#"{ "version": "2.0.0", "tasks": [{ "label": "make it", "type": "shell", "command": "make", "group": "build" }] }"#);
        let kept = open(&mut app, &root);
        assert!(app.tasks_here().iter().any(|t| t.name == "make it (VS Code)" && t.command == "make"));
        // The box says what comes from .vscode, and Import copies it in once OK is pressed.
        app.open_project_settings();
        {
            let Dialog::ProjectSettings(draft) = &mut app.dialog else { panic!() };
            assert!(draft.vscode.as_ref().is_some_and(|r| !r.shown.is_empty()));
            draft.tab = 0;
        }
        let ctx = Context::default();
        draw(&mut app, &ctx, Vec::new());
        let Dialog::ProjectSettings(mut draft) = std::mem::replace(&mut app.dialog, Dialog::None) else { panic!() };
        draft.import_vscode();
        app.apply_project_settings(*draft);
        let project = app.project.as_ref().unwrap();
        assert_eq!((project.prefs.tab_width, project.prefs.spaces, project.prefs.trim_trailing), (Some(2), Some(false), Some(true)));
        assert_eq!(project.prefs.search_exclude, vec!["**/dist".to_string()]);
        assert_eq!(project.autosave, Some(crate::settings::AutosaveChoice::When(crate::settings::AutosaveWhen::OnWindowChange)));
        assert_eq!(project.prefs.follow_vscode, Some(false));
        assert!(project.tasks.iter().any(|t| t.name == "make it" && t.kind == crate::tasks::Kind::Build));
        assert!(project.vscode.prefs.is_empty() && project.vscode.tasks.is_empty(), "no longer read");
        assert_eq!(std::fs::read_to_string(crate::vscode_project::words_file(&root)).unwrap(), "limoniter\n");
        // So a change to VS Code's file no longer reaches the project, and the task isn't listed twice.
        write(&root, "settings.json", r#"{ "editor.tabSize": 8 }"#);
        app.close_project();
        app.open_project(&kept);
        assert_eq!(app.tab_size_now(), 2);
        assert_eq!(app.tasks_here().iter().filter(|t| t.command == "make").count(), 1);
        // It's kept in the folder for Git, with the setting that says so.
        let written = std::fs::read_to_string(crate::project_prefs::file_in(&root)).unwrap();
        assert!(written.contains("\"follow_vscode\": false") && written.contains("\"tab_width\": 2"), "{written}");
        app.close_project();
        let _ = std::fs::remove_file(kept);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_launch_setup_is_chosen_from_the_menu_and_is_only_offered_when_there_are_some() {
        let (mut app, ctx) = app_with("");
        let root = folder("launch");
        write(&root, "launch.json", r#"{ "configurations": [ { "name": "Run it", "type": "python", "request": "launch", "program": "${file}" }, { "name": "Node", "type": "node", "request": "launch", "program": "${workspaceFolder}/a.js" } ] }"#);
        assert!(!app.can(Command::DebugVscodeSetup), "no project");
        let kept = open(&mut app, &root);
        assert!(app.can(Command::DebugVscodeSetup));
        command(&mut app, &ctx, Command::DebugVscodeSetup);
        assert_eq!(app.vscode.chooser, Some(0));
        draw(&mut app, &ctx, Vec::new());
        // The setup picked is made into the debugger's configuration for the file in front.
        let launch = app.project.as_ref().unwrap().vscode.launches[1].clone();
        let config = launch.to_config(&root, None);
        // Paths compared with / throughout, as Windows mixes \ and /.
        let slashed = |s: &str| s.replace('\\', "/");
        assert_eq!((config.kind, slashed(&config.program)), (crate::dap::Kind::JsDebug, slashed(root.join("a.js").to_str().unwrap())));
        // One that needs a file says so when none is saved.
        app.vscode.chooser = None;
        app.start_vscode_launch(0);
        assert!(app.debugging.is_none());
        app.close_project();
        let _ = std::fs::remove_file(kept);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_recommended_extensions_are_offered_once_for_each_project() {
        let (mut app, ctx) = app_with("");
        let root = folder("offer");
        write(&root, "extensions.json", r#"{ "recommendations": ["rust-lang.rust-analyzer", "pkief.material-icon-theme", "ms-vscode-remote.remote-ssh"] }"#);
        let kept = open(&mut app, &root);
        draw(&mut app, &ctx, Vec::new());
        let offered = app.vscode.offer.clone().expect("offered");
        assert_eq!(offered.len(), 3);
        // What's left for Open VSX leaves out the server Limoniter has and what only VS Code can use.
        assert_eq!(Limoniter::extensions_to_look_for(&offered), vec!["pkief.material-icon-theme".to_string()]);
        // Dismissed, it isn't offered again, not even when the project is opened afresh.
        app.vscode.offer = None;
        draw(&mut app, &ctx, Vec::new());
        assert!(app.vscode.offer.is_none());
        app.close_project();
        app.open_project(&kept);
        draw(&mut app, &ctx, Vec::new());
        assert!(app.vscode.offer.is_none(), "once for each project");
        app.close_project();
        let _ = std::fs::remove_file(kept);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_code_workspace_opens_its_first_folder_with_its_settings_and_names_the_rest() {
        let (mut app, _ctx) = app_with("");
        let root = folder("workspace");
        std::fs::create_dir_all(root.join("api").join(".vscode")).unwrap();
        std::fs::create_dir_all(root.join("web")).unwrap();
        std::fs::write(root.join("api").join(".vscode").join("settings.json"), r#"{ "editor.tabSize": 2 }"#).unwrap();
        let file = root.join("both.code-workspace");
        std::fs::write(&file, r#"{ "folders": [ { "path": "api" }, { "path": "web" } ], "settings": { "editor.insertSpaces": false }, // comment
 }"#).unwrap();
        assert!(app.open_path(&file));
        let project = app.project.as_ref().expect("the first folder is a project");
        assert_eq!(project.folder(), root.join("api"));
        assert_eq!(project.workspace_file.as_deref(), Some(file.as_path()));
        assert_eq!((project.eff().tab_width, project.eff().spaces), (Some(2), Some(false)));
        // The others weren't opened, and the message says so.
        assert!(matches!(&app.dialog, Dialog::Message { body, .. } if body.contains("web")), "told which weren't opened");
        // Kept with the project, so opening it again from the recent list still has the workspace's settings.
        let kept = project.path.clone();
        app.close_project();
        app.open_project(&kept);
        assert_eq!(app.project.as_ref().unwrap().eff().spaces, Some(false));
        app.close_project();
        let _ = std::fs::remove_file(kept);
        let _ = std::fs::remove_dir_all(&root);
    }
}
