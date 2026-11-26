//! The window's side of the Project settings box: tabs down the left for everything a project keeps (its name and
//! notes, what's hidden and left out of search, how files are edited and saved, what runs and how, which language
//! servers it uses, and how it looks), and the apply step that puts what was changed back in the project. The
//! values themselves are `crate::project_prefs`, `explorer_kept`, `file_colours` and the project's own fields: this
//! only draws them and writes them back, so there's no second copy of anything.

use super::*;
use crate::explorer_kept::Hide;
use crate::project_prefs::{Prefs, lines_of};

/// The tabs, by the words they're shown with.
pub(super) const TABS: [&str; 7] = ["General", "Files and folders", "Editing", "Saving", "Running", "Languages", "Look"];

/// What the settings in the person's own Settings are, so each "As in Settings" can say what that means.
#[derive(Clone, Debug, Default)]
pub(super) struct Own {
    pub spaces: bool,
    pub tab_width: usize,
    pub trim: bool,
    pub format_on_save: bool,
    pub preset: &'static str,
}

/// A project's settings as they're being changed: nothing is kept until OK.
pub(super) struct Draft {
    pub tab: usize,
    pub folder_project: bool,
    pub name: String,
    pub notes: String,
    /// What's hidden by folder, name or kind (the patterns are in `hide_patterns`).
    pub hidden: Vec<Hide>,
    /// Patterns hidden from the Explorer, one to a line.
    pub hide_patterns: String,
    pub show_hidden: bool,
    /// Patterns left out of search and Go to file, one to a line.
    pub search_exclude: String,
    pub follow_gitignore: bool,
    pub recency: u32,
    /// What the recency slider always shows, one to a line.
    pub always: String,
    pub prefs: Prefs,
    pub autosave: Option<crate::settings::AutosaveChoice>,
    pub colours: Vec<crate::file_colours::Rule>,
    /// The Python chosen for the project's folder, and those found there to choose from.
    pub python: Option<PathBuf>,
    pub envs: Vec<crate::py_env::Env>,
    /// The language servers there are: the language and the command.
    pub servers: Vec<(String, String)>,
    pub own: Own,
    /// Something to do once these are kept: Tools, Tasks.
    pub then_tasks: bool,
    /// What the folder's `.vscode` says, to show what's taken from it; and whether Import into Limoniter has
    /// been pressed, which copies its tasks and words in when these are kept.
    pub vscode: Option<crate::vscode_project::Read>,
    pub imported: bool,
}

/// What the box asks for.
#[derive(PartialEq, Eq, Debug)]
pub(super) enum Result {
    Open,
    Cancel,
    Ok,
}

/// Which formatters can be chosen, by language.
fn formatter_languages() -> Vec<&'static str> {
    let mut languages: Vec<&str> = crate::ext_formatters::ALL.iter().flat_map(|f| f.languages.iter().copied()).collect();
    languages.sort_unstable();
    languages.dedup();
    languages
}

/// A choice of on, off, or as in Settings, for a value the project may leave alone.
fn three_way(ui: &mut Ui, label: &str, value: &mut Option<bool>, own: bool, tip: &str) {
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr(label)).on_hover_text(crate::i18n::tr(tip));
        let shown = match value {
            None => crate::i18n::tf!("As in Settings ({})", if own { crate::i18n::tr("on") } else { crate::i18n::tr("off") }),
            Some(true) => crate::i18n::tr("On").to_string(),
            Some(false) => crate::i18n::tr("Off").to_string(),
        };
        egui::ComboBox::from_id_salt(label).selected_text(shown).show_ui(ui, |ui| {
            ui.selectable_value(value, None, crate::i18n::tr("As in Settings"));
            ui.selectable_value(value, Some(true), crate::i18n::tr("On"));
            ui.selectable_value(value, Some(false), crate::i18n::tr("Off"));
        });
    });
}

fn note(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(crate::i18n::tr(text)).color(theme::text_faint()).size(12.5));
}

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(crate::i18n::tr(text)).color(theme::text_dim()));
}

/// A line of text for a place in the project, with a button to choose it, kept as the person wrote it.
fn place_row(ui: &mut Ui, label: &str, hint: &str, value: &mut Option<String>, base: &Path, folder: bool) {
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr(label));
        let mut text = value.clone().unwrap_or_default();
        if ui.add(egui::TextEdit::singleline(&mut text).hint_text(crate::i18n::tr(hint)).desired_width(240.0)).changed() {
            *value = Some(text.trim().to_string()).filter(|t| !t.is_empty());
        }
        if ui.button(crate::i18n::tr("Choose\u{2026}")).clicked() {
            let picker = crate::sys_dialogs::file_dialog().set_directory(base);
            let chosen = if folder { picker.pick_folder() } else { picker.pick_file() };
            if let Some(chosen) = chosen {
                *value = Some(project::stored(base, &chosen));
            }
        }
        if value.is_some() && ui.button(crate::i18n::tr("As in Settings")).clicked() {
            *value = None;
        }
    });
}

