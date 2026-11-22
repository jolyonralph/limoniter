//! The window's side of the Tests panel (View, Panels, Tests): every test runner a project has (pytest, Jest,
//! Vitest, PHPUnit, testthat, `cargo test`, `go test`), its tests listed by file and name, and buttons to run one,
//! a file's, the ones that failed last time, or all. A run is an ordinary task, so what it prints is in the Output
//! panel too; when it's done, what it printed is read test by test and shown beside each name, with the time it
//! took, and a failure is marked at its line in the file with what was expected and what came.

use super::*;
use crate::tasks::{Kind, Task, TestResult};
use crate::test_runners::{self, Item, Runner, Scope};
use std::collections::HashMap;
use std::sync::mpsc::Receiver;

/// What the Tests panel knows about the project it's showing.
#[derive(Default)]
pub(super) struct Explorer {
    /// The project folder the list is of.
    folder: Option<PathBuf>,
    /// The runners found there, with the tests each listed.
    runners: Vec<(Runner, Vec<Item>)>,
    /// The listings still being collected, a runner at a time.
    collecting: Option<Receiver<(Runner, Result<Vec<Item>, String>)>>,
    waiting_for: usize,
    /// Why a listing couldn't be made, by runner.
    problems: Vec<(Runner, String)>,
    /// The last run's results, as they were read.
    results: Vec<TestResult>,
    /// For each runner's tests, which result (an index into `results`) is theirs.
    matched: HashMap<(usize, usize), usize>,
    /// The runner a run from this panel was started for, so its results are taken here.
    running: Option<Runner>,
}

impl Explorer {
    /// Matches each listed test to its result, once for each change of either.
    fn rematch(&mut self) {
        self.matched.clear();
        for (r, (_, items)) in self.runners.iter().enumerate() {
            for (i, item) in items.iter().enumerate() {
                if let Some(at) = self.results.iter().position(|result| test_runners::same_test(item, &result.name)) {
                    self.matched.insert((r, i), at);
                }
            }
        }
    }

    fn failed_items(&self, runner: usize) -> Vec<Item> {
        self.runners[runner].1.iter().enumerate().filter(|(i, _)| self.matched.get(&(runner, *i)).is_some_and(|&at| !self.results[at].passed)).map(|(_, item)| item.clone()).collect()
    }
}

/// What a click in the panel asks for.
enum Ask {
    Refresh,
    Close,
    Run(usize, OwnedScope),
    Go(String, usize),
}

enum OwnedScope {
    All,
    Group(String),
    One(Item),
    Failed,
}

impl Limoniter {
    /// Lists the project's tests afresh: the runners are found, and each is asked for its tests on a thread of its
    /// own, as listing can take a while (cargo builds first).
    fn tests_refresh(&mut self) {
        let folder = self.task_folder();
        let ex = &mut self.tests_ui;
        ex.folder = folder.clone();
        ex.runners.clear();
        ex.problems.clear();
        ex.collecting = None;
        ex.matched.clear();
        let Some(folder) = folder else { return };
        let (send, receive) = std::sync::mpsc::channel();
        let mut waiting = 0;
        for runner in test_runners::find(&folder) {
            ex.runners.push((runner, Vec::new()));
            let (send, folder) = (send.clone(), folder.clone());
            match test_runners::collect_command(runner, &folder) {
                None => {
                    let _ = send.send((runner, Ok(test_runners::parse_collected(runner, &folder, ""))));
                }
                Some(command) => {
                    std::thread::spawn(move || {
                        let result = test_runners::capture(&command, &folder).map(|out| test_runners::parse_collected(runner, &folder, &out));
                        let _ = send.send((runner, result));
                    });
                }
            }
            waiting += 1;
        }
        ex.waiting_for = waiting;
        ex.collecting = Some(receive);
    }

