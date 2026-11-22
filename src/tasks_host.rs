//! The window's side of tasks: Tools, Tasks lists what a project's files say and the project's own; a task runs
//! in its folder with what it prints in the Output panel; when it's done, the problems it printed are marked by
//! their lines in the files open, and a test task's results are listed test by test, failures first.

use super::*;
use crate::assemblers::Cpu;
use crate::tasks::{Kind, Task, TestResult};

/// A task running, to read back what it printed when it's done.
pub(super) struct TaskRun {
    pub task: Task,
    pub folder: PathBuf,
    /// Where its output starts in the Output panel.
    pub from: usize,
}

/// A test task's results.
pub(super) struct TestPanel {
    pub task: Task,
    pub folder: PathBuf,
    pub results: Vec<TestResult>,
    /// For assembly tests run on the built-in processors: the source they're in, and its processor.
    pub assembly: Option<(PathBuf, Cpu)>,
}

impl Limoniter {
    /// The folder tasks run in: the project's, or the one above the file in front with a project's own file.
    pub(super) fn task_folder(&self) -> Option<PathBuf> {
        if let Some(project) = &self.project {
            return Some(project.folder());
        }
        let doc = self.docs.get(self.active)?;
        doc.path.as_deref().map(crate::tasks::root_for)
    }

    /// The tasks there are: the project's own, then what the folder's files say.
    pub(super) fn tasks_here(&self) -> Vec<Task> {
        let mut tasks: Vec<Task> = self.project.as_ref().map(|p| p.tasks.clone()).unwrap_or_default();
        if let Some(folder) = self.task_folder() {
            // The languages of the files open in the folder say which microcontroller's flashing to offer.
            let languages: Vec<String> = self.docs.iter().filter(|d| d.path.as_deref().is_some_and(|p| p.starts_with(&folder))).map(|d| d.language_name()).collect();
            for detected in crate::tasks::detect(&folder).into_iter().chain(self.flashing.tasks(&folder, &languages)) {
                if !tasks.iter().any(|t| t.command == detected.command) {
                    tasks.push(detected);
                }
            }
            // Python's run in the file's environment; its packages installed into it; R Markdown knitted.
            self.python_tasks(&folder, &mut tasks);
        }
        for update in self.outdated_tasks() {
            if !tasks.iter().any(|t| t.command == update.command) {
                tasks.push(update);
            }
        }
        tasks
    }

    pub(super) fn run_task(&mut self, task: Task) {
        let Some(folder) = self.task_folder() else {
            return self.message("Tasks", crate::i18n::tr("Open a file in the project first: tasks run in its folder.").into());
        };
        // What was marked from the last build goes.
        for doc in &mut self.docs {
            if let Some(intel) = doc.text.intel.as_mut() {
                intel.build_problems.clear();
            }
        }
        if task.kind == Kind::Test {
            self.test_panel = None;
        }
        // Everything open is saved first, as a build wants it.
        for index in 0..self.docs.len() {
            if self.docs[index].path.is_some() && self.docs[index].is_modified() && !self.docs[index].text.read_only {
                self.save(index);
            }
        }
        let from = self.script_output.len();
        match self.run_tool_in(&task.command, false, Some(folder.clone())) {
            Ok(()) => {
                let from = if self.script_output.len() <= from { 0 } else { from };
                self.task_run = Some(TaskRun { task, folder, from });
            }
            Err(problem) => self.message("Tasks", problem),
        }
    }

    /// The first build task, for Ctrl+Shift+B.
    pub(super) fn run_build_task(&mut self) {
        match self.tasks_here().into_iter().find(|t| t.kind == Kind::Build) {
            Some(task) => self.run_task(task),
            None => self.message("Build", crate::i18n::tr("There's no build task here: no Cargo.toml, Makefile, package.json, go.mod or CMakeLists.txt above the file, and the project has none of its own (Tools, Tasks, Edit the project's tasks).").into()),
        }
    }