fn patterns_box(ui: &mut Ui, id: &str, text: &mut String) {
    ui.add(egui::TextEdit::multiline(text).id_salt(id).desired_rows(4).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
}

impl Draft {
    fn general(&mut self, ui: &mut Ui) {
        ui.label(crate::i18n::tr("Name:"));
        ui.text_edit_singleline(&mut self.name);
        ui.add_space(4.0);
        ui.label(crate::i18n::tr("Notes:"));
        ui.add(egui::TextEdit::multiline(&mut self.notes).desired_rows(8).desired_width(f32::INFINITY));
        self.show_vscode(ui);
    }

    /// What the folder's `.vscode` gives the project, and the button that makes it the project's own.
    fn show_vscode(&mut self, ui: &mut Ui) {
        let Some(read) = &self.vscode else { return };
        heading(ui, "From VS Code");
        if self.prefs.follow_vscode == Some(false) && !self.imported {
            note(ui, "This project has its own copy of VS Code's settings and tasks, and doesn't read the .vscode folder's for them any more.");
            if ui.button(crate::i18n::tr("Follow the .vscode folder again")).clicked() {
                self.prefs.follow_vscode = None;
            }
        } else {
            note(ui, "The .vscode folder's files are followed as they change, and the settings above win where both say something.");
            for (label, value) in &read.shown {
                ui.label(RichText::new(format!("{}: {value}", crate::i18n::tr(label))).size(12.5));
            }
            if !read.tasks.is_empty() {
                ui.label(RichText::new(crate::i18n::tf!("{} tasks, in Tools, Tasks", read.tasks.len())).size(12.5));
            }
            if !read.launches.is_empty() {
                ui.label(RichText::new(crate::i18n::tf!("{} debug setups, in Run, Debug with a VS Code setup", read.launches.len())).size(12.5));
            }
            if self.imported {
                note(ui, "Copied in. Press OK to keep them as the project's own.");
            } else if read.has_something_to_import() && ui.button(crate::i18n::tr("Import into Limoniter")).on_hover_text(crate::i18n::tr("Copies these settings and tasks into the project, where they stay as they are now. The .vscode folder's settings and tasks are no longer read for it afterwards.")).clicked() {
                self.import_vscode();
            }
        }
    }

    /// Copies what `.vscode` says into the draft, under what the project already said.
    pub(super) fn import_vscode(&mut self) {
        let Some(read) = self.vscode.clone() else { return };
        self.prefs = self.prefs.over(&read.prefs);
        if lines_of(&self.search_exclude).is_empty() {
            self.search_exclude = read.prefs.search_exclude.iter().map(|p| p.clone() + "\n").collect();
        }
        if self.autosave.is_none() {
            self.autosave = read.autosave;
        }
        self.prefs.follow_vscode = Some(false);
        self.imported = true;
    }

    fn files(&mut self, ui: &mut Ui) {
        if self.folder_project {
            workspace_host::hidden_list(ui, &mut self.hidden);
            ui.checkbox(&mut self.show_hidden, crate::i18n::tr("Show hidden items")).on_hover_text(crate::i18n::tr("What's hidden is shown faintly in the Explorer, to be found and shown again"));
            heading(ui, "Hidden by pattern");
            note(ui, "One pattern to a line, written as in a .gitignore file: *.log, build/, docs/**/*.tmp. They're hidden from the Explorer, Go to file and searching.");
            patterns_box(ui, "project-hide-patterns", &mut self.hide_patterns);
        }
        heading(ui, "Left out of searching");
        note(ui, "The same kind of patterns, for files that stay in the Explorer but are left out of Go to file and searching, such as generated or minified files.");
        patterns_box(ui, "project-search-exclude", &mut self.search_exclude);
        if self.folder_project {
            ui.add_space(6.0);
            ui.checkbox(&mut self.follow_gitignore, crate::i18n::tr("Leave out what .gitignore leaves out")).on_hover_text(crate::i18n::tr("Go to file and searching skip the files Git ignores. Turn it off to find them too."));
        }
        heading(ui, "Recently edited files");
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Share of the files the Explorer shows, the most recently edited:"));
            ui.add(egui::Slider::new(&mut self.recency, 1..=100).suffix("%"));
        });
        note(ui, "100% shows every file.");
        heading(ui, "Always shown");
        note(ui, "Files, folders and patterns the slider never hides, one to a line, written as in a .gitignore file: README.md, src/main.rs, docs/, *.toml. They're shown, and counted as shown, without taking a place among the newest. A right-click on a file or folder in the Explorer or the Project panel, Always show, adds it or takes it off.");
        patterns_box(ui, "project-always-shown", &mut self.always);
    }

    fn editing(&mut self, ui: &mut Ui) {
        note(ui, "These are this project's own. Where they say \"As in Settings\", your own settings stand. A file's .editorconfig still has the last word for the files it covers.");
        heading(ui, "Indenting");
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("The Tab key makes"));
            let shown = match self.prefs.spaces {
                None => crate::i18n::tf!("As in Settings ({})", if self.own.spaces { crate::i18n::tr("spaces") } else { crate::i18n::tr("tabs") }),
                Some(true) => crate::i18n::tr("Spaces").to_string(),
                Some(false) => crate::i18n::tr("Tab characters").to_string(),
            };
            egui::ComboBox::from_id_salt("project-spaces").selected_text(shown).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.prefs.spaces, None, crate::i18n::tr("As in Settings"));
                ui.selectable_value(&mut self.prefs.spaces, Some(true), crate::i18n::tr("Spaces"));
                ui.selectable_value(&mut self.prefs.spaces, Some(false), crate::i18n::tr("Tab characters"));
            });
        });
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Tab width:"));
            let mut own = self.prefs.tab_width.is_none();
            if ui.checkbox(&mut own, crate::i18n::tf!("As in Settings ({})", self.own.tab_width)).changed() {
                self.prefs.tab_width = if own { None } else { Some(self.own.tab_width.max(1)) };
            }
            if let Some(width) = &mut self.prefs.tab_width {
                ui.add(egui::DragValue::new(width).range(crate::text_buffer::MIN_TAB_SIZE..=crate::text_buffer::MAX_TAB_SIZE));
            }
        });
        heading(ui, "New files");
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Line endings and encoding:"));
            let shown = self.prefs.new_file_preset.as_deref().and_then(codec::preset_by_id).map_or_else(|| crate::i18n::tf!("As in Settings ({})", self.own.preset), |p| p.name.to_string());
            egui::ComboBox::from_id_salt("project-preset").selected_text(shown).width(260.0).show_ui(ui, |ui| {
                if ui.selectable_label(self.prefs.new_file_preset.is_none(), crate::i18n::tr("As in Settings")).clicked() {
                    self.prefs.new_file_preset = None;
                }
                for preset in &codec::PRESETS {
                    if ui.selectable_label(self.prefs.new_file_preset.as_deref() == Some(preset.id), preset.name).on_hover_text(crate::i18n::tr(preset.notes)).clicked() {
                        self.prefs.new_file_preset = Some(preset.id.to_string());
                    }
                }
            });
        });
        heading(ui, "Saving a file");
        three_way(ui, "Take trailing spaces off", &mut self.prefs.trim_trailing, self.own.trim, "As the file is saved, as .editorconfig's trim_trailing_whitespace does");
        three_way(ui, "End with a line ending", &mut self.prefs.final_newline, true, "Put a line ending at the end of the file as it's saved, as .editorconfig's insert_final_newline does");
        three_way(ui, "Format files as they're saved", &mut self.prefs.format_on_save, self.own.format_on_save, "By the language server, or the formatter chosen below, where there is one");
        heading(ui, "Formatters");
        note(ui, "Format document uses these where no language server formats the file. Only those installed can be chosen.");
        egui::Grid::new("project-formatters").num_columns(2).striped(true).show(ui, |ui| {
            for language in formatter_languages() {
                ui.label(crate::i18n::tr(language));
                let chosen = self.prefs.formatters.get(language).cloned();
                let shown = chosen.clone().map_or_else(|| crate::i18n::tr("As in Settings").to_string(), |name| if name == "none" { crate::i18n::tr("None").to_string() } else { name });
                egui::ComboBox::from_id_salt(("project-formatter", language)).selected_text(shown).width(200.0).show_ui(ui, |ui| {
                    if ui.selectable_label(chosen.is_none(), crate::i18n::tr("As in Settings")).clicked() {
                        self.prefs.formatters.remove(language);
                    }
                    if ui.selectable_label(chosen.as_deref() == Some("none"), crate::i18n::tr("None")).clicked() {
                        self.prefs.formatters.insert(language.to_string(), "none".to_string());
                    }
                    for formatter in crate::ext_formatters::for_language(language) {
                        let installed = crate::ext_formatters::installed(formatter);
                        if ui.add_enabled(installed, egui::Button::selectable(chosen.as_deref() == Some(formatter.name), formatter.name)).on_disabled_hover_text(crate::i18n::tf!("{} isn't installed", formatter.program)).clicked() {
                            self.prefs.formatters.insert(language.to_string(), formatter.name.to_string());
                        }
                    }
                });
                ui.end_row();
            }
        });
    }

    fn saving(&mut self, ui: &mut Ui) {
        ui.label(crate::i18n::tr("Save files by themselves:"));
        let mut choices: Vec<(Option<crate::settings::AutosaveChoice>, &str)> = vec![(None, "As in the settings"), (Some(crate::settings::AutosaveChoice::Off), "Not by themselves")];
        choices.extend(crate::settings::AutosaveWhen::ALL.into_iter().map(|when| (Some(crate::settings::AutosaveChoice::When(when)), when.label())));
        for (choice, label) in choices {
            ui.radio_value(&mut self.autosave, choice, crate::i18n::tr(label));
        }
    }

    fn running(&mut self, ui: &mut Ui, base: &Path) {
        place_row(ui, "The .env file:", ".env in the project's folder", &mut self.prefs.env_file, base, false);
        note(ui, "Its values are given to the commands and tasks run in the project.");
        ui.add_space(6.0);
        place_row(ui, "The folder tasks run in:", "The project's folder", &mut self.prefs.work_folder, base, true);
        heading(ui, "Python");
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Python environment:"));
            let shown = self.python.as_ref().map_or_else(|| crate::i18n::tr("The project's own, found for it").to_string(), |python| self.envs.iter().find(|e| e.python == *python).map_or_else(|| python.display().to_string(), crate::py_env::Env::status_words));
            egui::ComboBox::from_id_salt("project-python").selected_text(shown).width(300.0).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.python, None, crate::i18n::tr("The project's own, found for it"));
                for env in &self.envs {
                    ui.selectable_value(&mut self.python, Some(env.python.clone()), env.status_words()).on_hover_text(env.python.display().to_string());
                }
            });
        });
        note(ui, "Kept with your other settings, for this folder, as the Python choice in the status bar is.");
        heading(ui, "Tasks");
        ui.horizontal(|ui| {
            if ui.button(crate::i18n::tr("Edit tasks\u{2026}")).on_hover_text(crate::i18n::tr("Keeps these settings first, then opens the project's tasks")).clicked() {
                self.then_tasks = true;
            }
        });
    }

    fn languages(&mut self, ui: &mut Ui) {
        note(ui, "Which language servers run for this project. Where one is off here, its language gets no completions, hints or problems from a server. Servers are also switched on in Settings, Editing, Language servers.");
        ui.add_space(4.0);
        egui::Grid::new("project-servers").num_columns(2).striped(true).show(ui, |ui| {
            for (language, command) in &self.servers {
                let mut on = !self.prefs.server_off(language);
                if ui.checkbox(&mut on, crate::i18n::tr(language)).changed() {
                    self.prefs.servers_off.retain(|l| l != language);
                    if !on {
                        self.prefs.servers_off.push(language.clone());
                    }
                }
                ui.label(RichText::new(command).color(theme::text_faint()));
                ui.end_row();
            }
        });
    }

    fn look(&mut self, ui: &mut Ui, base: &Path) {
        file_colours_host::rules_list(ui, &mut self.colours);
        heading(ui, "File templates");
        place_row(ui, "The templates folder:", "templates beside the project", &mut self.prefs.templates_folder, base, true);
    }
}