    /// Takes in the listings that have come.
    fn tests_collect(&mut self, ctx: &Context) {
        let ex = &mut self.tests_ui;
        let Some(receive) = &ex.collecting else { return };
        let mut changed = false;
        while let Ok((runner, result)) = receive.try_recv() {
            ex.waiting_for = ex.waiting_for.saturating_sub(1);
            changed = true;
            match result {
                Ok(items) => {
                    if let Some(slot) = ex.runners.iter_mut().find(|(r, _)| *r == runner) {
                        slot.1 = items;
                    }
                }
                Err(problem) => ex.problems.push((runner, problem)),
            }
        }
        if ex.waiting_for == 0 {
            ex.collecting = None;
        } else {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        if changed {
            ex.rematch();
        }
    }

    /// Starts a run as a task, saying which runner it was for so its results come back to the panel.
    fn tests_run(&mut self, runner_index: usize, scope: OwnedScope) {
        let Some(folder) = self.tests_ui.folder.clone() else { return };
        let Some((runner, items)) = self.tests_ui.runners.get(runner_index) else { return };
        let runner = *runner;
        let failed;
        let command = match &scope {
            OwnedScope::All => test_runners::run_command(runner, &folder, Scope::All),
            OwnedScope::Group(group) => test_runners::run_command(runner, &folder, Scope::Group(group)),
            OwnedScope::One(item) => test_runners::run_command(runner, &folder, Scope::One(item)),
            OwnedScope::Failed => {
                failed = self.tests_ui.failed_items(runner_index);
                if failed.is_empty() {
                    return self.message("Tests", crate::i18n::tr("None of the tests failed last time.").into());
                }
                test_runners::run_command(runner, &folder, Scope::Some(&failed))
            }
        };
        let _ = items;
        self.tests_ui.running = Some(runner);
        self.run_task(Task { name: crate::i18n::tf!("{} tests", runner.label()), command, kind: Kind::Test });
    }

    /// When a run from the panel has finished: its results are kept for the panel to show.
    pub(super) fn tests_took(&mut self, results: &[TestResult]) {
        if self.tests_ui.running.take().is_none() {
            return;
        }
        for result in results {
            match self.tests_ui.results.iter_mut().find(|r| r.name == result.name) {
                Some(old) => *old = result.clone(),
                None => self.tests_ui.results.push(result.clone()),
            }
        }
        self.tests_ui.rematch();
    }

    /// Marks each failure at its line in the file, with what it said, if the file is open.
    pub(super) fn mark_test_failures(&mut self, folder: &Path, results: &[TestResult]) {
        for result in results.iter().filter(|r| !r.passed) {
            let (Some((file, number)), false) = (&result.place, result.detail.is_empty()) else { continue };
            let path = if Path::new(file).is_absolute() { PathBuf::from(file) } else { folder.join(file) };
            let words = result.detail.join("; ");
            if let Some(doc) = self.docs.iter_mut().find(|d| d.path.as_deref().is_some_and(|p| same_path(p, &path))) {
                let intel = doc.text.intel.get_or_insert_with(Box::default);
                let line = number.saturating_sub(1);
                match intel.build_problems.iter_mut().find(|(l, _)| *l == line) {
                    Some(existing) => existing.1 = words,
                    None => intel.build_problems.push((line, words)),
                }
            }
        }
    }

    /// The panel's own contents, in the right-hand panel the older test results use.
    pub(super) fn tests_explorer(&mut self, ui: &mut Ui) {
        let here = self.task_folder();
        if self.tests_ui.folder != here || (self.tests_ui.runners.is_empty() && self.tests_ui.collecting.is_none() && self.tests_ui.problems.is_empty() && here.is_some() && self.tests_ui.folder.is_none()) {
            self.tests_refresh();
        }
        self.tests_collect(ui.ctx());
        let ex = &self.tests_ui;
        let (passed, failed) = (ex.results.iter().filter(|r| r.passed).count(), ex.results.iter().filter(|r| !r.passed).count());
        let mut ask: Option<Ask> = None;
        Panel::right("tests-panel").default_size(340.0).resizable(true).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(crate::i18n::tr("Tests")).strong());
                if !ex.results.is_empty() {
                    ui.label(RichText::new(crate::i18n::tf!("{passed} passed, {failed} failed")).color(if failed > 0 { theme::problem_mark() } else { theme::accent() }));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::access::called(ui.small_button(crate::i18n::tr("\u{D7}")), "Close").clicked() {
                        ask = Some(Ask::Close);
                    }
                    if ui.small_button(crate::i18n::tr("Look again")).on_hover_text(crate::i18n::tr("List the tests afresh, as files have been added or changed.")).clicked() {
                        ask = Some(Ask::Refresh);
                    }
                });
            });
            if let Some(folder) = &ex.folder {
                ui.label(RichText::new(folder.display().to_string()).size(11.5).color(theme::text_faint()));
            } else {
                ui.label(RichText::new(crate::i18n::tr("Open a file in the project first: its tests are looked for in its folder.")).color(theme::text_dim()));
            }
            if ex.folder.is_some() && ex.runners.is_empty() && ex.collecting.is_none() {
                ui.label(RichText::new(crate::i18n::tr("No test runner was found here: pytest, Jest, Vitest, PHPUnit, testthat, cargo test and go test are looked for.")).color(theme::text_dim()));
            }
            if ex.waiting_for > 0 {
                ui.label(RichText::new(crate::i18n::tr("Listing the tests\u{2026}")).color(theme::text_dim()));
            }
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for (r, (runner, items)) in ex.runners.iter().enumerate() {
                    let failed_here = (0..items.len()).filter(|i| ex.matched.get(&(r, *i)).is_some_and(|&at| !ex.results[at].passed)).count();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::i18n::tf!("{} ({})", runner.label(), items.len())).strong());
                        if ui.small_button(crate::i18n::tr("Run all")).clicked() {
                            ask = Some(Ask::Run(r, OwnedScope::All));
                        }
                        if failed_here > 0 && ui.small_button(crate::i18n::tr("Run the failed ones again")).clicked() {
                            ask = Some(Ask::Run(r, OwnedScope::Failed));
                        }
                    });
                    if let Some((_, problem)) = ex.problems.iter().find(|(p, _)| p == runner) {
                        ui.add(egui::Label::new(RichText::new(problem).size(11.5).color(theme::problem_mark())).wrap());
                    }
                    let mut groups: Vec<&str> = Vec::new();
                    for item in items {
                        if !groups.contains(&item.group.as_str()) {
                            groups.push(&item.group);
                        }
                    }
                    for group in groups {
                        let members: Vec<usize> = (0..items.len()).filter(|&i| items[i].group == group).collect();
                        let bad = members.iter().filter(|&&i| ex.matched.get(&(r, i)).is_some_and(|&at| !ex.results[at].passed)).count();
                        let title = if bad > 0 { RichText::new(crate::i18n::tf!("{group} ({bad} failed)")).color(theme::problem_mark()) } else { RichText::new(group) };
                        egui::CollapsingHeader::new(title).id_salt(("tests-group", r, group)).default_open(bad > 0).show(ui, |ui| {
                            if ui.small_button(crate::i18n::tr("Run this file's tests")).clicked() {
                                ask = Some(Ask::Run(r, OwnedScope::Group(group.to_string())));
                            }
                            for i in members {
                                let result = ex.matched.get(&(r, i)).map(|&at| &ex.results[at]);
                                ui.horizontal(|ui| {
                                    let (mark, colour) = match result {
                                        Some(t) if t.passed => ("\u{2713}", theme::accent()),
                                        Some(_) => ("\u{2717}", theme::problem_mark()),
                                        None => ("\u{2022}", theme::text_faint()),
                                    };
                                    ui.label(RichText::new(mark).color(colour));
                                    let name = ui.add(egui::Label::new(RichText::new(&items[i].name).monospace().size(12.0).color(if result.is_some_and(|t| !t.passed) { theme::problem_mark() } else { theme::text_dim() })).sense(egui::Sense::click()));
                                    if let Some(place) = result.and_then(|t| t.place.as_ref()) {
                                        let name = crate::access::clickable(name).on_hover_text(crate::i18n::tf!("{}, line {}", place.0, place.1));
                                        if name.clicked() {
                                            ask = Some(Ask::Go(place.0.clone(), place.1));
                                        }
                                    }
                                    if let Some(note) = result.and_then(|t| t.note.as_ref()) {
                                        ui.label(RichText::new(note).size(11.5).color(theme::text_faint()));
                                    }
                                    if ui.small_button(crate::i18n::tr("Run")).clicked() {
                                        ask = Some(Ask::Run(r, OwnedScope::One(items[i].clone())));
                                    }
                                });
                                for line in result.filter(|t| !t.passed).map(|t| t.detail.as_slice()).unwrap_or(&[]) {
                                    ui.horizontal(|ui| {
                                        ui.add_space(18.0);
                                        ui.add(egui::Label::new(RichText::new(line).size(11.5).color(theme::text_dim())).wrap());
                                    });
                                }
                            }
                        });
                    }
                    ui.add_space(6.0);
                }
            });
        });
        let folder = self.tests_ui.folder.clone();
        match ask {
            Some(Ask::Close) => self.panels.tests = false,
            Some(Ask::Refresh) => {
                self.tests_ui.results.clear();
                self.tests_refresh();
            }
            Some(Ask::Run(r, scope)) => self.tests_run(r, scope),
            Some(Ask::Go(file, line)) => {
                let path = if Path::new(&file).is_absolute() { PathBuf::from(&file) } else { folder.unwrap_or_default().join(&file) };
                self.go_to_place(&path.display().to_string(), line);
            }
            None => {}
        }
    }
}

