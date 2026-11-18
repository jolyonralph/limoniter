//! The window's side of debugging an Amiga program in an emulator: the emulator started with its GDB server on,
//! told to stop the program as it starts, and the debugger's panel beside the source, where the other emulators'
//! is (see `amiga_debug` for the debugger, and `debug_panel::stopped_68k` for the panel's insides).

use super::*;
use crate::amiga_debug::{AmigaAsk, Program};
use crate::remote_debug::Ask;

/// Why an emulator can't debug Amiga programs, and what can.
const NO_SERVER: &str = "so the program runs without stopping at its breakpoints.\n\nTo debug Amiga programs, choose the FS-UAE that comes with uae-dap (for Linux, macOS and Windows), or WinUAE with Bartman's GDB server (from vscode-amiga-debug or uae-dap), in Devices and emulators.";

impl Limoniter {
    /// The program to debug, if the Amiga program run from this document can be debugged in this emulator: 68000
    /// source assembled here into hunks, run on an FS-UAE or WinUAE with a GDB server. If it can't be only because
    /// the emulator has no GDB server, and the source has breakpoints, says so.
    pub(super) fn amiga_debuggable(&mut self, doc_id: u64, emulator: &Emulator, program: &Path, run: &Runnable) -> Option<Program> {
        if run.format != crate::running::Format::Hunk || !matches!(emulator.id, "fs-uae" | "winuae") {
            return None;
        }
        let assembly = self.running.assembled.as_ref().filter(|c| c.doc_id == doc_id && c.runnable.is_ok()).map(|c| c.assembly.clone())?;
        if assembly.sections.is_empty() {
            return None;
        }
        if crate::amiga_debug::has_gdb_server(&on_path(program)) {
            return Some(Program::of(&assembly));
        }
        let breakpoints = self.docs.iter().find(|d| d.id == doc_id).and_then(|d| d.text.code.as_ref()).is_some_and(|c| !c.breakpoints.is_empty());
        if breakpoints {
            self.message("Run on machine", crate::i18n::tf!("This {} has no GDB server, {NO_SERVER}", emulator.name));
        }
        None
    }

    /// The addresses of a document's breakpoints in the Amiga program assembled from it, as assembled.
    pub(super) fn amiga_breakpoints(&self, doc_id: u64) -> Vec<i64> {
        match (self.running.assembled.as_ref().filter(|c| c.doc_id == doc_id), self.docs.iter().find(|d| d.id == doc_id).and_then(|d| d.text.code.as_ref())) {
            (Some(cache), Some(code)) => code.breakpoints.iter().filter_map(|line| cache.assembly.address_of(0, *line)).collect(),
            _ => Vec::new(),
        }
    }

