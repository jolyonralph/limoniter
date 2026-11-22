//! Debugging a program with a language's own debugger, through the Debug Adapter Protocol (see dap.rs): breakpoints
//! set beside the line numbers, the line it's stopped on marked as the assembler debuggers mark it, and a Debug
//! panel with the controls, the call stack, the variables (opened a level at a time) and a console that shows what
//! the program writes and works out expressions typed into it. While a session runs, F5 carries on, Shift+F5
//! stops, F10 steps over, F11 steps in and Shift+F11 steps out, as in VS Code. An adapter that can go backwards
//! (Copperline's) steps back with Shift+F10, and runs back to the breakpoint before; one that reads memory (also
//! Copperline's) has a window on it, as the assembler debuggers have.

use super::*;
use crate::dap::{self, Config, Frame, Session, Variable};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap, HashSet};

/// The most lines the console keeps.
const MOST_CONSOLE_LINES: usize = 3000;

/// Whether files in a language can be debugged here.
pub(super) fn debuggable(language: &str) -> bool {
    matches!(language, "Python" | "C" | "C++" | "Rust")
}

/// What a request was, to know what to do with its answer.
#[derive(Clone, Debug)]
enum Asked {
    Initialize,
    Launch,
    Threads,
    StackTrace,
    Scopes,
    Variables(i64),
    Evaluate(String),
    /// Where the memory window is, worked out from what was typed (a register or a label).
    MemoryAt(String),
    Memory,
    /// Copperline's own copperline/profile.
    Profile,
    /// Copperline's chipset.report and smc.report, asked through evaluate.
    Warnings(WarningKind),
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WarningKind {
    Chipset,
    Code,
}

/// What's Copperline's own in a session: where the program is (to say which line a warning is about), its
/// warnings when last looked for (or why they couldn't be read), and whether the stop is being read again.
#[derive(Default)]
pub(super) struct CopperlineView {
    pub program: dap::ProgramMap,
    chipset: Option<Result<Vec<dap::Warning>, String>>,
    code: Option<Result<Vec<dap::Warning>, String>>,
    looking: bool,
    /// Set for a picture, to fold the call stack and variables away once.
    fold: bool,
    /// Set when the stop is read again after a raw request (which makes the adapter forget its variables), so
    /// the memory window isn't taken to have changed.
    rereading: bool,
}

/// Copperline's CPU profile: how many frames to profile for, whether it's being made, and the last one (with the
/// text of its busiest lines), or why there isn't one.
#[derive(Default)]
pub(super) struct ProfileView {
    frames: u32,
    making: bool,
    made: Option<Result<(dap::CpuProfile, HashMap<(PathBuf, usize), String>), String>>,
}

/// The memory window: what's typed for where it is, where that was worked out to be, and what's there now and
/// was there at the stop before.
#[derive(Default)]
pub(super) struct MemoryView {
    pub at: String,
    /// What was last worked out, and what it came to.
    asked: Option<String>,
    address: Option<Result<u64, String>>,
    now: Option<(u64, Vec<u8>)>,
    before: Option<(u64, Vec<u8>)>,
}

/// How much of the memory is read at a time: eight rows of sixteen.
const MEMORY_SHOWN: u64 = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum State {
    Starting,
    Running,
    Paused,
    Ended,
}

pub(super) struct Debugging {
    session: Session,
    /// For js-debug, which starts a session for each program it runs: the sessions before this one, the first of
    /// which holds the adapter's process.
    parents: Vec<Session>,
    /// What a session js-debug asked for is started with (launch or attach, and the configuration it gave).
    child_launch: Option<(String, Value)>,
    pub config: Config,
    asked: HashMap<i64, Asked>,
    pub state: State,
    /// Why it stopped, and in which thread.
    reason: String,
    thread: i64,
    pub frames: Vec<Frame>,
    chosen: usize,
    scopes: Vec<Variable>,
    /// The parts of each variable (or scope) opened, by the number they're asked for by.
    pub parts: HashMap<i64, Vec<Variable>>,
    open: HashSet<i64>,
    pub console: Vec<String>,
    input: String,
    /// The breakpoints last told for each file, by line from 0.
    told: HashMap<PathBuf, BTreeSet<usize>>,
    /// Set once the adapter's been told the breakpoints and that it can go.
    configured: bool,
    /// Whether the console's last line is still being written.
    line_open: bool,
    /// What the adapter said it can do.
    pub can: dap::Capabilities,
    pub memory: MemoryView,
    profile: ProfileView,
    pub copperline: CopperlineView,
}

impl Debugging {
    /// Ends the adapter, and the sessions before this one, which hold its process.
    fn kill(&mut self) {
        self.session.stop();
        for parent in &mut self.parents {
            parent.stop();
        }
    }

    /// Whether the adapter's process has ended, and the last thing it said of why.
    fn adapter_ended(&mut self) -> Option<String> {
        match self.parents.first_mut() {
            Some(first) => first.ended(),
            None => self.session.ended(),
        }
    }

    fn ask(&mut self, command: &str, arguments: Value, asked: Asked) {
        let seq = self.session.request(command, arguments);
        self.asked.insert(seq, asked);
    }

    fn say(&mut self, text: &str) {
        // What's written comes in pieces: each goes on the line before until a line ends.
        for (n, piece) in text.split('\n').enumerate() {
            if n > 0 {
                self.line_open = false;
            }
            if piece.is_empty() {
                continue;
            }
            match self.console.last_mut() {
                Some(last) if self.line_open => last.push_str(piece),
                _ => self.console.push(piece.to_string()),
            }
            self.line_open = true;
        }
        let extra = self.console.len().saturating_sub(MOST_CONSOLE_LINES);
        self.console.drain(..extra);
    }

    /// Says a whole line in the console.
    fn line(&mut self, text: String) {
        self.line_open = false;
        self.say(&text);
        self.line_open = false;
    }

    fn step(&mut self, command: &str) {
        if self.state == State::Paused {
            let thread = self.thread;
            self.ask(command, json!({ "threadId": thread }), Asked::Other);
            self.state = State::Running;
        }
    }

    /// Works out where the memory window is (A0 when nothing's typed), and reads it, once it's stopped.
    fn look_at_memory(&mut self) {
        if !self.can.read_memory || self.state != State::Paused {
            return;
        }
        let typed = if self.memory.at.trim().is_empty() { "a0".to_string() } else { self.memory.at.trim().to_string() };
        self.memory.asked = Some(self.memory.at.clone());
        match dap::number(&typed) {
            Some(address) => self.read_memory(address),
            None => {
                let frame = self.frames.get(self.chosen).map(|f| f.id);
                self.ask("evaluate", json!({ "expression": typed, "frameId": frame, "context": "watch" }), Asked::MemoryAt(typed.clone()));
            }
        }
    }

    /// Asks Copperline for its warnings about the program: the chipset validator's and the self-modifying code
    /// detector's (armed by the run's configuration), as raw control requests through evaluate.
    fn look_for_warnings(&mut self) {
        if self.state != State::Paused || self.copperline.looking {
            return;
        }
        self.copperline.looking = true;
        self.ask("evaluate", json!({ "expression": "!chipset.report", "context": "repl" }), Asked::Warnings(WarningKind::Chipset));
        self.ask("evaluate", json!({ "expression": "!smc.report", "context": "repl" }), Asked::Warnings(WarningKind::Code));
    }

    /// One report read. Once both are, the stop is read again: a raw request makes the adapter forget the
    /// variables it numbered, so the ones shown would no longer open.
    fn took_warnings(&mut self, kind: WarningKind, read: Result<Vec<dap::Warning>, String>) {
        match kind {
            WarningKind::Chipset => self.copperline.chipset = Some(read),
            WarningKind::Code => {
                self.copperline.code = Some(read);
                self.copperline.looking = false;
                if self.state == State::Paused {
                    self.copperline.rereading = true;
                    let thread = self.thread;
                    self.ask("stackTrace", json!({ "threadId": thread, "startFrame": 0, "levels": 64 }), Asked::StackTrace);
                }
            }
        }
    }

    fn read_memory(&mut self, address: u64) {
        let start = address & !0xF;
        self.memory.address = Some(Ok(start));
        self.ask("readMemory", json!({ "memoryReference": format!("0x{start:X}"), "count": MEMORY_SHOWN }), Asked::Memory);
    }
}

/// A session dropped while it's going (Limoniter closed, say) is ended as Stop ends it, so an emulator its adapter
/// started (Copperline) is closed with it rather than left running.
impl Drop for Debugging {
    fn drop(&mut self) {
        if self.state != State::Ended {
            self.ask("disconnect", json!({ "terminateDebuggee": true }), Asked::Other);
            self.session.end_within(Duration::from_secs(if self.config.kind == dap::Kind::Copperline { 3 } else { 0 }));
            self.kill();
        }
    }
}

impl Limoniter {
    /// Starts debugging the file in front (Ctrl+F5): at once for a script, or for a program built from it, once
    /// it's been said which program that is.
    pub(super) fn start_debugging(&mut self) {
        if self.debugging.as_ref().is_some_and(|d| d.state != State::Ended) {
            return self.debug_control("continue");
        }
        let Some(path) = self.docs.get(self.active).and_then(|d| d.path.clone()).filter(|_| !self.no_file()) else {
            return self.message("Debug", crate::i18n::tr("Save the file first: a debugger runs it from the disk.").into());
        };
        let language = self.docs[self.active].language_name();
        let Some((kind, adapter)) = dap::adapter_for(&language) else {
            return self.message("Debug", crate::i18n::tr("Python, JavaScript, C, C++ and Rust files can be debugged here, with debugpy, js-debug, gdb or lldb-dap.").into());
        };
        if self.docs[self.active].is_modified() && !self.save(self.active) {
            return;
        }
        match self.kept_debug_config(&path) {
            // Kept from before, but its program isn't there (not built yet, or moved): the box is shown again,
            // filled in as it was, to say so and to choose another.
            Some(config) if !config.kind.runs_the_file() && !Path::new(&config.program).exists() => self.debug_setup = Some((path, config)),
            Some(config) => self.launch_debugger(config),
            // A script is run as it is; for a compiled program, which one is asked first.
            None if kind.runs_the_file() => self.launch_debugger(dap::guess(kind, adapter, &path)),
            None => self.debug_setup = Some((path.clone(), dap::guess(kind, adapter, &path))),
        }
    }

    /// Whether a file's debug settings are kept with the open project (the file's in it, or in its folder),
    /// rather than in the settings file.
    fn debug_kept_in_project(&self, file: &Path) -> bool {
        self.project.as_ref().is_some_and(|project| project.contains(file) || file.starts_with(project.folder()))
    }

    /// How a file was debugged before, kept in its project or in the settings.
    fn kept_debug_config(&self, file: &Path) -> Option<Config> {
        let in_project = self.project.as_ref().filter(|_| self.debug_kept_in_project(file)).and_then(|project| project.debug.get(file));
        in_project.or_else(|| self.debug_configs.get(file)).cloned()
    }