impl Limoniter {
    /// For screenshots: the Tests panel open with the project's tests listed, and (with "run") run, all of them.
    pub(super) fn tests_screenshot_step(&mut self, value: &str) {
        self.panels.tests = true;
        self.tests_refresh();
        let ctx = self.waker.clone().unwrap_or_default();
        let began = std::time::Instant::now();
        while self.tests_ui.collecting.is_some() && began.elapsed() < Duration::from_secs(30) {
            self.tests_collect(&ctx);
            std::thread::sleep(Duration::from_millis(50));
        }
        if value == "run" && !self.tests_ui.runners.is_empty() {
            self.tests_run(0, OwnedScope::All);
            while began.elapsed() < Duration::from_secs(60) {
                let running = self.check_run_tool();
                self.task_tick(running);
                if !running && self.task_run.is_none() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(30));
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    /// A project with a fake PHPUnit: its tests are listed, one is run, all are run, and the failure is marked at
    /// its line in the file open, with what was expected and what came.
    #[test]
    fn the_tests_panel_lists_runs_and_marks_failures() {
        use std::os::unix::fs::PermissionsExt;
        let (mut app, ctx) = app_with("");
        let folder = std::env::temp_dir().join(format!("limoniter-tests-panel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("vendor/bin")).unwrap();
        std::fs::create_dir_all(folder.join("tests")).unwrap();
        std::fs::write(folder.join("phpunit.xml"), "<phpunit/>").unwrap();
        // composer.json is what says where a PHP project's folder is.
        std::fs::write(folder.join("composer.json"), "{}").unwrap();
        let test_file = folder.join("tests/MathTest.php");
        std::fs::write(&test_file, "<?php\nclass MathTest {\n    function testAdd() {}\n    function testSub() { $this->assertSame(1, 0); }\n}\n").unwrap();
        let place = test_file.display().to_string();
        let teamcity = format!("##teamcity[testStarted name='testAdd' locationHint='php_qn://{place}::\\\\MathTest::testAdd' flowId='1']\n##teamcity[testFinished name='testAdd' duration='3' flowId='1']\n##teamcity[testStarted name='testSub' locationHint='php_qn://{place}::\\\\MathTest::testSub' flowId='1']\n##teamcity[testFailed name='testSub' message='Failed asserting that 0 is identical to 1.' details=' |n{place}:4|n ' duration='1' type='comparisonFailure' actual='0' expected='1' flowId='1']\n##teamcity[testFinished name='testSub' duration='1' flowId='1']");
        let fake = folder.join("vendor/bin/phpunit");
        // With a filter, it runs only that test: the fake says so by printing just the passing one.
        std::fs::write(&fake, format!("#!/bin/sh\nif [ \"$1\" = \"--list-tests\" ]; then printf 'Available test(s):\\n - MathTest::testAdd\\n - MathTest::testSub\\n'; exit 0; fi\ncase \"$*\" in *--filter*testAdd*) cat <<'EOT'\n{}\nEOT\n;; *) cat <<'EOT'\n{teamcity}\nEOT\nexit 1;; esac\n", teamcity.lines().take(2).collect::<Vec<_>>().join("\n"))).unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(app.open_path(&test_file));
        app.panels.tests = true;
        app.tests_refresh();
        let began = std::time::Instant::now();
        while app.tests_ui.collecting.is_some() && began.elapsed() < Duration::from_secs(20) {
            app.tests_collect(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(app.tests_ui.runners.len(), 1);
        let items = &app.tests_ui.runners[0].1;
        assert_eq!(items.iter().map(|i| (i.group.as_str(), i.name.as_str())).collect::<Vec<_>>(), [("tests/MathTest.php", "testAdd"), ("tests/MathTest.php", "testSub")]);
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
        // One test: only its result comes, and it's matched to the right name.
        let one = app.tests_ui.runners[0].1[0].clone();
        app.tests_run(0, OwnedScope::One(one));
        wait(&mut app);
        assert_eq!(app.tests_ui.results.len(), 1, "{:?}", app.script_output);
        assert!(app.tests_ui.matched.contains_key(&(0, 0)) && !app.tests_ui.matched.contains_key(&(0, 1)));
        // All of them: the failure is marked at its line in the file open, and "failed" can be run again.
        app.tests_run(0, OwnedScope::All);
        wait(&mut app);
        assert_eq!(app.tests_ui.results.iter().map(|r| r.passed).collect::<Vec<_>>(), [true, false]);
        assert_eq!(app.tests_ui.failed_items(0).iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["testSub"]);
        let marks = app.doc().text.intel.as_ref().map(|i| i.build_problems.clone()).unwrap_or_default();
        assert_eq!(marks, [(3, "Expected: 1; Got: 0".to_string())], "{:?}", app.script_output);
        let _ = std::fs::remove_dir_all(folder);
    }
}
