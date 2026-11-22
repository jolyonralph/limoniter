//! A project's own file templates in the window: New from template (File, New as, under the project's name, and the
//! Explorer's menu), Save as template, and new files in a folder that starts from the template chosen for it. The
//! work on the templates themselves is in `file_templates.rs`.

use super::*;
use crate::file_templates::{self as templates, Blank, Template, Values};

pub(super) struct TemplateBox {
    pub(super) dir: PathBuf,
    pub(super) root: PathBuf,
    pub(super) templates: Vec<Template>,
    pub(super) selected: usize,
    pub(super) name: String,
    pub(super) target: PathBuf,
    /// The blanks of the person's own that the template picked asks for, each with what's been typed.
    pub(super) asked: Vec<(Blank, String)>,
    pub(super) asked_for: Option<usize>,
    /// Whether new files in the target folder start from this template without asking.
    pub(super) by_default: bool,
    pub(super) problem: String,
}

pub(super) struct SaveTemplateBox {
    pub(super) dir: PathBuf,
    pub(super) name: String,
    pub(super) text: String,
    pub(super) stem: String,
    pub(super) name_blank: bool,
    pub(super) problem: String,
}

impl Limoniter {
    /// The folder a project's templates are kept in, and the project's folder, if a project is open.
    pub(super) fn template_places(&self) -> Option<(PathBuf, PathBuf)> {
        let project = self.project.as_ref()?;
        let root = project.folder();
        let own = project.prefs.templates_folder.as_deref().map(|written| crate::project_prefs::Prefs::place(&root, written));
        Some((own.unwrap_or_else(|| templates::dir_for(project.root.as_deref(), &root)), root))
    }

    /// The project's templates, for the menu.
    pub(super) fn project_templates(&self) -> Vec<Template> {
        self.template_places().map(|(dir, _)| templates::list(&dir)).unwrap_or_default()
    }

    /// Who new files are by: the name git has been given, or else the person's login name.
    fn template_author(&self, root: &Path) -> String {
        crate::git::identity(Some(root)).0.or_else(|| std::env::var("USER").ok().or_else(|| std::env::var("USERNAME").ok())).unwrap_or_default()
    }

    fn template_values(&self, name: &str, target: &Path, root: &Path, ending: &str) -> Values {
        let now = chrono::Local::now();
        Values {
            name: name.to_string(),
            folder: target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            namespace: templates::namespace_for(root, target, ending),
            date: now.format("%Y-%m-%d").to_string(),
            year: now.format("%Y").to_string(),
            author: self.template_author(root),
            extra: Default::default(),
        }
    }

    /// Opens the New from template box, with the template at `selected` chosen, for files in `folder` (the folder of
    /// the file in front, if it's in the project, or else the project's own).
    pub(super) fn open_template_box(&mut self, folder: Option<PathBuf>, selected: usize) {
        let Some((dir, root)) = self.template_places() else { return };
        let target = folder.or_else(|| self.docs.get(self.active).and_then(|doc| doc.path.as_deref()).and_then(Path::parent).filter(|parent| parent.starts_with(&root)).map(Path::to_path_buf)).unwrap_or_else(|| root.clone());
        let list = templates::list(&dir);
        // A folder with a template chosen for it starts with that one.
        let selected = match templates::rule_for(&dir, &root, &target).and_then(|rule| list.iter().position(|t| *t == rule)) {
            Some(by_rule) if selected == 0 => by_rule,
            _ => selected.min(list.len().saturating_sub(1)),
        };
        self.dialog = Dialog::Template(Box::new(TemplateBox { dir, root, templates: list, selected, name: String::new(), target, asked: Vec::new(), asked_for: None, by_default: false, problem: String::new() }));
    }

    pub(super) fn open_save_template_box(&mut self) {
        let Some((dir, _)) = self.template_places() else { return };
        let doc = &self.docs[self.active];
        if doc.hex_mode || doc.text.is_disk_backed() {
            self.message("Save as template", crate::i18n::tr("Only a file shown as text, and not too large, can be kept as a template.").into());
            return;
        }
        let text = doc.text.all_lines().iter().map(|line| line.trim_end_matches(['\r', '\n'])).collect::<Vec<_>>().join("\n");
        let path = doc.path.clone().unwrap_or_else(|| PathBuf::from(doc.name()));
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.dialog = Dialog::SaveTemplate(Box::new(SaveTemplateBox { dir, name, text, stem, name_blank: true, problem: String::new() }));
    }