    /// Keeps how a file is debugged, for next time: in the project's file for one of its files, and otherwise in
    /// the settings file.
    fn keep_debug_config(&mut self, file: PathBuf, config: Config) {
        if self.debug_kept_in_project(&file)
            && let Some(project) = self.project.as_mut()
        {
            project.debug.insert(file, config);
            self.save_project();
        } else {
            self.debug_configs.insert(file, config);
        }
    }

    /// Finds the scripts of a `package.json` in front, for the "Debug" link beside each; other files have none.
    pub(super) fn package_script_links(&mut self) {
        let Some(doc) = self.docs.get_mut(self.active) else { return };
        let is_package = doc.path.as_deref().and_then(Path::file_name).is_some_and(|n| n == "package.json") && !doc.hex_mode && doc.browser.is_none();
        let version = doc.text.version();
        if !is_package {
            if doc.text_state.script_links_version.take().is_some() {
                doc.text_state.script_links.clear();
            }
            return;
        }
        if doc.text_state.script_links_version == Some(version) {
            return;
        }
        let text = doc.text.all_lines().iter().map(|l| l.as_ref()).collect::<Vec<&str>>().join("\n");
        doc.text_state.script_links = crate::node_tools::script_lines(&text);
        doc.text_state.script_links_version = Some(version);
    }

    /// Debugs one of a project's `package.json` scripts: js-debug runs it with the project's package manager, in
    /// the project's folder.
    pub(super) fn debug_npm_script(&mut self, folder: &Path, script: &str) {
        let Some((kind, adapter)) = dap::adapter_for("JavaScript") else { return };
        let config = Config { kind, adapter, program: format!("script:{script}"), arguments: String::new(), folder: folder.display().to_string(), emulator: None, env: Vec::new(), stop_on_entry: false };
        self.launch_debugger(config);
    }

    /// Starts the adapter and the program in it.
    pub(super) fn launch_debugger(&mut self, mut config: Config) {
        // Python is debugged in the environment chosen for the file, so it sees the same packages as F9 does.
        if config.kind == dap::Kind::Debugpy
            && config.adapter == dap::DEBUGPY
            && let Some(env) = self.python_env_for(Path::new(&config.program)).filter(|e| e.kind != crate::py_env::Kind::System)
        {
            // In double quotes, as the adapter's command is split.
            config.adapter = format!("\"{}\" -m debugpy.adapter", env.python.display());
        }
        // One still going is ended first, as Stop ends it: an adapter that started an emulator closes it.
        self.debug_control("stop");
        if let Some(mut old) = self.debugging.take() {
            old.kill();
        }
        self.clear_stopped_marks();
        if config.kind == dap::Kind::Debugpy && !python_has_debugpy(&config.adapter) {
            let python = crate::emulators::split_options(&config.adapter).first().cloned().unwrap_or_default();
            let install = if config.adapter != dap::DEBUGPY {
                format!("{} -m pip install debugpy", crate::py_env::quoted(&python))
            } else if cfg!(windows) {
                "py -m pip install debugpy".to_string()
            } else {
                "python3 -m pip install --user debugpy".to_string()
            };
            return self.message("Debug", crate::i18n::tf!("Python is debugged with debugpy, which isn't installed. In a terminal (Ctrl+`):\n\n{install}"));
        }
        if config.kind == dap::Kind::JsDebug {
            if !crate::lsp::on_path("node") {
                return self.message("Debug", crate::i18n::tr("JavaScript is debugged with js-debug, which runs on Node.js, and Node.js isn't installed. It's at nodejs.org.").into());
            }
            // The adapter is Limoniter's own download, or one the person has put on the PATH.
            if crate::emulators::split_options(&config.adapter).first().is_some_and(|first| first == "js-debug-dap") && !crate::lsp::on_path("js-debug-dap") {
                self.node.pending_debug = Some(config);
                self.dialog = Dialog::JsDebugMissing;
                return;
            }
            return match Session::start_tcp(&config.adapter, Path::new(&config.folder)) {
                Ok(session) => self.begin_debugging(config, session),
                Err(problem) => self.message("Debug", problem),
            };
        }
        if config.kind == dap::Kind::Xdebug {
            if config.adapter.is_empty() {
                return self.xdebug_download(config);
            }
            if !crate::lsp::on_path("node") {
                return self.message("Debug", crate::i18n::tr("PHP is debugged with VS Code's PHP Debug adapter, which is a Node.js program, and Node.js isn't installed.").into());
            }
            // Listening for a page in the browser needs nothing of the PHP on this computer, but a script is run by it.
            if !config.program.is_empty() && crate::xdebug_get::php_has_xdebug() == Some(false) {
                return self.message("Debug", crate::i18n::tr("The PHP on this computer doesn't have Xdebug, the extension that debugs it. Install it (pecl install xdebug, or your system's php-xdebug package) and try again.").into());
            }
        }
        if !matches!(config.kind, dap::Kind::Debugpy | dap::Kind::Xdebug | dap::Kind::Copperline) && !crate::lsp::on_path(crate::emulators::split_options(&config.adapter).first().map_or("", String::as_str)) {
            return self.message("Debug", crate::i18n::tr("C, C++ and Rust are debugged with gdb (version 14 or later) or lldb-dap, and neither is installed.").into());
        }
        match Session::start(&config.adapter, Path::new(&config.folder)) {
            Ok(session) => self.begin_debugging(config, session),
            Err(problem) => self.message("Debug", problem),
        }
    }

    /// Waits for Xdebug to connect, for PHP run by a web server (a page in the browser) or from the terminal.
    pub(super) fn debug_php_listen(&mut self) {
        let Some(folder) = self.task_folder().or_else(|| std::env::current_dir().ok()) else { return };
        let config = dap::Config { kind: dap::Kind::Xdebug, adapter: crate::xdebug_get::adapter_command().unwrap_or_default(), program: String::new(), arguments: String::new(), folder: folder.display().to_string(), emulator: None, env: Vec::new(), stop_on_entry: false };
        self.launch_debugger(config);
    }

    /// Downloads VS Code's PHP Debug adapter, in the background, and starts debugging when it's there.
    fn xdebug_download(&mut self, config: Config) {
        match crate::xdebug_get::start_download(Some(config)) {
            Ok(()) => self.script_print(crate::i18n::tr("Downloading the PHP debug adapter (VS Code's PHP Debug, from open-vsx.org)\u{2026}").into(), true),
            Err(problem) => self.message("Debug", problem),
        }
    }

    /// While the PHP debug adapter downloads: when it's done, debugging starts, or why it couldn't be got is said.
    pub(super) fn xdebug_tick(&mut self) -> bool {
        let done = {
            let mut going = crate::xdebug_get::DOWNLOADING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some((receive, _)) = going.as_ref() else { return false };
            match receive.try_recv() {
                Ok(result) => going.take().map(|(_, then)| (result, then)),
                Err(std::sync::mpsc::TryRecvError::Empty) => return true,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => going.take().map(|(_, then)| (Err(crate::i18n::tr("The download stopped unexpectedly.").into()), then)),
            }
        };
        match done {
            Some((Ok(_), then)) => {
                self.script_print(crate::i18n::tr("The PHP debug adapter is downloaded.").into(), false);
                if let (Some(mut config), Some(adapter)) = (then, crate::xdebug_get::adapter_command()) {
                    config.adapter = adapter;
                    self.launch_debugger(config);
                }
            }
            Some((Err(problem), _)) => self.message("Debug", crate::i18n::tf!("The PHP debug adapter couldn't be downloaded.\n\n{problem}")),
            None => {}
        }
        false
    }

    /// Starts talking to an adapter that's been started: it's asked what it can do, and then (as it answers) to
    /// launch the program.
    fn begin_debugging(&mut self, config: Config, session: Session) {
        let mut debugging = Debugging { session, parents: Vec::new(), child_launch: None, config, asked: HashMap::new(), state: State::Starting, reason: String::new(), thread: 0, frames: Vec::new(), chosen: 0, scopes: Vec::new(), parts: HashMap::new(), open: HashSet::new(), console: Vec::new(), input: String::new(), told: HashMap::new(), configured: false, line_open: false, can: Default::default(), memory: Default::default(), profile: Default::default(), copperline: Default::default() };
        let adapter_id = debugging.config.kind.adapter_name();
        debugging.ask("initialize", json!({ "clientID": "limoniter", "clientName": "Limoniter", "adapterID": adapter_id, "linesStartAt1": true, "columnsStartAt1": true, "pathFormat": "path", "supportsVariableType": true, "supportsRunInTerminalRequest": false }), Asked::Initialize);
        let program = Path::new(&debugging.config.program).file_name().map_or_else(|| debugging.config.program.clone(), |n| n.to_string_lossy().into_owned());
        if debugging.config.program.is_empty() {
            let port = crate::xdebug_get::PORT;
            debugging.line(crate::i18n::tf!("Waiting for Xdebug on port {port}: open the page in the browser, or run the script from the terminal with XDEBUG_SESSION=1"));
        } else {
            debugging.line(crate::i18n::tf!("Debugging {program} with {adapter_id}"));
        }
        self.debugging = Some(Box::new(debugging));
    }

    /// Runs an Amiga program assembled here in Copperline through its debug adapter, which starts the emulator with
    /// the program and stops at the source's breakpoints, in the Debug panel. `program` is the copy made for the
    /// run, with its configuration beside it; `emulator` is Copperline, `ctl` its copperline-ctl, and `options`
    /// the person's own for the emulator.
    pub(super) fn debug_in_copperline(&mut self, program: &Path, emulator: &Path, ctl: &Path, options: &str, assembly: Option<&crate::asm::Assembly>) {
        let folder = program.parent().map(Path::to_path_buf).unwrap_or_default();
        let emulator = Some(dap::Emulated { program: emulator.display().to_string(), options: crate::emulators::split_options(options) });
        let config = Config { kind: dap::Kind::Copperline, adapter: dap::copperline_adapter(ctl), program: program.display().to_string(), arguments: String::new(), folder: folder.display().to_string(), emulator, env: Vec::new(), stop_on_entry: false };
        self.launch_debugger(config);
        if let (Some(d), Some(assembly)) = (self.debugging.as_mut(), assembly) {
            d.copperline.program = dap::ProgramMap::of(assembly);
        }
    }

    /// Takes the stopped line's mark off every file.
    fn clear_stopped_marks(&mut self) {
        for doc in &mut self.docs {
            if let Some(code) = doc.text.code.as_mut() {
                code.stopped = None;
            }
        }
    }

    /// The breakpoints set in each open file, by its path.
    fn breakpoints_set(&self) -> HashMap<PathBuf, BTreeSet<usize>> {
        self.docs.iter().filter_map(|d| Some((d.path.clone()?, d.text.code.as_ref()?.breakpoints.clone()))).filter(|(_, b)| !b.is_empty()).collect()
    }

