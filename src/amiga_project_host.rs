//! The window's side of Amiga C projects and catalogs: the New Amiga C project box, importing a folder, keeping
//! the makefiles up to date as files are saved, added and taken away, and checking and making catalogs, with what
//! they find in the Output panel, where clicking a problem goes to its line.

use super::*;
use crate::amiga_catalog as catalog;
use crate::amiga_project as amiga;

/// The New Amiga C project box.
pub(super) struct NewProjectBox {
    template: usize,
    name: String,
    folder: Option<PathBuf>,
    problem: String,
}

/// Whether a path ends with one of these endings.
fn ending_is(path: &Path, endings: &[&str]) -> bool {
    path.extension().is_some_and(|e| endings.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

impl NewProjectBox {
    /// For a screenshot: "template;folder".
    pub(super) fn show_for_screenshot(&mut self, value: &str) {
        let (template, folder) = value.split_once(';').unwrap_or((value, ""));
        self.template = template.parse().unwrap_or(0).min(amiga::TEMPLATES.len() - 1);
        if !folder.is_empty() {
            self.folder = Some(PathBuf::from(folder));
        }
    }
}

impl Limoniter {
    pub(super) fn open_amiga_project_box(&mut self) {
        // It starts in the folder above the project open, or the file in front's.
        let folder = self.project.as_ref().map(|p| p.folder()).and_then(|f| f.parent().map(Path::to_path_buf)).or_else(|| self.docs.get(self.active).and_then(|d| d.path.as_ref()).and_then(|p| p.parent().map(Path::to_path_buf)));
        self.dialog = Dialog::AmigaProject(Box::new(NewProjectBox { template: 0, name: "hello".into(), folder, problem: String::new() }));
    }

    pub(super) fn amiga_project_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::AmigaProject(state) = &mut self.dialog else {
            return false;
        };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut make = false;
        egui::Window::new(crate::i18n::tr("New Amiga C project")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(560.0);
            ui.label(crate::i18n::tr("A folder with the program's C, its makefiles for amiga-gcc (Makefile.gcc), vbcc (Makefile.vbcc) and SAS/C (smakefile), and an icon for a Workbench program. Limoniter keeps the makefiles' lists of files up to date as files are added and taken away, and F9 builds and runs it."));
            ui.add_space(6.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(250.0);
                    for (index, template) in amiga::TEMPLATES.iter().enumerate() {
                        ui.radio_value(&mut state.template, index, crate::i18n::tr(template.name));
                    }
                });
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.set_width(280.0);
                    ui.label(RichText::new(crate::i18n::tr(amiga::TEMPLATES[state.template].about)).color(theme::text_dim()));
                });
            });
            ui.add_space(8.0);
            egui::Grid::new("amiga-project").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                ui.label(crate::i18n::tr("Program's name"));
                ui.text_edit_singleline(&mut state.name);
                ui.end_row();
                ui.label(crate::i18n::tr("Make it in"));
                ui.horizontal(|ui| {
                    let shown = state.folder.as_ref().map_or_else(|| crate::i18n::tr("(choose a folder)").to_string(), |f| f.join(&state.name).display().to_string());
                    ui.label(shown);
                    if ui.button(crate::i18n::tr("Choose\u{2026}")).clicked()
                        && let Some(folder) = rfd::FileDialog::new().set_title(crate::i18n::tr("The folder to make the project in")).pick_folder()
                    {
                        state.folder = Some(folder);
                    }
                });
                ui.end_row();
            });
            if let Some(problem) = amiga::name_problem(&state.name) {
                ui.colored_label(theme::change_unsaved(), problem);
            } else if !state.problem.is_empty() {
                ui.colored_label(theme::change_unsaved(), &state.problem);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                make = ui.add_enabled(state.folder.is_some() && amiga::name_problem(&state.name).is_none(), egui::Button::new(crate::i18n::tr("Make the project"))).clicked();
                close |= ui.button(crate::i18n::tr("Cancel")).clicked();
            });
        });
        if make {
            let (template, name, folder) = (amiga::TEMPLATES[state.template], state.name.clone(), state.folder.clone().unwrap_or_default());
            match amiga::create(&folder, &template, &name) {
                Ok((project, main)) => {
                    self.dialog = Dialog::None;
                    self.open_project(&project);
                    self.open_path(&main);
                    self.script_print(crate::i18n::tf!("Made {} in {}: {}.", crate::i18n::tr(template.name), folder.join(&name).display(), crate::i18n::tr("F9 builds and runs it; Tasks has make -f Makefile.gcc and make -f Makefile.vbcc; the smakefile is for SAS/C on an Amiga")), true);
                }
                Err(problem) => {
                    if let Dialog::AmigaProject(state) = &mut self.dialog {
                        state.problem = problem;
                    }
                }
            }
        } else if close {
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
        true
    }

    /// Imports a folder of Amiga C as a project, says how its files were sorted, and opens it.
    pub(super) fn import_amiga_project(&mut self) {
        let Some(folder) = rfd::FileDialog::new().set_title(crate::i18n::tr("Import an Amiga C project")).pick_folder() else {
            return;
        };
        match amiga::import(&folder) {
            Ok(imported) => {
                self.open_project(&imported.project);
                self.script_print(crate::i18n::tf!("Imported {} as a project.", folder.display()), true);
                for (group, files) in imported.sorted.groups() {
                    if !files.is_empty() {
                        self.script_print(format!("{}: {}", crate::i18n::tr(group), files.join(", ")), false);
                    }
                }
                if imported.written.is_empty() {
                    self.script_print(crate::i18n::tr("No makefiles were written: it has them already, or no C to build.").into(), false);
                } else {
                    self.script_print(crate::i18n::tf!("Written: {}. Limoniter keeps their lists of files up to date.", imported.written.join(", ")), false);
                }
            }
            Err(problem) => self.message("Import an Amiga C project", problem),
        }
    }

    /// The makefiles of the Amiga project the file in front (or the project) is in, written again from its files.
    /// `say` says what came of it even when nothing changed.
    pub(super) fn refresh_amiga_makefiles(&mut self, say: bool) {
        let file = self.docs.get(self.active).and_then(|d| d.path.clone());
        let folder = file.as_deref().and_then(amiga::project_folder_of).or_else(|| self.project.as_ref().map(|p| p.folder()).filter(|f| amiga::build_of(f).is_some()));
        let Some(folder) = folder else {
            if say {
                self.message("Makefiles", crate::i18n::tr("There are no makefiles Limoniter keeps here. A project made with New Amiga C project, or imported, has them: Makefile.gcc, Makefile.vbcc and smakefile, with lines between # limoniter-begin and # limoniter-end.").into());
            }
            return;
        };
        match amiga::refresh_folder(&folder) {
            Ok(changed) if changed.is_empty() => {
                if say {
                    self.script_print(crate::i18n::tf!("The makefiles in {} are up to date.", folder.display()), true);
                }
            }
            Ok(changed) => {
                let names: Vec<String> = changed.iter().filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
                self.script_print(crate::i18n::tf!("Brought up to date with the files in {}: {}.", folder.display(), names.join(", ")), say);
            }
            Err(problem) => self.message("Makefiles", problem),
        }
    }

    /// A file of an Amiga project made, saved, renamed or deleted: the makefiles follow a C file or header, and a
    /// catalog description's C header is made again.
    pub(super) fn amiga_files_changed(&mut self, path: &Path) {
        if !ending_is(path, &["c", "h", "cd", "ct"]) {
            return;
        }
        if let Some(folder) = amiga::project_folder_of(path)
            && let Ok(changed) = amiga::refresh_folder(&folder)
            && !changed.is_empty()
        {
            let names: Vec<String> = changed.iter().filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
            self.script_print(crate::i18n::tf!("Brought up to date with the files in {}: {}.", folder.display(), names.join(", ")), false);
        }
    }

    /// After a file is saved: an Amiga project's makefiles follow it, and a description's C header, where it
    /// has one already, is made again from it.
    pub(super) fn amiga_saved(&mut self, path: &Path) {
        self.amiga_files_changed(path);
        if ending_is(path, &["cd"]) {
            let header = header_path(path);
            if std::fs::read_to_string(&header).is_ok_and(|t| t.contains("CATCOMP_NUMBERS"))
                && let Ok(text) = std::fs::read(path)
            {
                let cd = catalog::read_description(&catalog::decode(&text));
                let made = catalog::c_header(&cd, &stem_of(path));
                if std::fs::read_to_string(&header).ok().as_deref() != Some(made.as_str()) {
                    let _ = crate::codec::write_file(&header, made.as_bytes());
                }
            }
        }
    }

    /// A file's text: the open document's, with its unsaved changes, or else as it is on the disk.
    fn text_of(&self, path: &Path) -> Option<String> {
        if let Some(doc) = self.docs.iter().find(|d| d.path.as_deref().is_some_and(|p| same_path(p, path))) {
            return Some(doc.text.all_lines().iter().map(|l| l.as_ref()).collect::<Vec<&str>>().join("\n"));
        }
        std::fs::read(path).ok().map(|b| catalog::decode(&b))
    }

    /// The description and translations the file in front belongs with, or why not.
    fn catalog_files(&self) -> Result<(PathBuf, Vec<PathBuf>), String> {
        let path = self.docs.get(self.active).and_then(|d| d.path.clone()).filter(|p| ending_is(p, &["cd", "ct"])).ok_or_else(|| crate::i18n::tr("Open a catalog description (.cd) or translation (.ct) first, saved as a file.").to_string())?;
        if ending_is(&path, &["cd"]) {
            return Ok((path.clone(), catalog::translations_for(&path)));
        }
        let ct = catalog::read_translation(&self.text_of(&path).unwrap_or_default());
        let cd = catalog::description_for(&path, &ct).ok_or_else(|| crate::i18n::tf!("No catalog description (.cd) was found beside {} or in the folders above it.", path.display()))?;
        Ok((cd, vec![path]))
    }

    /// Puts problems in the Output panel and beside their lines: errors as assemblers print them, warnings as gcc
    /// does, which the Output panel knows to go to. How many were errors.
    fn report_catalog_problems(&mut self, path: &Path, problems: &[catalog::Problem]) -> usize {
        for problem in problems {
            let line = if problem.warning { format!("{}:{}: warning: {}", path.display(), problem.line, problem.message) } else { format!("{}({}): {}", path.display(), problem.line, problem.message) };
            self.script_print(line, false);
        }
        if let Some(doc) = self.docs.iter_mut().find(|d| d.path.as_deref().is_some_and(|p| same_path(p, path))) {
            let intel = doc.text.intel.get_or_insert_with(Box::default);
            intel.build_problems.clear();
            for problem in problems {
                let words = if problem.warning { format!("warning: {}", problem.message) } else { problem.message.clone() };
                intel.build_problems.push((problem.line.saturating_sub(1), words));
            }
        }
        problems.iter().filter(|p| !p.warning).count()
    }

    /// Checks the description and its translations, or the translation in front against its description.
    pub(super) fn check_catalogs(&mut self) {
        let (cd_path, translations) = match self.catalog_files() {
            Ok(found) => found,
            Err(problem) => return self.message("Check the catalogs", problem),
        };
        let cd = catalog::read_description(&self.text_of(&cd_path).unwrap_or_default());
        self.script_print(crate::i18n::tf!("Checking {} ({} strings)\u{2026}", cd_path.display(), cd.entries.len()), true);
        let mut errors = self.report_catalog_problems(&cd_path, &cd.problems);
        let mut warnings = cd.problems.iter().filter(|p| p.warning).count();
        for ct_path in &translations {
            let ct = catalog::read_translation(&self.text_of(ct_path).unwrap_or_default());
            let problems = catalog::check(&cd, &ct);
            warnings += problems.iter().filter(|p| p.warning).count();
            errors += self.report_catalog_problems(ct_path, &problems);
        }
        if translations.is_empty() {
            self.script_print(crate::i18n::tr("There are no translations (.ct) beside it or in the folders below it yet. Tools, Amiga, New translation makes one.").into(), false);
        }
        self.script_print(crate::i18n::tf!("Errors: {errors}. Warnings: {warnings}. Translations checked: {}.", translations.len()), false);
    }

    /// Makes each translation's catalog (by Limoniter, or FlexCat), in Catalogs/<language>/ beside the
    /// description, and the strings' C header beside it.
    pub(super) fn make_catalogs(&mut self, flexcat: bool) {
        let (cd_path, translations) = match self.catalog_files() {
            Ok(found) => found,
            Err(problem) => return self.message("Make the catalogs", problem),
        };
        let flexcat_program = if flexcat {
            match catalog::flexcat() {
                Some(program) => Some(program),
                None => return self.message("Make the catalogs", crate::i18n::tr("FlexCat wasn't found on this computer. Get it from github.com/adtools/flexcat (or Aminet's dev/misc), and put it on the PATH. Limoniter can make the catalogs itself without it.").into()),
            }
        } else {
            None
        };
        // The descriptions and translations are saved first, so what's made is what's shown.
        for path in std::iter::once(&cd_path).chain(&translations) {
            if let Some(index) = self.docs.iter().position(|d| d.path.as_deref().is_some_and(|p| same_path(p, path)))
                && self.docs[index].is_modified()
            {
                self.save(index);
            }
        }
        let cd = catalog::read_description(&self.text_of(&cd_path).unwrap_or_default());
        self.script_print(crate::i18n::tf!("Making the catalogs for {}\u{2026}", cd_path.display()), true);
        if self.report_catalog_problems(&cd_path, &cd.problems) > 0 {
            self.script_print(crate::i18n::tr("The description has errors, so nothing was made.").into(), false);
            return;
        }
        let header = header_path(&cd_path);
        match crate::codec::write_file(&header, catalog::c_header(&cd, &stem_of(&cd_path)).as_bytes()) {
            Ok(()) => self.script_print(crate::i18n::tf!("The strings' C header: {}", header.display()), false),
            Err(e) => self.script_print(crate::i18n::tf!("{} couldn't be saved: {e}", header.display()), false),
        }
        let catalog_name = format!("{}.catalog", stem_of(&cd_path));
        for ct_path in &translations {
            let ct = catalog::read_translation(&self.text_of(ct_path).unwrap_or_default());
            let problems = catalog::check(&cd, &ct);
            if self.report_catalog_problems(ct_path, &problems) > 0 {
                self.script_print(crate::i18n::tf!("{} has errors, so its catalog wasn't made.", ct_path.display()), false);
                continue;
            }
            let language = catalog::language_of(&ct, ct_path);
            let target = catalog::catalog_path(&cd_path, &language);
            if let Some(parent) = target.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                self.script_print(crate::i18n::tf!("{} couldn't be made: {e}", parent.display()), false);
                continue;
            }
            let made = match &flexcat_program {
                Some(program) => std::process::Command::new(program)
                    .args(catalog::flexcat_arguments(&cd_path, ct_path, &target))
                    .output()
                    .map_err(|e| e.to_string())
                    .and_then(|out| {
                        for line in String::from_utf8_lossy(&out.stdout).lines().chain(String::from_utf8_lossy(&out.stderr).lines()) {
                            self.script_print(line.to_string(), false);
                        }
                        if out.status.success() { Ok(()) } else { Err(crate::i18n::tr("FlexCat stopped with an error.").into()) }
                    }),
                None => catalog::write_catalog(&cd, &ct, &catalog_name, &language, true).and_then(|bytes| crate::codec::write_file(&target, &bytes).map_err(|e| e.to_string())),
            };
            // What was made is read back, to say how many strings it holds.
            let held = std::fs::read(&target).ok().and_then(|bytes| catalog::read_catalog(&bytes).ok()).map_or(0, |c| c.strings.len());
            match made {
                Ok(()) => self.script_print(crate::i18n::tf!("{language}: {} ({held} translated strings in it)", target.display()), false),
                Err(problem) => self.script_print(crate::i18n::tf!("{}: {problem}", ct_path.display()), false),
            }
        }
    }

    /// A translation for the description in front: a new one to fill in, or one there already with the strings
    /// it's missing added to its end.
    pub(super) fn new_translation(&mut self) {
        let Some(cd_path) = self.docs.get(self.active).and_then(|d| d.path.clone()).filter(|p| ending_is(p, &["cd"])) else {
            return self.message("New translation", crate::i18n::tr("Open the catalog description (.cd) to translate first, saved as a file.").into());
        };
        let folder = cd_path.parent().map(|f| f.join("locale")).unwrap_or_default();
        let _ = std::fs::create_dir_all(&folder);
        let Some(ct_path) = rfd::FileDialog::new().set_title(crate::i18n::tr("The translation: named after its language, such as deutsch.ct")).set_directory(&folder).set_file_name("deutsch.ct").add_filter(crate::i18n::tr("Catalog translation"), &["ct"]).save_file() else {
            return;
        };
        let cd = catalog::read_description(&self.text_of(&cd_path).unwrap_or_default());
        let existing = std::fs::read(&ct_path).ok();
        // Kept in the encoding it's in: Latin 1 for the Amiga's, unless it's in UTF-8 already.
        let utf8 = existing.as_deref().is_some_and(|b| std::str::from_utf8(b).is_ok() && !b.is_ascii());
        let text = match &existing {
            Some(bytes) => {
                let (text, added) = catalog::add_missing(&cd, &catalog::decode(bytes));
                self.script_print(crate::i18n::tf!("{added} strings added to {} to translate.", ct_path.display()), true);
                text
            }
            None => {
                let language = ct_path.file_stem().map_or_else(String::new, |s| s.to_string_lossy().to_lowercase());
                let date = { use chrono::Datelike; let now = chrono::Local::now().date_naive(); catalog::amiga_date(now.year(), now.month(), now.day()) };
                crate::i18n::comments(&catalog::blank_translation(&cd, &stem_of(&cd_path), &language, &date))
            }
        };
        let bytes = if utf8 { text.into_bytes() } else { catalog::latin1(&text) };
        if let Err(e) = crate::codec::write_file(&ct_path, &bytes) {
            return self.message("New translation", crate::i18n::tf!("{} couldn't be saved: {e}", ct_path.display()));
        }
        self.open_path(&ct_path);
    }
}

fn stem_of(path: &Path) -> String {
    path.file_stem().map_or_else(|| "program".to_string(), |s| s.to_string_lossy().into_owned())
}

/// Where a description's C header goes: hello.cd's is hello_strings.h beside it.
fn header_path(cd: &Path) -> PathBuf {
    cd.with_file_name(format!("{}_strings.h", stem_of(cd)))
}
