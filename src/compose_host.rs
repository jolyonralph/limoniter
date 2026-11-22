//! The window's side of Docker Compose: the Containers panel (View, Panels) listing a project's services with
//! their state and ports and a button for each thing to do; and the logs of the services being followed, each
//! in a tab of a panel along the bottom.

use super::*;
use crate::compose::{Action, LogStream, Poller, Snapshot};

/// How often the panel asks Compose for the state while it's open.
const EVERY: std::time::Duration = std::time::Duration::from_secs(4);

#[derive(Default)]
pub(super) struct ComposeState {
    /// The folder being looked at and what's looking.
    poller: Option<(PathBuf, Poller)>,
    pub(super) logs: Vec<LogStream>,
    /// Which log is in front.
    front: usize,
}

enum Act {
    Refresh,
    Do(Action),
    Logs(String),
    Shell(String),
    Open(String),
}

/// What to call a state in the panel, and the colour to show it in.
fn state_words(state: &str, health: &str) -> (String, egui::Color32) {
    let words = match state {
        "running" if health == "unhealthy" => crate::i18n::tr("Up, unhealthy").to_string(),
        "running" if health == "starting" => crate::i18n::tr("Up, starting").to_string(),
        "running" => crate::i18n::tr("Up").to_string(),
        "restarting" => crate::i18n::tr("Restarting").to_string(),
        "exited" => crate::i18n::tr("Exited").to_string(),
        "paused" => crate::i18n::tr("Paused").to_string(),
        "created" => crate::i18n::tr("Created").to_string(),
        "dead" => crate::i18n::tr("Dead").to_string(),
        "down" => crate::i18n::tr("Down").to_string(),
        other => other.to_string(),
    };
    let colour = match state {
        "running" if health == "unhealthy" => theme::problem_mark(),
        "running" => theme::accent(),
        "exited" | "dead" | "restarting" => theme::problem_mark(),
        _ => theme::text_dim(),
    };
    (words, colour)
}

impl Limoniter {
    /// The folder with compose files in it: the one tasks run in, or the one the file in front is in.
    fn compose_folder(&self) -> Option<PathBuf> {
        let here = self.task_folder().filter(|f| crate::compose::find(f).is_some());
        here.or_else(|| self.docs.get(self.active).and_then(|d| d.path.as_deref()).and_then(Path::parent).filter(|f| crate::compose::find(f).is_some()).map(Path::to_path_buf))
    }

    /// Draws the Containers panel, if it's open; while it's shut nothing is asked of Compose.
    pub(super) fn containers_panel(&mut self, ui: &mut Ui) {
        if !self.panels.containers {
            self.compose.poller = None;
            return;
        }
        let folder = self.compose_folder();
        match &folder {
            Some(folder) if self.compose.poller.as_ref().is_none_or(|(f, _)| f != folder) => {
                self.compose.poller = Some((folder.clone(), Poller::start(folder.clone(), None, EVERY, Some(ui.ctx().clone()))));
            }
            None => self.compose.poller = None,
            _ => {}
        }
        let snapshot = self.compose.poller.as_ref().map(|(_, p)| p.snapshot()).unwrap_or_default();
        let files = folder.as_ref().and_then(|f| crate::compose::find(f));
        let mut open = true;
        let mut acts = Vec::new();
        panels::dock(ui, false, "containers-panel", "Containers", 300.0, &mut open, |ui| containers_tree(ui, folder.as_deref(), files.as_ref(), &snapshot, &mut acts));
        self.panels.containers = open;
        for act in acts {
            self.compose_act(act, folder.clone(), &snapshot);
        }
    }