    pub(super) fn run_test_task(&mut self) {
        // Assembly with tests written beside it runs them on the built-in processor, with no task needed.
        if let Some(cpu) = self.doc_cpu(self.active).filter(|c| crate::asm_tests::runs_on(*c)) {
            let doc = &self.docs[self.active];
            let text = doc.text.all_lines().iter().map(|l| l.as_ref()).collect::<Vec<&str>>().join("\n");
            if crate::asm_tests::has_tests(doc.path.as_deref(), &text) {
                let path = std::path::absolute(doc.path_or_name()).unwrap_or_else(|_| doc.path_or_name());
                return self.run_assembly_tests(&path, cpu, None);
            }
        }
        match self.tasks_here().into_iter().find(|t| t.kind == Kind::Test) {
            Some(task) => self.run_task(task),
            None => self.message("Tests", crate::i18n::tr("There's no test task here.").into()),
        }
    }

    /// When a task has finished: its problems marked by their lines, and a test task's results listed.
    pub(super) fn task_tick(&mut self, running: bool) {
        if running {
            return;
        }
        let Some(run) = self.task_run.take() else {
            return;
        };
        let lines: Vec<String> = self.script_output.get(run.from..).unwrap_or(&[]).to_vec();
        let mut marked = 0;
        for line in &lines {
            let Some((file, number, words)) = crate::tasks::place_in(line).or_else(|| crate::assemblers::problem_line(line)) else {
                continue;
            };
            let path = if Path::new(&file).is_absolute() { PathBuf::from(&file) } else { run.folder.join(&file) };
            if let Some(doc) = self.docs.iter_mut().find(|d| d.path.as_deref().is_some_and(|p| same_path(p, &path))) {
                let intel = doc.text.intel.get_or_insert_with(Box::default);
                if !intel.build_problems.iter().any(|(l, _)| *l + 1 == number) {
                    intel.build_problems.push((number.saturating_sub(1), if words.is_empty() { crate::i18n::tr("A problem here").into() } else { words }));
                    marked += 1;
                }
            }
        }
        if marked > 0 {
            self.script_print(if marked == 1 { crate::i18n::tf!("{marked} problem marked in the files open.") } else { crate::i18n::tf!("{marked} problems marked in the files open.") }, false);
        }
        if run.task.kind == Kind::Test {
            let mut results = crate::tasks::test_results(&lines);
            results.sort_by_key(|r| r.passed);
            self.mark_test_failures(&run.folder, &results);
            self.tests_took(&results);
            self.test_panel = Some(TestPanel { task: run.task, folder: run.folder, results, assembly: None });
        }
    }