    /// Starts the emulator with its GDB server on, told to stop the program as it starts, and the debugger, which
    /// waits for it. `config` is the configuration made for the run, beside the Amiga's drive.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_amiga_debugging(&mut self, device: &Device, emulator: &'static Emulator, run: &Runnable, config: &Path, args: Vec<String>, program: &Path, debug: Program) -> Result<(), String> {
        let doc_id = self.docs[self.active].id;
        let folder = config.parent().ok_or(crate::i18n::tr("The run has no folder."))?.to_path_buf();
        // The program's name as the startup-sequence runs it: the emulator stops as that starts.
        let trigger = std::fs::read_to_string(folder.join("dh0").join("s").join("startup-sequence")).ok().and_then(|text| text.lines().next().map(|l| l.trim().to_string())).filter(|t| !t.is_empty()).ok_or(crate::i18n::tr("The Amiga's drive has no program to start."))?;
        let port = if emulator.id == "winuae" {
            let port = crate::amiga_debug::WINUAE_PORT;
            if std::net::TcpListener::bind(("127.0.0.1", port)).is_err() {
                return Err(crate::i18n::tf!("WinUAE's GDB server always listens on port {port}, and something else is using it: another WinUAE being debugged, perhaps."));
            }
            port
        } else {
            free_port().ok_or(crate::i18n::tr("No port was free for the emulator's debugger."))?
        };
        let mut text = std::fs::read_to_string(config).map_err(|e| e.to_string())?;
        text.push_str(&crate::amiga_debug::config_lines(emulator.id, port, &trigger));
        std::fs::write(config, text).map_err(|e| crate::i18n::tf!("The emulator's configuration couldn't be written.\n\n{e}"))?;
        let log = folder.join("emulator.log");
        let out = std::fs::File::create(&log).map_err(|e| e.to_string())?;
        let err = out.try_clone().map_err(|e| e.to_string())?;
        let mut command = std::process::Command::new(program);
        command.args(&args).stdin(std::process::Stdio::null()).stdout(out).stderr(err);
        if let Some(home) = program.parent().filter(|p| !p.as_os_str().is_empty()) {
            command.current_dir(home);
            // uae-dap's FS-UAE for Linux keeps its libraries beside it.
            if cfg!(target_os = "linux") && home.join("libSDL2-2.0.so.0").is_file() {
                command.env("LD_LIBRARY_PATH", home);
            }
        }
        let child = command.spawn().map_err(|e| crate::i18n::tf!("Couldn't start {} ({}).\n\n{e}", emulator.name, program.display()))?;
        let wake = self.waker.clone();
        let remote = crate::remote_debug::RemoteDebug::start_amiga(port, debug, self.amiga_breakpoints(doc_id), move || {
            if let Some(ctx) = &wake {
                ctx.request_repaint();
            }
        });
        let debug = Some(Box::new(Debugging { doc_id, remote, breakpoints: Vec::new(), watches: Vec::new(), memory_before: None, memory_now: None, z80: false }));
        self.running.sessions.push(Session { target: device.name.clone(), machine: run.machine, file: run.name.clone(), kind: SessionKind::App { child: Some(child), emulator, port: Some(port), log, debug }, started: Instant::now() });
        Ok(())
    }

    /// While an Amiga program from the document in front is being debugged: where it's stopped, its registers and
    /// the rest, and the buttons, in the debugger's panel beside the source.
    pub(super) fn amiga_debug_panel(&mut self, ui: &mut Ui, doc_id: u64) {
        let assembly = self.running.assembled.as_ref().filter(|c| c.doc_id == doc_id).map(|c| c.assembly.clone());
        let breakpoints = self.amiga_breakpoints(doc_id);
        let doc_index = self.docs.iter().position(|d| d.id == doc_id);
        let cursor = doc_index.and_then(|i| assembly.as_ref().and_then(|a| a.address_of(0, self.docs[i].text.caret_line)));
        let mut code = doc_index.and_then(|i| self.docs[i].text.code.take());
        let line_texts: std::collections::BTreeMap<usize, String> = match (doc_index, &code) {
            (Some(i), Some(code)) => code.breakpoints.iter().filter(|l| **l < self.docs[i].text.line_count()).map(|l| (*l, self.docs[i].text.line(*l).to_string())).collect(),
            _ => Default::default(),
        };
        let found = self.running.sessions.iter_mut().find_map(|s| match &mut s.kind {
            SessionKind::App { debug: Some(d), .. } if d.doc_id == doc_id => Some((s.target.clone(), d)),
            _ => None,
        });
        let Some((target, debugging)) = found else {
            if let (Some(i), Some(code)) = (doc_index, code) {
                self.docs[i].text.code = Some(code);
            }
            return;
        };
        let remote = &mut debugging.remote;
        remote.check();
        let mut shown = remote.amiga.take().unwrap_or_default();
        let mut asks: Vec<Ask> = Vec::new();
        // A new stop: the one before is kept, for what's changed to stand out, and a breakpoint's condition is
        // tested, carrying straight on if it isn't true.
        if let Some(now) = remote.stopped.as_ref().and_then(|s| s.amiga.as_deref()).cloned() {
            let new = shown.now.as_ref().map(|n| n.serial) != Some(now.serial);
            if new {
                shown.before = shown.now.take();
                if let (Some(code), Some(assembly), Some(source)) = (&code, &assembly, now.source)
                    && let Some((0, line)) = assembly.line_at(source)
                    && code.breakpoints.contains(&line)
                    && let Some(condition) = code.conditions.get(&line)
                {
                    let labels = crate::amiga_debug::relocate_symbols(&assembly.symbols, &shown.program, &now.bases);
                    let names = |name: &str| crate::amiga_debug::register_value(name, &now.registers);
                    let peek = |a: u32| now.peek(a).unwrap_or(0);
                    let wide = crate::debug_panel::Wide { names: &names, peek: &peek };
                    let reason = match crate::debug_panel::eval_wide(condition, &wide, Some(&labels)) {
                        Ok(0) => {
                            asks.push(Ask::CarryOn);
                            None
                        }
                        Ok(_) => Some(crate::i18n::tf!("Stopped because {condition}")),
                        Err(problem) => Some(crate::i18n::tf!("The condition {condition} couldn't be worked out: {problem}")),
                    };
                    if let Some(stop) = remote.stopped.as_mut() {
                        stop.reason = reason.or(stop.reason.take());
                    }
                }
            }
            shown.now = Some(now);
        }
        if shown.breakpoints.as_ref() != Some(&breakpoints) {
            shown.breakpoints = Some(breakpoints.clone());
            asks.push(Ask::Amiga(AmigaAsk::Breakpoints(breakpoints)));
        }
        // The addresses whose writing stops it, once it's known where the program is.
        if let (Some(code), Some(now)) = (&code, &shown.now) {
            let labels = assembly.as_ref().map(|a| crate::amiga_debug::relocate_symbols(&a.symbols, &shown.program, &now.bases));
            let watches = write_watches(code, labels.as_ref());
            if shown.watches.as_ref() != Some(&watches) {
                shown.watches = Some(watches.clone());
                asks.push(Ask::Amiga(AmigaAsk::Watches(watches)));
            }
        }
        let stopped = remote.stopped.clone();
        let problem = remote.problem.clone();
        let waiting = shown.now.is_none();
        let mut pause = false;
        Panel::right("remote-debug").default_size(400.0).resizable(true).frame(egui::Frame::new().fill(theme::field()).inner_margin(egui::Margin::symmetric(8, 6))).show(ui, |ui| {
            ui.label(RichText::new(crate::i18n::tf!("Debugging in {target}")).color(theme::text_dim()).size(12.5));
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                if let Some(problem) = &problem {
                    ui.add(egui::Label::new(RichText::new(problem).color(theme::problem_mark())).wrap());
                }
                match &stopped {
                    Some(stop) => {
                        let mut scratch = crate::text_buffer::CodeMarks::default();
                        let marks: &mut crate::text_buffer::CodeMarks = match code.as_mut() {
                            Some(code) => code,
                            None => &mut scratch,
                        };
                        asks.extend(crate::debug_panel::stopped_68k(ui, stop, &mut shown, assembly.as_deref(), marks, cursor));
                    }
                    None if problem.is_some() => {}
                    None if waiting => {
                        ui.add(egui::Label::new(RichText::new(crate::i18n::tr("Waiting for the Amiga to start the program: it stops at its first breakpoint, or on its first line if there's one there.")).color(theme::text_dim())).wrap());
                    }
                    None => {
                        ui.horizontal(|ui| {
                            ui.label(crate::i18n::tr("Running"));
                            pause = ui.small_button(crate::i18n::tr("Pause")).on_hover_text(crate::i18n::tr("Stops it where it is")).clicked();
                        });
                        ui.add(egui::Label::new(RichText::new(crate::i18n::tr("Click beside a line's number for a breakpoint; it stops there.")).color(theme::text_faint()).size(11.5)).wrap());
                    }
                }
                if let (Some(code), true) = (code.as_mut(), assembly.is_some()) {
                    crate::debug_panel::breakpoints(ui, code, &|line| line_texts.get(&line).cloned().unwrap_or_default(), true, true);
                }
            });
        });
        if pause {
            asks.push(Ask::Pause);
        }
        for ask in asks {
            remote.ask(ask);
        }
        let stopped_line = stopped.as_ref().and_then(|s| s.amiga.as_ref()).and_then(|a| a.source).and_then(|source| assembly.as_ref().and_then(|a| a.line_at(source))).filter(|(file, _)| *file == 0).map(|(_, line)| line);
        remote.amiga = Some(shown);
        if let (Some(i), Some(code)) = (doc_index, code) {
            self.docs[i].text.code = Some(code);
        }
        if let Some(doc) = self.docs.iter_mut().find(|d| d.id == doc_id)
            && let Some(code) = &mut doc.text.code
            && code.stopped != stopped_line
        {
            code.stopped = stopped_line;
            if let Some(line) = stopped_line {
                doc.text.move_caret(line, 0, false);
            }
        }
    }
}