    /// Tells the adapter of breakpoints set or cleared since it was last told.
    fn tell_breakpoints(&mut self) {
        let now = self.breakpoints_set();
        let Some(d) = self.debugging.as_mut() else { return };
        let files: BTreeSet<PathBuf> = now.keys().chain(d.told.keys()).cloned().collect();
        for file in files {
            let lines = now.get(&file).cloned().unwrap_or_default();
            if d.told.get(&file) == Some(&lines) {
                continue;
            }
            let points: Vec<Value> = lines.iter().map(|l| json!({ "line": l + 1 })).collect();
            d.ask("setBreakpoints", json!({ "source": { "path": file, "name": file.file_name().map(|n| n.to_string_lossy().into_owned()) }, "breakpoints": points }), Asked::Other);
            d.told.insert(file, lines);
        }
    }

    /// Shows where a frame is: its file in front, its line marked.
    fn show_frame(&mut self, frame: &Frame) {
        self.clear_stopped_marks();
        let Some(path) = frame.path.clone().filter(|p| p.exists()) else { return };
        self.go_to_place(&path.display().to_string(), frame.line + 1);
        let doc = &mut self.docs[self.active];
        doc.text.code.get_or_insert_with(Box::default).stopped = Some(frame.line);
    }

    /// What the adapter has said, dealt with; and the breakpoints kept up to date.
    pub(super) fn debug_tick(&mut self, ctx: &Context) {
        if self.debugging.as_ref().is_some_and(|d| d.state != State::Ended) {
            ctx.request_repaint_after(Duration::from_millis(100));
            self.debug_poll();
        }
    }

    /// Takes in what the adapter's said.
    pub(super) fn debug_poll(&mut self) {
        let Some(d) = self.debugging.as_mut() else { return };
        if d.state == State::Ended {
            return;
        }
        let said = d.session.poll();
        // The sessions before this one still talk: what they print is shown, and what they ask for is declined.
        let mut from_parents = Vec::new();
        for parent in &mut d.parents {
            from_parents.extend(parent.poll());
        }
        for message in from_parents {
            match message.get("type").and_then(Value::as_str) {
                Some("request") => d.parents[0].decline(&message),
                Some("event") if message.get("event").and_then(Value::as_str) == Some("output") => {
                    let body = message.get("body").cloned().unwrap_or(Value::Null);
                    if body.get("category").and_then(Value::as_str) != Some("telemetry") {
                        let text = body.get("output").and_then(Value::as_str).unwrap_or("").trim_end().to_string();
                        if !text.is_empty() {
                            d.line(text);
                        }
                    }
                }
                _ => {}
            }
        }
        let mut show: Option<Frame> = None;
        for message in said {
            let Some(d) = self.debugging.as_mut() else { return };
            match message.get("type").and_then(Value::as_str) {
                Some("request") if message.get("command").and_then(Value::as_str) == Some("startDebugging") && d.config.kind == dap::Kind::JsDebug && d.parents.is_empty() => {
                    // js-debug runs the program in a session of its own, on another connection to the same adapter.
                    match d.session.another() {
                        Ok(child) => {
                            d.session.accept(&message);
                            let request = message.pointer("/arguments/request").and_then(Value::as_str).unwrap_or("launch").to_string();
                            let configuration = message.pointer("/arguments/configuration").cloned().unwrap_or(Value::Null);
                            let parent = std::mem::replace(&mut d.session, child);
                            d.parents.push(parent);
                            d.child_launch = Some((request, configuration));
                            d.asked.clear();
                            d.state = State::Starting;
                            d.configured = false;
                            d.told.clear();
                            let id = d.config.kind.adapter_name();
                            d.ask("initialize", json!({ "clientID": "limoniter", "clientName": "Limoniter", "adapterID": id, "linesStartAt1": true, "columnsStartAt1": true, "pathFormat": "path", "supportsVariableType": true, "supportsRunInTerminalRequest": false, "supportsStartDebuggingRequest": true }), Asked::Initialize);
                        }
                        Err(problem) => {
                            d.session.decline(&message);
                            d.line(problem);
                        }
                    }
                }
                Some("request") => d.session.decline(&message),
                Some("event") => {
                    let body = message.get("body").cloned().unwrap_or(Value::Null);
                    match message.get("event").and_then(Value::as_str).unwrap_or("") {
                        "initialized" => {
                            d.configured = true;
                            self.tell_breakpoints();
                            if let Some(d) = self.debugging.as_mut() {
                                // Copperline stops at an address error, an illegal instruction and TRAP #7 unless
                                // told not to, which is what's wanted of a program being written.
                                let filters = if d.config.kind == dap::Kind::Copperline { d.can.exception_defaults.clone() } else { Vec::new() };
                                d.ask("setExceptionBreakpoints", json!({ "filters": filters }), Asked::Other);
                                d.ask("configurationDone", json!({}), Asked::Other);
                            }
                        }
                        "stopped" => {
                            d.state = State::Paused;
                            d.reason = body.get("reason").and_then(Value::as_str).unwrap_or("").to_string();
                            // Copperline says why it stopped where the reason alone wouldn't: the exception, or
                            // what a breakpoint set in its own window was.
                            if let Some(why) = body.get("description").and_then(Value::as_str).filter(|w| !w.is_empty() && d.config.kind == dap::Kind::Copperline) {
                                d.line(why.to_string());
                            }
                            match body.get("threadId").and_then(Value::as_i64) {
                                Some(thread) => {
                                    d.thread = thread;
                                    d.ask("stackTrace", json!({ "threadId": thread, "startFrame": 0, "levels": 64 }), Asked::StackTrace);
                                }
                                None => d.ask("threads", json!({}), Asked::Threads),
                            }
                        }
                        "continued" => {
                            d.state = State::Running;
                            d.frames.clear();
                            d.scopes.clear();
                            d.parts.clear();
                            self.clear_stopped_marks();
                        }
                        "output" => {
                            if body.get("category").and_then(Value::as_str) != Some("telemetry") {
                                let text = body.get("output").and_then(Value::as_str).unwrap_or("").to_string();
                                if let Some(bases) = dap::loaded_at(&text).filter(|_| d.config.kind == dap::Kind::Copperline) {
                                    d.copperline.program.bases = bases;
                                }
                                d.say(&text);
                            }
                        }
                        "exited" => {
                            let code = body.get("exitCode").and_then(Value::as_i64).unwrap_or(0);
                            d.line(crate::i18n::tf!("The program ended, with code {code}."));
                        }
                        "terminated" => {
                            d.state = State::Ended;
                            d.kill();
                            self.clear_stopped_marks();
                        }
                        _ => {}
                    }
                }
                Some("response") => {
                    let seq = message.get("request_seq").and_then(Value::as_i64).unwrap_or(0);
                    let asked = d.asked.remove(&seq).unwrap_or(Asked::Other);
                    let body = message.get("body").cloned().unwrap_or(Value::Null);
                    if message.get("success").and_then(Value::as_bool) == Some(false) {
                        let why = message.get("message").and_then(Value::as_str).unwrap_or("").to_string();
                        let detail = message.pointer("/body/error/format").and_then(Value::as_str).unwrap_or("").to_string();
                        match asked {
                            Asked::Evaluate(_) => d.line(if detail.is_empty() { why } else { detail }),
                            Asked::MemoryAt(_) | Asked::Memory => d.memory.address = Some(Err(if detail.is_empty() { why } else { detail })),
                            Asked::Profile => {
                                d.profile.making = false;
                                d.profile.made = Some(Err(if detail.is_empty() { why } else { detail }));
                            }
                            Asked::Warnings(kind) => d.took_warnings(kind, Err(if detail.is_empty() { why } else { detail })),
                            Asked::Launch => {
                                d.line(crate::i18n::tf!("It couldn't be started: {} {}", why, detail));
                                d.state = State::Ended;
                                d.kill();
                            }
                            _ if !why.is_empty() && why != "cancelled" => d.line(why),
                            _ => {}
                        }
                        continue;
                    }
                    match asked {
                        Asked::Initialize => {
                            d.can = dap::capabilities(&body);
                            // A session js-debug asked for is told what it was asked to be.
                            let (command, launch) = d.child_launch.take().unwrap_or_else(|| ("launch".to_string(), dap::launch_arguments(&d.config)));
                            d.ask(&command, launch, Asked::Launch);
                            d.state = State::Running;
                        }
                        Asked::Threads => {
                            if let Some(thread) = body.pointer("/threads/0/id").and_then(Value::as_i64) {
                                d.thread = thread;
                                d.ask("stackTrace", json!({ "threadId": thread, "startFrame": 0, "levels": 64 }), Asked::StackTrace);
                            }
                        }
                        Asked::StackTrace => {
                            d.frames = dap::frames(&body);
                            // The first frame in source there is (not in the runtime's own code).
                            d.chosen = d.frames.iter().position(|f| f.path.as_ref().is_some_and(|p| p.exists())).unwrap_or(0);
                            if let Some(frame) = d.frames.get(d.chosen).cloned() {
                                d.ask("scopes", json!({ "frameId": frame.id }), Asked::Scopes);
                                show = Some(frame);
                            }
                            // What was in the memory window is kept, for what's changed since to stand out.
                            if !std::mem::take(&mut d.copperline.rereading) {
                                if d.memory.now.is_some() {
                                    d.memory.before = d.memory.now.take();
                                }
                                d.look_at_memory();
                            }
                        }
                        Asked::Scopes => {
                            d.scopes = dap::scopes(&body);
                            d.parts.clear();
                            // The first scope (the frame's own variables) opened.
                            if let Some(first) = d.scopes.first().map(|s| s.parts) {
                                d.open.insert(first);
                                d.ask("variables", json!({ "variablesReference": first }), Asked::Variables(first));
                            }
                        }
                        Asked::Variables(number) => {
                            d.parts.insert(number, dap::variables(&body));
                        }
                        Asked::Evaluate(expression) => {
                            let result = body.get("result").and_then(Value::as_str).unwrap_or("").to_string();
                            d.line(format!("{expression} = {result}"));
                        }
                        Asked::MemoryAt(expression) => {
                            let result = body.get("result").and_then(Value::as_str).unwrap_or("");
                            match body.get("memoryReference").and_then(Value::as_str).and_then(dap::number).or_else(|| dap::address_in(result)) {
                                Some(address) => d.read_memory(address),
                                None => d.memory.address = Some(Err(crate::i18n::tf!("{expression} isn't an address: {result}"))),
                            }
                        }
                        Asked::Memory => d.memory.now = dap::memory(&body),
                        Asked::Warnings(kind) => {
                            let read = dap::warnings(body.get("result").and_then(Value::as_str).unwrap_or(""));
                            d.took_warnings(kind, read);
                        }
                        Asked::Profile => {
                            d.profile.making = false;
                            let path = body.get("path").and_then(Value::as_str).unwrap_or("").to_string();
                            d.profile.made = Some(std::fs::read_to_string(&path).map_err(|e| crate::i18n::tf!("The profile {path} couldn't be read: {e}")).and_then(|text| dap::cpu_profile(&text)).map(|profile| {
                                let texts = source_lines(profile.lines.iter().take(PROFILE_LINES).map(|(p, l, _)| (p.clone(), *l)));
                                (profile, texts)
                            }));
                        }
                        Asked::Launch | Asked::Other => {}
                    }
                }
                _ => {}
            }
        }
        if let Some(frame) = show {
            self.show_frame(&frame);
        }
        if let Some(d) = self.debugging.as_mut()
            && d.state != State::Ended
            && let Some(why) = d.adapter_ended()
        {
            d.state = State::Ended;
            if !why.is_empty() {
                d.line(crate::i18n::tf!("The debugger stopped: {why}"));
            }
            self.clear_stopped_marks();
        }
        if self.debugging.as_ref().is_some_and(|d| d.configured && d.state != State::Ended) {
            self.tell_breakpoints();
        }
    }