    fn compose_act(&mut self, act: Act, folder: Option<PathBuf>, snapshot: &Snapshot) {
        let (Some(folder), Some(tool)) = (folder, snapshot.tool.clone()) else {
            if let Act::Refresh = act {
                if let Some((_, poller)) = &self.compose.poller {
                    poller.refresh();
                }
            } else if let Act::Open(address) = act {
                let _ = crate::platform::open_web_page(&address);
            }
            return;
        };
        match act {
            Act::Refresh => {
                if let Some((_, poller)) = &self.compose.poller {
                    poller.refresh();
                }
            }
            Act::Do(action) => {
                match self.run_tool_in(&tool.line(&action.args()), false, Some(folder)) {
                    Ok(()) => {
                        self.show_results = true;
                        if let Some((_, poller)) = &self.compose.poller {
                            poller.refresh();
                        }
                    }
                    Err(problem) => self.message("Containers", problem),
                }
            }
            Act::Logs(service) => {
                if let Some(at) = self.compose.logs.iter().position(|l| l.service == service) {
                    self.compose.front = at;
                    return;
                }
                match LogStream::start(&tool, &folder, &service, self.waker.clone()) {
                    Ok(stream) => {
                        self.compose.logs.push(stream);
                        self.compose.front = self.compose.logs.len() - 1;
                    }
                    Err(problem) => self.message("Containers", problem),
                }
            }
            Act::Shell(service) => {
                let mut command = portable_pty::CommandBuilder::new(&tool.program);
                command.args(&tool.prefix);
                command.args(crate::compose::shell_args(&service));
                match crate::terminal::Terminal::start_with(command, &folder, 12, 80) {
                    Ok(terminal) => self.show_terminal(terminal),
                    Err(problem) => self.message("Containers", problem),
                }
            }
            Act::Open(address) => {
                if let Err(problem) = crate::platform::open_web_page(&address) {
                    self.message("Containers", problem);
                }
            }
        }
    }

    /// The followed logs, in tabs along the bottom. Closing a tab stops its command.
    pub(super) fn compose_logs_panel(&mut self, ui: &mut Ui) {
        if self.compose.logs.is_empty() {
            return;
        }
        let front = self.compose.front.min(self.compose.logs.len() - 1);
        let mut close = None;
        let mut switch = None;
        let font_size = self.text_view_options().font_size;
        let logs = &self.compose.logs;
        Panel::bottom("compose-logs-panel").default_size(200.0).resizable(true).show(ui, |ui| {
            ui.horizontal(|ui| {
                for (at, log) in logs.iter().enumerate() {
                    if ui.selectable_label(at == front, RichText::new(&log.service).size(12.5)).clicked() {
                        switch = Some(at);
                    }
                    if crate::access::called(ui.small_button(crate::i18n::tr("\u{D7}")), "Close").on_hover_text(crate::i18n::tr("Stop following this service's logs")).clicked() {
                        close = Some(at);
                    }
                    ui.add_space(6.0);
                }
            });
            let log = &logs[front];
            if log.ended() {
                ui.label(RichText::new(crate::i18n::tr("The logs have ended: the service has stopped, or Compose could not be reached.")).size(11.5).color(theme::text_faint()));
            }
            ui.separator();
            let lines = log.lines();
            let height = ui.text_style_height(&egui::TextStyle::Monospace).max(font_size);
            egui::ScrollArea::both().auto_shrink([false, false]).stick_to_bottom(true).show_rows(ui, height, lines.len(), |ui, range| {
                for line in &lines[range] {
                    ui.add(egui::Label::new(RichText::new(line).monospace().size(font_size * 0.9)).extend());
                }
            });
        });
        if let Some(at) = switch {
            self.compose.front = at;
        }
        if let Some(at) = close {
            // Dropping the stream ends the command that's following.
            self.compose.logs.remove(at);
            self.compose.front = self.compose.front.min(self.compose.logs.len().saturating_sub(1));
        }
    }
}