/// The memory whose writing stops it, from what's written in the list: an address or label, or a range as
/// label..label+n.
fn write_watches(code: &crate::text_buffer::CodeMarks, labels: Option<&std::collections::BTreeMap<String, i64>>) -> Vec<(u32, u32)> {
    let names = |_: &str| None;
    let peek = |_: u32| 0;
    let wide = crate::debug_panel::Wide { names: &names, peek: &peek };
    let value = |text: &str| crate::debug_panel::eval_wide(text, &wide, labels).ok();
    code.write_watches
        .iter()
        .filter_map(|watch| match watch.split_once("..") {
            Some((from, to)) => {
                let (from, to) = (value(from)?, value(to)?);
                Some((from as u32, (to - from + 1).clamp(1, 0x1000) as u32))
            }
            None => Some((value(watch)? as u32, 1)),
        })
        .collect()
}

/// A program named without a folder, found where the system looks for programs.
fn on_path(program: &Path) -> PathBuf {
    if program.components().count() > 1 || program.is_file() {
        return program.to_path_buf();
    }
    std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|dir| dir.join(program)).find(|p| p.is_file())).unwrap_or_else(|| program.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amiga_debug::tests as fake;
    use crate::remote_debug::RemoteDebug;

    /// The test program in a document, assembled.
    fn app_with_program() -> (Limoniter, Context) {
        crate::highlight::load_now();
        let (mut app, ctx) = crate::app::tests::app_with("");
        app.new_document();
        app.doc().untitled = "t.s".into();
        app.doc().text.set_content(fake::SOURCE.lines().map(str::to_string).collect());
        app.doc().update_syntax();
        let run = app.runnable().unwrap().unwrap();
        assert_eq!(run.format, crate::running::Format::Hunk);
        (app, ctx)
    }

    fn marks(app: &mut Limoniter) -> &mut crate::text_buffer::CodeMarks {
        app.doc().text.code.get_or_insert_with(Default::default)
    }

    /// The panel, against a made-up GDB server: breakpoints from the gutter go to it, the line it stops at is
    /// marked, watches have their memory read, and a breakpoint whose condition isn't true is carried on from.
    #[test]
    fn the_panel_follows_an_amiga_program() {
        let (mut app, ctx) = app_with_program();
        let doc_id = app.doc().id;
        marks(&mut app).breakpoints.insert(2);
        let assembly = app.running.assembled.as_ref().unwrap().assembly.clone();
        let (port, heard) = fake::fake_server(&assembly);
        let remote = RemoteDebug::start_amiga(port, Program::of(&assembly), app.amiga_breakpoints(doc_id), || {});
        let debug = Some(Box::new(Debugging { doc_id, remote, breakpoints: Vec::new(), watches: Vec::new(), memory_before: None, memory_now: None, z80: false }));
        let emulator = emulators::by_id("fs-uae").unwrap();
        app.running.sessions.push(Session { target: "FS-UAE".into(), machine: Machine::Amiga, file: "t.exe".into(), kind: SessionKind::App { child: None, emulator, port: Some(port), log: PathBuf::new(), debug }, started: Instant::now() });
        let until = |app: &mut Limoniter, line: usize| {
            let began = Instant::now();
            while began.elapsed() < Duration::from_secs(10) && marks(app).stopped != Some(line) {
                ctx.run_ui(Default::default(), |ui| app.remote_debug_panel(ui)).textures_delta.clear();
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(marks(app).stopped, Some(line));
        };
        until(&mut app, 2);
        assert_eq!(app.doc().text.caret_line, 2, "the cursor goes to the line");
        // A watch on memory that hasn't been read; a breakpoint on line 7 only when D7 is 99, which it never is;
        // and one on line 9.
        marks(&mut app).watches = vec!["long[value]".into(), "d0".into()];
        marks(&mut app).breakpoints.insert(6);
        marks(&mut app).conditions.insert(6, "d7 == 99".into());
        marks(&mut app).breakpoints.insert(8);
        for _ in 0..20 {
            ctx.run_ui(Default::default(), |ui| app.remote_debug_panel(ui)).textures_delta.clear();
            std::thread::sleep(Duration::from_millis(10));
        }
        match &mut app.running.sessions[0].kind {
            SessionKind::App { debug: Some(d), .. } => d.remote.ask(Ask::CarryOn),
            _ => panic!("debugging"),
        }
        until(&mut app, 8);
        let heard = heard.lock().unwrap().clone();
        assert!(heard.contains(&format!("Z0,{:x},2", fake::line_address(&assembly, 6))), "{heard:?}");
        assert!(heard.iter().any(|p| p.starts_with(&format!("m{:x},", fake::BASES[1] & !0x7F))), "the watch's memory is read: {heard:?}");
        app.running.sessions.clear();
    }

    /// With LIMONITER_AMIGA_GDB naming uae-dap's FS-UAE (with a screen, as xvfb-run gives): Run on machine boots
    /// the program on FS-UAE's own AROS and stops at the breakpoint set beside its line, which is marked; a step
    /// goes to the next line.
    #[test]
    #[ignore]
    fn an_amiga_program_stops_at_its_breakpoint_in_a_real_fs_uae() {
        let Some(program) = std::env::var_os("LIMONITER_AMIGA_GDB") else { return };
        crate::highlight::load_now();
        let (mut app, ctx) = crate::app::tests::app_with("");
        app.sends.devices = vec![Device { name: "FS-UAE".into(), place: Place::Emulator { emulator: "fs-uae".into(), program: program.into(), options: String::new() }, preset: None, eject: false }];
        app.new_document();
        app.doc().untitled = "hello.s".into();
        app.doc().text.set_content(include_str!("asm/testdata/amiga_hello.s").lines().map(str::to_string).collect());
        app.doc().update_syntax();
        app.runnable().unwrap();
        marks(&mut app).breakpoints.insert(6);
        app.run_on_target(Target::Device(0));
        assert!(matches!(app.dialog, Dialog::None));
        let until = |app: &mut Limoniter, line: usize, wait: u64| {
            let began = Instant::now();
            while began.elapsed() < Duration::from_secs(wait) && marks(app).stopped != Some(line) {
                ctx.run_ui(Default::default(), |ui| app.remote_debug_panel(ui)).textures_delta.clear();
                std::thread::sleep(Duration::from_millis(50));
            }
        };
        until(&mut app, 6, 120);
        let problem = match &app.running.sessions.first().map(|s| &s.kind) {
            Some(SessionKind::App { debug: Some(d), .. }) => d.remote.problem.clone(),
            _ => Some("the session ended".into()),
        };
        assert_eq!(marks(&mut app).stopped, Some(6), "{problem:?}");
        match &mut app.running.sessions[0].kind {
            SessionKind::App { debug: Some(d), .. } => d.remote.ask(Ask::Step),
            _ => panic!("debugging"),
        }
        until(&mut app, 7, 20);
        assert_eq!(marks(&mut app).stopped, Some(7));
        app.stop_all_sessions();
    }

    /// Run on machine with an FS-UAE that has a GDB server: its configuration turns the server on, told the
    /// program's name, and the debugger waits for it (a stand-in program, with the server's option written in
    /// it). Without a server, with a breakpoint set, it runs, and says why it won't stop.
    #[cfg(unix)]
    #[test]
    fn running_on_an_emulator_with_a_gdb_server_debugs() {
        let (mut app, _ctx) = app_with_program();
        let folder = std::env::temp_dir().join(format!("limoniter-amiga-debug-host-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let program = folder.join("fs-uae");
        std::fs::write(&program, "#!/bin/sh\n# remote_debugger_trigger\nsleep 3\n").unwrap();
        std::process::Command::new("chmod").arg("+x").arg(&program).status().unwrap();
        app.sends.devices = vec![Device { name: "FS-UAE".into(), place: Place::Emulator { emulator: "fs-uae".into(), program: program.clone(), options: String::new() }, preset: None, eject: false }];
        app.run_on_target(Target::Device(0));
        assert!(matches!(app.dialog, Dialog::None));
        let SessionKind::App { debug: Some(debug), log, port: Some(port), .. } = &app.running.sessions[0].kind else { panic!("debugging") };
        assert!(debug.remote.amiga.is_some());
        let config = std::fs::read_to_string(log.parent().unwrap().join("amiga.fs-uae")).unwrap();
        assert!(config.contains(&format!("remote_debugger_port = {port}\nremote_debugger_trigger = t\n")), "{config}");
        app.stop_all_sessions();
        std::fs::write(&program, "#!/bin/sh\nsleep 3\n").unwrap();
        marks(&mut app).breakpoints.insert(2);
        app.run_on_target(Target::Device(0));
        let Dialog::Message { body, .. } = &app.dialog else { panic!("it says why") };
        assert!(body.contains("no GDB server"), "{body}");
        assert!(matches!(&app.running.sessions[0].kind, SessionKind::App { debug: None, .. }));
        app.stop_all_sessions();
        let _ = std::fs::remove_dir_all(&folder);
    }
}
