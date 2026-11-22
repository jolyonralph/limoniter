//! The window's side of outdated packages: Tools, Tasks, Outdated packages asks each package manager the project
//! uses (on threads of their own, as it needs the network), gathers what's out of date into one table in the data
//! viewer, and offers updating each package, or all of a manager's, as tasks in the Tasks list.

use super::*;
use crate::data_table::{Cell, ColumnKind, DataColumn, DataTable};
use crate::outdated::{self, Manager, Outdated};
use crate::tasks::{Kind, Task};
use std::sync::mpsc::Receiver;

#[derive(Default)]
pub(super) struct State {
    receive: Option<Receiver<(Manager, Result<Vec<Outdated>, String>)>>,
    waiting: usize,
    /// What was found, and the folder it was found for.
    found: Vec<Outdated>,
    folder: Option<PathBuf>,
    problems: Vec<String>,
}

impl Limoniter {
    /// Asks each package manager the project uses what's out of date.
    pub(super) fn outdated_start(&mut self) {
        if self.outdated_ui.receive.is_some() {
            return;
        }
        let Some(folder) = self.task_folder() else {
            return self.message("Outdated packages", crate::i18n::tr("Open a file in the project first: its packages are looked for in its folder.").into());
        };
        let managers = outdated::detect(&folder);
        if managers.is_empty() {
            return self.message("Outdated packages", crate::i18n::tr("No package.json, requirements.txt, pyproject.toml, composer.json or R package was found in the project's folder.").into());
        }
        let (send, receive) = std::sync::mpsc::channel();
        for &manager in &managers {
            let (send, folder) = (send.clone(), folder.clone());
            std::thread::spawn(move || {
                let result = crate::test_runners::capture(&outdated::command(manager, &folder), &folder).map(|out| outdated::parse(manager, &out));
                let _ = send.send((manager, result));
            });
        }
        self.script_print(crate::i18n::tr("Looking for outdated packages\u{2026}").into(), true);
        self.outdated_ui = State { receive: Some(receive), waiting: managers.len(), folder: Some(folder), ..State::default() };
    }

    /// Takes in what the managers say as it comes, and shows the table when all have answered. True while waiting.
    pub(super) fn outdated_tick(&mut self) -> bool {
        let Some(receive) = &self.outdated_ui.receive else { return false };
        let mut came = Vec::new();
        while let Ok(answer) = receive.try_recv() {
            came.push(answer);
        }
        let state = &mut self.outdated_ui;
        for (manager, result) in came {
            state.waiting = state.waiting.saturating_sub(1);
            match result {
                Ok(list) => state.found.extend(list),
                Err(problem) => state.problems.push(crate::i18n::tf!("{}: {problem}", manager.label())),
            }
        }
        if state.waiting > 0 {
            return true;
        }
        state.receive = None;
        let found = state.found.clone();
        let problems = std::mem::take(&mut state.problems);
        for problem in problems {
            self.script_print(problem, false);
        }
        if found.is_empty() {
            self.message("Outdated packages", crate::i18n::tr("Everything is up to date, or the package managers couldn't say (see the Output panel).").into());
            return false;
        }
        let column = |name: &str| DataColumn { name: crate::i18n::tr(name).to_string(), kind: ColumnKind::Text };
        let text = |s: &str| if s.is_empty() { Cell::Null } else { Cell::Text(s.to_string()) };
        let table = DataTable {
            columns: vec![column("Package"), column("Manager"), column("Now"), column("Wanted"), column("Newest")],
            rows: found.iter().map(|p| vec![Cell::Text(p.package.clone()), Cell::Text(p.manager.label().to_string()), text(&p.now), text(&p.wanted), text(&p.newest)]).collect(),
            note: Some(crate::i18n::tr("Wanted is the newest version the project's own limits allow. Tools, Tasks lists the updates.").to_string()),
        };
        self.open_data_table(crate::i18n::tr("Outdated packages"), table);
        false
    }

    /// The update tasks for what was found: all of each manager's, then each package.
    pub(super) fn outdated_tasks(&self) -> Vec<Task> {
        let state = &self.outdated_ui;
        let Some(folder) = &state.folder else { return Vec::new() };
        let mut tasks = Vec::new();
        for manager in [Manager::Npm, Manager::Pip, Manager::Composer, Manager::R] {
            let of: Vec<&Outdated> = state.found.iter().filter(|p| p.manager == manager).collect();
            if of.is_empty() {
                continue;
            }
            // Pip and R have no way to say "all", so they're given the names.
            let all: &[&Outdated] = if matches!(manager, Manager::Npm | Manager::Composer) { &[] } else { &of };
            tasks.push(Task { name: crate::i18n::tf!("Update all outdated {} packages", manager.label()), command: outdated::update_command(manager, folder, all), kind: Kind::Other });
            for package in of.iter().take(40) {
                tasks.push(Task { name: crate::i18n::tf!("Update {} ({})", package.package, manager.label()), command: outdated::update_command(manager, folder, &[package]), kind: Kind::Other });
            }
        }
        tasks
    }
}
