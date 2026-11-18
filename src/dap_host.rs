//! Debugging a program with a language's own debugger, through the Debug Adapter Protocol (see dap.rs): breakpoints
//! set beside the line numbers, the line it's stopped on marked as the assembler debuggers mark it, and a Debug
//! panel with the controls, the call stack, the variables (opened a level at a time) and a console that shows what
//! the program writes and works out expressions typed into it. While a session runs, F5 carries on, Shift+F5
//! stops, F10 steps over, F11 steps in and Shift+F11 steps out, as in VS Code.

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
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum State {
    Starting,
    Running,
    Paused,
    Ended,
}

pub(super) struct Debugging {
    session: Session,
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
}

impl Debugging {
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
            return self.message("Debug", crate::i18n::tr("Python, C, C++ and Rust files can be debugged here, with debugpy, gdb or lldb-dap.").into());
        };
        if self.docs[self.active].is_modified() && !self.save(self.active) {
            return;
        }
        match self.debug_configs.get(&path).cloned() {
            Some(config) => self.launch_debugger(config),
            // A script is run as it is; for a compiled program, which one is asked first.
            None if kind.runs_the_file() => self.launch_debugger(dap::guess(kind, adapter, &path)),
            None => self.debug_setup = Some((path.clone(), dap::guess(kind, adapter, &path))),
        }
    }

    /// Starts the adapter and the program in it.
    pub(super) fn launch_debugger(&mut self, config: Config) {
        if let Some(mut old) = self.debugging.take() {
            old.session.stop();
        }
        self.clear_stopped_marks();
        if config.kind == dap::Kind::Debugpy && !python_has_debugpy(&config.adapter) {
            let install = if cfg!(windows) { "py -m pip install debugpy" } else { "python3 -m pip install --user debugpy" };
            return self.message("Debug", crate::i18n::tf!("Python is debugged with debugpy, which isn't installed. In a terminal (Ctrl+`):\n\n{install}"));
        }
        if config.kind != dap::Kind::Debugpy && !crate::lsp::on_path(crate::emulators::split_options(&config.adapter).first().map_or("", String::as_str)) {
            return self.message("Debug", crate::i18n::tr("C, C++ and Rust are debugged with gdb (version 14 or later) or lldb-dap, and neither is installed.").into());
        }
        let session = match Session::start(&config.adapter, Path::new(&config.folder)) {
            Ok(session) => session,
            Err(problem) => return self.message("Debug", problem),
        };
        let mut debugging = Debugging { session, config, asked: HashMap::new(), state: State::Starting, reason: String::new(), thread: 0, frames: Vec::new(), chosen: 0, scopes: Vec::new(), parts: HashMap::new(), open: HashSet::new(), console: Vec::new(), input: String::new(), told: HashMap::new(), configured: false, line_open: false };
        let adapter_id = debugging.config.kind.adapter_name();
        debugging.ask("initialize", json!({ "clientID": "limoniter", "clientName": "Limoniter", "adapterID": adapter_id, "linesStartAt1": true, "columnsStartAt1": true, "pathFormat": "path", "supportsVariableType": true, "supportsRunInTerminalRequest": false }), Asked::Initialize);
        let program = Path::new(&debugging.config.program).file_name().map_or_else(|| debugging.config.program.clone(), |n| n.to_string_lossy().into_owned());
        debugging.line(crate::i18n::tf!("Debugging {program} with {adapter_id}"));
        self.debugging = Some(Box::new(debugging));
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
        let mut show: Option<Frame> = None;
        for message in said {
            let Some(d) = self.debugging.as_mut() else { return };
            match message.get("type").and_then(Value::as_str) {
                Some("request") => d.session.decline(&message),
                Some("event") => {
                    let body = message.get("body").cloned().unwrap_or(Value::Null);
                    match message.get("event").and_then(Value::as_str).unwrap_or("") {
                        "initialized" => {
                            d.configured = true;
                            self.tell_breakpoints();
                            if let Some(d) = self.debugging.as_mut() {
                                d.ask("setExceptionBreakpoints", json!({ "filters": [] }), Asked::Other);
                                d.ask("configurationDone", json!({}), Asked::Other);
                            }
                        }
                        "stopped" => {
                            d.state = State::Paused;
                            d.reason = body.get("reason").and_then(Value::as_str).unwrap_or("").to_string();
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
                                d.say(&text);
                            }
                        }
                        "exited" => {
                            let code = body.get("exitCode").and_then(Value::as_i64).unwrap_or(0);
                            d.line(crate::i18n::tf!("The program ended, with code {code}."));
                        }
                        "terminated" => {
                            d.state = State::Ended;
                            d.session.stop();
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
                            Asked::Launch => {
                                d.line(crate::i18n::tf!("It couldn't be started: {} {}", why, detail));
                                d.state = State::Ended;
                                d.session.stop();
                            }
                            _ if !why.is_empty() && why != "cancelled" => d.line(why),
                            _ => {}
                        }
                        continue;
                    }
                    match asked {
                        Asked::Initialize => {
                            let launch = dap::launch_arguments(&d.config);
                            let asked = Asked::Launch;
                            d.ask("launch", launch, asked);
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
            && let Some(why) = d.session.ended()
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
            "pause" if d.state == State::Running => {
                let thread = d.thread;
                d.ask("pause", json!({ "threadId": thread }), Asked::Other);
            }
            "stop" if d.state != State::Ended => {
                d.ask("disconnect", json!({ "terminateDebuggee": true }), Asked::Other);
                d.state = State::Ended;
                d.line(crate::i18n::tr("Stopped.").to_string());
                // A moment for it to end the program, then it's ended.
                std::thread::sleep(Duration::from_millis(100));
                d.session.stop();
                self.clear_stopped_marks();
            }
            _ => {}
        }
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
        ];
        for (modifiers, key, what) in keys {
            if ctx.input_mut(|i| i.consume_key(modifiers, key)) {
                self.debug_control(what);
            }
        }
    }

    /// The box saying which program is debugged, and how, for a file built into one.
    pub(super) fn debug_setup_window(&mut self, ctx: &Context) {
        let Some((path, config)) = self.debug_setup.as_mut() else { return };
        let (mut go, mut close) = (false, ctx.input(|i| i.key_pressed(Key::Escape)));
        egui::Window::new(crate::i18n::tr("Debug")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
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
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                go = ui.button(crate::i18n::tr("Debug")).clicked();
                close |= ui.button(crate::i18n::tr("Cancel")).clicked();
            });
        });
        if go {
            let (path, config) = self.debug_setup.take().unwrap();
            self.debug_configs.insert(path, config.clone());
            self.launch_debugger(config);
        } else if close {
            self.debug_setup = None;
        }
    }

    /// The Debug panel, at the right.
    pub(super) fn debug_panel(&mut self, ui: &mut Ui) {
        let Some(d) = self.debugging.as_mut() else { return };
        let (mut control, mut close, mut again, mut frame_chosen, mut evaluate) = (None, false, false, None, None);
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
            egui::ScrollArea::vertical().id_salt("debug-state").max_height(height * 0.55).auto_shrink([false, true]).show(ui, |ui| {
                egui::CollapsingHeader::new(crate::i18n::tr("Call stack")).default_open(true).show(ui, |ui| {
                    for (n, frame) in d.frames.iter().enumerate() {
                        let place = frame.path.as_ref().and_then(|p| p.file_name()).map(|f| format!("  {}:{}", f.to_string_lossy(), frame.line + 1)).unwrap_or_default();
                        if ui.selectable_label(n == d.chosen, RichText::new(format!("{}{place}", frame.name)).monospace().size(12.0)).clicked() {
                            frame_chosen = Some(n);
                        }
                    }
                });
                egui::CollapsingHeader::new(crate::i18n::tr("Variables")).default_open(true).show(ui, |ui| {
                    let scopes = d.scopes.clone();
                    for scope in &scopes {
                        variable_rows(ui, d, scope, 0, &mut toggled);
                    }
                });
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
        if let Some(n) = frame_chosen
            && let Some(frame) = d.frames.get(n).cloned()
        {
            d.chosen = n;
            d.ask("scopes", json!({ "frameId": frame.id }), Asked::Scopes);
            self.show_frame(&frame);
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
        for part in parts.iter().take(500) {
            variable_rows(ui, d, part, depth + 1, toggled);
        }
    }
}

/// Whether the Python an adapter command runs has debugpy.
fn python_has_debugpy(adapter: &str) -> bool {
    let parts = crate::emulators::split_options(adapter);
    let Some(python) = parts.first() else { return false };
    // py -m ... on Windows: the launcher, asked the same way.
    std::process::Command::new(python).args(["-c", "import debugpy"]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

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
        let config = Config { kind: dap::Kind::Debugpy, adapter: format!("{python} -m debugpy.adapter"), program: file.display().to_string(), arguments: String::new(), folder: folder.display().to_string() };
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

    #[test]
    fn the_console_joins_what_comes_in_pieces() {
        let (reader, writer) = std::io::pipe().unwrap();
        let session = Session::over(Box::new(writer), reader);
        let mut d = Debugging { session, config: dap::guess(dap::Kind::Debugpy, String::new(), Path::new("/w/a.py")), asked: HashMap::new(), state: State::Running, reason: String::new(), thread: 0, frames: Vec::new(), chosen: 0, scopes: Vec::new(), parts: HashMap::new(), open: HashSet::new(), console: Vec::new(), input: String::new(), told: HashMap::new(), configured: false, line_open: false };
        d.say("res");
        d.say("ult 8\nnext ");
        d.say("line\n");
        d.line("done".into());
        assert_eq!(d.console, ["result 8", "next line", "done"]);
    }
}
