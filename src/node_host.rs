//! The window's side of Node projects: F9 on a front-end's file starts its dev server and opens the browser once
//! the server says where it's listening, F9 on a plain script runs it with Node; debugging Node needs js-debug,
//! which is offered and downloaded when it's missing; and a `.env` file that git isn't ignoring gets a warning
//! with a button that puts it in `.gitignore`.

use super::*;
use crate::node_tools::{self, RunPlan};
use std::sync::mpsc::Receiver;
use std::time::Instant;

/// A dev server started by F9: where its output starts in the Output panel, and the address once it's said.
struct DevServer {
    from: usize,
    url: Option<String>,
}

enum Installing {
    Said(String),
    Done(Result<(PathBuf, String), String>),
}

#[derive(Default)]
pub(super) struct NodeState {
    dev: Option<DevServer>,
    installing: Option<Receiver<Installing>>,
    /// The debug run that was waiting for js-debug to be installed.
    pub pending_debug: Option<crate::dap::Config>,
    /// The `.env` file last checked against git, what it said (Some(true) when ignored), and when.
    env: Option<(PathBuf, Option<bool>, Instant)>,
}

/// A path or program as the shell wants it, in quotes.
fn quoted(text: &str) -> String {
    if cfg!(windows) { format!("\"{text}\"") } else { format!("'{}'", text.replace('\'', "'\\''")) }
}

/// How long git's answer about a `.env` file is trusted before it's asked again.
const RECHECK: Duration = Duration::from_secs(5);

impl Limoniter {
    /// F9 on a file in a Node project: a front-end's dev server, or a script run with Node. False if the file in
    /// front isn't for Node, so F9 goes on to do what it does for other files.
    pub(super) fn node_run_on_f9(&mut self) -> bool {
        if self.no_file() || self.docs[self.active].hex_mode {
            return false;
        }
        let Some(path) = self.docs[self.active].path.clone() else { return false };
        match node_tools::plan_for(&path) {
            RunPlan::Nothing => false,
            RunPlan::Script => {
                if !crate::lsp::on_path("node") {
                    self.message("Run Node", crate::i18n::tr("Node.js isn't installed, or isn't where Limoniter can find it. It's at nodejs.org; put it on the PATH.").into());
                    return true;
                }
                let line = format!("node {}", quoted(&path.display().to_string()));
                let folder = node_tools::project_for(&path).or_else(|| path.parent().map(Path::to_path_buf));
                if let Err(problem) = self.run_tool_in(&line, true, folder) {
                    self.message("Run Node", problem);
                }
                true
            }
            RunPlan::Server(folder, script) => {
                self.start_dev_server(folder, &script);
                true
            }
        }
    }

    /// Starts the project's server with the package manager it uses (installing the packages first if there are
    /// none yet), or, if it's running already, opens it in the browser again.
    pub(super) fn start_dev_server(&mut self, folder: PathBuf, script: &str) {
        if self.tool_running() {
            match self.node.dev.as_ref().map(|d| d.url.clone()) {
                Some(Some(url)) => {
                    if let Err(problem) = crate::platform::open_web_page(&url) {
                        self.message("Dev server", problem);
                    }
                }
                Some(None) => self.message("Dev server", crate::i18n::tr("The server is still starting. The browser opens once it says where it's listening.").into()),
                None => self.message("Dev server", crate::i18n::tr("A command is still running. Stop it first, in the Output panel.").into()),
            }
            return;
        }
        if !crate::lsp::on_path("node") {
            return self.message("Dev server", crate::i18n::tr("Node.js isn't installed, or isn't where Limoniter can find it. It's at nodejs.org; put it on the PATH.").into());
        }
        let manager = node_tools::manager_for(&folder);
        let run = manager.run_script(script);
        let line = if folder.join("node_modules").is_dir() { run } else { format!("{} && {run}", manager.install()) };
        let from = self.script_output.len();
        match self.run_tool_in(&line, true, Some(folder)) {
            Ok(()) => {
                let from = if self.script_output.len() <= from { 0 } else { from };
                self.node.dev = Some(DevServer { from, url: None });
            }
            Err(problem) => self.message("Dev server", problem),
        }
    }