/// Draws the box.
pub(super) fn window(ctx: &Context, draft: &mut Draft, base: &Path) -> Result {
    let mut result = if ctx.input(|i| i.key_pressed(egui::Key::Escape)) { Result::Cancel } else { Result::Open };
    crate::fit::Dialog::new(crate::i18n::tr("Project settings")).collapsible(false).resizable(true).default_size(egui::vec2(720.0, 440.0)).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show_with_buttons(ctx, |ui, part| match part {
        crate::fit::Part::Body => {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(140.0);
                    for (place, name) in TABS.iter().enumerate() {
                        if ui.add(Button::selectable(draft.tab == place, crate::i18n::tr(name)).frame_when_inactive(false).min_size(egui::vec2(140.0, 0.0))).clicked() {
                            draft.tab = place;
                        }
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_min_width(480.0);
                    // As tall as the window is now, so making it taller shows more of the tab, never empty space
                    // under a fixed-height part (440 points to begin with, and never more than the screen has).
                    let room = ui.available_height();
                    ui.set_height(if room.is_finite() && room > 120.0 { room } else { 440.0_f32.min(crate::fit::height_left(ui)) });
                    egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt(("project-settings-tab", draft.tab)).show(ui, |ui| match draft.tab {
                        0 => draft.general(ui),
                        1 => draft.files(ui),
                        2 => draft.editing(ui),
                        3 => draft.saving(ui),
                        4 => draft.running(ui, base),
                        5 => draft.languages(ui),
                        _ => draft.look(ui, base),
                    });
                });
            });
        }
        crate::fit::Part::Buttons => {
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("OK")).clicked() {
                    result = Result::Ok;
                }
                if ui.button(crate::i18n::tr("Cancel")).clicked() {
                    result = Result::Cancel;
                }
            });
        }
    });
    if draft.then_tasks && result == Result::Open {
        result = Result::Ok;
    }
    result
}