    /// Carry on, step, pause or stop.
    pub(super) fn debug_control(&mut self, what: &str) {
        let Some(d) = self.debugging.as_mut() else { return };
        match what {
            "continue" | "next" | "stepIn" | "stepOut" => d.step(what),
            "stepBack" | "reverseContinue" if d.can.step_back => d.step(what),
            "pause" if d.state == State::Running => {
                let thread = d.thread;
                d.ask("pause", json!({ "threadId": thread }), Asked::Other);
            }
            "stop" if d.state != State::Ended => {
                d.ask("disconnect", json!({ "terminateDebuggee": true }), Asked::Other);
                d.state = State::Ended;
                d.line(crate::i18n::tr("Stopped.").to_string());
                // A moment for it to end the program, then it's ended. Copperline's adapter closes the emulator
                // it started first, which takes a little longer.
                if d.config.kind == dap::Kind::Copperline {
                    d.session.end_within(Duration::from_secs(3));
                } else {
                    std::thread::sleep(Duration::from_millis(100));
                    d.kill();
                }
                self.clear_stopped_marks();
            }
            _ => {}
        }
    }

    /// Whether a picture should wait for the debugger: until the program stops, and everything asked of the adapter
    /// (a profile, the warnings) has been answered.
    pub(super) fn debug_busy_for_picture(&self) -> bool {
        self.debugging.as_ref().is_some_and(|d| matches!(d.state, State::Starting | State::Running) || !d.asked.is_empty() || d.profile.making || d.copperline.looking)
    }

    /// The screenshot steps for debugging in Copperline: `copperline-debug:EMULATOR;LINE` runs the program in front
    /// in Copperline (its Kickstart from LIMONITER_KICKSTART, if set) with a breakpoint on LINE, the picture waiting
    /// for it to stop there; `debug-memory:WHERE`, `debug-profile:FRAMES`, `debug-warnings:on` and
    /// `copperline-window:analyzer` do what the Debug panel's memory window and buttons do, and `debug-fold:on`
    /// folds the call stack and variables away so what's below is in the picture.
    pub(super) fn copperline_screenshot_step(&mut self, name: &str, value: &str) {
        match name {
            "copperline-debug" => {
                let (emulator, line) = value.split_once(';').unwrap_or((value, "1"));
                let mut settings = crate::amiga_run::settings();
                settings.set_kickstart(settings.model, std::env::var_os("LIMONITER_KICKSTART").map(PathBuf::from));
                crate::amiga_run::set_settings(settings);
                let line = line.parse::<usize>().unwrap_or(1).saturating_sub(1);
                self.docs[self.active].text.code.get_or_insert_with(Box::default).breakpoints.insert(line);
                self.sends.devices = vec![crate::deploy::Device { name: "Copperline".into(), place: crate::deploy::Place::Emulator { emulator: "copperline".into(), program: emulator.into(), options: "--noaudio".into() }, preset: None, eject: false }];
                self.run_on_target(run_host::Target::Device(0));
            }
            _ => {
                let Some(d) = self.debugging.as_mut() else { return };
                match name {
                    "debug-memory" => d.memory.at = value.to_string(),
                    "debug-profile" => {
                        d.profile.frames = value.parse().unwrap_or(1);
                        d.profile.making = true;
                        let frames = d.profile.frames.max(1);
                        d.ask("copperline/profile", json!({ "frames": frames }), Asked::Profile);
                    }
                    "debug-warnings" => d.look_for_warnings(),
                    "debug-fold" => d.copperline.fold = true,
                    _ => d.ask("copperline/ui.show", json!({ "window": value }), Asked::Other),
                }
            }
        }
    }

    /// Whether the program being debugged is paused where its debugger can step back.
    pub(super) fn can_step_back(&self) -> bool {
        self.debugging.as_ref().is_some_and(|d| d.can.step_back && d.state == State::Paused)
    }

    /// The debugging keys, taken first while a session runs.
    pub(super) fn debug_keys(&mut self, ctx: &Context) {
        if self.debugging.as_ref().is_none_or(|d| d.state == State::Ended) {
            return;
        }
        let keys = [
            (Modifiers::NONE, Key::F5, "continue"),
            (Modifiers::SHIFT, Key::F5, "stop"),
            (Modifiers::NONE, Key::F10, "next"),
            (Modifiers::NONE, Key::F11, "stepIn"),
            (Modifiers::SHIFT, Key::F11, "stepOut"),
            (Modifiers::SHIFT, Key::F10, "stepBack"),
        ];
        for (modifiers, key, what) in keys {
            if ctx.input_mut(|i| i.consume_key(modifiers, key)) {
                self.debug_control(what);
            }
        }
    }

    /// The box saying which program is debugged, and how, for a file built into one.
    pub(super) fn debug_setup_window(&mut self, ctx: &Context) {
        let Some(file) = self.debug_setup.as_ref().map(|(path, _)| path.clone()) else { return };
        let kept_where = match self.project.as_ref().filter(|_| self.debug_kept_in_project(&file)) {
            Some(project) => crate::i18n::tf!("Kept with the project {}, for this file.", project.name),
            None => crate::i18n::tr("Kept in Limoniter's settings, for this file.").to_string(),
        };
        let Some((path, config)) = self.debug_setup.as_mut() else { return };
        let (mut go, mut close) = (false, ctx.input(|i| i.key_pressed(Key::Escape)));
        crate::fit::Dialog::new(crate::i18n::tr("Debug")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(crate::i18n::tf!("How {} is debugged, with {}:", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), config.kind.adapter_name()));
            ui.add_space(4.0);
            egui::Grid::new("debug-setup").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                ui.label(crate::i18n::tr("Program"));
                ui.add(egui::TextEdit::singleline(&mut config.program).desired_width(380.0)).on_hover_text(crate::i18n::tr("The program built from the source, with its debugging information (a debug build)"));
                ui.end_row();
                ui.label(crate::i18n::tr("Arguments"));
                ui.add(egui::TextEdit::singleline(&mut config.arguments).desired_width(380.0));
                ui.end_row();
                ui.label(crate::i18n::tr("In the folder"));
                ui.add(egui::TextEdit::singleline(&mut config.folder).desired_width(380.0));
                ui.end_row();
                ui.label(crate::i18n::tr("Debugger"));
                ui.add(egui::TextEdit::singleline(&mut config.adapter).desired_width(380.0)).on_hover_text(crate::i18n::tr("The command that starts the debug adapter"));
                ui.end_row();
            });
            if !config.kind.runs_the_file() && !Path::new(&config.program).exists() {
                ui.label(RichText::new(crate::i18n::tr("There's no program there yet: build it first (Ctrl+Shift+B).")).color(theme::text_faint()).size(12.0));
            }
            ui.label(RichText::new(&kept_where).color(theme::text_faint()).size(12.0));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                go = ui.button(crate::i18n::tr("Debug")).clicked();
                close |= ui.button(crate::i18n::tr("Cancel")).clicked();
            });
        });
        if go {
            let (path, config) = self.debug_setup.take().unwrap();
            self.keep_debug_config(path, config.clone());
            self.launch_debugger(config);
        } else if close {
            self.debug_setup = None;
        }
    }

    /// The Debug panel, at the right.
    pub(super) fn debug_panel(&mut self, ui: &mut Ui) {
        let Some(d) = self.debugging.as_mut() else { return };
        let (mut control, mut close, mut again, mut frame_chosen, mut evaluate) = (None, false, false, None, None);
        let (mut profile, mut go_to): (bool, Option<(PathBuf, usize)>) = (false, None);
        let (mut show_window, mut look) = (None, false);
        let mut toggled: Vec<i64> = Vec::new();
        Panel::right("debug-panel").default_size(360.0).size_range(240.0..=900.0).resizable(true).frame(egui::Frame::new().fill(theme::window()).inner_margin(egui::Margin::symmetric(8, 6))).show(ui, |ui| {
            ui.horizontal(|ui| {
                let name = Path::new(&d.config.program).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                ui.label(RichText::new(crate::i18n::tf!("Debugging {name}")).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = crate::access::called(ui.small_button(crate::i18n::tr("\u{D7}")), "Close").on_hover_text(crate::i18n::tr("Close")).clicked();
                });
            });
            ui.horizontal_wrapped(|ui| {
                let paused = d.state == State::Paused;
                let button = |ui: &mut Ui, on: bool, text: &str, tip: &str| ui.add_enabled(on, egui::Button::new(crate::i18n::tr(text))).on_hover_text(crate::i18n::tr(tip)).clicked();
                if button(ui, paused, "\u{25B6} Carry on", "F5") {
                    control = Some("continue");
                }
                if button(ui, paused, "Step over", "F10") {
                    control = Some("next");
                }
                if button(ui, paused, "Step in", "F11") {
                    control = Some("stepIn");
                }
                if button(ui, paused, "Step out", "Shift+F11") {
                    control = Some("stepOut");
                }
                // Back through what it's done, replayed from the emulator's snapshots.
                if d.can.step_back {
                    if button(ui, paused, "\u{25C0} Step back", "Shift+F10: back to the line before") {
                        control = Some("stepBack");
                    }
                    if button(ui, paused, "Run back", "Runs backwards to the breakpoint before, or to where it can go back no further") {
                        control = Some("reverseContinue");
                    }
                }
                if button(ui, d.state == State::Running, "Pause", "Stops it where it is") {
                    control = Some("pause");
                }
                if d.state == State::Ended {
                    again = button(ui, true, "Debug again", "Ctrl+F5");
                } else if button(ui, true, "\u{25A0} Stop", "Shift+F5") {
                    control = Some("stop");
                }
            });
            let status = match d.state {
                State::Starting => crate::i18n::tr("Starting\u{2026}").to_string(),
                State::Running => crate::i18n::tr("Running").to_string(),
                State::Paused => {
                    let at = d.frames.get(d.chosen).map(|f| format!("{}:{}", f.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), f.line + 1)).unwrap_or_default();
                    crate::i18n::tf!("Paused at {at} ({})", d.reason)
                }
                State::Ended => crate::i18n::tr("Ended").to_string(),
            };
            ui.label(RichText::new(status).color(theme::text_dim()).size(12.5));
            ui.separator();
            let height = ui.available_height();
            // Copperline's session has more above the console (memory, the profile, its warnings), and says less in it.
            let share = if d.config.kind == dap::Kind::Copperline { 0.72 } else { 0.55 };
            egui::ScrollArea::vertical().id_salt("debug-state").max_height(height * share).auto_shrink([false, true]).show(ui, |ui| {
                // A picture of what's below them folds these first (see `copperline_screenshot_step`).
                let fold = std::mem::take(&mut d.copperline.fold).then_some(false);
                egui::CollapsingHeader::new(crate::i18n::tr("Call stack")).default_open(true).open(fold).show(ui, |ui| {
                    for (n, frame) in d.frames.iter().enumerate() {
                        let place = frame.path.as_ref().and_then(|p| p.file_name()).map(|f| format!("  {}:{}", f.to_string_lossy(), frame.line + 1)).unwrap_or_default();
                        if ui.selectable_label(n == d.chosen, RichText::new(format!("{}{place}", frame.name)).monospace().size(12.0)).clicked() {
                            frame_chosen = Some(n);
                        }
                    }
                });
                egui::CollapsingHeader::new(crate::i18n::tr("Variables")).default_open(true).open(fold).show(ui, |ui| {
                    let scopes = d.scopes.clone();
                    for scope in &scopes {
                        variable_rows(ui, d, scope, 0, &mut toggled);
                    }
                });
                if d.can.read_memory && d.memory.address.is_none() {
                    egui::CollapsingHeader::new(crate::i18n::tr("Memory")).id_salt("debug-memory").default_open(true).show(ui, |ui| {
                        ui.label(RichText::new(crate::i18n::tr("It's read when the program stops.")).color(theme::text_faint()).size(11.5));
                    });
                } else if d.can.read_memory {
                    let memory = &mut d.memory;
                    let address = memory.address.clone();
                    let peek = |at: u32| memory.now.as_ref().and_then(|(start, bytes)| u64::from(at).checked_sub(*start).and_then(|i| bytes.get(i as usize)).copied());
                    let before = |at: u32| memory.before.as_ref().and_then(|(start, bytes)| u64::from(at).checked_sub(*start).and_then(|i| bytes.get(i as usize)).copied());
                    let start = |_: &str| match &address {
                        Some(Ok(at)) => Ok(*at as i64),
                        Some(Err(problem)) => Err(problem.clone()),
                        None => Err(String::new()),
                    };
                    let mut at = memory.at.clone();
                    crate::debug_panel::memory_section(ui, &mut at, "A0, $DFF180, a label...", &start, 6, &peek, &before);
                    memory.at = at;
                }
                if d.config.kind == dap::Kind::Copperline {
                    profile_section(ui, d, &mut profile, &mut go_to);
                    warnings_section(ui, d, &mut look, &mut go_to);
                    if dap::has_display() {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(crate::i18n::tr("In Copperline's window:")).color(theme::text_dim()).size(12.0));
                            for (window, label, tip) in [("analyzer", "Frame analyser", "Copperline's frame analyser: who used the chip bus in each slot of a frame, the register writes and the CPU's waits"), ("debugger", "Its debugger", "Copperline's own debugger, in its window"), ("console", "Its console", "Copperline's debugger console, in its window")] {
                                if ui.small_button(crate::i18n::tr(label)).on_hover_text(crate::i18n::tr(tip)).clicked() {
                                    show_window = Some(window);
                                }
                            }
                        });
                    }
                }
            });
            ui.separator();
            ui.label(RichText::new(crate::i18n::tr("Console")).color(theme::text_dim()).size(12.0));
            let field = ui.add(egui::TextEdit::singleline(&mut d.input).hint_text(crate::i18n::tr("An expression to work out, while paused")).desired_width(f32::INFINITY));
            if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !d.input.trim().is_empty() {
                evaluate = Some(std::mem::take(&mut d.input));
                field.request_focus();
            }
            egui::ScrollArea::vertical().id_salt("debug-console").stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                for line in &d.console {
                    ui.add(egui::Label::new(RichText::new(line.as_str()).monospace().size(12.0)).wrap_mode(egui::TextWrapMode::Wrap));
                }
            });
        });
        let d = self.debugging.as_mut().unwrap();
        // Somewhere else typed for the memory window: worked out and read, if it's stopped.
        if d.memory.asked.as_ref() != Some(&d.memory.at) && d.state == State::Paused {
            d.look_at_memory();
        }
        for number in toggled {
            if !d.open.remove(&number) {
                d.open.insert(number);
                if !d.parts.contains_key(&number) {
                    d.ask("variables", json!({ "variablesReference": number }), Asked::Variables(number));
                }
            }
        }
        if let Some(expression) = evaluate {
            let frame = d.frames.get(d.chosen).map(|f| f.id);
            d.line(format!("> {expression}"));
            d.ask("evaluate", json!({ "expression": expression, "frameId": frame, "context": "repl" }), Asked::Evaluate(expression.clone()));
        }
        if let Some(window) = show_window {
            d.ask("copperline/ui.show", json!({ "window": window }), Asked::Other);
        }
        if look {
            d.look_for_warnings();
        }
        if profile && d.state == State::Paused {
            let frames = d.profile.frames.max(1);
            d.profile.making = true;
            d.ask("copperline/profile", json!({ "frames": frames }), Asked::Profile);
        }
        if let Some(n) = frame_chosen
            && let Some(frame) = d.frames.get(n).cloned()
        {
            d.chosen = n;
            d.ask("scopes", json!({ "frameId": frame.id }), Asked::Scopes);
            self.show_frame(&frame);
        }
        if let Some((path, line)) = go_to {
            self.go_to_place(&path.display().to_string(), line + 1);
        }
        if let Some(what) = control {
            self.debug_control(what);
        }
        if again {
            let config = self.debugging.as_ref().unwrap().config.clone();
            self.launch_debugger(config);
        }
        if close {
            self.debug_control("stop");
            self.debugging = None;
            self.pending_focus = true;
        }
    }
}