fn containers_tree(ui: &mut Ui, folder: Option<&Path>, files: Option<&crate::compose::Files>, snapshot: &Snapshot, acts: &mut Vec<Act>) {
    let Some(folder) = folder else {
        ui.label(RichText::new(crate::i18n::tr("No compose.yaml, compose.yml, docker-compose.yml or docker-compose.yaml was found beside the file in front or in the project's folder.")).color(theme::text_dim()));
        return;
    };
    ui.label(RichText::new(folder.display().to_string()).size(11.5).color(theme::text_faint()));
    if let Some(files) = files {
        let names: Vec<String> = files.main.iter().chain(&files.others).filter_map(|f| f.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
        ui.label(RichText::new(names.join(", ")).size(11.5).color(theme::text_faint()));
    }
    match &snapshot.tool {
        Some(tool) => {
            ui.label(RichText::new(crate::i18n::tf!("Using {}", tool.name())).size(11.5).color(theme::text_faint()));
        }
        None if snapshot.rounds == 0 => {
            ui.label(RichText::new(crate::i18n::tr("Looking\u{2026}")).color(theme::text_dim()));
        }
        None => {}
    }
    if let Some(problem) = &snapshot.problem {
        ui.add(egui::Label::new(RichText::new(problem).size(11.5).color(theme::problem_mark())).wrap());
    }
    ui.horizontal_wrapped(|ui| {
        let ready = snapshot.tool.is_some();
        for (label, hint, action) in [("Up", "Creates and starts every service: docker compose up -d", Action::Up), ("Down", "Stops and removes the containers: docker compose down", Action::Down), ("Pull", "Fetches newer images for the services: docker compose pull", Action::Pull)] {
            if ui.add_enabled(ready, egui::Button::new(crate::i18n::tr(label)).small()).on_hover_text(crate::i18n::tr(hint)).clicked() {
                acts.push(Act::Do(action));
            }
        }
        if ui.small_button(crate::i18n::tr("Look again")).on_hover_text(crate::i18n::tr("Asks Compose again now, rather than waiting")).clicked() {
            acts.push(Act::Refresh);
        }
    });
    ui.separator();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        if snapshot.services.is_empty() && snapshot.rounds > 0 {
            ui.label(RichText::new(crate::i18n::tr("No services were found.")).color(theme::text_dim()));
        }
        for service in &snapshot.services {
            let state = crate::compose::state_of(&service.name, &snapshot.containers);
            let health = snapshot.containers.iter().find(|c| c.service == service.name).map(|c| c.health.as_str()).unwrap_or("");
            let (words, colour) = state_words(&state, health);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&service.name).strong());
                ui.label(RichText::new(words).color(colour));
            });
            let detail = snapshot.containers.iter().find(|c| c.service == service.name).map(|c| c.status.clone()).filter(|s| !s.is_empty());
            let image = service.image.clone().or_else(|| service.builds.then(|| crate::i18n::tr("built here").to_string()));
            if let Some(line) = [image, detail].into_iter().flatten().reduce(|a, b| format!("{a}  \u{B7}  {b}")) {
                ui.label(RichText::new(line).size(11.5).color(theme::text_faint()));
            }
            // A running container's ports are what's really published; the file's, otherwise.
            let running = snapshot.containers.iter().find(|c| c.service == service.name && !c.ports.is_empty());
            let ports = running.map_or(&service.ports, |c| &c.ports);
            if !ports.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for port in ports {
                        match port.url() {
                            Some(address) => {
                                if ui.link(port.label()).on_hover_text(crate::i18n::tf!("Open {address} in the browser")).clicked() {
                                    acts.push(Act::Open(address));
                                }
                            }
                            None => {
                                ui.label(RichText::new(port.label()).size(12.0).color(theme::text_dim()));
                            }
                        }
                    }
                });
            }
            let up = state == "running";
            ui.horizontal_wrapped(|ui| {
                let ready = snapshot.tool.is_some();
                let name = service.name.clone();
                if ui.add_enabled(ready, egui::Button::new(crate::i18n::tr("Up")).small()).on_hover_text(crate::i18n::tr("Starts this service")).clicked() {
                    acts.push(Act::Do(Action::ServiceUp(name.clone())));
                }
                if ui.add_enabled(ready && up, egui::Button::new(crate::i18n::tr("Down")).small()).on_hover_text(crate::i18n::tr("Stops this service: docker compose stop")).clicked() {
                    acts.push(Act::Do(Action::ServiceDown(name.clone())));
                }
                if ui.add_enabled(ready, egui::Button::new(crate::i18n::tr("Restart")).small()).clicked() {
                    acts.push(Act::Do(Action::Restart(name.clone())));
                }
                if ui.add_enabled(ready, egui::Button::new(crate::i18n::tr("Rebuild")).small()).on_hover_text(crate::i18n::tr("Builds the image again and starts the service: up --build")).clicked() {
                    acts.push(Act::Do(Action::Rebuild(name.clone())));
                }
                if ui.add_enabled(ready, egui::Button::new(crate::i18n::tr("Logs")).small()).on_hover_text(crate::i18n::tr("Follows what the service prints, in a panel along the bottom")).clicked() {
                    acts.push(Act::Logs(name.clone()));
                }
                if ui.add_enabled(ready && up, egui::Button::new(crate::i18n::tr("Shell")).small()).on_hover_text(crate::i18n::tr("Opens a shell inside the service, in the terminal")).clicked() {
                    acts.push(Act::Shell(name));
                }
            });
            ui.separator();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_have_words_and_unhealthy_is_a_problem() {
        assert_eq!(state_words("running", "").0, "Up");
        assert_eq!(state_words("running", "unhealthy").0, "Up, unhealthy");
        assert_eq!(state_words("exited", "").1, theme::problem_mark());
        assert_eq!(state_words("down", "").0, "Down");
        assert_eq!(state_words("weird", "").0, "weird");
    }

    #[cfg(unix)]
    fn fake_project(name: &str) -> (PathBuf, crate::compose::Tool) {
        let folder = std::env::temp_dir().join(format!("limoniter-compose-host-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        for (file, text) in [("docker.sh", include_str!("compose_testdata/docker.sh")), ("compose.yaml", include_str!("compose_testdata/compose.yaml"))] {
            std::fs::write(folder.join(file), text).unwrap();
        }
        let tool = crate::compose::Tool { program: "sh".into(), prefix: vec![folder.join("docker.sh").display().to_string(), "compose".into()] };
        (folder, tool)
    }

    #[test]
    fn a_compose_file_in_front_gets_hovers_and_other_files_do_not() {
        let (mut app, _ctx) = super::super::tests::app_with("");
        app.new_document();
        app.doc().path = Some(std::env::temp_dir().join("limoniter-hover").join("docker-compose.yml"));
        app.assembly_tick();
        let intel = app.docs[app.active].text.intel.as_ref().expect("a compose file is given hovers");
        assert!(intel.describe(0, "    image: nginx:1.27", 8).unwrap().contains("Tag 1.27"));
        assert!(intel.describe(0, "      - \"8080:80\"", 8).unwrap().contains("Port 8080"));
        assert!(intel.describe(0, "    restart: always", 8).is_none());
        app.doc().path = Some(std::env::temp_dir().join("limoniter-hover").join("settings.yml"));
        app.assembly_tick();
        assert!(app.docs[app.active].text.intel.as_ref().is_none_or(|i| !i.compose), "another YAML file has none");
    }

    #[cfg(unix)]
    #[test]
    fn following_a_service_s_logs_opens_a_tab_and_closing_it_stops_them() {
        let (folder, tool) = fake_project("logs");
        let (mut app, _ctx) = super::super::tests::app_with("");
        let snapshot = Snapshot { tool: Some(tool), ..Snapshot::default() };
        app.compose_act(Act::Logs("web".into()), Some(folder.clone()), &snapshot);
        app.compose_act(Act::Logs("web".into()), Some(folder), &snapshot);
        assert_eq!(app.compose.logs.len(), 1, "a service has one tab of its own");
        for _ in 0..200 {
            if app.compose.logs[0].ended() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert_eq!(app.compose.logs[0].lines().len(), 2);
        app.compose.logs.clear();
    }

    #[cfg(unix)]
    #[test]
    fn a_project_button_runs_its_command_into_the_output_panel() {
        let (folder, tool) = fake_project("up");
        let (mut app, _ctx) = super::super::tests::app_with("");
        let snapshot = Snapshot { tool: Some(tool), ..Snapshot::default() };
        app.compose_act(Act::Do(Action::Rebuild("web".into())), Some(folder), &snapshot);
        for _ in 0..200 {
            if !app.check_run_tool() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(app.script_output.iter().any(|l| l.contains("ran up -d --build web")), "{:?}", app.script_output);
    }
}