    /// Watches a dev server's output for the address it's listening on, and opens it. Takes in js-debug's
    /// download as it goes. True while there's something to watch.
    pub(super) fn node_tick(&mut self) -> bool {
        self.js_debug_install_tick();
        if !self.tool_running() {
            self.node.dev = None;
        }
        let Some(dev) = self.node.dev.as_mut() else { return false };
        if dev.url.is_some() {
            return false;
        }
        if dev.from > self.script_output.len() {
            dev.from = 0;
        }
        let found = self.script_output[dev.from..].iter().find_map(|line| node_tools::listening_url(line));
        dev.from = self.script_output.len();
        if let Some(url) = found {
            dev.url = Some(url.clone());
            match crate::platform::open_web_page(&url) {
                Ok(()) => self.script_print(crate::i18n::tf!("The server is listening at {url}. It's opened in the browser."), false),
                Err(problem) => self.script_print(problem, false),
            }
            return false;
        }
        true
    }

    // ---------- js-debug ----------

    /// Starts downloading js-debug, with what it says shown in the Output panel.
    pub(super) fn install_js_debug(&mut self) {
        if self.node.installing.is_some() {
            return;
        }
        let Some(home) = crate::js_debug_get::folder() else {
            return self.message("js-debug", crate::i18n::tr("There's nowhere to keep it: Limoniter's settings folder couldn't be found.").into());
        };
        self.script_print(crate::i18n::tr("Installing js-debug\u{2026}").into(), true);
        let (send, receive) = std::sync::mpsc::channel();
        let wake = self.waker.clone();
        std::thread::spawn(move || {
            let said = send.clone();
            let result = crate::js_debug_get::install(&home, &|line| {
                let _ = said.send(Installing::Said(line));
            });
            let _ = send.send(Installing::Done(result));
            if let Some(ctx) = wake {
                ctx.request_repaint();
            }
        });
        self.node.installing = Some(receive);
    }

    fn js_debug_install_tick(&mut self) {
        let Some(receive) = &self.node.installing else { return };
        let mut said = Vec::new();
        let mut done = None;
        loop {
            match receive.try_recv() {
                Ok(Installing::Said(line)) => said.push(line),
                Ok(Installing::Done(result)) => {
                    done = Some(result);
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    done = Some(Err(crate::i18n::tr("The installation stopped unexpectedly.").into()));
                    break;
                }
            }
        }
        for line in said {
            self.script_print(line, false);
        }
        let Some(result) = done else { return };
        self.node.installing = None;
        match result {
            Ok((script, version)) => {
                self.script_print(crate::i18n::tf!("js-debug is installed ({version}), as {}.", script.display()), false);
                if matches!(self.dialog, Dialog::JsDebugMissing) {
                    self.dialog = Dialog::None;
                }
                // The debugging that was waiting for it goes ahead.
                if let Some(mut config) = self.node.pending_debug.take() {
                    config.adapter = crate::js_debug_get::command(&script);
                    self.launch_debugger(config);
                }
            }
            Err(problem) => {
                self.node.pending_debug = None;
                let words = crate::i18n::tf!("js-debug couldn't be installed: {problem}");
                self.script_print(words.clone(), false);
                self.message("js-debug", words);
            }
        }
    }