impl Limoniter {
    /// Opens the project's settings, as they are now.
    pub(super) fn open_project_settings(&mut self) {
        let Some(project) = &self.project else { return };
        let folder = project.folder();
        let prefs = project.prefs.clone();
        let prefs_follow_off = prefs.follow_vscode == Some(false);
        let own = Own {
            spaces: self.prefs.edit.spaces_for_tab,
            tab_width: self.view.tab_size,
            trim: self.view.trim_on_save,
            format_on_save: self.lsp.format_on_save,
            preset: self.new_file_preset.name,
        };
        let (hidden, patterns): (Vec<Hide>, Vec<Hide>) = project.explorer.hidden.iter().cloned().partition(|rule| !matches!(rule, Hide::Glob(_)));
        let envs = crate::py_env::find(&folder, None);
        let python = self.python.chosen.get(&folder).map(Path::to_path_buf);
        self.dialog = Dialog::ProjectSettings(Box::new(Draft {
            tab: 0,
            folder_project: project.root.is_some(),
            name: project.name.clone(),
            notes: project.notes.clone(),
            hidden,
            hide_patterns: patterns.iter().map(|rule| rule.to_text() + "\n").collect(),
            show_hidden: project.explorer.show_hidden,
            search_exclude: prefs.search_exclude.iter().map(|p| p.clone() + "\n").collect(),
            follow_gitignore: prefs.follow_gitignore.unwrap_or(true),
            recency: project.explorer.recency.clamp(1, 100),
            always: project.explorer.always.iter().map(|line| line.clone() + "\n").collect(),
            prefs,
            autosave: project.autosave,
            colours: project.colours.rules.clone(),
            python,
            envs,
            servers: self.lsp.settings.iter().map(|s| (s.language.clone(), s.command.clone())).collect(),
            own,
            then_tasks: false,
            vscode: Some(project.vscode.clone()).filter(|read| !read.is_empty() || prefs_follow_off),
            imported: false,
        }));
    }

