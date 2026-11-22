//! The window's side of Python and R: the Python environment in use, shown in the status bar with a menu to choose
//! another or make one; F9 on a script; the console beside the editor (on a Jupyter kernel, see `repl`), which
//! Ctrl+Enter sends the line or selection to and a script's `# %%` cells run in; installing a project's packages
//! and knitting R Markdown as tasks.

use super::*;
use crate::code_cells::{self, Style};
use crate::notebook_output::{self as output, Shown};
use crate::py_env::{self, Env};
use crate::repl::{Console, Lang};
use crate::tasks::{Kind, Task};
use std::collections::HashMap;
use std::sync::mpsc::Receiver;

/// What's asked for from Tools, Python and R, and from a cell's links.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PyAction {
    /// Ctrl+Enter: the selection, or the statement at the cursor, to the console.
    SendToConsole,
    /// The cell the cursor's in; every cell; those above the cursor's and it.
    RunCell,
    RunAllCells,
    RunCellsAbove,
    /// From the links above a cell, by its marker's line.
    RunCellAt(usize),
    RunAboveAt(usize),
    InterruptConsole,
    RestartConsole,
    ClearConsole,
    MakeEnvironment,
    InstallPackages,
    FindEnvironments,
    KnitRender,
}

impl PyAction {
    pub(super) fn works_without_file(self) -> bool {
        matches!(self, PyAction::InterruptConsole | PyAction::RestartConsole | PyAction::ClearConsole)
    }
}

/// What the status bar's environment menu asked for, passed back through egui's memory as the bar can't change
/// anything itself.
#[derive(Clone, Debug, PartialEq)]
enum EnvAsk {
    Use(PathBuf),
    Make,
    Install(String),
    Again,
}

const ENV_ASKED: &str = "python-env-asked";

/// The environments found for the file in front, and the one used.
#[derive(Clone, Debug)]
pub(super) struct Current {
    pub folder: PathBuf,
    pub found: Vec<Env>,
    pub env: Env,
    when: std::time::Instant,
    file: PathBuf,
}

#[derive(Default)]
pub(super) struct PythonState {
    /// The environment chosen for each folder, kept in the settings file.
    pub chosen: py_env::Chosen,
    pub current: Option<Current>,
    /// Where Poetry said each folder's environment is, once asked; and the question being asked.
    poetry: HashMap<PathBuf, Option<PathBuf>>,
    poetry_asking: Option<(PathBuf, Receiver<Option<PathBuf>>)>,
    /// The consoles, one for each language, made when first wanted.
    python: Option<Console>,
    r: Option<Console>,
    /// The console shown in the panel.
    pub shown: Option<Lang>,
    pub open: bool,
    /// An environment being made in this folder, to choose once it's there.
    making: Option<PathBuf>,
    /// A file being knitted, the page it makes, and when it began, to open the page when it's done.
    knitting: Option<(PathBuf, std::time::SystemTime)>,
    /// Whether the Python environment has ipykernel, found away from the window, for the console's note.
    kernel_note: Option<Receiver<Option<String>>>,
    /// Pictures made into textures, by what they were made from.
    textures: HashMap<u64, eframe::egui::TextureHandle>,
    /// The input box's answer to a running `input()`.
    answer: String,
    /// Asked to put the keyboard in the console's box.
    focus_input: bool,
}

impl PythonState {
    pub fn from_settings(settings: &str) -> Self {
        let open = settings.lines().any(|l| l == "python_console=true");
        PythonState { chosen: py_env::Chosen::from_settings(settings), open, ..Default::default() }
    }

    pub fn to_settings(&self) -> String {
        self.chosen.to_settings() + if self.open { "python_console=true\n" } else { "" }
    }

    fn console(&mut self, lang: Lang) -> Option<&mut Console> {
        match lang {
            Lang::Python => self.python.as_mut(),
            Lang::R => self.r.as_mut(),
        }
    }
}

/// A hash of a picture's data, to know when it's a new one.
fn picture_key(value: &serde_json::Value) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.to_string().hash(&mut hasher);
    hasher.finish()
}

impl Limoniter {
    /// The script style of a document: Python or R code in a text tab (not a notebook).
    pub(super) fn code_style(&self, index: usize) -> Option<Style> {
        let doc = self.docs.get(index)?;
        if doc.placeholder || doc.hex_mode || doc.browser.is_some() || self.notebook_shown(index) {
            return None;
        }
        Style::for_language(&doc.language_name())
    }