/// A variable's row, and under it (opened) its parts, each a row of its own.
fn variable_rows(ui: &mut Ui, d: &Debugging, variable: &Variable, depth: usize, toggled: &mut Vec<i64>) {
    let open = variable.parts > 0 && d.open.contains(&variable.parts);
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * 12.0);
        let arrow = if variable.parts == 0 { "  " } else if open { "\u{25BE}" } else { "\u{25B8}" };
        let mut words = format!("{arrow} {}", variable.name);
        if !variable.value.is_empty() {
            words += &format!(" = {}", variable.value);
        }
        let row = ui.add(egui::Label::new(RichText::new(words).monospace().size(12.0)).truncate().sense(egui::Sense::click()));
        let row = if variable.kind.is_empty() { row } else { row.on_hover_text(variable.kind.as_str()) };
        if row.clicked() && variable.parts > 0 {
            toggled.push(variable.parts);
        }
    });
    if open && let Some(parts) = d.parts.get(&variable.parts) {
        // Many short-named values with nothing inside them (a 68000's D0-D7 and A0-A7) go two to a row, as the
        // assembler debuggers show registers, so what's below them is still in sight.
        let (short, rest): (Vec<&Variable>, Vec<&Variable>) = parts.iter().take(500).partition(|p| p.parts == 0 && p.name.chars().count() <= 3);
        let (short, rest) = if short.len() >= 8 { (short, rest) } else { (Vec::new(), parts.iter().take(500).collect()) };
        if !short.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space((depth + 1) as f32 * 12.0 + 12.0);
                egui::Grid::new(("debug-short-values", variable.parts)).num_columns(2).spacing([16.0, 2.0]).show(ui, |ui| {
                    for pair in short.chunks(2) {
                        for part in pair {
                            // Just the value's first word ("0x00C05564" of "0x00C05564 (12604772)"); all of it on
                            // hovering.
                            let first = part.value.split_whitespace().next().unwrap_or("");
                            let words = RichText::new(format!("{} = {first}", part.name)).monospace().size(12.0);
                            let whole = if part.kind.is_empty() { part.value.clone() } else { format!("{} ({})", part.value, part.kind) };
                            ui.label(words).on_hover_text(whole);
                        }
                        ui.end_row();
                    }
                });
            });
        }
        for part in rest {
            variable_rows(ui, d, part, depth + 1, toggled);
        }
    }
}

/// The most of a profile's busiest lines shown.
const PROFILE_LINES: usize = 15;

/// Copperline's profile, in the Debug panel: a button that runs the program on for some frames counting where its
/// time goes (and stops it again), and then the routines and source lines that took most, a line's click going to
/// it.
fn profile_section(ui: &mut Ui, d: &mut Debugging, profile: &mut bool, go_to: &mut Option<(PathBuf, usize)>) {
    egui::CollapsingHeader::new(crate::i18n::tr("Profile")).id_salt("debug-profile").default_open(true).show(ui, |ui| {
        ui.horizontal(|ui| {
            let ready = d.state == State::Paused && !d.profile.making;
            *profile = ui.add_enabled(ready, egui::Button::new(crate::i18n::tr("Profile"))).on_hover_text(crate::i18n::tr("Runs the program on for this many frames, counting where its time goes, and stops it again")).clicked();
            d.profile.frames = d.profile.frames.max(1);
            ui.add(egui::DragValue::new(&mut d.profile.frames).range(1..=50));
            ui.label(crate::i18n::tr("frames"));
            if d.profile.making {
                ui.spinner();
            }
        });
        let mono = |text: String| RichText::new(text).monospace().size(12.0);
        match &d.profile.made {
            None => {}
            Some(Err(problem)) => {
                ui.add(egui::Label::new(RichText::new(problem).color(theme::problem_mark()).size(11.5)).wrap());
            }
            Some(Ok((made, texts))) => {
                let share = |time: u64| if made.total == 0 { 0.0 } else { time as f64 * 100.0 / made.total as f64 };
                egui::Grid::new("debug-profile-routines").num_columns(2).spacing([14.0, 2.0]).striped(true).show(ui, |ui| {
                    ui.label(RichText::new(crate::i18n::tr("Routine")).strong().size(12.0));
                    ui.label(RichText::new(crate::i18n::tr("Share")).strong().size(12.0));
                    ui.end_row();
                    for (name, time) in made.routines.iter().take(10) {
                        ui.label(mono(name.clone()));
                        ui.label(mono(crate::i18n::tf!("{:.1}%", share(*time))));
                        ui.end_row();
                    }
                });
                if made.lines.is_empty() {
                    ui.label(RichText::new(crate::i18n::tr("No time was spent on the program's own source lines.")).color(theme::text_faint()).size(11.5));
                    return;
                }
                ui.add_space(4.0);
                egui::Grid::new("debug-profile-lines").num_columns(3).spacing([14.0, 2.0]).striped(true).show(ui, |ui| {
                    ui.label(RichText::new(crate::i18n::tr("Line")).strong().size(12.0));
                    ui.label(RichText::new(crate::i18n::tr("Share")).strong().size(12.0));
                    ui.label(RichText::new(crate::i18n::tr("Source")).strong().size(12.0));
                    ui.end_row();
                    for (path, line, time) in made.lines.iter().take(PROFILE_LINES) {
                        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        let place = crate::access::clickable(ui.add(egui::Label::new(mono(format!("{name}:{}", line + 1))).sense(egui::Sense::click())));
                        if place.clicked() {
                            *go_to = Some((path.clone(), *line));
                        }
                        ui.label(mono(crate::i18n::tf!("{:.1}%", share(*time))));
                        ui.add(egui::Label::new(mono(texts.get(&(path.clone(), *line)).cloned().unwrap_or_default())).truncate());
                        ui.end_row();
                    }
                });
            }
        }
    });
}