    /// The box offered when Node is to be debugged and js-debug isn't installed.
    pub(super) fn node_dialog(&mut self, ctx: &Context) -> bool {
        if !matches!(self.dialog, Dialog::JsDebugMissing) {
            return false;
        }
        let installing = self.node.installing.is_some();
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut install = false;
        crate::fit::Dialog::new(crate::i18n::tr("Debug")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(520.0);
            ui.label(crate::i18n::tr("JavaScript is debugged with js-debug, the debugger VS Code uses for Node, and it isn't installed."));
            ui.add_space(6.0);
            if installing {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(crate::i18n::tr("js-debug is being installed. The Output panel shows how it's going."));
                });
            } else {
                ui.add(egui::Label::new(RichText::new(crate::i18n::tr("Limoniter downloads the latest release from github.com/microsoft/vscode-js-debug, where its authors put it, and keeps it in a folder beside its own settings. It runs on the Node.js you already have. Nothing else on the computer is changed.")).color(theme::text_dim()).size(12.0)).wrap());
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if !installing {
                    install = ui.button(crate::i18n::tr("Download js-debug")).clicked();
                }
                close |= ui.button(crate::i18n::tr("Close")).clicked();
            });
        });
        if install {
            self.install_js_debug();
        }
        if close {
            self.node.pending_debug = None;
            self.dialog = Dialog::None;
            self.pending_focus = true;
        }
        true
    }

    // ---------- .env files ----------

    /// The warning over a `.env` file that git isn't ignoring, with a button that adds it to `.gitignore`.
    pub(super) fn env_bar(&mut self, ui: &mut Ui) {
        let Some(doc) = self.docs.get(self.active).filter(|d| !d.hex_mode && d.browser.is_none()) else { return };
        let Some(path) = doc.path.clone().filter(|p| crate::env_file::is_env_file(p)) else { return };
        let fresh = self.node.env.as_ref().is_some_and(|(p, _, when)| *p == path && when.elapsed() < RECHECK);
        if !fresh {
            self.node.env = Some((path.clone(), crate::env_file::ignored_by_git(&path), Instant::now()));
        }
        if self.node.env.as_ref().map(|(_, ignored, _)| *ignored) != Some(Some(false)) {
            return;
        }
        let mut add = false;
        Panel::top("env-not-ignored").frame(egui::Frame::new().fill(theme::selected()).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(crate::i18n::tr("This file usually holds secrets, and git isn't ignoring it, so it could be committed by mistake.")).strong());
                add = ui.button(crate::i18n::tr("Add to .gitignore")).on_hover_text(crate::i18n::tr("Adds this file's name to the .gitignore in its folder, which is made if there isn't one.")).clicked();
            });
        });
        if add {
            match crate::env_file::add_to_gitignore(&path) {
                Ok(ignore) => {
                    self.script_print(crate::i18n::tf!("{} is in {} now.", path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()), ignore.display()), false);
                    self.node.env = Some((path.clone(), crate::env_file::ignored_by_git(&path), Instant::now()));
                }
                Err(problem) => self.message(".env", problem),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("limoniter-nodehost-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    /// A project whose dev script is a little Node program that says what Vite says.
    fn fake_front_end(folder: &Path) {
        std::fs::write(folder.join("package.json"), r#"{"scripts": {"dev": "node dev.js"}, "devDependencies": {"vite": "5"}}"#).unwrap();
        std::fs::write(folder.join("package-lock.json"), "{}").unwrap();
        std::fs::create_dir_all(folder.join("node_modules")).unwrap();
        std::fs::write(folder.join("dev.js"), "console.log('  \\x1b[32m->\\x1b[39m  Local:   http://localhost:5199/');\nsetTimeout(() => {}, 300);\n").unwrap();
        std::fs::create_dir_all(folder.join("src")).unwrap();
        std::fs::write(folder.join("src").join("App.jsx"), "export default () => <div/>;\n").unwrap();
    }

    fn wait_for(app: &mut Limoniter, what: impl Fn(&Limoniter) -> bool) -> bool {
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(20) {
            let running = app.check_run_tool();
            app.node_tick();
            let _ = running;
            if what(app) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn f9_starts_the_dev_server_and_reads_its_address() {
        if !crate::lsp::on_path("node") {
            eprintln!("Node isn't installed, so the dev server wasn't run.");
            return;
        }
        let folder = scratch("dev");
        fake_front_end(&folder);
        let (mut app, _ctx) = app_with("");
        app.open_path(&folder.join("src").join("App.jsx"));
        assert!(app.node_run_on_f9(), "a component of a front-end is the server's");
        assert!(app.tool_running());
        assert!(wait_for(&mut app, |a| a.script_output.iter().any(|l| l.contains("opened in the browser"))), "{:?}", app.script_output);
        assert!(app.script_output.iter().any(|l| l.contains("http://localhost:5199/")));
        assert!(app.script_output[0].contains("npm run dev"), "run with the manager the lock file names: {:?}", app.script_output);
        assert!(wait_for(&mut app, |a| a.node.dev.is_none()), "forgotten once it ends");
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn f9_runs_a_plain_script_with_node() {
        if !crate::lsp::on_path("node") {
            eprintln!("Node isn't installed, so the script wasn't run.");
            return;
        }
        let folder = scratch("script");
        std::fs::write(folder.join("package.json"), r#"{"scripts": {"start": "node hello.js"}}"#).unwrap();
        std::fs::write(folder.join("hello.js"), "console.log('hello from node', process.env.LIMONITER_NODE_TEST_VALUE);\n").unwrap();
        std::fs::write(folder.join(".env"), "LIMONITER_NODE_TEST_VALUE=from-the-env-file\n").unwrap();
        let (mut app, _ctx) = app_with("");
        app.open_path(&folder.join("hello.js"));
        assert!(app.node_run_on_f9());
        assert!(wait_for(&mut app, |a| !a.tool_running()), "{:?}", app.script_output);
        assert!(app.script_output.iter().any(|l| l.contains("hello from node from-the-env-file")), "the project's .env is given to what runs: {:?}", app.script_output);
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn f9_leaves_other_files_alone() {
        let folder = scratch("other");
        std::fs::write(folder.join("notes.txt"), "x").unwrap();
        let (mut app, _ctx) = app_with("");
        app.open_path(&folder.join("notes.txt"));
        assert!(!app.node_run_on_f9());
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