    /// The folder a file's Python environments belong to: the project's, if the file is in it.
    fn python_folder_of(&self, file: &Path) -> PathBuf {
        if let Some(project) = &self.project
            && file.starts_with(project.folder())
        {
            return project.folder();
        }
        py_env::project_folder(file)
    }

    /// The Python a file is run with: the environment chosen for its folder, or the folder's own first one.
    pub(super) fn python_env_for(&self, file: &Path) -> Option<Env> {
        if let Some(current) = self.python.current.as_ref().filter(|c| file.starts_with(&c.folder)) {
            return Some(current.env.clone());
        }
        let folder = self.python_folder_of(file);
        let poetry = self.python.poetry.get(&folder).cloned().flatten();
        let found = py_env::find(&folder, poetry.as_deref());
        self.python.chosen.pick_for_project(&folder, &found, self.project.as_ref().and_then(|p| p.eff().python.clone()).as_deref())
    }

    /// The Python commands run with when nothing more particular is known: the one for the file in front.
    fn python_in_front(&self) -> Env {
        self.python.current.as_ref().map(|c| c.env.clone()).unwrap_or_else(Env::system)
    }

    /// Each frame: the file in front's cells and environment, Poetry's answer, the consoles' kernels, and what
    /// finished tasks leave to do. True while waiting on something.
    pub(super) fn python_tick(&mut self, ctx: &Context) -> bool {
        let mut busy = false;
        let style = self.code_style(self.active);
        if let Some(doc) = self.docs.get_mut(self.active) {
            doc.text_state.cells = style;
        }
        self.python_follow_front();
        if let Some((folder, receive)) = &self.python.poetry_asking
            && let Ok(answer) = receive.try_recv()
        {
            self.python.poetry.insert(folder.clone(), answer);
            self.python.poetry_asking = None;
            self.python.current = None;
            self.python_follow_front();
        }
        busy |= self.python.poetry_asking.is_some();
        if let Some(receive) = &self.python.kernel_note
            && let Ok(note) = receive.try_recv()
        {
            if let Some(console) = self.python.python.as_mut() {
                console.note = note;
            }
            self.python.kernel_note = None;
        }
        for console in [self.python.python.as_mut(), self.python.r.as_mut()].into_iter().flatten() {
            console.session.set_repaint(ctx);
            console.pump();
            busy |= console.session.is_running_anything() || console.status() == crate::jupyter::link::KernelStatus::Starting;
        }
        // An environment made: it's found, and chosen.
        if self.run_job.is_none()
            && let Some(folder) = self.python.making.take()
        {
            let made = folder.join(".venv");
            if let Some(python) = py_env::python_in(&made) {
                self.python.chosen.set(&folder, &python);
                self.python.current = None;
                self.python_follow_front();
                self.save_settings_if_changed();
                self.script_print(crate::i18n::tf!("The environment {} is made, and used for this folder's Python.", made.display()), false);
            }
        }
        // A page knitted: it's opened, if it was made afresh.
        if self.run_job.is_none()
            && let Some((page, began)) = self.python.knitting.take()
        {
            let fresh = std::fs::metadata(&page).and_then(|m| m.modified()).is_ok_and(|when| when >= began);
            if fresh {
                let _ = crate::platform::open_web_page(&crate::lsp::uri(&page));
            }
        }
        busy
    }

    /// Finds the environments for the Python file in front, now and then, and tells what uses Python of the one
    /// chosen when it changes.
    fn python_follow_front(&mut self) {
        let Some(path) = self.docs.get(self.active).filter(|_| self.code_style(self.active) == Some(Style::Python) || self.notebook_shown(self.active)).and_then(|d| d.path.clone()) else {
            return;
        };
        if self.python.current.as_ref().is_some_and(|c| c.file == path && c.when.elapsed() < Duration::from_secs(5)) {
            return;
        }
        let folder = self.python_folder_of(&path);
        if py_env::uses_poetry(&folder) && !self.python.poetry.contains_key(&folder) && self.python.poetry_asking.is_none() {
            let (send, receive) = std::sync::mpsc::channel();
            let asked = folder.clone();
            std::thread::spawn(move || {
                let _ = send.send(py_env::poetry_env(&asked));
            });
            self.python.poetry_asking = Some((folder.clone(), receive));
        }
        let poetry = self.python.poetry.get(&folder).cloned().flatten();
        let found = py_env::find(&folder, poetry.as_deref());
        let Some(env) = self.python.chosen.pick_for_project(&folder, &found, self.project.as_ref().and_then(|p| p.eff().python.clone()).as_deref()) else { return };
        let changed = self.python.current.as_ref().is_none_or(|c| c.env != env || c.folder != folder);
        self.python.current = Some(Current { folder: folder.clone(), found, env: env.clone(), when: std::time::Instant::now(), file: path });
        if changed {
            py_env::set_chosen(Some(env.clone()));
            self.lsp.tell_python_servers(&folder, &py_env::lsp_settings(&env));
        }
    }