/// Copperline's warnings about the program, looked for when asked (the program paused): each with the source
/// line that did it, a click going there. Those from Kickstart or other programs are only counted.
fn warnings_section(ui: &mut Ui, d: &mut Debugging, look: &mut bool, go_to: &mut Option<(PathBuf, usize)>) {
    let header = match (&d.copperline.chipset, &d.copperline.code) {
        (Some(Ok(chipset)), Some(Ok(code))) => {
            let mine = chipset.iter().chain(code).filter(|w| d.copperline.program.contains(w.by)).count();
            crate::i18n::tf!("Warnings ({mine})")
        }
        _ => crate::i18n::tr("Warnings").to_string(),
    };
    egui::CollapsingHeader::new(header).id_salt("debug-warnings").default_open(true).show(ui, |ui| {
        ui.horizontal(|ui| {
            let ready = d.state == State::Paused && !d.copperline.looking;
            *look = ui.add_enabled(ready, egui::Button::new(crate::i18n::tr("Look for warnings"))).on_hover_text(crate::i18n::tr("What Copperline saw the program do that the chips ignore or get wrong (a byte written to a word register, a read-only register written, a pointer past chip memory, a blit started over another) and code written after it ran")).clicked();
            if d.copperline.looking {
                ui.spinner();
            }
        });
        let program = &d.copperline.program;
        for (title, found) in [(crate::i18n::tr("The custom chips"), &d.copperline.chipset), (crate::i18n::tr("Self-modifying code"), &d.copperline.code)] {
            let Some(found) = found else { continue };
            ui.label(RichText::new(title).strong().size(12.0));
            match found {
                Err(problem) => {
                    ui.add(egui::Label::new(RichText::new(problem).color(theme::problem_mark()).size(11.5)).wrap());
                }
                Ok(found) => {
                    let mine: Vec<&dap::Warning> = found.iter().filter(|w| program.contains(w.by)).collect();
                    if mine.is_empty() {
                        ui.label(RichText::new(crate::i18n::tr("None from the program.")).color(theme::text_faint()).size(11.5));
                    }
                    for warning in mine {
                        let place = program.place(warning.by);
                        ui.horizontal_wrapped(|ui| {
                            if let Some((path, line)) = &place {
                                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                                if crate::access::clickable(ui.add(egui::Label::new(RichText::new(format!("{name}:{}", line + 1)).monospace().size(12.0).color(theme::accent())).sense(egui::Sense::click()))).clicked() {
                                    *go_to = Some((path.clone(), *line));
                                }
                            }
                            ui.add(egui::Label::new(RichText::new(&warning.text).size(11.5)).wrap());
                        });
                    }
                    let others = found.iter().filter(|w| !program.contains(w.by)).count();
                    if others > 0 {
                        ui.label(RichText::new(crate::i18n::tf!("And {others} from Kickstart or other programs.")).color(theme::text_faint()).size(11.5));
                    }
                }
            }
        }
    });
}

/// The text of some source lines (from 0), each file read once.
fn source_lines(wanted: impl Iterator<Item = (PathBuf, usize)>) -> HashMap<(PathBuf, usize), String> {
    let mut files: HashMap<PathBuf, Vec<String>> = HashMap::new();
    let mut texts = HashMap::new();
    for (path, line) in wanted {
        let lines = files.entry(path.clone()).or_insert_with(|| std::fs::read_to_string(&path).map(|t| t.lines().map(|l| l.trim().to_string()).collect()).unwrap_or_default());
        if let Some(text) = lines.get(line) {
            texts.insert((path, line), text.clone());
        }
    }
    texts
}