    /// Keeps what was changed in the box, and brings what depends on it up to date.
    pub(super) fn apply_project_settings(&mut self, draft: Draft) {
        let Draft { name, notes, mut hidden, hide_patterns, show_hidden, search_exclude, follow_gitignore, recency, always, mut prefs, autosave, colours, python, then_tasks, folder_project, vscode, imported, .. } = draft;
        let Some(project) = &mut self.project else { return };
        if !name.trim().is_empty() {
            project.name = name.trim().to_string();
        }
        project.notes = notes;
        if folder_project {
            // A line that isn't a kind or a folder is a pattern, as a name in .gitignore is, unless it reaches out of the project.
            hidden.extend(lines_of(&hide_patterns).into_iter().filter(|line| !line.starts_with(['#', '!']) && !line.replace('\\', "/").split('/').any(|part| part == "..")).map(|line| Hide::from_text(&line).unwrap_or(Hide::Glob(line))));
            let mut unique: Vec<Hide> = Vec::new();
            for rule in hidden {
                if !unique.contains(&rule) {
                    unique.push(rule);
                }
            }
            project.explorer.hidden = unique;
            project.explorer.show_hidden = show_hidden;
        }
        project.explorer.recency = if recency >= 100 { 100 } else { recency };
        let mut lines: Vec<String> = Vec::new();
        for line in always.lines().filter_map(crate::explorer_kept::always_line) {
            if !lines.contains(&line) {
                lines.push(line);
            }
        }
        project.explorer.always = lines;
        prefs.search_exclude = lines_of(&search_exclude);
        prefs.follow_gitignore = (!follow_gitignore).then_some(false);
        // A value the person left on its own setting is never written.
        let servers_changed = prefs.servers_off != project.prefs.servers_off;
        let python_folder = project.folder();
        project.prefs = prefs;
        project.autosave = autosave;
        // Import into Limoniter: the tasks become the project's own, without the mark, and the words go in its list.
        if imported && let Some(read) = &vscode {
            let mark = format!(" ({})", crate::vscode_project::MARK);
            for task in &read.tasks {
                let mut own = task.clone();
                own.name = own.name.strip_suffix(&mark).unwrap_or(&own.name).to_string();
                if !project.tasks.iter().any(|t| t.name == own.name) {
                    project.tasks.push(own);
                }
            }
            if !read.words.is_empty() {
                let _ = crate::vscode_project::keep_words(&python_folder, &read.words);
            }
        }
        project.read_vscode();
        project.colours.rules = file_colours_host::tidy_rules(&colours);
        match python {
            Some(python) => self.python.chosen.set(&python_folder, &python),
            None => self.python.chosen.clear(&python_folder),
        }
        if servers_changed {
            // Stopped, so the files that may still have one are looked after again from the start.
            self.lsp.stop_all();
            for doc in &mut self.docs {
                if let Some(intel) = doc.text.intel.as_mut() {
                    intel.lsp = false;
                    intel.diagnostics.clear();
                }
            }
        }
        self.apply_tab_size();
        self.save_project();
        self.index_project();
        self.save_settings_if_changed();
        if then_tasks {
            self.edit_project_tasks();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{app_with, command, draw};
    use super::*;

    fn folder(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-prj-settings-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::path::absolute(&dir).unwrap()
    }

    fn files(root: &Path, names: &[&str]) {
        for name in names {
            std::fs::create_dir_all(root.join(name).parent().unwrap()).unwrap();
            std::fs::write(root.join(name), "text  \n").unwrap();
        }
    }

    /// Opens the box, lets the caller change the draft, and presses OK.
    fn change(app: &mut Limoniter, edit: impl FnOnce(&mut Draft)) {
        app.open_project_settings();
        let Dialog::ProjectSettings(mut draft) = std::mem::replace(&mut app.dialog, Dialog::None) else { panic!("the box opened") };
        edit(&mut draft);
        app.apply_project_settings(*draft);
    }

    fn relative(app: &Limoniter, root: &Path) -> Vec<String> {
        app.project.as_ref().unwrap().files.iter().map(|f| crate::explorer::relative(root, f)).collect()
    }

    #[test]
    fn the_menu_and_the_gear_open_the_box_only_when_there_is_a_project() {
        let (mut app, ctx) = app_with("");
        assert!(!app.can(Command::ProjectSettings), "not offered with no project");
        command(&mut app, &ctx, Command::ProjectSettings);
        assert!(matches!(app.dialog, Dialog::None));
        let dir = folder("open");
        app.new_project(dir.join("p.limproj"));
        assert!(app.can(Command::ProjectSettings));
        command(&mut app, &ctx, Command::ProjectSettings);
        assert!(matches!(&app.dialog, Dialog::ProjectSettings(draft) if !draft.folder_project));
        app.dialog = Dialog::None;
        // The gear on the panel's title bar asks the same.
        app.project_compare(crate::project::PanelAction::Settings);
        assert!(matches!(app.dialog, Dialog::ProjectSettings(_)));
        // Each tab draws.
        for tab in 0..TABS.len() {
            if let Dialog::ProjectSettings(draft) = &mut app.dialog {
                draft.tab = tab;
            }
            draw(&mut app, &ctx, Vec::new());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_project_file_keeps_what_was_set_and_nothing_else() {
        let (mut app, _ctx) = app_with("");
        let dir = folder("limproj");
        let path = dir.join("p.limproj");
        app.new_project(path.clone());
        // Nothing set: the file has no settings in it.
        assert!(!std::fs::read_to_string(&path).unwrap().contains("settings"));
        change(&mut app, |draft| {
            draft.name = "Named".into();
            draft.notes = "some notes".into();
            draft.prefs.spaces = Some(true);
            draft.prefs.tab_width = Some(3);
            draft.prefs.trim_trailing = Some(true);
            draft.prefs.final_newline = Some(false);
            draft.prefs.format_on_save = Some(true);
            draft.prefs.new_file_preset = Some("classic-mac".into());
            draft.prefs.formatters.insert("Python".into(), "none".into());
            draft.prefs.env_file = Some("dev.env".into());
            draft.prefs.work_folder = Some("build".into());
            draft.prefs.templates_folder = Some("tpl".into());
            draft.prefs.servers_off.push("Rust".into());
            draft.search_exclude = "*.min.js\nvendor/\n".into();
            draft.follow_gitignore = false;
            draft.autosave = Some(crate::settings::AutosaveChoice::Off);
            draft.recency = 40;
            draft.colours = vec![crate::file_colours::Rule::new("admin/", crate::file_colours::Tint::Blue)];
        });
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("spaces_for_tab") && text.contains("\"settings\""));
        let loaded = Project::load(&path).unwrap();
        assert_eq!(loaded.name, "Named");
        assert_eq!(loaded.notes, "some notes");
        assert_eq!(loaded.prefs, app.project.as_ref().unwrap().prefs);
        assert_eq!(loaded.prefs.search_exclude, ["*.min.js", "vendor/"]);
        assert_eq!(loaded.prefs.follow_gitignore, Some(false));
        assert_eq!(loaded.autosave, Some(crate::settings::AutosaveChoice::Off));
        assert_eq!(loaded.explorer.recency, 40);
        assert_eq!(loaded.colours.rules.len(), 1);
        // The box shows it back as it was kept.
        app.open_project_settings();
        let Dialog::ProjectSettings(draft) = &app.dialog else { panic!() };
        assert_eq!((draft.prefs.tab_width, draft.prefs.spaces, draft.follow_gitignore, draft.search_exclude.as_str()), (Some(3), Some(true), false, "*.min.js\nvendor/\n"));
        // Put back to "As in Settings", it's all gone from the file again.
        change(&mut app, |draft| {
            draft.prefs = Prefs::default();
            draft.search_exclude.clear();
            draft.follow_gitignore = true;
            draft.autosave = None;
            draft.recency = 100;
        });
        assert!(!std::fs::read_to_string(&path).unwrap().contains("settings"));
        assert!(!std::fs::read_to_string(&path).unwrap().contains("autosave"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_folder_project_keeps_them_in_its_folder_for_git_and_takes_the_file_away_when_empty() {
        let (mut app, _ctx) = app_with("");
        let root = folder("folder");
        files(&root, &["a.txt"]);
        app.open_folder(&root);
        let kept = crate::project_prefs::file_in(&root);
        assert!(!kept.exists(), "nothing's written for a project that never set anything");
        change(&mut app, |draft| {
            draft.prefs.spaces = Some(false);
            draft.search_exclude = "dist/".into();
        });
        let written: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&kept).unwrap()).unwrap();
        assert_eq!(written, serde_json::json!({ "spaces": false, "search_exclude": ["dist/"] }));
        let project_file = app.project.as_ref().unwrap().path.clone();
        assert!(!std::fs::read_to_string(&project_file).unwrap().contains("search_exclude"), "not in the project's file, which stays out of Git");
        // Opened again from the disk, it's all there.
        let loaded = Project::load(&project_file).unwrap();
        assert_eq!(loaded.prefs.spaces, Some(false));
        assert_eq!(loaded.prefs.search_exclude, ["dist/"]);
        change(&mut app, |draft| {
            draft.prefs = Prefs::default();
            draft.search_exclude.clear();
        });
        assert!(!kept.exists());
        app.close_project();
        let _ = std::fs::remove_file(project_file);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn exclude_patterns_leave_files_out_of_the_index_and_search_but_not_the_explorer() {
        let (mut app, ctx) = app_with("");
        let root = folder("exclude");
        files(&root, &["src/a.rs", "src/gen/b.rs", "web/app.min.js", "web/app.js", "readme.md"]);
        app.open_folder(&root);
        let index = |app: &mut Limoniter| {
            app.index_project();
            for _ in 0..500 {
                app.project_index_tick(&ctx);
                if app.indexing.job.is_none() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            // The project's own settings file is in the index too, so it can be found and edited.
            relative(app, &root).into_iter().filter(|f| !f.starts_with(".limoniter")).collect::<Vec<_>>()
        };
        assert_eq!(index(&mut app).len(), 5);
        change(&mut app, |draft| draft.search_exclude = "*.min.js\nsrc/gen/\n".into());
        let found = index(&mut app);
        assert_eq!(found, ["readme.md", "src/a.rs", "web/app.js"]);
        // Search and Go to file look among those, and the Explorer still has them.
        let (_, candidates) = app.go_to_file_candidates();
        assert_eq!(candidates.iter().filter(|c| !c.to_string_lossy().contains(".limoniter")).count(), 3);
        let (_, searched) = app.search_scope().unwrap();
        assert_eq!(searched.unwrap().iter().filter(|c| !c.to_string_lossy().contains(".limoniter")).count(), 3);
        let project = app.project.as_ref().unwrap();
        assert!(project.explorer.hidden.is_empty(), "nothing is hidden from the Explorer");
        draw(&mut app, &ctx, Vec::new());
        assert!(app.panels.project_panel.explorer.names_for_test(&root.join("web")).contains(&"app.min.js".to_string()));
        // Following .gitignore can be turned off: the ignored file then shows up in the index.
        std::fs::write(root.join(".gitignore"), "readme.md\n").unwrap();
        assert!(!index(&mut app).contains(&"readme.md".to_string()));
        change(&mut app, |draft| draft.follow_gitignore = false);
        assert!(index(&mut app).contains(&"readme.md".to_string()));
        app.close_project();
        let _ = std::fs::remove_file(app.recent_projects[0].clone());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hide_patterns_hide_from_the_explorer_and_the_index_and_come_back_as_they_were_written() {
        let (mut app, _ctx) = app_with("");
        let root = folder("hide");
        files(&root, &["notes/old_1.txt", "notes/new.txt", "x.log", "src/a.rs"]);
        app.open_folder(&root);
        change(&mut app, |draft| draft.hide_patterns = "*.log\nnotes/old_*\n".into());
        let hidden = app.project.as_ref().unwrap().explorer.hidden.clone();
        assert_eq!(hidden, [Hide::Kind("log".into()), Hide::Glob("notes/old_*".into())]);
        let (found, _) = crate::workspace::index(&root, &hidden);
        assert_eq!(found.iter().map(|f| crate::explorer::relative(&root, f)).collect::<Vec<_>>(), ["notes/new.txt", "src/a.rs"]);
        app.open_project_settings();
        let Dialog::ProjectSettings(draft) = &app.dialog else { panic!() };
        assert_eq!(draft.hide_patterns, "notes/old_*\n", "the patterns that aren't kinds or folders");
        assert!(draft.hidden.contains(&Hide::Kind("log".into())));
        app.dialog = Dialog::None;
        let kept = app.project.as_ref().unwrap().path.clone();
        app.close_project();
        let _ = std::fs::remove_file(kept);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_project_s_indent_beats_the_person_s_and_an_editorconfig_beats_both() {
        let (mut app, _ctx) = app_with("");
        let dir = folder("indent");
        app.view.tab_size = 8;
        app.prefs.edit.spaces_for_tab = false;
        assert_eq!((app.tab_size_now(), app.spaces_for_tab_now(), app.indent_unit()), (8, false, "\t".to_string()));
        app.new_project(dir.join("p.limproj"));
        change(&mut app, |draft| {
            draft.prefs.spaces = Some(true);
            draft.prefs.tab_width = Some(2);
        });
        assert_eq!((app.tab_size_now(), app.spaces_for_tab_now(), app.indent_unit()), (2, true, "  ".to_string()));
        // An .editorconfig that says something wins; one that says nothing about it doesn't.
        app.editor_config = Some((dir.join("a.c"), crate::editorconfig::Settings { spaces: Some(false), indent_size: Some(4), ..Default::default() }));
        assert_eq!((app.tab_size_now(), app.spaces_for_tab_now()), (4, false));
        app.editor_config = Some((dir.join("a.c"), crate::editorconfig::Settings::default()));
        assert_eq!((app.tab_size_now(), app.spaces_for_tab_now()), (2, true));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saving_tidies_a_file_as_the_project_says_and_an_editorconfig_still_wins() {
        let (mut app, _ctx) = app_with("");
        let dir = folder("save");
        let path = dir.join("a.txt");
        std::fs::write(&path, "one   \ntwo").unwrap();
        app.new_project(dir.join("p.limproj"));
        assert!(app.open_path(&path));
        let index = app.active;
        let saved = |app: &mut Limoniter| String::from_utf8(app.bytes_for(index).unwrap()).unwrap();
        app.view.trim_on_save = false;
        assert_eq!(saved(&mut app), "one   \ntwo", "as in Settings: left alone");
        change(&mut app, |draft| {
            draft.prefs.trim_trailing = Some(true);
            draft.prefs.final_newline = Some(true);
        });
        assert_eq!(saved(&mut app), "one\ntwo\n");
        // .editorconfig says not to, and has the last word.
        std::fs::write(dir.join(".editorconfig"), "root = true\n[*.txt]\ntrim_trailing_whitespace = false\n").unwrap();
        let mut again = app_with("").0;
        again.project = app.project.take();
        assert!(again.open_path(&path));
        let index = again.active;
        assert_eq!(String::from_utf8(again.bytes_for(index).unwrap()).unwrap(), "one   \ntwo\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn format_on_save_the_formatters_and_new_files_come_from_the_project_when_it_says() {
        let (mut app, _ctx) = app_with("");
        let dir = folder("format");
        app.lsp.format_on_save = false;
        app.formatter_choices.insert("Python".into(), "Black".into());
        app.new_file_preset = &crate::codec::PRESETS[0];
        assert!(!app.format_on_save_now());
        app.new_project(dir.join("p.limproj"));
        change(&mut app, |draft| {
            draft.prefs.format_on_save = Some(true);
            draft.prefs.formatters.insert("Python".into(), "Ruff".into());
            draft.prefs.new_file_preset = Some("linux".into());
        });
        assert!(app.format_on_save_now());
        assert_eq!(app.formatter_choices_now().get("Python").map(String::as_str), Some("Ruff"));
        assert_eq!(app.formatter_choices.get("Python").map(String::as_str), Some("Black"), "the person's own are left as they were");
        app.new_document();
        assert_eq!(app.docs[app.active].text.format.encoding_name(), crate::codec::preset_by_id("linux").unwrap().format().encoding_name());
        assert_eq!(app.new_file_preset_now().id, "linux");
        // Closing the project puts it all back to the person's own.
        app.close_project();
        assert!(!app.format_on_save_now());
        assert_eq!(app.formatter_choices_now().get("Python").map(String::as_str), Some("Black"));
        assert_eq!(app.new_file_preset_now().id, app.new_file_preset.id);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn tasks_run_where_the_project_says_and_templates_are_kept_where_it_says() {
        let (mut app, _ctx) = app_with("");
        let dir = folder("running");
        std::fs::create_dir_all(dir.join("build")).unwrap();
        app.new_project(dir.join("p.limproj"));
        assert_eq!(app.task_folder().unwrap(), dir);
        change(&mut app, |draft| {
            draft.prefs.work_folder = Some("build".into());
            draft.prefs.templates_folder = Some("my templates".into());
        });
        assert_eq!(app.task_folder().unwrap(), dir.join("build"));
        assert_eq!(app.template_places().unwrap().0, dir.join("my templates"));
        // A folder that isn't there isn't run in.
        change(&mut app, |draft| draft.prefs.work_folder = Some("gone".into()));
        assert_eq!(app.task_folder().unwrap(), dir);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_project_s_env_file_is_the_one_given_to_what_runs() {
        let dir = folder("env");
        std::fs::write(dir.join(".env"), "PRJ_SETTINGS_TEST=plain\n").unwrap();
        std::fs::write(dir.join("dev.env"), "PRJ_SETTINGS_TEST=dev\n").unwrap();
        let prefs = Prefs { env_file: Some("dev.env".into()), ..Prefs::default() };
        assert_eq!(crate::env_file::environment_from(&prefs.env_path(&dir)), [("PRJ_SETTINGS_TEST".to_string(), "dev".to_string())]);
        assert_eq!(crate::env_file::environment_from(&Prefs::default().env_path(&dir)), [("PRJ_SETTINGS_TEST".to_string(), "plain".to_string())]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn the_env_file_reaches_the_command_that_is_run() {
        let dir = folder("env-run");
        std::fs::write(dir.join("dev.env"), "PRJ_SETTINGS_RUN=from_dev\n").unwrap();
        let mut job = file_tools::start_run_with("echo $PRJ_SETTINGS_RUN", Some(&dir), Some(&dir.join("dev.env")), || {}).unwrap();
        let mut printed = Vec::new();
        for _ in 0..500 {
            let (lines, finished) = job.take_output();
            printed.extend(lines);
            if finished.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(printed.iter().any(|l| l.contains("from_dev")), "{printed:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn turning_a_language_server_off_stops_the_servers_and_is_kept() {
        let (mut app, _ctx) = app_with("");
        let dir = folder("servers");
        app.new_project(dir.join("p.limproj"));
        assert!(app.lsp.settings.iter().any(|s| s.language == "Rust"));
        change(&mut app, |draft| {
            assert!(draft.servers.iter().any(|(language, _)| language == "Rust"));
            draft.prefs.servers_off.push("Rust".into());
        });
        assert!(app.project.as_ref().unwrap().prefs.server_off("Rust"));
        assert!(!app.project.as_ref().unwrap().prefs.server_off("C"));
        assert!(Project::load(&dir.join("p.limproj")).unwrap().prefs.server_off("Rust"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