    /// Uses another environment for the folder of the file in front.
    pub(super) fn python_use_env(&mut self, python: &Path) {
        let Some(current) = self.python.current.clone() else { return };
        let Some(env) = current.found.iter().find(|e| e.python == python).cloned() else { return };
        self.python.chosen.set(&current.folder, python);
        self.python.current = Some(Current { env: env.clone(), when: std::time::Instant::now(), ..current.clone() });
        py_env::set_chosen(Some(env.clone()));
        self.lsp.tell_python_servers(&current.folder, &py_env::lsp_settings(&env));
        self.save_settings_if_changed();
        // The console's kernel was started on the other Python: it starts again on this one when next wanted.
        if let Some(mut console) = self.python.python.take() {
            console.session.stop();
            self.script_print(crate::i18n::tf!("The Python console will start again with {}.", env.status_words()), false);
        }
    }

    /// The environment in the status bar, for a Python file: a click lists the others, and making or filling one.
    pub(super) fn python_status(&self, ui: &mut Ui) {
        let Some(current) = self.python.current.as_ref().filter(|_| self.code_style(self.active) == Some(Style::Python) || self.notebook_shown(self.active)) else {
            return;
        };
        let words = current.env.status_words();
        let shown = ui.add(egui::Label::new(RichText::new(&words).color(theme::text_faint())).sense(egui::Sense::click()));
        crate::access::button(&shown, &words);
        let shown = shown.on_hover_text(crate::i18n::tf!("The Python this folder's files are run, debugged and tested with: {}. Click to choose another", current.env.python.display()));
        let mut asked = None;
        crate::fit::popup_menu(&shown, |ui| {
            ui.set_min_width(260.0);
            ui.label(RichText::new(crate::i18n::tr("Python environment")).strong());
            for env in &current.found {
                let label = format!("{} ({})", env.name, crate::i18n::tr(env.kind.label()));
                if ui.add(egui::Button::selectable(env.python == current.env.python, label)).on_hover_text(env.python.display().to_string()).clicked() {
                    asked = Some(EnvAsk::Use(env.python.clone()));
                    ui.close();
                }
            }
            ui.separator();
            if !current.folder.join(".venv").exists() && ui.add(egui::Button::new(crate::i18n::tr("Make a new environment (.venv)")).frame(false)).on_hover_text(py_env::make_command(crate::lsp::on_path("uv"))).clicked() {
                asked = Some(EnvAsk::Make);
                ui.close();
            }
            for (what, command) in py_env::install_commands(&current.folder, &current.env, crate::lsp::on_path("uv")) {
                if ui.add(egui::Button::new(what).frame(false)).on_hover_text(command.as_str()).clicked() {
                    asked = Some(EnvAsk::Install(command));
                    ui.close();
                }
            }
            if ui.add(egui::Button::new(crate::i18n::tr("Look for environments again")).frame(false)).clicked() {
                asked = Some(EnvAsk::Again);
                ui.close();
            }
        });
        if let Some(asked) = asked {
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(ENV_ASKED), Some(asked)));
        }
    }

    /// Does what the status bar's environment menu asked.
    pub(super) fn python_status_asked(&mut self, ctx: &Context) {
        let Some(asked) = ctx.data_mut(|d| d.remove_temp::<Option<EnvAsk>>(egui::Id::unique(ENV_ASKED))).flatten() else { return };
        match asked {
            EnvAsk::Use(python) => self.python_use_env(&python),
            EnvAsk::Make => self.python_command(PyAction::MakeEnvironment),
            EnvAsk::Install(command) => self.run_python_task(crate::i18n::tr("Install packages"), &command),
            EnvAsk::Again => self.python_command(PyAction::FindEnvironments),
        }
    }

    fn run_python_task(&mut self, name: &str, command: &str) {
        self.run_task(Task { name: name.to_string(), command: command.to_string(), kind: Kind::Other });
    }

    /// Whether a Python and R command can be chosen now.
    pub(super) fn python_can(&self, action: PyAction) -> bool {
        let style = self.code_style(self.active);
        match action {
            PyAction::SendToConsole | PyAction::RunCell | PyAction::RunAllCells | PyAction::RunCellsAbove | PyAction::RunCellAt(_) | PyAction::RunAboveAt(_) => style.is_some(),
            PyAction::InterruptConsole | PyAction::RestartConsole | PyAction::ClearConsole => self.python.python.is_some() || self.python.r.is_some(),
            PyAction::MakeEnvironment | PyAction::InstallPackages | PyAction::FindEnvironments => style == Some(Style::Python) || self.notebook_shown(self.active),
            PyAction::KnitRender => self.docs.get(self.active).and_then(|d| d.path.as_deref()).is_some_and(crate::rmarkdown::is_rmarkdown),
        }
    }

    pub(super) fn python_command(&mut self, action: PyAction) {
        match action {
            PyAction::SendToConsole => self.send_from_editor(),
            PyAction::RunCell => self.run_cells(Which::Here),
            PyAction::RunAllCells => self.run_cells(Which::All),
            PyAction::RunCellsAbove => self.run_cells(Which::Above),
            PyAction::RunCellAt(line) => self.run_cells_at(line, Which::Here),
            PyAction::RunAboveAt(line) => self.run_cells_at(line, Which::Above),
            PyAction::InterruptConsole | PyAction::RestartConsole | PyAction::ClearConsole => {
                let lang = self.python.shown.unwrap_or(Lang::Python);
                if let Some(console) = self.python.console(lang) {
                    match action {
                        PyAction::InterruptConsole => console.interrupt(),
                        PyAction::RestartConsole => console.restart(),
                        _ => console.clear(),
                    }
                }
            }
            PyAction::MakeEnvironment => {
                let Some(folder) = self.python.current.as_ref().map(|c| c.folder.clone()) else { return };
                if folder.join(".venv").exists() {
                    return self.message("Python environment", crate::i18n::tf!("{} already has a .venv.", folder.display()));
                }
                let command = py_env::make_command(crate::lsp::on_path("uv"));
                match self.run_tool_in(&command, false, Some(folder.clone())) {
                    Ok(()) => self.python.making = Some(folder),
                    Err(problem) => self.message("Python environment", problem),
                }
            }
            PyAction::InstallPackages => {
                let Some(current) = self.python.current.clone() else { return };
                match py_env::install_commands(&current.folder, &current.env, crate::lsp::on_path("uv")).into_iter().next() {
                    Some((name, command)) => self.run_python_task(&name, &command),
                    None => self.message("Install packages", crate::i18n::tr("There's no requirements.txt, pyproject.toml or environment.yml in the project's folder to install from.").into()),
                }
            }
            PyAction::FindEnvironments => {
                if let Some(folder) = self.python.current.as_ref().map(|c| c.folder.clone()) {
                    self.python.poetry.remove(&folder);
                }
                self.python.current = None;
                self.python_follow_front();
            }
            PyAction::KnitRender => self.knit(),
        }
    }

    /// Tasks for Python and R in a folder: installing the packages into the environment, and knitting the R
    /// Markdown or Quarto file in front. pytest's task is made to use the environment's Python.
    pub(super) fn python_tasks(&self, folder: &Path, tasks: &mut Vec<Task>) {
        let file = self.docs.get(self.active).and_then(|d| d.path.clone()).filter(|_| !self.no_file());
        let env = file.as_deref().and_then(|f| self.python_env_for(f)).filter(|e| e.kind != py_env::Kind::System);
        if let Some(env) = &env {
            for task in tasks.iter_mut() {
                if task.command.starts_with("python ") {
                    task.command = py_env::with_python(&task.command, env);
                }
            }
            for (name, command) in py_env::install_commands(folder, env, crate::lsp::on_path("uv")) {
                tasks.push(Task { name, command, kind: Kind::Other });
            }
        }
        if let Some(file) = file.filter(|f| crate::rmarkdown::is_rmarkdown(f))
            && let Some(command) = crate::rmarkdown::render_command(&file, crate::lsp::on_path("quarto"), crate::lsp::on_path("Rscript"))
        {
            tasks.push(Task { name: crate::i18n::tr("Knit / Render to HTML").to_string(), command, kind: Kind::Build });
        }
    }

    /// Knits the R Markdown or renders the Quarto file in front, opening the page once it's made.
    fn knit(&mut self) {
        let Some(path) = self.docs.get(self.active).and_then(|d| d.path.clone()) else { return };
        let Some(command) = crate::rmarkdown::render_command(&path, crate::lsp::on_path("quarto"), crate::lsp::on_path("Rscript")) else {
            return self.message("Knit / Render", crate::i18n::tr("Knitting needs R with the rmarkdown package (and Pandoc), or Quarto (quarto.org) for Quarto files. Neither was found.").into());
        };
        if self.docs[self.active].is_modified() && !self.save(self.active) {
            return;
        }
        match self.run_tool_in(&command, false, path.parent().map(Path::to_path_buf)) {
            Ok(()) => self.python.knitting = Some((crate::rmarkdown::output_html(&path), std::time::SystemTime::now() - Duration::from_secs(1))),
            Err(problem) => self.message("Knit / Render", problem),
        }
    }

    /// F9 on a Python or R script: it runs, with the environment's Python or with Rscript, printing into the
    /// Output panel.
    pub(super) fn python_run_on_f9(&mut self) -> bool {
        let Some(style) = self.code_style(self.active) else { return false };
        let index = self.active;
        if self.docs[index].path.is_none() && !self.save_as(index) {
            return true;
        }
        let Some(path) = self.docs[index].path.clone() else { return true };
        let program = match style {
            Style::Python => self.python_env_for(&path).unwrap_or_else(Env::system).command(),
            Style::R => if cfg!(windows) { "Rscript.exe".to_string() } else { "Rscript".to_string() },
        };
        let line = format!("{program} {}", py_env::quoted(&path.display().to_string()));
        if let Err(problem) = self.run_tool_in(&line, true, path.parent().map(Path::to_path_buf)) {
            self.message("Run", problem);
        }
        true
    }

    /// The console for a language, started if it isn't: on the environment's Python, for Python.
    fn console_for(&mut self, lang: Lang) -> &mut Console {
        let folder = self.task_folder().unwrap_or_else(std::env::temp_dir);
        let env = self.python_in_front();
        let slot = match lang {
            Lang::Python => &mut self.python.python,
            Lang::R => &mut self.python.r,
        };
        let console = slot.get_or_insert_with(|| Console::new(lang, crate::notebook_run::kernels(), &folder));
        if !console.has_kernel() {
            let kernels = crate::notebook_run::kernels();
            let specs = (kernels.list)();
            let spec = crate::repl::pick_spec(&specs, lang).or_else(|| {
                // A Python environment with ipykernel needs no kernel of its own to be installed.
                (lang == Lang::Python && env.kind != py_env::Kind::System).then(|| crate::jupyter::link::KernelSpec {
                    name: "python3".into(),
                    display_name: env.status_words(),
                    language: "python".into(),
                    argv: vec![env.python.display().to_string(), "-m".into(), "ipykernel_launcher".into(), "-f".into(), "{connection_file}".into()],
                    env: Vec::new(),
                    dir: PathBuf::new(),
                })
            });
            match spec {
                Some(spec) => {
                    console.note = None;
                    console.session.set_folder(&folder);
                    console.start(spec.clone());
                    if lang == Lang::Python && env.kind != py_env::Kind::System {
                        let (send, receive) = std::sync::mpsc::channel();
                        std::thread::spawn(move || {
                            let has = py_env::has_module(&env.python, "ipykernel");
                            let _ = send.send(py_env::kernel_in(&spec, &env, has).1);
                        });
                        self.python.kernel_note = Some(receive);
                    }
                }
                None => {
                    let install = crate::jupyter::specs::install_command(if lang == Lang::R { "R" } else { "python" }).map(|(c, _)| c).unwrap_or_default();
                    console.note = Some(crate::i18n::tf!("There's no Jupyter kernel for {} on this computer, and the console runs on one. To install it, in a terminal: {install}", lang.name()));
                }
            }
        }
        match lang {
            Lang::Python => self.python.python.as_mut().expect("made above"),
            Lang::R => self.python.r.as_mut().expect("made above"),
        }
    }

    /// Runs code in the console for a language, opening the panel to show it.
    pub(super) fn send_to_console(&mut self, lang: Lang, code: &str) {
        self.python.open = true;
        self.python.shown = Some(lang);
        let console = self.console_for(lang);
        if !console.send(code) && console.note.is_none() {
            console.note = Some(crate::i18n::tr("The console's kernel couldn't run it. Restart the console and try again.").to_string());
        }
        self.save_settings_if_changed();
    }

    /// Ctrl+Enter: the selection, or else the statement at the cursor (moving on past it), to the console.
    fn send_from_editor(&mut self) {
        let Some(style) = self.code_style(self.active) else { return };
        let doc = &mut self.docs[self.active];
        let code = if doc.text.has_selection() {
            doc.text.selection_text()
        } else {
            let all = doc.text.all_lines();
            let lines: Vec<&str> = all.iter().map(|l| l.as_ref()).collect();
            let (start, end) = code_cells::statement_at(&lines, doc.text.caret_line, style);
            let next = code_cells::next_code_line(&lines, end);
            let code = code_cells::code_of(&lines, start, end);
            doc.text.move_caret(next, 0, false);
            doc.text.reveal = true;
            code
        };
        if !code.trim().is_empty() {
            self.send_to_console(Lang::from_style(style), &code);
        }
    }

    fn run_cells(&mut self, which: Which) {
        let line = self.docs[self.active].text.caret_line;
        self.run_cells_at(line, which);
    }

    /// Runs cells in the console: the one a line is in, those above it and it, or all of them. In a file with no
    /// markers, the whole file is one cell.
    fn run_cells_at(&mut self, line: usize, which: Which) {
        let Some(style) = self.code_style(self.active) else { return };
        let all = self.docs[self.active].text.all_lines();
        let lines: Vec<&str> = all.iter().map(|l| l.as_ref()).collect();
        let mut cells = code_cells::cells(&lines, style);
        if cells.is_empty() {
            cells.push((0, lines.len()));
        }
        // A marker's own line belongs to the cell it starts.
        let here = cells.iter().position(|&(start, end)| line + 1 >= start && line < end.max(start)).unwrap_or(0);
        let chosen: Vec<(usize, usize)> = match which {
            Which::Here => cells[here..=here].to_vec(),
            Which::Above => cells[..=here].to_vec(),
            Which::All => cells,
        };
        let codes: Vec<String> = chosen.iter().map(|&(start, end)| code_cells::code_of(&lines, start, end)).filter(|c| !c.trim().is_empty()).collect();
        for code in codes {
            self.send_to_console(Lang::from_style(style), &code);
        }
    }

    /// The console panel, at the right, if it's open.
    pub(super) fn console_panel(&mut self, ui: &mut Ui) {
        if !self.python.open {
            return;
        }
        let mut open = true;
        let font_size = self.view.font_size;
        let dark = !theme::is_light();
        let mut acts = Vec::new();
        let state = &mut self.python;
        crate::panels::dock_right(ui, "console-panel", "Console", 420.0, &mut open, |ui| {
            acts = draw_console(ui, state, font_size, dark);
        });
        for act in acts {
            match act {
                PanelAct::Show(lang) => {
                    self.python.shown = Some(lang);
                    self.console_for(lang);
                }
                PanelAct::Do(action) => self.python_command(action),
            }
        }
        if !open {
            self.python.open = false;
            self.save_settings_if_changed();
        }
    }

    /// A step of a picture of the console (see `screenshot_step`): code to send, as "python;code" or "r;code".
    pub(super) fn console_screenshot_step(&mut self, value: &str) {
        let (lang, code) = value.split_once(';').unwrap_or(("python", value));
        let lang = if lang.eq_ignore_ascii_case("r") { Lang::R } else { Lang::Python };
        self.send_to_console(lang, &code.replace("\\n", "\n"));
        let began = std::time::Instant::now();
        while began.elapsed() < Duration::from_secs(60) {
            let console = self.console_for(lang);
            console.pump();
            if console.has_kernel() && !console.session.is_running_anything() && console.status() == crate::jupyter::link::KernelStatus::Idle && console.book.cells.last().is_some_and(|c| c.execution_count.is_some()) {
                break;
            }
            if console.session.problem.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[cfg(test)]
impl Limoniter {
    /// What a console has been sent, entry by entry.
    pub(super) fn python_console_sources(&mut self, lang: Lang) -> Vec<String> {
        self.python.console(lang).map(|c| c.book.cells.iter().map(|cell| cell.source.clone()).collect()).unwrap_or_default()
    }

    pub(super) fn python_console_transcript(&mut self, lang: Lang) -> String {
        self.python.console(lang).map(|c| c.transcript()).unwrap_or_default()
    }
}

/// Which cells to run.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Which {
    Here,
    Above,
    All,
}

enum PanelAct {
    Show(Lang),
    Do(PyAction),
}

/// The console: which language's, its kernel's state and buttons, the plot shown, what's been run, and the box to
/// type in. Enter runs what's typed (Shift+Enter starts another line); up and down go through what was typed.
fn draw_console(ui: &mut Ui, state: &mut PythonState, font_size: f32, dark: bool) -> Vec<PanelAct> {
    let mut acts = Vec::new();
    let lang = state.shown.unwrap_or(Lang::Python);
    ui.horizontal(|ui| {
        for each in [Lang::Python, Lang::R] {
            if ui.add(egui::Button::selectable(each == lang, crate::i18n::tr(each.name()))).clicked() && each != lang {
                acts.push(PanelAct::Show(each));
            }
        }
        if let Some(console) = state.console(lang) {
            let (words, colour) = match console.status() {
                _ if !console.has_kernel() => ("Not started", theme::text_faint()),
                crate::jupyter::link::KernelStatus::Idle => ("Idle", theme::git_added()),
                crate::jupyter::link::KernelStatus::Busy => ("Busy", theme::stopped_mark()),
                crate::jupyter::link::KernelStatus::Starting => ("Starting", theme::stopped_mark()),
                crate::jupyter::link::KernelStatus::Dead => ("Stopped", theme::problem_mark()),
            };
            ui.label(RichText::new(crate::i18n::tr(words)).color(colour).size(12.0));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button(crate::i18n::tr("Clear")).on_hover_text(crate::i18n::tr("Clear what's shown; the variables stay")).clicked() {
                acts.push(PanelAct::Do(PyAction::ClearConsole));
            }
            if ui.small_button(crate::i18n::tr("Restart")).on_hover_text(crate::i18n::tr("Start the kernel afresh: its variables are lost")).clicked() {
                acts.push(PanelAct::Do(PyAction::RestartConsole));
            }
            if ui.small_button(crate::i18n::tr("Stop")).on_hover_text(crate::i18n::tr("Interrupt what's running")).clicked() {
                acts.push(PanelAct::Do(PyAction::InterruptConsole));
            }
            if let Some(console) = state.console(lang)
                && ui.small_button(crate::i18n::tr("Copy")).on_hover_text(crate::i18n::tr("Copy everything shown, as text")).clicked()
            {
                ui.ctx().copy_text(console.transcript());
            }
        });
    });
    let PythonState { python, r, textures, answer, focus_input, .. } = state;
    let Some(console) = (match lang {
        Lang::Python => python.as_mut(),
        Lang::R => r.as_mut(),
    }) else {
        ui.add_space(8.0);
        ui.label(RichText::new(crate::i18n::tf!("Press Ctrl+Enter in a {} file to send the line or selection here, or type below.", lang.name())).color(theme::text_dim()));
        if ui.button(crate::i18n::tf!("Start the {} console", lang.name())).clicked() {
            acts.push(PanelAct::Show(lang));
        }
        return acts;
    };
    if let Some(note) = console.note.clone().or_else(|| console.session.problem.clone()) {
        ui.label(RichText::new(note).color(theme::problem_mark()).size(12.0));
    }
    let font = egui::FontId::monospace(font_size);
    // The plot shown, with the others kept to step back and forth through.
    if let Some(plot) = console.plot().cloned() {
        let shown = output::choose(&plot);
        let key = picture_key(&plot);
        if !textures.contains_key(&key)
            && let Some(texture) = super::notebook_host::texture_of(ui.ctx(), format!("console-plot-{key}"), &shown)
        {
            textures.insert(key, texture);
        }
        if let Some(texture) = textures.get(&key) {
            let size = texture.size_vec2();
            let scale = (ui.available_width() / size.x).min(280.0 / size.y).min(1.0);
            ui.add(egui::Image::new((texture.id(), size * scale)));
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(console.plot_at > 0, egui::Button::new(crate::i18n::tr("\u{25C0}")).small()).on_hover_text(crate::i18n::tr("The plot before")).clicked() {
                console.plot_back();
            }
            ui.label(RichText::new(crate::i18n::tf!("Plot {} of {}", console.plot_at + 1, console.plots.len())).size(12.0).color(theme::text_dim()));
            if ui.add_enabled(console.plot_at + 1 < console.plots.len(), egui::Button::new(crate::i18n::tr("\u{25B6}")).small()).on_hover_text(crate::i18n::tr("The plot after")).clicked() {
                console.plot_forward();
            }
            if ui.small_button(crate::i18n::tr("Clear plots")).clicked() {
                console.clear_plots();
                textures.clear();
            }
        });
        ui.separator();
    }
    let syntax = crate::highlight::syntax_named(lang.name());
    let input_height = 90.0;
    egui::ScrollArea::vertical().id_salt(("console-transcript", lang.name())).stick_to_bottom(true).auto_shrink([false, false]).max_height((ui.available_height() - input_height).max(60.0)).show(ui, |ui| {
        for cell in &console.book.cells {
            let mut job = egui::text::LayoutJob::default();
            job.wrap.max_width = f32::INFINITY;
            job.append(&format!("{} ", lang.prompt()), 0.0, egui::TextFormat::simple(font.clone(), theme::text_faint()));
            for (piece, colour) in crate::highlight::colour_pieces(syntax, &cell.source) {
                job.append(&piece, 0.0, egui::TextFormat::simple(font.clone(), colour));
            }
            ui.add(egui::Label::new(job).selectable(true));
            for out in &cell.outputs {
                match output::choose(out) {
                    Shown::Png(_) | Shown::Jpeg(_) | Shown::Svg(_) => {
                        ui.label(RichText::new(crate::i18n::tr("(A plot, shown above)")).color(theme::text_faint()).size(12.0));
                    }
                    Shown::Stream { name, text } => {
                        let colour = if name == "stderr" { theme::problem_mark() } else { theme::editor_text() };
                        ui.add(egui::Label::new(super::notebook_host::ansi_job(text.trim_end_matches('\n'), &font, dark, colour)).selectable(true));
                    }
                    Shown::Error { name, message, traceback } => {
                        let text = if traceback.trim().is_empty() { format!("{name}: {message}") } else { traceback };
                        ui.add(egui::Label::new(super::notebook_host::ansi_job(&text, &font, dark, theme::problem_mark())).selectable(true));
                    }
                    Shown::Markdown(text) => {
                        let blocks = crate::preview::render(crate::preview::Kind::Markdown, &text);
                        crate::preview::draw_inline(ui, &blocks);
                    }
                    _ => {
                        let text = output::plain_text_of(out);
                        ui.add(egui::Label::new(super::notebook_host::ansi_job(text.trim_end_matches('\n'), &font, dark, theme::editor_text())).selectable(true));
                    }
                }
            }
        }
    });
    ui.separator();
    // A running input() asks here.
    if let Some(ask) = console.session.input.clone() {
        let mut send = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new(&ask.prompt).monospace());
            let field = ui.add(egui::TextEdit::singleline(answer).password(ask.password).desired_width(200.0));
            send = (field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) || ui.button(crate::i18n::tr("Send")).clicked();
        });
        if send {
            let text = std::mem::take(answer);
            console.session.answer(&text);
        }
        return acts;
    }
    let id = egui::Id::unique(match lang {
        Lang::Python => "console-input-python",
        Lang::R => "console-input-r",
    });
    let focused = ui.ctx().memory(|m| m.has_focus(id));
    if focused {
        let single = !console.draft.contains('\n');
        let (enter, up, down) = ui.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, egui::Key::Enter), single && i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp), single && i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)));
        if enter {
            console.send_draft();
        }
        if up {
            console.history_back();
        }
        if down {
            console.history_forward();
        }
    }
    ui.horizontal_top(|ui| {
        ui.label(RichText::new(lang.prompt()).font(font.clone()).color(theme::text_faint()));
        let edit = egui::TextEdit::multiline(&mut console.draft).id(id).font(font.clone()).desired_rows(2).desired_width(f32::INFINITY).hint_text(crate::i18n::tr("Type code, then Enter. Shift+Enter for another line; up and down for what was typed"));
        let response = ui.add(edit);
        if std::mem::take(focus_input) {
            response.request_focus();
        }
    });
    acts
}