    pub(super) fn tests_panel(&mut self, ui: &mut Ui) {
        let Some(panel) = &self.test_panel else {
            return;
        };
        let (passed, failed) = (panel.results.iter().filter(|r| r.passed).count(), panel.results.iter().filter(|r| !r.passed).count());
        let mut close = false;
        let mut again = false;
        let mut only = None;
        let mut go = None;
        Panel::right("tests-panel").default_size(320.0).resizable(true).show(ui, |ui| {
            ui.horizontal(|ui| {
                let summary = format!("{passed} passed, {failed} failed");
                ui.label(RichText::new(summary).strong().color(if failed > 0 { theme::problem_mark() } else { theme::accent() }));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = crate::access::called(ui.small_button(crate::i18n::tr("\u{D7}")), "Close").clicked();
                    again = ui.small_button(crate::i18n::tr("Run again")).clicked();
                });
            });
            ui.label(RichText::new(crate::i18n::tf!("{} in {}", panel.task.command, panel.folder.display())).size(11.5).color(theme::text_faint()));
            if panel.results.is_empty() {
                ui.label(RichText::new(crate::i18n::tr("No test results were found in what it printed: see the Output panel.")).color(theme::text_dim()));
            }
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for result in &panel.results {
                    ui.horizontal(|ui| {
                        let (mark, colour) = if result.passed { ("\u{2713}", theme::text_dim()) } else { ("\u{2717}", theme::problem_mark()) };
                        ui.label(RichText::new(mark).color(colour));
                        let name = crate::access::clickable(ui.add(egui::Label::new(RichText::new(&result.name).monospace().size(12.0).color(if result.passed { theme::text_dim() } else { theme::problem_mark() })).sense(egui::Sense::click())));
                        if let Some(place) = &result.place {
                            name.clone().on_hover_text(crate::i18n::tf!("{}, line {}", place.0, place.1));
                            if name.clicked() {
                                go = Some(place.clone());
                            }
                        }
                        if let Some(note) = &result.note {
                            ui.label(RichText::new(note).size(11.5).color(theme::text_faint()));
                        }
                        if !result.passed && ui.small_button(crate::i18n::tr("Run only this")).clicked() {
                            only = Some(result.name.clone());
                        }
                    });
                    for line in &result.detail {
                        ui.horizontal(|ui| {
                            ui.add_space(18.0);
                            ui.add(egui::Label::new(RichText::new(line).size(11.5).color(theme::text_dim())).wrap());
                        });
                    }
                }
            });
        });
        let (task, folder, assembly) = (panel.task.clone(), panel.folder.clone(), panel.assembly.clone());
        let assembly_none = assembly.is_none();
        if close {
            self.test_panel = None;
        }
        if let Some((path, cpu)) = assembly.filter(|_| again || only.is_some()) {
            self.run_assembly_tests(&path, cpu, only.as_deref());
        } else if again {
            self.run_task(task.clone());
        }
        if let Some(name) = only.filter(|_| assembly_none) {
            self.run_task(Task { name: format!("{} {name}", task.name), command: crate::tasks::only_this(&task.command, &name), kind: Kind::Test });
        }
        if let Some((file, line)) = go {
            let path = if Path::new(&file).is_absolute() { PathBuf::from(&file) } else { folder.join(&file) };
            self.go_to_place(&path.display().to_string(), line);
        }
    }

    /// The assembly tests of the source in front, run on the built-in Z80 or 6502, into the Tests panel.
    pub(super) fn assembly_tests_here(&mut self) {
        let Some(cpu) = self.doc_cpu(self.active) else {
            return self.message("Tests", crate::i18n::tr("Assembly tests are for Z80 and 6502 source: open the source with the tests in its comments.").into());
        };
        let doc = &self.docs[self.active];
        let path = std::path::absolute(doc.path_or_name()).unwrap_or_else(|_| doc.path_or_name());
        self.run_assembly_tests(&path, cpu, None);
    }

    /// Runs a source's assembly tests (all, or one by name), as it is in the editor if it's open, and lists them.
    pub(super) fn run_assembly_tests(&mut self, path: &Path, cpu: Cpu, only: Option<&str>) {
        let open_text = |path: &Path| {
            self.docs.iter().find(|d| d.path.as_deref().is_some_and(|p| same_path(p, path)) || (d.path.is_none() && d.path_or_name() == path)).map(|d| d.text.all_lines().iter().map(|l| l.as_ref()).collect::<Vec<&str>>().join("\n"))
        };
        let Some(text) = open_text(path).or_else(|| std::fs::read_to_string(path).ok()) else {
            return self.message("Tests", crate::i18n::tf!("Couldn't read {}.", path.display()));
        };
        let tests_file = crate::asm_tests::tests_file_for(path);
        let tests_text = open_text(&tests_file).or_else(|| std::fs::read_to_string(&tests_file).ok());
        let outcomes = crate::asm_tests::run(cpu, path, &text, tests_text.as_deref().map(|t| (tests_file.as_path(), t)), only);
        if outcomes.is_empty() {
            return self.message("Tests", crate::i18n::tr("There are no tests in this source. Write one in a comment above the routine:\n\n; test: adds one\n; given: A=5\n; expect: A=6 flags: Z=0\n; within: 20 cycles\n\nor in a file beside it with .tests on the end of its name.").into());
        }
        let mut results: Vec<TestResult> = outcomes
            .into_iter()
            .map(|o| TestResult {
                name: o.name,
                passed: o.passed,
                place: Some((o.file.display().to_string(), o.line)),
                note: o.cycles.map(|c| if cpu == Cpu::Z80 { crate::i18n::tf!("{c} T-states") } else { crate::i18n::tf!("{c} cycles") }),
                detail: o.differences,
            })
            .collect();
        results.sort_by_key(|r| r.passed);
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let task = Task { name: crate::i18n::tr("Assembly tests").into(), command: format!("{name} ({})", cpu.name()), kind: Kind::Test };
        self.test_panel = Some(TestPanel { task, folder: path.parent().map(Path::to_path_buf).unwrap_or_default(), results, assembly: Some((path.to_path_buf(), cpu)) });
    }

    /// The box editing the project's own tasks.
    pub(super) fn tasks_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::Tasks(list) = &mut self.dialog else {
            return false;
        };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut keep = false;
        crate::fit::Dialog::new(crate::i18n::tr("The project's tasks")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(crate::i18n::tr("Commands run in the project's folder, from Tools, Tasks. Ctrl+Shift+B runs the first build task; a test task's results are listed test by test."));
            ui.add_space(6.0);
            let mut gone = None;
            egui::Grid::new("project-tasks").num_columns(4).spacing([8.0, 4.0]).show(ui, |ui| {
                for (index, task) in list.iter_mut().enumerate() {
                    ui.add(egui::TextEdit::singleline(&mut task.name).hint_text(crate::i18n::tr("Name")).desired_width(140.0));
                    ui.add(egui::TextEdit::singleline(&mut task.command).hint_text(crate::i18n::tr("Command")).desired_width(300.0));
                    egui::ComboBox::from_id_salt(("task-kind", index)).selected_text(crate::i18n::tr(task.kind.label())).show_ui(ui, |ui| {
                        for kind in Kind::ALL {
                            ui.selectable_value(&mut task.kind, kind, crate::i18n::tr(kind.label()));
                        }
                    });
                    if crate::access::called(ui.small_button(crate::i18n::tr("\u{D7}")), "Remove this task").clicked() {
                        gone = Some(index);
                    }
                    ui.end_row();
                }
            });
            if let Some(index) = gone {
                list.remove(index);
            }
            if ui.button(crate::i18n::tr("Add")).clicked() {
                list.push(Task { name: String::new(), command: String::new(), kind: Kind::Build });
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                keep = ui.button(crate::i18n::tr("OK")).clicked();
                close |= ui.button(crate::i18n::tr("Cancel")).clicked();
            });
        });
        if keep {
            if let Dialog::Tasks(list) = std::mem::replace(&mut self.dialog, Dialog::None)
                && let Some(project) = self.project.as_mut()
            {
                project.tasks = list.into_iter().filter(|t| !t.command.trim().is_empty()).map(|mut t| {
                    if t.name.trim().is_empty() {
                        t.name = t.command.clone();
                    }
                    t
                }).collect();
                if let Err(e) = project.save() {
                    self.message("Tasks", crate::i18n::tf!("Couldn't save the project.\n\n{e}"));
                }
            }
        } else if close {
            self.dialog = Dialog::None;
        }
        true
    }

    pub(super) fn edit_project_tasks(&mut self) {
        match &self.project {
            Some(project) => self.dialog = Dialog::Tasks(project.tasks.clone()),
            None => self.message("Tasks", crate::i18n::tr("A project keeps tasks of its own: open or make one (File, Project). Without one, the tasks a folder's own files say (Cargo, Make, npm, Go, pytest, CMake) are still listed.").into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    /// With LIMONITER_CARGO_TESTS set: a real crate's cargo test, one test passing and one failing, read back with
    /// where the failure is.
    #[test]
    #[ignore]
    fn real_cargo_test_results_are_read() {
        if std::env::var_os("LIMONITER_CARGO_TESTS").is_none() {
            return;
        }
        let (mut app, _ctx) = app_with("");
        let folder = std::env::temp_dir().join(format!("limoniter-cargo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("src")).unwrap();
        std::fs::write(folder.join("Cargo.toml"), "[package]\nname = \"sums\"\nversion = \"0.1.0\"\nedition = \"2021\"\n").unwrap();
        std::fs::write(folder.join("src/lib.rs"), "pub fn add(a: i32, b: i32) -> i32 { a + b }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn adds() { assert_eq!(super::add(2, 2), 4); }\n    #[test]\n    fn wrong() { assert_eq!(super::add(2, 2), 5); }\n}\n").unwrap();
        assert!(app.open_path(&folder.join("src/lib.rs")));
        app.run_test_task();
        let began = std::time::Instant::now();
        loop {
            let running = app.check_run_tool();
            app.task_tick(running);
            if !running && app.task_run.is_none() || began.elapsed() > Duration::from_secs(300) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let results = app.test_panel.as_ref().map(|p| p.results.clone()).unwrap_or_default();
        assert_eq!(results.iter().map(|r| (r.name.as_str(), r.passed)).collect::<Vec<_>>(), [("tests::wrong", false), ("tests::adds", true)], "{:?}", app.script_output);
        assert_eq!(results[0].place, Some(("src/lib.rs".to_string(), 8)));
        let _ = std::fs::remove_dir_all(folder);
    }

    /// Z80 source with tests in its comments and in a tests file beside it: Ctrl+Shift+T runs them on the built-in
    /// Z80 into the Tests panel, failures first with what differed, each with its T-states, and Run only this
    /// runs one again.
    #[test]
    fn assembly_tests_run_into_the_tests_panel() {
        let (mut app, _ctx) = app_with("");
        let folder = std::env::temp_dir().join(format!("limoniter-asm-tests-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let source = folder.join("maths.z80");
        std::fs::write(&source, "        org $8000\n; test: doubles\n; given: A=21\n; expect: A=42\ndouble: add a,a\n        ret\n").unwrap();
        std::fs::write(folder.join("maths.z80.tests"), "test: wrong\ngiven: A=1\ncall: double\nexpect: A=3\n").unwrap();
        assert!(app.open_path(&source));
        highlight::load_now();
        app.doc().update_syntax();
        assert_eq!(app.doc_cpu(app.active), Some(Cpu::Z80));
        app.run_test_task();
        let panel = app.test_panel.as_ref().expect("the Tests panel");
        let results: Vec<(&str, bool, Option<&str>)> = panel.results.iter().map(|r| (r.name.as_str(), r.passed, r.note.as_deref())).collect();
        assert_eq!(results, [("wrong", false, Some("14 T-states")), ("doubles", true, Some("14 T-states"))]);
        assert_eq!(panel.results[0].detail, ["A is $02, not $03"]);
        assert_eq!(panel.results[1].place, Some((source.display().to_string(), 2)));
        app.run_assembly_tests(&source, Cpu::Z80, Some("doubles"));
        assert_eq!(app.test_panel.as_ref().unwrap().results.len(), 1);
        let _ = std::fs::remove_dir_all(folder);
    }

    /// A build that fails: the compiler's problem is marked in the open file; a test task's results are listed.
    #[cfg(unix)]
    #[test]
    fn tasks_run_and_what_they_print_is_read_back() {
        let (mut app, _ctx) = app_with("");
        let folder = std::env::temp_dir().join(format!("limoniter-tasks-host-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("Makefile"), "all:\n\t@echo 'main.c:2:5: error: expected semicolon'; exit 1\ntest:\n\t@printf 'test maths::adds ... ok\\ntest maths::subs ... FAILED\\n'\n").unwrap();
        std::fs::write(folder.join("main.c"), "int main() {\n    return 0\n}\n").unwrap();
        assert!(app.open_path(&folder.join("main.c")));
        let tasks = app.tasks_here();
        assert!(tasks.iter().any(|t| t.name == "make" && t.kind == Kind::Build), "{tasks:?}");
        let wait = |app: &mut Limoniter| {
            let began = std::time::Instant::now();
            loop {
                let running = app.check_run_tool();
                app.task_tick(running);
                if !running && app.task_run.is_none() || began.elapsed() > Duration::from_secs(20) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        app.run_build_task();
        wait(&mut app);
        let problems = app.doc().text.intel.as_ref().map(|i| i.build_problems.clone()).unwrap_or_default();
        assert_eq!(problems, [(1, "expected semicolon".to_string())], "{:?}", app.script_output);
        app.run_test_task();
        wait(&mut app);
        let results = app.test_panel.as_ref().map(|p| p.results.clone()).unwrap_or_default();
        assert_eq!(results.iter().map(|r| (r.name.as_str(), r.passed)).collect::<Vec<_>>(), [("maths::subs", false), ("maths::adds", true)], "failures first");
        assert!(app.doc().text.intel.as_ref().is_none_or(|i| i.build_problems.is_empty()), "cleared for the next task");
        let _ = std::fs::remove_dir_all(folder);
    }
}