/// Whether the Python an adapter command runs has debugpy.
fn python_has_debugpy(adapter: &str) -> bool {
    let parts = crate::emulators::split_options(adapter);
    let Some(python) = parts.first() else { return false };
    // py -m ... on Windows: the launcher, asked the same way.
    crate::platform::command(python).args(["-c", "import debugpy"]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::super::tests::{app_with, draw};
    use super::*;

    fn folder(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-debug-kept-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Ctrl+F5 on the file in front, and the Debug box it shows (which, for a C file whose program isn't built,
    /// it always does).
    fn debug_box_for(app: &mut Limoniter, file: &Path) -> Config {
        crate::highlight::load_now();
        assert!(app.open_path(file));
        app.doc().update_syntax();
        assert_eq!(app.doc().language_name(), "C");
        app.start_debugging();
        let (path, config) = app.debug_setup.clone().expect("the Debug box is shown");
        assert_eq!(path, file);
        config
    }

    #[test]
    fn a_projects_debug_settings_are_kept_with_it_and_fill_the_box_next_time() {
        let dir = folder("project");
        let file = dir.join("src").join("life.c");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "int main(void) { return 0; }\n").unwrap();
        let (mut app, ctx) = app_with("");
        app.new_project(dir.join("life"));
        let mut config = debug_box_for(&mut app, &file);
        // The guess is the program a build would make beside it: life.exe on Windows.
        let guess = if cfg!(windows) { "life.exe" } else { "life" };
        assert_eq!(config.program, dir.join("src").join(guess).display().to_string(), "first, a guess");
        draw(&mut app, &ctx, Vec::new());
        config.program = dir.join("build").join("life").display().to_string();
        config.arguments = "--size 40".into();
        app.debug_setup = None;
        app.keep_debug_config(file.clone(), config.clone());
        assert!(app.debug_configs.is_empty(), "not in the settings");
        let saved = std::fs::read_to_string(dir.join("life.limproj")).unwrap();
        assert!(saved.contains("\"src/life.c\"") && saved.contains("\"build/life\"") && saved.contains("--size 40"), "{saved}");
        drop(app);

        // Limoniter started afresh, with the project moved to another folder: the box comes back filled in, with
        // the program found in the new place.
        let moved = folder("project-moved");
        std::fs::remove_dir_all(&moved).unwrap();
        std::fs::rename(&dir, &moved).unwrap();
        let (mut again, ctx) = app_with("");
        again.open_project(&moved.join("life.limproj"));
        let filled = debug_box_for(&mut again, &moved.join("src").join("life.c"));
        assert_eq!(filled.program, moved.join("build").join("life").display().to_string());
        assert_eq!(filled.arguments, "--size 40");
        assert_eq!(filled.adapter, config.adapter);
        draw(&mut again, &ctx, Vec::new());
        drop(again);
        std::fs::remove_dir_all(&moved).unwrap();
    }

    #[test]
    fn a_loose_files_debug_settings_are_kept_in_the_settings() {
        let dir = folder("loose");
        let file = dir.join("tool.c");
        std::fs::write(&file, "int main(void) { return 0; }\n").unwrap();
        let (mut app, ctx) = app_with("");
        let mut config = debug_box_for(&mut app, &file);
        draw(&mut app, &ctx, Vec::new());
        config.arguments = "-v".into();
        config.folder = dir.join("out").display().to_string();
        app.debug_setup = None;
        app.keep_debug_config(file.clone(), config.clone());
        let settings = app.current_settings();
        assert!(settings.contains("debug_settings="), "{settings}");
        drop(app);
        let (mut again, ctx) = app_with(&settings);
        assert_eq!(debug_box_for(&mut again, &file), config);
        draw(&mut again, &ctx, Vec::new());
        // A project open that the file isn't in doesn't take it over.
        again.new_project(folder("other").join("other"));
        assert_eq!(again.kept_debug_config(&file), Some(config));
        assert!(!again.debug_kept_in_project(&file));
        drop(again);
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(folder("other")).unwrap();
    }

    /// A stand-in for js-debug, in Node: it listens on the port it's given, and when the program is launched on the
    /// first connection it asks for a session of its own (startDebugging), which stops at a breakpoint when it's
    /// launched, as js-debug does.
    const FAKE_JS_DEBUG: &str = r#"
const net = require('net');
const port = +process.argv[2];
const frame = (o) => { const b = JSON.stringify(o); return `Content-Length: ${Buffer.byteLength(b)}\r\n\r\n${b}`; };
let connections = 0;
net.createServer((sock) => {
  const id = ++connections;
  let buf = Buffer.alloc(0);
  let seq = 1;
  const send = (o) => sock.write(frame({ seq: seq++, ...o }));
  sock.on('error', () => {});
  sock.on('data', (d) => {
    buf = Buffer.concat([buf, d]);
    for (;;) {
      const i = buf.indexOf('\r\n\r\n');
      if (i < 0) break;
      const len = +/Content-Length: (\d+)/.exec(buf.slice(0, i).toString())[1];
      if (buf.length < i + 4 + len) break;
      const m = JSON.parse(buf.slice(i + 4, i + 4 + len).toString());
      buf = buf.slice(i + 4 + len);
      if (m.type !== 'request') continue;
      const ok = (body = {}) => send({ type: 'response', request_seq: m.seq, success: true, command: m.command, body });
      if (m.command === 'initialize') { ok({ supportsConfigurationDoneRequest: true }); send({ type: 'event', event: 'initialized' }); }
      else if (m.command === 'launch') {
        ok();
        if (id === 1) send({ type: 'request', command: 'startDebugging', arguments: { request: 'launch', configuration: { type: 'pwa-node', name: 'child', __pendingTargetId: 'target-1', program: m.arguments.program } } });
        else send({ type: 'event', event: 'stopped', body: { reason: 'breakpoint', threadId: 1 } });
      }
      else if (m.command === 'stackTrace') ok({ stackFrames: [{ id: 1, name: 'main', line: 2, column: 1, source: { path: process.env.FAKE_SOURCE } }] });
      else if (m.command === 'scopes') ok({ scopes: [] });
      else if (m.command === 'threads') ok({ threads: [{ id: 1, name: 'main' }] });
      else ok();
    }
  });
}).listen(port, '127.0.0.1');
"#;

    /// js-debug asks for a session of its own for the program, on another connection to the same adapter; the
    /// debugger carries on in that one, and what it says is shown.
    #[test]
    fn js_debug_s_own_session_for_the_program_is_followed() {
        if !crate::lsp::on_path("node") {
            eprintln!("Node isn't installed, so the stand-in for js-debug wasn't run.");
            return;
        }
        crate::highlight::load_now();
        let (mut app, ctx) = app_with("");
        let dir = folder("jsdebug");
        let file = dir.join("app.js");
        std::fs::write(&file, "const a = 1;\nconsole.log(a);\n").unwrap();
        let fake = dir.join("fake-js-debug.js");
        std::fs::write(&fake, FAKE_JS_DEBUG.replace("process.env.FAKE_SOURCE", &serde_json::to_string(&file.display().to_string()).unwrap())).unwrap();
        assert!(app.open_path(&file));
        app.doc().update_syntax();
        let (kind, _) = dap::adapter_for("JavaScript").expect("JavaScript has a debugger");
        assert_eq!(kind, dap::Kind::JsDebug);
        let config = Config { kind, adapter: format!("node \"{}\"", fake.display()), program: file.display().to_string(), arguments: String::new(), folder: dir.display().to_string(), emulator: None, env: Vec::new(), stop_on_entry: false };
        app.launch_debugger(config);
        let began = std::time::Instant::now();
        while began.elapsed() < Duration::from_secs(20) && !app.debugging.as_ref().is_some_and(|d| d.state == State::Paused && !d.frames.is_empty()) {
            app.debug_tick(&ctx);
            std::thread::sleep(Duration::from_millis(20));
        }
        let d = app.debugging.as_ref().expect("debugging");
        assert_eq!(d.state, State::Paused, "{:?}", d.console);
        assert_eq!(d.parents.len(), 1, "the first session is kept, as it holds the adapter's process");
        assert_eq!((d.frames[0].name.as_str(), d.frames[0].line), ("main", 1));
        assert_eq!(app.docs[app.active].text.code.as_ref().unwrap().stopped, Some(1), "the line's marked");
        app.debug_control("stop");
        assert_eq!(app.debugging.as_ref().unwrap().state, State::Ended);
        drop(app);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A package.json script is debugged: from the task, or the link beside it, js-debug is asked for with the
    /// script named (here it isn't installed, so it's offered for download with the script kept to run after).
    #[test]
    fn an_npm_script_starts_the_debugger_with_the_script_named() {
        if !crate::lsp::on_path("node") || crate::lsp::on_path("js-debug-dap") || crate::js_debug_get::installed().is_some() {
            return;
        }
        let (mut app, _ctx) = app_with("");
        let dir = folder("npmscript");
        std::fs::write(dir.join("package.json"), "{\n  \"scripts\": {\n    \"dev\": \"node dev.js\"\n  }\n}\n").unwrap();
        std::fs::write(dir.join("package-lock.json"), "{}").unwrap();
        assert!(app.open_path(&dir.join("package.json")));
        app.package_script_links();
        assert_eq!(app.docs[app.active].text_state.script_links, [(2, "dev".to_string())], "the link is above the script's line");
        let tasks = app.tasks_here();
        let at = tasks.iter().position(|t| t.command == "npm run dev").expect("the script is a task");
        app.debug_task(&tasks[at]);
        let config = app.node.pending_debug.clone().expect("asked to wait for js-debug");
        assert_eq!((config.kind, config.program.as_str()), (dap::Kind::JsDebug, "script:dev"));
        assert!(matches!(app.dialog, Dialog::JsDebugMissing));
        // A task that isn't a script says so.
        app.dialog = Dialog::None;
        let install = tasks.iter().find(|t| t.command == "npm install").cloned().unwrap();
        app.debug_task(&install);
        assert!(matches!(app.dialog, Dialog::Message { .. }));
        // Other files have no links.
        let other = dir.join("notes.txt");
        std::fs::write(&other, "x").unwrap();
        assert!(app.open_path(&other));
        app.package_script_links();
        assert!(app.docs[app.active].text_state.script_links.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// With LIMONITER_DEBUGPY naming a Python that has debugpy: a breakpoint in a function stops there, its
    /// variables are read, stepping and carrying on work, and what the program prints comes to the console.
    #[test]
    #[ignore]
    fn a_python_program_is_debugged() {
        let Ok(python) = std::env::var("LIMONITER_DEBUGPY") else {
            return;
        };
        crate::highlight::load_now();
        let (mut app, ctx) = app_with("");
        let folder = std::env::temp_dir().join(format!("limoniter-dap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("tool.py");
        std::fs::write(&file, "def double(n):\n    twice = n * 2\n    return twice\n\nprint('result', double(4))\n").unwrap();
        assert!(app.open_path(&file));
        app.doc().update_syntax();
        app.doc().text.code.get_or_insert_with(Box::default).breakpoints.insert(2);
        let config = Config { kind: dap::Kind::Debugpy, adapter: format!("{python} -m debugpy.adapter"), program: file.display().to_string(), arguments: String::new(), folder: folder.display().to_string(), emulator: None, env: Vec::new(), stop_on_entry: false };
        app.launch_debugger(config);
        let wait = |app: &mut Limoniter, done: &dyn Fn(&Limoniter) -> bool| {
            let began = std::time::Instant::now();
            while began.elapsed() < Duration::from_secs(30) && !done(app) {
                app.debug_tick(&ctx);
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(done(app), "{:?}", app.debugging.as_ref().map(|d| (d.state, d.console.clone())));
        };
        // Stopped at the breakpoint, line 3 (return twice), in double, with n and twice to see.
        wait(&mut app, &|app| app.debugging.as_ref().is_some_and(|d| d.state == State::Paused && d.parts.values().any(|v| v.iter().any(|v| v.name == "twice"))));
        let d = app.debugging.as_ref().unwrap();
        assert_eq!((d.frames[0].name.as_str(), d.frames[0].line), ("double", 2));
        let locals: Vec<(String, String)> = d.parts.values().flatten().map(|v| (v.name.clone(), v.value.clone())).collect();
        assert!(locals.contains(&("n".into(), "4".into())) && locals.contains(&("twice".into(), "8".into())), "{locals:?}");
        assert_eq!(app.docs[app.active].text.code.as_ref().unwrap().stopped, Some(2), "the line's marked");
        // An expression, worked out in the frame.
        let frame = app.debugging.as_ref().unwrap().frames[0].id;
        app.debugging.as_mut().unwrap().ask("evaluate", json!({ "expression": "n + 1", "frameId": frame, "context": "repl" }), Asked::Evaluate("n + 1".into()));
        wait(&mut app, &|app| app.debugging.as_ref().is_some_and(|d| d.console.iter().any(|l| l.contains("n + 1 = 5"))));
        // Out of the function, then on to the end.
        app.debug_control("stepOut");
        wait(&mut app, &|app| app.debugging.as_ref().is_some_and(|d| d.state == State::Paused && d.frames.first().is_some_and(|f| f.line == 4)));
        app.debug_control("continue");
        wait(&mut app, &|app| app.debugging.as_ref().is_some_and(|d| d.state == State::Ended));
        let console = app.debugging.as_ref().unwrap().console.join("\n");
        assert!(console.contains("result 8"), "{console}");
        assert_eq!(app.docs[app.active].text.code.as_ref().unwrap().stopped, None);
        let _ = std::fs::remove_dir_all(folder);
    }

    /// A stand-in for copperline-ctl --dap, answering as Copperline 1.1.0 was seen to: it can step back and read
    /// memory, and stops at address errors and illegal instructions unless told otherwise. It stops at line 4 of
    /// `file` when configured, and at line 3 when stepped back. Everything asked of it is kept in `heard`.
    fn fake_copperline(file: PathBuf) -> (Session, std::sync::Arc<std::sync::Mutex<Vec<Value>>>) {
        let (from_us, to_it) = std::io::pipe().unwrap();
        let (from_it, to_us) = std::io::pipe().unwrap();
        let heard: std::sync::Arc<std::sync::Mutex<Vec<Value>>> = Default::default();
        let kept = heard.clone();
        std::thread::spawn(move || {
            let mut out = to_us;
            let mut seq = 0;
            let mut say = |message: Value| {
                use std::io::Write;
                seq += 1;
                let mut message = message;
                message["seq"] = json!(seq);
                let body = message.to_string();
                let _ = out.write_all(format!("Content-Length: {}\r\n\r\n{body}", body.len()).as_bytes());
            };
            let (send, asked) = std::sync::mpsc::channel();
            std::thread::spawn(move || crate::lsp::read_messages(from_us, &send));
            let mut line = 4;
            for request in asked {
                kept.lock().unwrap().push(request.clone());
                let command = request["command"].as_str().unwrap_or("").to_string();
                let answer = |body: Value| json!({ "type": "response", "request_seq": request["seq"], "command": command, "success": true, "body": body });
                match command.as_str() {
                    "initialize" => say(answer(json!({ "supportsStepBack": true, "supportsReadMemoryRequest": true, "exceptionBreakpointFilters": [{ "filter": "bus-error", "label": "Bus error", "default": false }, { "filter": "address-error", "label": "Address error", "default": true }, { "filter": "illegal", "label": "Illegal instruction", "default": true }] }))),
                    "launch" => {
                        say(answer(json!({})));
                        say(json!({ "type": "event", "event": "initialized" }));
                        say(json!({ "type": "event", "event": "output", "body": { "category": "console", "output": "count loaded at $C0F4D0\n" } }));
                    }
                    "setBreakpoints" => say(answer(json!({ "breakpoints": [{ "verified": false }] }))),
                    "configurationDone" => {
                        say(answer(json!({})));
                        say(json!({ "type": "event", "event": "stopped", "body": { "reason": "breakpoint", "threadId": 1, "allThreadsStopped": true } }));
                    }
                    "stackTrace" => say(answer(json!({ "stackFrames": [{ "id": 1000, "name": "loop", "line": line, "column": 0, "source": { "path": file } }], "totalFrames": 1 }))),
                    "scopes" => say(answer(json!({ "scopes": [{ "name": "Registers", "presentationHint": "registers", "variablesReference": 1 }, { "name": "Chipset", "variablesReference": 4, "expensive": true }] }))),
                    "variables" => say(answer(json!({ "variables": [{ "name": "d0", "value": format!("0x{line:08X} ({line})"), "type": "long", "variablesReference": 0 }, { "name": "a0", "value": "0x00000400 (1024)", "type": "long", "variablesReference": 0 }] }))),
                    // Copperline's reports, as raw control requests: a byte written to COLOR00 by line 4's
                    // instruction (at $C0F4D4), and one of Kickstart's.
                    "evaluate" if request["arguments"]["expression"] == "!chipset.report" => say(answer(json!({ "result": json!({ "armed": true, "findings": [{ "addr": 0xC0F4D4u32, "count": 3, "detail": "COLOR00 accessed a byte at a time" }, { "addr": 0xFC6D6Cu32, "count": 118, "detail": "COP2LCH aimed at 0xC00000" }] }).to_string(), "variablesReference": 0 }))),
                    "evaluate" if request["arguments"]["expression"] == "!smc.report" => say(answer(json!({ "result": json!({ "armed": true, "writes": [] }).to_string(), "variablesReference": 0 }))),
                    "evaluate" => say(answer(json!({ "result": "0x00000400 (1024)", "memoryReference": "0x400", "variablesReference": 0 }))),
                    "readMemory" => {
                        let bytes: Vec<u8> = (0..128).map(|i| (i as u8).wrapping_add(line as u8)).collect();
                        say(answer(json!({ "address": request["arguments"]["memoryReference"], "data": crate::conversions::base64_encode(&bytes, false) })));
                    }
                    "stepBack" => {
                        line = 3;
                        say(answer(json!({})));
                        say(json!({ "type": "event", "event": "stopped", "body": { "reason": "step", "threadId": 1 } }));
                    }
                    "disconnect" => {
                        say(answer(json!({})));
                        return;
                    }
                    _ => say(answer(json!({}))),
                }
            }
        });
        (Session::over(Box::new(to_it), from_it), heard)
    }

    /// Debugging in Copperline, against the stand-in: its exceptions are left on, the breakpoint beside the source
    /// is told, the line it stops at is marked with its registers shown and memory read (at A0, where nothing's
    /// typed), and stepping back goes back a line, with the memory that changed known.
    #[test]
    fn an_amiga_program_is_debugged_through_copperlines_adapter() {
        crate::highlight::load_now();
        let (mut app, ctx) = app_with("");
        let folder = std::env::temp_dir().join(format!("limoniter-dap-fake-copperline-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("count.s");
        std::fs::write(&file, "\tsection code,code\nstart:\tmoveq\t#0,d0\n\tmoveq\t#5,d1\nloop:\taddq.l\t#1,d0\n\tsubq.l\t#1,d1\n\tbne.s\tloop\n\trts\n").unwrap();
        assert!(app.open_path(&file));
        app.doc().text.code.get_or_insert_with(Box::default).breakpoints.insert(3);
        let (session, heard) = fake_copperline(file.clone());
        let config = Config { kind: dap::Kind::Copperline, adapter: String::new(), program: folder.join("count").display().to_string(), arguments: String::new(), folder: folder.display().to_string(), emulator: Some(dap::Emulated { program: "copperline".into(), options: Vec::new() }), env: Vec::new(), stop_on_entry: false };
        app.begin_debugging(config, session);
        let text = std::fs::read_to_string(&file).unwrap();
        let (assembly, _) = crate::asm::hunk::assemble_executable(&file, &text, &|_| Err("no".into()), false, &crate::asm::Options::default());
        app.debugging.as_mut().unwrap().copperline.program = dap::ProgramMap::of(&assembly);
        let wait = |app: &mut Limoniter, done: &dyn Fn(&Limoniter) -> bool| {
            let began = std::time::Instant::now();
            while began.elapsed() < Duration::from_secs(10) && !done(app) {
                app.debug_tick(&ctx);
                ctx.run_ui(Default::default(), |ui| app.debug_panel(ui)).textures_delta.clear();
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(done(app), "{:?}", app.debugging.as_ref().map(|d| (d.state, d.console.clone())));
        };
        wait(&mut app, &|app| app.debugging.as_ref().is_some_and(|d| d.state == State::Paused && d.memory.now.is_some() && d.parts.contains_key(&1)));
        assert_eq!(app.doc().text.code.as_ref().unwrap().stopped, Some(3), "line 4 is marked");
        assert!(app.can_step_back());
        let d = app.debugging.as_ref().unwrap();
        assert_eq!(d.parts[&1][0], Variable { name: "d0".into(), value: "0x00000004 (4)".into(), kind: "long".into(), parts: 0 }, "the registers are the first scope, opened");
        assert_eq!(d.memory.now.as_ref().map(|(at, bytes)| (*at, bytes[0])), Some((0x400, 4)), "read at A0");
        {
            let heard = heard.lock().unwrap();
            let asked = |command: &str| heard.iter().find(|r| r["command"] == command).cloned().unwrap_or(Value::Null);
            assert_eq!(asked("setExceptionBreakpoints")["arguments"]["filters"], json!(["address-error", "illegal"]));
            assert_eq!(asked("setBreakpoints")["arguments"]["source"]["path"], json!(file.display().to_string()));
            assert_eq!(asked("setBreakpoints")["arguments"]["breakpoints"], json!([{ "line": 4 }]));
            assert_eq!(asked("launch")["arguments"]["copperline"], "copperline");
            assert_eq!(asked("initialize")["arguments"]["adapterID"], "copperline");
            assert_eq!(asked("evaluate")["arguments"]["expression"], "a0");
        }
        // Step back, as Shift+F10 or the menu's Step back asks.
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| app.run(Command::DebugStepBack, ui));
        wait(&mut app, &|app| app.debugging.as_ref().is_some_and(|d| d.state == State::Paused && d.memory.before.is_some() && d.memory.now.as_ref().is_some_and(|(_, b)| b[0] == 3)));
        assert_eq!(app.doc().text.code.as_ref().unwrap().stopped, Some(2), "back to line 3");
        assert!(heard.lock().unwrap().iter().any(|r| r["command"] == "stepBack" && r["arguments"]["threadId"] == 1));
        // Copperline's warnings: the program's own put at its line; Kickstart's only counted. The stop is read
        // again afterwards, the adapter having forgotten its variables.
        let traces = |heard: &[Value]| heard.iter().filter(|r| r["command"] == "stackTrace").count();
        let before = traces(&heard.lock().unwrap());
        app.debugging.as_mut().unwrap().look_for_warnings();
        wait(&mut app, &|app| app.debugging.as_ref().is_some_and(|d| d.copperline.code.is_some() && d.asked.is_empty()));
        let d = app.debugging.as_ref().unwrap();
        let chipset = d.copperline.chipset.clone().unwrap().unwrap();
        assert_eq!(chipset.len(), 2);
        assert_eq!(d.copperline.program.place(chipset[0].by), Some((file.clone(), 3)), "line 4");
        assert!(!d.copperline.program.contains(chipset[1].by), "Kickstart's");
        assert_eq!(traces(&heard.lock().unwrap()), before + 1);
        app.debug_control("stop");
        let told = || heard.lock().unwrap().iter().any(|r| r["command"] == "disconnect" && r["arguments"]["terminateDebuggee"] == true);
        let began = std::time::Instant::now();
        while !told() && began.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(told(), "told to end, and to close the emulator");
        assert_eq!(app.doc().text.code.as_ref().unwrap().stopped, None);
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// With LIMONITER_COPPERLINE naming Copperline (its AppImage will do) and copperline-ctl beside it, on its
    /// path or in its tools folder beside it, and LIMONITER_KICKSTART a Kickstart 1.3 (else its own AROS is used):
    /// Run on machine (F9) assembles the program with its source lines, Copperline boots it and stops at the
    /// breakpoint beside line 5, with D0 read; a step goes to line 6, stepping back returns to line 5, and memory
    /// is read. Run without a display (no DISPLAY), Copperline has no window.
    #[test]
    #[ignore]
    fn an_amiga_program_is_debugged_in_a_real_copperline() {
        let Some(emulator) = std::env::var_os("LIMONITER_COPPERLINE") else { return };
        crate::highlight::load_now();
        let (mut app, ctx) = app_with("");
        let folder = std::env::temp_dir().join(format!("limoniter-real-copperline-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("count.s");
        std::fs::write(&file, "\tsection code,code\nstart:\tmove.l\t4.w,a6\n\tmoveq\t#0,d0\n\tmoveq\t#5,d1\nloop:\taddq.l\t#1,d0\n\tsubq.l\t#1,d1\n\tbne.s\tloop\n\trts\n").unwrap();
        let mut settings = crate::amiga_run::Settings::default();
        settings.set_kickstart(crate::amiga_run::Model::A500, std::env::var_os("LIMONITER_KICKSTART").map(PathBuf::from));
        crate::amiga_run::set_settings(settings);
        assert!(app.open_path(&file));
        app.doc().update_syntax();
        assert_eq!(app.doc().language_name(), "Assembly 680x0");
        app.doc().text.code.get_or_insert_with(Box::default).breakpoints.insert(4);
        app.sends.devices = vec![crate::deploy::Device { name: "Copperline".into(), place: crate::deploy::Place::Emulator { emulator: "copperline".into(), program: emulator.into(), options: "--noaudio".into() }, preset: None, eject: false }];
        app.runnable().unwrap();
        app.run_on_target(run_host::Target::Device(0));
        if let Dialog::Message { body, .. } = &app.dialog {
            panic!("{body}");
        }
        assert!(matches!(app.dialog, Dialog::None));
        let wait = |app: &mut Limoniter, seconds: u64, done: &dyn Fn(&Limoniter) -> bool| {
            let began = std::time::Instant::now();
            while began.elapsed() < Duration::from_secs(seconds) && !done(app) {
                app.debug_tick(&ctx);
                ctx.run_ui(Default::default(), |ui| app.debug_panel(ui)).textures_delta.clear();
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(done(app), "{:?}", app.debugging.as_ref().map(|d| (d.state, d.console.clone(), d.frames.clone())));
        };
        // Stopped there, with everything asked at the stop (the registers among it) answered.
        let stopped_at = |app: &Limoniter, line: usize| app.debugging.as_ref().is_some_and(|d| d.state == State::Paused && d.frames.first().is_some_and(|f| f.line == line) && d.asked.is_empty() && d.parts.contains_key(&d.scopes.first().map_or(-1, |s| s.parts)));
        wait(&mut app, 180, &|app| stopped_at(app, 4));
        let d0 = |app: &Limoniter| app.debugging.as_ref().and_then(|d| d.parts.values().flatten().find(|v| v.name.eq_ignore_ascii_case("d0")).map(|v| v.value.clone())).unwrap_or_default();
        println!("stopped at line 5, d0 = {}", d0(&app));
        assert!(d0(&app).starts_with("0x00000000"), "{}", d0(&app));
        assert_eq!(app.doc().text.code.as_ref().unwrap().stopped, Some(4));
        assert!(app.can_step_back(), "Copperline says it can step back");
        app.debug_control("next");
        wait(&mut app, 30, &|app| stopped_at(app, 5));
        println!("stepped to line 6, d0 = {}", d0(&app));
        assert!(d0(&app).starts_with("0x00000001"), "{}", d0(&app));
        app.debug_control("stepBack");
        wait(&mut app, 30, &|app| stopped_at(app, 4));
        println!("stepped back to line 5, d0 = {}", d0(&app));
        assert!(d0(&app).starts_with("0x00000000"), "{}", d0(&app));
        app.debugging.as_mut().unwrap().memory.at = "$0".into();
        wait(&mut app, 30, &|app| app.debugging.as_ref().is_some_and(|d| d.memory.now.as_ref().is_some_and(|(at, b)| *at == 0 && b.len() == 128)));
        println!("memory at 0: {:02X?}", &app.debugging.as_ref().unwrap().memory.now.as_ref().unwrap().1[..8]);
        println!("{}", app.debugging.as_ref().unwrap().console.join("\n"));
        app.debug_control("stop");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_console_joins_what_comes_in_pieces() {
        let (reader, writer) = std::io::pipe().unwrap();
        let session = Session::over(Box::new(writer), reader);
        let mut d = Debugging { session, parents: Vec::new(), child_launch: None, config: dap::guess(dap::Kind::Debugpy, String::new(), Path::new("/w/a.py")), asked: HashMap::new(), state: State::Running, reason: String::new(), thread: 0, frames: Vec::new(), chosen: 0, scopes: Vec::new(), parts: HashMap::new(), open: HashSet::new(), console: Vec::new(), input: String::new(), told: HashMap::new(), configured: false, line_open: false, can: Default::default(), memory: Default::default(), profile: Default::default(), copperline: Default::default() };
        d.say("res");
        d.say("ult 8\nnext ");
        d.say("line\n");
        d.line("done".into());
        assert_eq!(d.console, ["result 8", "next line", "done"]);
    }
}