    pub(super) fn templates_dialog(&mut self, ctx: &Context) -> bool {
        match &self.dialog {
            Dialog::Template(_) => self.new_from_template_dialog(ctx),
            Dialog::SaveTemplate(_) => self.save_template_dialog(ctx),
            _ => return false,
        }
        true
    }

    fn new_from_template_dialog(&mut self, ctx: &Context) {
        let Dialog::Template(state) = &mut self.dialog else { return };
        // The blanks to ask for change with the template chosen, keeping what's been typed under the same name.
        if state.asked_for != Some(state.selected) {
            state.asked_for = Some(state.selected);
            let typed: Vec<(String, String)> = state.asked.iter().map(|(blank, value)| (blank.label.clone(), value.clone())).collect();
            state.asked = state.templates.get(state.selected).map(templates::blanks_of).unwrap_or_default().into_iter().map(|blank| {
                let value = typed.iter().find(|(label, _)| *label == blank.label).map_or_else(|| blank.default.clone(), |(_, value)| value.clone());
                (blank, value)
            }).collect();
            state.by_default = state.templates.get(state.selected).is_some_and(|t| templates::rule_for(&state.dir, &state.root, &state.target).is_some_and(|rule| rule == *t));
        }
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut make = false;
        let folder_note = templates::relative_folder(&state.root, &state.target).filter(|rel| !rel.is_empty()).unwrap_or_else(|| ".".to_string());
        crate::fit::Dialog::new(crate::i18n::tr("New from template")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show_with_buttons(ctx, |ui, part| match part {
            crate::fit::Part::Body => {
                if state.templates.is_empty() {
                    ui.label(crate::i18n::tf!("This project has no templates yet. Put files or folders in {}, or use File, Project, Save as template.", state.dir.display()));
                    return;
                }
                ui.label(crate::i18n::tf!("New files go in {folder_note}."));
                ui.add_space(4.0);
                egui::Grid::new("template-box").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                    ui.label(crate::i18n::tr("Template"));
                    let shown = state.templates.get(state.selected).map(|t| if t.folder { format!("{}/", t.name) } else { t.name.clone() }).unwrap_or_default();
                    egui::ComboBox::from_id_salt("template-pick").width(260.0).selected_text(shown).show_ui(ui, |ui| {
                        for (index, template) in state.templates.iter().enumerate() {
                            let label = if template.folder { format!("{}/", template.name) } else { template.name.clone() };
                            ui.selectable_value(&mut state.selected, index, label);
                        }
                    });
                    ui.end_row();
                    ui.label(crate::i18n::tr("Name"));
                    let name = ui.add(egui::TextEdit::singleline(&mut state.name).desired_width(260.0));
                    if !name.has_focus() && state.name.is_empty() {
                        name.request_focus();
                    }
                    ui.end_row();
                    for (blank, value) in &mut state.asked {
                        ui.label(blank.label.as_str());
                        ui.add(egui::TextEdit::singleline(value).desired_width(260.0));
                        ui.end_row();
                    }
                });
                ui.add_space(4.0);
                ui.checkbox(&mut state.by_default, crate::i18n::tr("Start new files in this folder from this template")).on_hover_text(crate::i18n::tr("The Explorer's New file then fills them in from it. Kept in the templates folder, in the file .rules"));
                if !state.problem.is_empty() {
                    ui.colored_label(theme::rainbow_mistake(), state.problem.as_str());
                }
            }
            crate::fit::Part::Buttons => {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    make = ui.add_enabled(!state.templates.is_empty() && !state.name.trim().is_empty(), egui::Button::new(crate::i18n::tr("Create"))).clicked();
                    close |= ui.button(crate::i18n::tr("Cancel")).clicked();
                });
            }
        });
        if make {
            self.make_from_template();
        } else if close {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
    }

    pub(super) fn make_from_template(&mut self) {
        let Dialog::Template(state) = &mut self.dialog else { return };
        let Some(template) = state.templates.get(state.selected).cloned() else { return };
        let typed = state.name.trim().to_string();
        let stem = Path::new(&typed).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| typed.clone());
        let (target, root, dir, by_default) = (state.target.clone(), state.root.clone(), state.dir.clone(), state.by_default);
        let extra = state.asked.iter().map(|(blank, value)| (blank.label.clone(), value.clone())).collect();
        let ending = templates::ending_of_template(&template);
        let mut values = self.template_values(&stem, &target, &root, &ending);
        values.extra = extra;
        let result = templates::plan(&template, &target, &typed, &values).and_then(|plan| templates::write(&plan).map(|()| plan));
        match result {
            Ok(plan) => {
                // What was ticked is kept, and un-ticking takes the rule back.
                if let Some(folder) = templates::relative_folder(&root, &target) {
                    let has = templates::rule_for(&dir, &root, &target).is_some_and(|rule| rule == template);
                    if by_default != has {
                        let _ = templates::set_rule(&dir, &folder, by_default.then_some(template.name.as_str()));
                    }
                }
                self.dialog = Dialog::None;
                self.open_made(&plan);
                self.panels.project_panel.explorer.refresh();
            }
            Err(problem) => {
                if let Dialog::Template(state) = &mut self.dialog {
                    state.problem = problem;
                }
            }
        }
    }

    /// Opens the files a template made, the first in front, with the caret where `${cursor}` was.
    fn open_made(&mut self, plan: &templates::Plan) {
        let mut first = None;
        for (index, (path, _)) in plan.files.iter().enumerate() {
            if self.open_path(path) {
                if let Some((_, line, column)) = plan.cursor.filter(|(at, _, _)| *at == index) {
                    self.doc().text.move_caret_to_char(line, column);
                }
                first = first.or(Some(self.active));
            }
        }
        // The file with the cursor is left in front when there is one.
        if let Some((at, _, _)) = plan.cursor
            && let Some(index) = self.docs.iter().position(|doc| doc.path.as_deref().is_some_and(|p| same_path(p, &plan.files[at].0)))
        {
            self.active = index;
        } else if let Some(index) = first {
            self.active = index;
        }
        self.pending_focus = true;
    }

    fn save_template_dialog(&mut self, ctx: &Context) {
        let Dialog::SaveTemplate(state) = &mut self.dialog else { return };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut save = false;
        crate::fit::Dialog::new(crate::i18n::tr("Save as template")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show_with_buttons(ctx, |ui, part| match part {
            crate::fit::Part::Body => {
                ui.label(crate::i18n::tf!("The file in front is kept in {} for new files to start from.", state.dir.display()));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Template name"));
                    ui.add(egui::TextEdit::singleline(&mut state.name).desired_width(240.0));
                });
                if state.stem.chars().count() >= 3 {
                    ui.checkbox(&mut state.name_blank, crate::i18n::tf!("Turn {} in the text into the name of the new file", state.stem));
                }
                if !state.problem.is_empty() {
                    ui.colored_label(theme::rainbow_mistake(), state.problem.as_str());
                }
            }
            crate::fit::Part::Buttons => {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    save = ui.button(crate::i18n::tr("Save")).clicked();
                    close |= ui.button(crate::i18n::tr("Cancel")).clicked();
                });
            }
        });
        if save {
            let result = templates::save_as(&state.dir, &state.name, &state.text, &state.stem, state.name_blank);
            match result {
                Ok(path) => {
                    self.dialog = Dialog::None;
                    self.print_status = crate::i18n::tf!("Saved the template {}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                    self.pending_focus = true;
                }
                Err(problem) => state.problem = problem,
            }
        } else if close {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
    }

    /// The Explorer's New file, in a folder that has a template chosen for it: the file starts from the template,
    /// under the name typed. Returns false when no template is chosen, so the empty file is made as usual.
    pub(super) fn made_from_folder_template(&mut self, path: &Path) -> bool {
        let Some((dir, root)) = self.template_places() else { return false };
        let Some(parent) = path.parent() else { return false };
        let Some(template) = templates::rule_for(&dir, &root, parent) else { return false };
        let typed = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut values = self.template_values(&stem, parent, &root, &templates::ending_of_template(&template));
        // There's no box to ask in, so a blank of the person's own gets its default, or nothing.
        values.extra = templates::blanks_of(&template).into_iter().map(|blank| (blank.label, blank.default)).collect();
        let result = templates::plan(&template, parent, &typed, &values).map(|mut plan| {
            // A single file keeps exactly the name typed.
            if !template.folder {
                plan.files[0].0 = path.to_path_buf();
            }
            plan
        });
        match result.and_then(|plan| if !template.folder && path.exists() { Err(String::new()) } else { templates::write(&plan).map(|()| plan) }) {
            Ok(plan) => self.open_made(&plan),
            Err(problem) => self.message("New", crate::i18n::tf!("Couldn't make {}.\n\n{problem}", path.display())),
        }
        true
    }
}
