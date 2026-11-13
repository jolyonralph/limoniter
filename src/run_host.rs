//! The window's side of running programs: Run on machine (F9) and Run on; the box that asks where, or which
//! machine a listing is for; and the sessions running, each in the status bar with Stop. See RUNNING.md.

use super::*;
use crate::deploy::{Device, Payload, Place};
use crate::emulators::{self, Emulator, Link};
use crate::running::{Machine, Runnable};
use std::process::Child;
use std::time::Instant;

/// Somewhere a program can run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    /// The Spectrum built into Limoniter, in the preview pane.
    BuiltIn,
    /// A device or emulator, by its place in the list.
    Device(usize),
}

pub(super) const BUILT_IN_NAME: &str = "Built-in Spectrum";
/// Where the status bar's Stop button leaves word that it was pressed.
pub(super) const STOP_ASKED: &str = "stop-running-asked";

/// A program running somewhere Limoniter can follow it.
pub(super) struct Session {
    pub target: String,
    pub machine: Machine,
    pub file: String,
    pub kind: SessionKind,
    pub started: Instant,
}

pub(super) enum SessionKind {
    /// An emulator started as a program of its own, reachable on `port` if it can be reached at all.
    App { child: Child, emulator: &'static Emulator, port: Option<u16>, log: PathBuf, debug: Option<Box<Debugging>> },
    /// The built-in Spectrum, drawn in the preview pane of the document it was run from.
    BuiltIn { machine: Box<crate::zx_machine::BuiltIn>, doc_id: u64, run: Box<Runnable>, texture: Option<egui::TextureHandle>, error: Option<String> },
}

/// A program from assembler source being debugged in an emulator: the document, the debugger, and the
/// breakpoints it was last given.
pub(super) struct Debugging {
    pub doc_id: u64,
    pub remote: crate::remote_debug::RemoteDebug,
    pub breakpoints: Vec<u16>,
    /// The addresses whose writing stops it, as last sent.
    pub watches: Vec<u16>,
    /// Memory as it was at the stop before this one, and at this one.
    pub memory_before: Option<std::sync::Arc<Vec<u8>>>,
    pub memory_now: Option<std::sync::Arc<Vec<u8>>>,
    /// Whether it's a Z80, whose registers conditions can name.
    pub z80: bool,
}

/// Everything about running, kept by the window.
#[derive(Default)]
pub(super) struct Running {
    pub sessions: Vec<Session>,
    /// For each kind of machine, by its id, the name of where its programs run.
    pub defaults: Vec<(String, String)>,
    number: u64,
    /// Something to say in the status bar, and since when.
    pub note: Option<(String, Instant)>,
    /// A run waiting for the add-ons' on_run functions: the document, and where it's to run.
    pending: Option<(u64, Target)>,
    /// For each processor, by its id, the assembler its source is assembled with.
    pub assemblers: Vec<(String, String)>,
    /// The last program assembled.
    pub assembled: Option<super::asm_host::AssembledCache>,
}

impl Running {
    pub(super) fn from_settings(settings: &str) -> Self {
        let defaults = list_setting(settings, "run_on").iter().filter_map(|value| value.split_once('\u{1}').map(|(machine, target)| (machine.to_string(), target.to_string()))).collect();
        let assemblers = list_setting(settings, "assembler").iter().filter_map(|value| value.split_once('\u{1}').map(|(cpu, choice)| (cpu.to_string(), choice.to_string()))).collect();
        Running { defaults, assemblers, ..Running::default() }
    }

    pub(super) fn to_settings(&self) -> String {
        let runs: String = self.defaults.iter().map(|(machine, target)| format!("run_on={machine}\u{1}{target}\n")).collect();
        let assemblers: String = self.assemblers.iter().map(|(cpu, choice)| format!("assembler={cpu}\u{1}{choice}\n")).collect();
        runs + assemblers.as_str()
    }

    fn default_for(&self, machine: Machine) -> Option<&str> {
        self.defaults.iter().find(|(id, _)| id == machine.id()).map(|(_, target)| target.as_str())
    }

    fn set_default(&mut self, machine: Machine, target: &str) {
        self.defaults.retain(|(id, _)| id != machine.id());
        self.defaults.push((machine.id().to_string(), target.to_string()));
    }
}

/// The box asking where to run something, or which machine's BASIC a text is.
pub(super) enum RunBox {
    Where { runnable: Box<Runnable>, remember: bool },
    Which { doc_id: u64 },
}

/// The machines a plain text can be run as the BASIC of.
const LISTING_MACHINES: [(basic::Machine, &str); 4] = [
    (basic::Machine::Spectrum, "ZX Spectrum 48K BASIC"),
    (basic::Machine::Next, "ZX Spectrum Next BASIC (NextBASIC)"),
    (basic::Machine::Commodore(basic::CbmMachine::C64), "Commodore 64 BASIC"),
    (basic::Machine::Bbc, "BBC BASIC"),
];

impl Limoniter {
    /// What the file in front runs as, if anything: a listing tokenised, a file as it is, or the file selected in
    /// an image. None, with nothing said, if it's text that might be a listing for a machine not yet chosen.
    pub(super) fn runnable(&mut self) -> Result<Option<Runnable>, String> {
        self.runnable_reporting(true)
    }

    /// The same, but for a menu: source isn't assembled by another program, and its problems aren't shown.
    pub(super) fn runnable_quietly(&mut self) -> Result<Option<Runnable>, String> {
        self.runnable_reporting(false)
    }

    fn runnable_reporting(&mut self, report: bool) -> Result<Option<Runnable>, String> {
        if let Some(cpu) = self.doc_cpu(self.active) {
            return self.assemble_to_run(cpu, report).map(Some);
        }
        let doc = &self.docs[self.active];
        if let Some(browser) = &doc.browser {
            return match browser.selected_file() {
                Some(Ok((name, bytes))) => Runnable::from_file(&name, bytes).map(Some).ok_or_else(|| format!("{name} isn't a program or disk Limoniter knows how to run.")),
                Some(Err(problem)) => Err(problem),
                None => {
                    // The whole image, if it's one a machine mounts.
                    let name = doc.name();
                    let bytes = doc.path.as_deref().map(std::fs::read).transpose().map_err(|e| e.to_string())?.unwrap_or_default();
                    Runnable::from_file(&name, bytes).map(Some).ok_or_else(|| "Select the program to run in the list first.".to_string())
                }
            };
        }
        if let Some((machine, stem)) = doc.listing_of.clone().filter(|_| !doc.hex_mode) {
            return Runnable::from_listing(&doc.text.all_lines(), machine, &stem).map(Some).map_err(|problem| format!("This listing couldn't be tokenised, so it can't be run.\n\n{problem}"));
        }
        if !doc.hex_mode && doc.text.is_disk_backed() {
            return Err("This file is too large to run.".into());
        }
        let name = doc.name();
        let bytes = self.document_bytes();
        if let Some(runnable) = Runnable::from_file(&name, bytes) {
            return Ok(Some(runnable));
        }
        let doc = &self.docs[self.active];
        if doc.hex_mode || doc.picture.is_some() {
            return Err(format!("{name} isn't a program or disk Limoniter knows how to run."));
        }
        Ok(None)
    }

    /// The name a target goes by.
    fn target_name(&self, target: Target) -> String {
        match target {
            Target::BuiltIn => BUILT_IN_NAME.to_string(),
            Target::Device(n) => self.sends.devices.get(n).map_or_else(String::new, |device| device.name.clone()),
        }
    }

    fn target_named(&self, name: &str) -> Option<Target> {
        if name == BUILT_IN_NAME {
            return Some(Target::BuiltIn);
        }
        self.sends.devices.iter().position(|device| device.name == name).map(Target::Device)
    }

    /// Whether a target can take this; if not, why not.
    pub(super) fn suits(&self, target: Target, run: &Runnable) -> Result<(), String> {
        match target {
            Target::BuiltIn => crate::zx_machine::BuiltIn::takes(run).map_err(|why| format!("The built-in Spectrum {why}")),
            Target::Device(n) => {
                let Some(device) = self.sends.devices.get(n) else {
                    return Err("It's no longer set up.".into());
                };
                match &device.place {
                    Place::Emulator { emulator, .. } => match emulators::by_id(emulator) {
                        Some(known) if known.runs(run.machine, run.format) => Ok(()),
                        Some(known) if known.machines.contains(&run.machine) => Err(format!("{} doesn't take .{} files.", known.name, run.format.ending())),
                        Some(known) => Err(format!("{} doesn't run {} programs.", known.name, run.machine.name())),
                        None => Err("It isn't an emulator Limoniter knows.".into()),
                    },
                    Place::NextSync { .. } if run.machine == Machine::Next || run.machine.is_classic_spectrum() => Ok(()),
                    Place::NextSync { .. } => Err(format!("A Spectrum Next doesn't run {} programs.", run.machine.name())),
                    Place::Ultimate { .. } if run.machine == Machine::C64 && crate::deploy::ultimate_route(&run.name).is_some() => Ok(()),
                    Place::Ultimate { .. } => Err(format!("An Ultimate doesn't run {}.", run.name)),
                    _ => Ok(()),
                }
            }
        }
    }

    /// Every target, with whether it can take this.
    pub(super) fn targets_for(&self, run: &Runnable) -> Vec<(Target, String, Result<(), String>)> {
        let mut all = vec![(Target::BuiltIn, BUILT_IN_NAME.to_string(), self.suits(Target::BuiltIn, run))];
        all.extend((0..self.sends.devices.len()).map(|n| (Target::Device(n), self.sends.devices[n].name.clone(), self.suits(Target::Device(n), run))));
        all
    }

    /// Run on machine (F9): runs the file in front where its kind of program runs, asking the first time.
    pub(super) fn run_on_machine(&mut self) {
        let runnable = match self.runnable() {
            Ok(Some(runnable)) => runnable,
            Ok(None) => {
                self.dialog = Dialog::Run(Box::new(RunBox::Which { doc_id: self.docs[self.active].id }));
                return;
            }
            Err(problem) if problem.is_empty() => return,
            Err(problem) => return self.message("Run on machine", problem),
        };
        let chosen = self.running.default_for(runnable.machine).and_then(|name| self.target_named(name)).filter(|target| self.suits(*target, &runnable).is_ok());
        match chosen {
            Some(target) => self.begin_run(target),
            None => self.dialog = Dialog::Run(Box::new(RunBox::Where { runnable: Box::new(runnable), remember: true })),
        }
    }

    /// Starts a run, letting the add-ons' on_run functions have their turn first if any are on.
    fn begin_run(&mut self, target: Target) {
        let doc = &self.docs[self.active];
        let (id, text_view) = (doc.id, !doc.hex_mode && doc.browser.is_none());
        if self.script.is_none() && text_view && self.queue_hooks(crate::addons::Event::Run, id) {
            self.running.pending = Some((id, target));
            if let Some(last) = self.addon_queue.back_mut() {
                last.after = Some(super::script_host::After::Run(id));
            }
            return;
        }
        self.running.pending = Some((id, target));
        self.finish_run(id);
    }

    /// Runs the document where it was asked to, made from it as the add-ons left it.
    pub(super) fn finish_run(&mut self, doc_id: u64) {
        let Some((_, target)) = self.running.pending.take_if(|(id, _)| *id == doc_id) else {
            return;
        };
        let Some(index) = self.docs.iter().position(|doc| doc.id == doc_id) else {
            return;
        };
        self.active = index;
        match self.runnable() {
            Ok(Some(runnable)) => match self.suits(target, &runnable) {
                Ok(()) => self.run_on(target, runnable),
                Err(why) => self.message("Run on", why),
            },
            Ok(None) => {}
            Err(problem) if problem.is_empty() => {}
            Err(problem) => self.message("Run on", problem),
        }
    }

    /// Run on, with the place chosen.
    pub(super) fn run_on_target(&mut self, target: Target) {
        match self.runnable() {
            Ok(Some(runnable)) => match self.suits(target, &runnable) {
                Ok(()) => self.begin_run(target),
                Err(why) => self.message("Run on", why),
            },
            Ok(None) => self.dialog = Dialog::Run(Box::new(RunBox::Which { doc_id: self.docs[self.active].id })),
            Err(problem) if problem.is_empty() => {}
            Err(problem) => self.message("Run on", problem),
        }
    }

    pub(super) fn run_on(&mut self, target: Target, run: Runnable) {
        let name = self.target_name(target);
        // An emulator, the built-in Spectrum and an Ultimate run it; anywhere else it's sent, to be started there.
        let mut runs = true;
        let outcome = match target {
            Target::BuiltIn => self.run_built_in(run.clone()),
            Target::Device(n) => {
                let device = self.sends.devices[n].clone();
                match &device.place {
                    Place::Emulator { .. } => self.run_in_emulator(&device, &run),
                    place => {
                        runs = matches!(place, Place::Ultimate { .. });
                        self.deliver(device, &run);
                        Ok(())
                    }
                }
            }
        };
        match outcome {
            Ok(()) if runs => self.running.note = Some((format!("Running {} on {name}", run.name), Instant::now())),
            Ok(()) => self.running.note = Some((format!("Sent {} to {name}: start it there", run.name), Instant::now())),
            Err(problem) => self.message("Run on machine", problem),
        }
    }

    /// Sends it to a real machine, or a card or folder, as Send to device would.
    fn deliver(&mut self, device: Device, run: &Runnable) {
        let payload = Payload::Bytes(run.bytes.clone());
        if let Place::NextSync { folder } = &device.place {
            self.offer_to_next(folder, &run.name, payload);
            return;
        }
        let doc_id = self.docs[self.active].id;
        self.start_send(doc_id, device, run.name.clone(), payload, true);
    }

    /// Starts an emulator with the file, or, if one it can reach is already running it, loads it there.
    fn run_in_emulator(&mut self, device: &Device, run: &Runnable) -> Result<(), String> {
        let Place::Emulator { emulator, program, options } = &device.place else {
            return Err("It isn't an emulator.".into());
        };
        let emulator = emulators::by_id(emulator).ok_or("It isn't an emulator Limoniter knows.")?;
        // Assembler source run on ZEsarUX is debugged there: its breakpoints set at their lines' addresses.
        let doc_id = self.docs[self.active].id;
        let kind = match emulator.link {
            Link::Zrcp if matches!(run.machine, Machine::Spectrum48 | Machine::Spectrum128 | Machine::Next) => Some(crate::remote_debug::Emulator::Zesarux),
            Link::ViceMonitor if run.format == crate::running::Format::Prg => Some(crate::remote_debug::Emulator::Vice),
            Link::Gdb if run.machine == Machine::CoCo => Some(crate::remote_debug::Emulator::Xroar),
            _ => None,
        };
        // Hatari is debugged through its own debugger, on its console.
        let hatari = emulator.id == "hatari" && run.format == crate::running::Format::Tos;
        let debugging = (kind.is_some() || hatari) && self.running.assembled.as_ref().is_some_and(|c| c.doc_id == doc_id && c.runnable.is_ok());
        let mut run = run.clone();
        if debugging
            && run.format == crate::running::Format::Tap
            && let Some(start) = self.running.assembled.as_ref().and_then(|c| c.assembly.start())
        {
            // A tape won't load while ZEsarUX is being debugged, so it's loaded here and handed over as a snapshot.
            let snapshot = crate::zx_machine::BuiltIn::snapshot_at(&run, start as u16)?;
            run = Runnable { format: crate::running::Format::Sna, name: run.name.replace(".tap", ".sna"), bytes: std::sync::Arc::new(snapshot), ..run };
        }
        let run = &run;
        self.running.number += 1;
        let folder = std::env::temp_dir().join(format!("limoniter-run-{}", std::process::id())).join(self.running.number.to_string());
        std::fs::create_dir_all(&folder).map_err(|e| format!("Couldn't make the copy to run.\n\n{e}"))?;
        let file = folder.join(&run.name);
        std::fs::write(&file, run.bytes.as_slice()).map_err(|e| format!("Couldn't make the copy to run.\n\n{e}"))?;
        // An emulator already running, that can be reached, loads it where it is.
        if let Some(index) = self.running.sessions.iter().position(|s| s.target == device.name)
            && !debugging
            && let SessionKind::App { port: Some(port), emulator: running, debug: None, .. } = &self.running.sessions[index].kind
            && running.link == Link::Zrcp
            && crate::machine_link::zrcp_load(*port, &file).is_ok()
        {
            let session = &mut self.running.sessions[index];
            session.machine = run.machine;
            session.file = run.name.clone();
            return Ok(());
        }
        self.stop_session(&device.name);
        let port = (emulator.link != Link::None).then(free_port).flatten();
        let program = emulator.program_for(program, run.machine);
        let mut args = emulator.arguments(run.machine, run.format, &file, port, options);
        // Being debugged, ZEsarUX and VICE start empty, and load the program once its breakpoints are set. XRoar
        // stops as it's connected to, before it's loaded the program; Hatari stops as the program starts.
        if debugging && matches!(kind, Some(crate::remote_debug::Emulator::Zesarux | crate::remote_debug::Emulator::Vice)) {
            let named = file.display().to_string();
            args.retain(|arg| *arg != named && arg != "-autostart");
        }
        if debugging && hatari {
            let start = folder.join("start.ini");
            // TEXT is the program running; the desktop's is in ROM.
            std::fs::write(&start, "b pc = TEXT && TEXT < $e00000 :once\n").map_err(|e| e.to_string())?;
            let at = args.len().saturating_sub(1);
            args.splice(at..at, ["--parse".to_string(), start.display().to_string()]);
        }
        let log = folder.join("emulator.log");
        let out = std::fs::File::create(&log).map_err(|e| e.to_string())?;
        let err = out.try_clone().map_err(|e| e.to_string())?;
        let mut command = std::process::Command::new(&program);
        if debugging && hatari {
            command.args(&args).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
            drop(err);
        } else {
            command.args(&args).stdin(std::process::Stdio::null()).stdout(out).stderr(err);
        }
        if let Some(home) = program.parent() {
            command.current_dir(home);
        }
        let mut child = command.spawn().map_err(|e| format!("Couldn't start {} ({}).\n\n{e}", emulator.name, program.display()))?;
        if debugging && hatari && let (Some(input), Some(output), Some(errors)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) {
            let breakpoints = self.breakpoint_addresses(doc_id);
            let wake = self.waker.clone();
            let console = crate::remote_debug::Console::new(input, output, errors);
            let remote = crate::remote_debug::RemoteDebug::start_hatari(console, child.id(), breakpoints.clone(), move || {
                if let Some(ctx) = &wake {
                    ctx.request_repaint();
                }
            });
            let debug = Some(Box::new(Debugging { doc_id, remote, breakpoints, watches: Vec::new(), memory_before: None, memory_now: None, z80: false }));
            self.running.sessions.push(Session { target: device.name.clone(), machine: run.machine, file: run.name.clone(), kind: SessionKind::App { child, emulator, port, log, debug }, started: Instant::now() });
            return Ok(());
        }
        let debug = match (kind.filter(|_| debugging), port) {
            (Some(kind), Some(port)) => {
                let breakpoints = self.breakpoint_addresses(doc_id);
                let wake = self.waker.clone();
                let remote = crate::remote_debug::RemoteDebug::start(kind, port, Some(file.clone()), breakpoints.clone(), move || {
                    if let Some(ctx) = &wake {
                        ctx.request_repaint();
                    }
                });
                let z80 = kind == crate::remote_debug::Emulator::Zesarux;
                Some(Box::new(Debugging { doc_id, remote, breakpoints, watches: Vec::new(), memory_before: None, memory_now: None, z80 }))
            }
            _ => None,
        };
        self.running.sessions.push(Session { target: device.name.clone(), machine: run.machine, file: run.name.clone(), kind: SessionKind::App { child, emulator, port, log, debug }, started: Instant::now() });
        Ok(())
    }

    /// Stops what's running on a target, if anything is.
    pub(super) fn stop_session(&mut self, target: &str) {
        self.running.sessions.retain_mut(|session| {
            if session.target != target {
                return true;
            }
            if let SessionKind::App { child, .. } = &mut session.kind {
                let _ = child.kill();
                let _ = child.wait();
            }
            false
        });
    }

    /// Follows the sessions: an emulator that ended is let go, and one that ended at once says why.
    pub(super) fn check_sessions(&mut self) -> bool {
        let mut ended = Vec::new();
        for (index, session) in self.running.sessions.iter_mut().enumerate() {
            if let SessionKind::App { child, emulator, log, .. } = &mut session.kind
                && let Ok(Some(status)) = child.try_wait()
            {
                let quick = session.started.elapsed() < Duration::from_secs(3);
                let said = std::fs::read_to_string(log).unwrap_or_default();
                let said: String = said.trim().chars().rev().take(1500).collect::<Vec<_>>().into_iter().rev().collect();
                let problem = (quick && !status.success()).then(|| format!("{} stopped as it started ({status}).\n\n{said}", emulator.name));
                ended.push((index, problem));
            }
        }
        for (index, problem) in ended.into_iter().rev() {
            self.running.sessions.remove(index);
            if let Some(problem) = problem {
                self.message("Run on machine", problem);
            }
        }
        false
    }

    /// Starts the built-in Spectrum on this, in place of anything it was running for the same file.
    fn run_built_in(&mut self, run: Runnable) -> Result<(), String> {
        let machine = crate::zx_machine::BuiltIn::start_with(&run, self.speaker())?;
        let doc_id = self.docs[self.active].id;
        self.running.sessions.retain(|s| !matches!(s.kind, SessionKind::BuiltIn { doc_id: d, .. } if d == doc_id));
        self.running.sessions.push(Session { target: BUILT_IN_NAME.into(), machine: run.machine, file: run.name.clone(), kind: SessionKind::BuiltIn { machine: Box::new(machine), doc_id, run: Box::new(run), texture: None, error: None }, started: Instant::now() });
        Ok(())
    }

    /// The speakers, for the built-in Spectrum's sound; none in a test or a screenshot.
    fn speaker(&self) -> Option<crate::speaker::Speaker> {
        if cfg!(test) || self.screenshot.is_some() {
            return None;
        }
        crate::speaker::Speaker::open()
    }

    /// Stops everything running: emulators are closed, and the built-in Spectrum is switched off.
    pub(super) fn stop_all_sessions(&mut self) {
        let names: Vec<String> = self.running.sessions.iter().map(|s| s.target.clone()).collect();
        for name in names {
            self.stop_session(&name);
        }
        self.pending_focus = true;
    }

    /// The built-in Spectrum's panel, beside the file it's running a program from: the screen, which takes the
    /// keyboard when clicked, and buttons to pause, restart and switch it off.
    pub(super) fn machine_panel(&mut self, ui: &mut Ui) {
        let Some(doc_id) = self.docs.get(self.active).map(|doc| doc.id) else {
            return;
        };
        let Some(index) = self.running.sessions.iter().position(|s| matches!(&s.kind, SessionKind::BuiltIn { doc_id: d, .. } if *d == doc_id)) else {
            return;
        };
        let focus_id = egui::Id::unique("built-in-spectrum");
        let (mut stop, mut restart) = (false, false);
        // A program assembled from this document can be debugged: its breakpoints are set at their lines' addresses.
        let assembly = self.running.assembled.as_ref().filter(|c| c.doc_id == doc_id).map(|c| c.assembly.clone());
        let breakpoints: Vec<u16> = match (&assembly, self.docs.iter().find(|d| d.id == doc_id).and_then(|d| d.text.code.as_ref())) {
            (Some(assembly), Some(code)) => code.breakpoints.iter().filter_map(|line| assembly.address_of(0, *line)).map(|a| a as u16).collect(),
            _ => Vec::new(),
        };
        let mut debug: Option<Debug> = None;
        // The debugger's watches, conditions and memory window live with the source; borrowed while it's drawn.
        let doc_index = self.docs.iter().position(|d| d.id == doc_id);
        let mut code = doc_index.and_then(|i| self.docs[i].text.code.take());
        let line_texts: std::collections::BTreeMap<usize, String> = match (doc_index, &code) {
            (Some(i), Some(code)) => code.breakpoints.iter().filter(|l| **l < self.docs[i].text.line_count()).map(|l| (*l, self.docs[i].text.line(*l).to_string())).collect(),
            _ => Default::default(),
        };
        let ctx = ui.ctx().clone();
        let session = &mut self.running.sessions[index];
        let title = format!("{}: {}", session.machine.name(), session.file);
        let SessionKind::BuiltIn { machine, texture, error, .. } = &mut session.kind else {
            return;
        };
        machine.set_breakpoints(breakpoints);
        // A Spectrum BASIC listing: its breakpoints by line number, and the lines of the listing by number.
        let basic_listing = code.is_some() && assembly.is_none() && doc_index.is_some_and(|i| self.docs[i].text.intel.as_ref().is_some_and(|intel| intel.cpu.is_none()));
        let listing_lines: std::collections::BTreeMap<u16, usize> = match (basic_listing, doc_index) {
            (true, Some(i)) => (0..self.docs[i].text.line_count()).filter_map(|l| crate::zx_basic_debug::line_number(&self.docs[i].text.line(l)).map(|n| (n, l))).collect(),
            _ => Default::default(),
        };
        if basic_listing {
            let lines = code.as_ref().map(|code| listing_lines.iter().filter(|(_, l)| code.breakpoints.contains(l)).map(|(n, _)| *n).collect()).unwrap_or_default();
            machine.set_basic_breakpoints(Some(lines));
        }
        if let (Some(assembly), Some(code)) = (&assembly, &code) {
            machine.set_conditions(crate::debug_panel::conditions_by_address(code, assembly), Some(std::sync::Arc::new(assembly.symbols.clone())));
            machine.set_write_watches(crate::debug_panel::write_addresses(code, Some(&assembly.symbols)));
        }
        if error.is_none()
            && let Err(problem) = machine.catch_up()
        {
            *error = Some(problem);
        }
        let options = egui::TextureOptions { magnification: egui::TextureFilter::Nearest, minification: egui::TextureFilter::Linear, ..egui::TextureOptions::default() };
        let image = egui::ColorImage::from_rgba_unmultiplied([crate::zx_machine::WIDTH, crate::zx_machine::HEIGHT], machine.picture());
        match texture {
            Some(texture) => texture.set(image, options),
            None => *texture = Some(ctx.load_texture("built-in-spectrum", image, options)),
        }
        let focused = ctx.memory(|m| m.has_focus(focus_id));
        let keyboard_id = egui::Id::unique("built-in-spectrum-keyboard");
        let mut keyboard = ctx.data_mut(|d| *d.get_persisted_mut_or(keyboard_id, true));
        let mut keyboard_used = false;
        Panel::right("machine-panel").default_size(crate::zx_machine::WIDTH as f32 * 2.0 + 16.0).size_range(240.0..=1400.0).resizable(true).frame(egui::Frame::new().fill(theme::field()).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).color(theme::text_dim()).size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    stop = ui.add(Button::new(RichText::new("\u{D7}").color(theme::text_dim())).frame(false)).on_hover_text("Switch it off").clicked();
                    if ui.selectable_label(keyboard, "Keyboard").on_hover_text("Shows the Spectrum's keyboard under its screen, to click").clicked() {
                        keyboard = !keyboard;
                    }
                    restart = ui.small_button("Restart").on_hover_text("Runs the program again from the start").clicked();
                    let pause = if machine.paused { "Carry on" } else { "Pause" };
                    if ui.small_button(pause).clicked() {
                        machine.paused = !machine.paused;
                    }
                    if machine.has_speaker() {
                        let sound = if machine.muted { "Sound on" } else { "Mute" };
                        if ui.small_button(sound).clicked() {
                            let muted = !machine.muted;
                            machine.set_muted(muted);
                        }
                    }
                });
            });
            ui.separator();
            // The screen as big as it can be with room left for the words under it, and the keyboard if shown.
            let under = 40.0 + if keyboard { crate::zx_keyboard::height(ui.available_width()) } else { 0.0 };
            let room = (ui.available_height() - under).max(120.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let ratio = crate::zx_machine::HEIGHT as f32 / crate::zx_machine::WIDTH as f32;
            let width = ui.available_width().min(room / ratio);
            let size = egui::vec2(width, width * ratio);
            let (rect, _) = ui.horizontal(|ui| {
                ui.add_space((ui.available_width() - width).max(0.0) / 2.0);
                ui.allocate_exact_size(size, egui::Sense::hover())
            }).inner;
            let response = ui.interact(rect, focus_id, egui::Sense::click());
            if let Some(texture) = texture {
                ui.painter().image(texture.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
            }
            if response.clicked() {
                response.request_focus();
            }
            if focused {
                ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(2.0, theme::accent()), egui::StrokeKind::Outside);
                ui.memory_mut(|m| m.set_focus_lock_filter(focus_id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true }));
            }
            let words = match error {
                Some(problem) => problem.clone(),
                None if focused => "Typing goes to the Spectrum. Ctrl is Symbol Shift, Esc is Break. Click the text to type there again. On the keyboard below, click Caps Shift or Symbol Shift and then a key to press them together; both shifts is Extended mode.".into(),
                None => "Click the screen to type on the Spectrum.".into(),
            };
            ui.add(egui::Label::new(RichText::new(words).color(theme::text_faint()).size(12.0)).wrap());
            let mut basic_action = None;
            if let (Some((line, statement)), None) = (machine.basic_stopped, error.as_ref()) {
                basic_action = basic_debugger(ui, line, statement, &crate::zx_basic_debug::variables(&|a| machine.peek(a)));
            } else if basic_listing && !machine.paused && error.is_none() {
                ui.horizontal(|ui| {
                    if ui.small_button("Stop at the next statement").on_hover_text("Pauses the BASIC program before its next statement, to step through it").clicked() {
                        machine.pause_basic();
                    }
                    ui.label(RichText::new("or click beside a line's number for a breakpoint").color(theme::text_faint()).size(11.5));
                });
            }
            match basic_action {
                Some(Debug::Step) => {
                    if let Err(problem) = machine.step_basic() {
                        *error = Some(problem);
                    }
                }
                Some(_) => machine.carry_on(),
                None => {}
            }
            if machine.paused && error.is_none() && machine.basic_stopped.is_none() {
                let mut stop = crate::remote_debug::Stop::z80(machine.registers());
                stop.reason = machine.stop_reason.clone();
                let memory = machine.memory();
                match code.as_mut() {
                    Some(code) => {
                        debug = crate::debug_panel::stopped(ui, &stop, Some(&memory), machine.memory_before.as_deref().map(|m| m.as_slice()), assembly.as_deref(), code).map(Debug::from);
                    }
                    None => {
                        let mut scratch = crate::text_buffer::CodeMarks::default();
                        debug = crate::debug_panel::stopped(ui, &stop, Some(&memory), None, None, &mut scratch).map(Debug::from);
                    }
                }
            }
            if let (Some(code), true) = (code.as_mut(), assembly.is_some()) {
                crate::debug_panel::breakpoints(ui, code, &|line| line_texts.get(&line).cloned().unwrap_or_default(), true, true);
            }
            if keyboard {
                ui.add_space(4.0);
                keyboard_used = crate::zx_keyboard::draw(ui, machine);
            }
            });
        });
        ctx.data_mut(|d| d.insert_persisted(keyboard_id, keyboard));
        if keyboard_used {
            // Typing carries on going to the Spectrum after a key's clicked.
            ctx.memory_mut(|m| m.request_focus(focus_id));
        }
        if focused {
            let events = ctx.input(|i| i.events.clone());
            let ctrl = ctx.input(|i| i.modifiers.ctrl || i.modifiers.alt);
            spectrum_keys(machine, &events, ctrl);
        } else {
            machine.release_all();
        }
        if let (Some(i), Some(code)) = (doc_index, code.take()) {
            self.docs[i].text.code = Some(code);
        }
        let SessionKind::BuiltIn { machine, error, .. } = &mut self.running.sessions[index].kind else {
            return;
        };
        if let Some(action) = debug {
            // What memory was at this stop, so what the program changes before the next stands out.
            machine.memory_before = Some(std::sync::Arc::new(machine.memory()));
            let done = match action {
                Debug::CarryOn => {
                    machine.carry_on();
                    Ok(())
                }
                Debug::Step => machine.step(),
                Debug::StepOver => machine.step_over(),
            };
            if let Err(problem) = done {
                *error = Some(problem);
            }
        }
        // Where it's stopped, marked in the source, which is brought to that line when it first stops there.
        let stopped_line = match machine.basic_stopped {
            Some((line, _)) => listing_lines.get(&line).copied(),
            None if machine.paused => assembly.as_ref().and_then(|a| a.line_at(machine.registers().pc as i64)).filter(|(file, _)| *file == 0).map(|(_, line)| line),
            None => None,
        };
        if !machine.paused && error.is_none() {
            ctx.request_repaint_after(Duration::from_millis(15));
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
        if restart {
            let SessionKind::BuiltIn { run, .. } = &self.running.sessions[index].kind else {
                return;
            };
            let run = (**run).clone();
            let muted = matches!(&self.running.sessions[index].kind, SessionKind::BuiltIn { machine, .. } if machine.muted);
            match crate::zx_machine::BuiltIn::start_with(&run, self.speaker()) {
                Ok(mut fresh) => {
                    fresh.set_muted(muted);
                    if let SessionKind::BuiltIn { machine, error, .. } = &mut self.running.sessions[index].kind {
                        **machine = fresh;
                        *error = None;
                    }
                }
                Err(problem) => self.message("Restart", problem),
            }
        }
        if stop {
            self.running.sessions.remove(index);
            self.pending_focus = true;
        }
    }

    /// For a screenshot: runs the built-in Spectrum until it stops at a breakpoint, half a minute of its time at most.
    pub(super) fn run_until_stopped_for_screenshot(&mut self) {
        let doc_id = self.docs[self.active].id;
        let breakpoints: Vec<u16> = match (self.running.assembled.as_ref().filter(|c| c.doc_id == doc_id), self.docs[self.active].text.code.as_ref()) {
            (Some(cache), Some(code)) => code.breakpoints.iter().filter_map(|line| cache.assembly.address_of(0, *line)).map(|a| a as u16).collect(),
            _ => return,
        };
        let conditions = match (self.running.assembled.as_ref().filter(|c| c.doc_id == doc_id), self.docs[self.active].text.code.as_ref()) {
            (Some(cache), Some(code)) => Some((crate::debug_panel::conditions_by_address(code, &cache.assembly), std::sync::Arc::new(cache.assembly.symbols.clone()))),
            _ => None,
        };
        for session in &mut self.running.sessions {
            if let SessionKind::BuiltIn { machine, doc_id: d, .. } = &mut session.kind
                && *d == doc_id
            {
                machine.set_breakpoints(breakpoints.clone());
                if let Some((conditions, labels)) = conditions.clone() {
                    machine.set_conditions(conditions, Some(labels));
                }
                for _ in 0..50 * 30 {
                    if machine.frame().is_err() || machine.stopped.is_some() {
                        break;
                    }
                }
            }
        }
    }

    /// For a screenshot: the built-in machines run on for a number of frames, as if that much time had passed.
    pub(super) fn run_frames_for_screenshot(&mut self, frames: usize) {
        for session in &mut self.running.sessions {
            if let SessionKind::BuiltIn { machine, .. } = &mut session.kind {
                for _ in 0..frames {
                    if machine.frame().is_err() {
                        break;
                    }
                }
            }
        }
    }

    /// The addresses of a document's breakpoints, in the program assembled from it.
    pub(super) fn breakpoint_addresses(&self, doc_id: u64) -> Vec<u16> {
        match (self.running.assembled.as_ref().filter(|c| c.doc_id == doc_id), self.docs.iter().find(|d| d.id == doc_id).and_then(|d| d.text.code.as_ref())) {
            (Some(cache), Some(code)) => code.breakpoints.iter().filter_map(|line| cache.assembly.address_of(0, *line)).map(|a| a as u16).collect(),
            _ => Vec::new(),
        }
    }

    /// While a program from the document in front is being debugged in an emulator: where it's stopped, its
    /// registers, and the buttons, beside the source.
    pub(super) fn remote_debug_panel(&mut self, ui: &mut Ui) {
        let Some(doc_id) = self.docs.get(self.active).map(|doc| doc.id) else {
            return;
        };
        let breakpoints = self.breakpoint_addresses(doc_id);
        let assembly = self.running.assembled.as_ref().filter(|c| c.doc_id == doc_id).map(|c| c.assembly.clone());
        let doc_index = self.docs.iter().position(|d| d.id == doc_id);
        let mut code = doc_index.and_then(|i| self.docs[i].text.code.take());
        let line_texts: std::collections::BTreeMap<usize, String> = match (doc_index, &code) {
            (Some(i), Some(code)) => code.breakpoints.iter().filter(|l| **l < self.docs[i].text.line_count()).map(|l| (*l, self.docs[i].text.line(*l).to_string())).collect(),
            _ => Default::default(),
        };
        let Some((target, debugging)) = self.running.sessions.iter_mut().find_map(|s| match &mut s.kind {
            SessionKind::App { debug: Some(d), .. } if d.doc_id == doc_id => Some((s.target.clone(), d)),
            _ => None,
        }) else {
            if let (Some(i), Some(code)) = (doc_index, code) {
                self.docs[i].text.code = Some(code);
            }
            return;
        };
        let newly_stopped = debugging.remote.check();
        if debugging.breakpoints != breakpoints {
            debugging.breakpoints = breakpoints.clone();
            debugging.remote.ask(crate::remote_debug::Ask::Breakpoints(breakpoints));
        }
        let labels = assembly.as_ref().map(|a| &a.symbols);
        let watches = code.as_ref().map(|c| crate::debug_panel::write_addresses(c, labels)).unwrap_or_default();
        if debugging.watches != watches {
            debugging.watches = watches.clone();
            debugging.remote.ask(crate::remote_debug::Ask::Watches(watches));
        }
        // A stop at a breakpoint whose condition isn't true carries straight on.
        if newly_stopped && let Some(stop) = debugging.remote.stopped.clone() {
            debugging.memory_before = debugging.memory_now.take();
            debugging.memory_now = stop.memory.clone();
            let condition = match (&code, &assembly) {
                (Some(code), Some(assembly)) => crate::debug_panel::conditions_by_address(code, assembly).get(&stop.pc).cloned(),
                _ => None,
            };
            if let (Some(condition), Some(registers), Some(memory)) = (condition, stop.z80, stop.memory.as_ref()) {
                let peek = |a: u16| memory[a as usize];
                let seen = crate::debug_panel::Seen { registers: Some(&registers), peek: &peek, labels };
                match crate::debug_panel::eval(&condition, &seen) {
                    Ok(0) => debugging.remote.ask(crate::remote_debug::Ask::CarryOn),
                    Ok(_) => debugging.remote.stopped.as_mut().into_iter().for_each(|s| s.reason = Some(format!("Stopped because {condition}"))),
                    Err(problem) => debugging.remote.stopped.as_mut().into_iter().for_each(|s| s.reason = Some(format!("The condition {condition} couldn't be worked out: {problem}"))),
                }
            }
        }
        let stopped = debugging.remote.stopped.clone();
        let problem = debugging.remote.problem.clone();
        let before = debugging.memory_before.clone();
        let z80 = debugging.z80;
        let mut action = None;
        let mut pause = false;
        Panel::right("remote-debug").default_size(380.0).resizable(true).frame(egui::Frame::new().fill(theme::field()).inner_margin(egui::Margin::symmetric(8, 6))).show(ui, |ui| {
            ui.label(RichText::new(format!("Debugging in {target}")).color(theme::text_dim()).size(12.5));
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                match (&problem, &stopped) {
                    (Some(problem), _) => {
                        ui.add(egui::Label::new(RichText::new(problem).color(theme::problem_mark())).wrap());
                    }
                    (None, Some(stop)) => {
                        let mut scratch = crate::text_buffer::CodeMarks::default();
                        let marks: &mut crate::text_buffer::CodeMarks = match code.as_mut() { Some(code) => code, None => &mut scratch };
                        action = crate::debug_panel::stopped(ui, stop, stop.memory.as_deref().map(|m| m.as_slice()), before.as_deref().map(|m| m.as_slice()), assembly.as_deref(), marks).map(Debug::from);
                    }
                    (None, None) => {
                        ui.horizontal(|ui| {
                            ui.label("Running");
                            pause = ui.small_button("Pause").on_hover_text("Stops it where it is").clicked();
                        });
                        ui.add(egui::Label::new(RichText::new("Click beside a line's number for a breakpoint; it stops there.").color(theme::text_faint()).size(11.5)).wrap());
                    }
                }
                if let (Some(code), true) = (code.as_mut(), assembly.is_some()) {
                    crate::debug_panel::breakpoints(ui, code, &|line| line_texts.get(&line).cloned().unwrap_or_default(), z80, true);
                }
            });
        });
        if pause {
            debugging.remote.ask(crate::remote_debug::Ask::Pause);
        }
        if let Some(action) = action {
            debugging.remote.ask(match action {
                Debug::CarryOn => crate::remote_debug::Ask::CarryOn,
                Debug::Step => crate::remote_debug::Ask::Step,
                Debug::StepOver => crate::remote_debug::Ask::StepOver,
            });
        }
        if let (Some(i), Some(code)) = (doc_index, code) {
            self.docs[i].text.code = Some(code);
        }
        let stopped_line = stopped.as_ref().and_then(|r| assembly.as_ref().and_then(|a| a.line_at(r.pc as i64))).filter(|(file, _)| *file == 0).map(|(_, line)| line);
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

    /// Words for the status bar about what's running.
    pub(super) fn run_status(&self) -> Option<String> {
        if let Some(session) = self.running.sessions.last() {
            let more = if self.running.sessions.len() > 1 { format!(" (and {} more)", self.running.sessions.len() - 1) } else { String::new() };
            return Some(format!("Running {} on {}{more}", session.file, session.target));
        }
        self.running.note.as_ref().filter(|(_, when)| when.elapsed() < Duration::from_secs(8)).map(|(words, _)| words.clone())
    }

    /// The box asking where to run something, or which machine's BASIC a text is.
    pub(super) fn run_dialog(&mut self, ctx: &Context) -> bool {
        let Dialog::Run(state) = &self.dialog else {
            return false;
        };
        // What's shown, read first, so the box can be drawn and then acted on.
        let shown = match state.as_ref() {
            RunBox::Where { runnable, remember } => Some((runnable.name.clone(), runnable.machine, self.targets_for(runnable), *remember)),
            RunBox::Which { .. } => None,
        };
        let mut close = ctx.input(|i| i.key_pressed(Key::Escape));
        let mut go: Option<Target> = None;
        let mut as_listing: Option<basic::Machine> = None;
        let mut manage = false;
        let mut keep = shown.as_ref().is_some_and(|(.., remember)| *remember);
        egui::Window::new("Run on machine").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| match &shown {
            Some((file, machine, targets, _)) => {
                ui.label(format!("Where should {file} run? It's a {} program.", machine.name()));
                ui.add_space(6.0);
                let mut any = false;
                for (target, name, suits) in targets {
                    match suits {
                        Ok(()) => {
                            any = true;
                            if ui.button(name.as_str()).clicked() {
                                go = Some(*target);
                            }
                        }
                        Err(why) => {
                            ui.add_enabled(false, egui::Button::new(name.as_str())).on_disabled_hover_text(why.as_str());
                        }
                    }
                }
                if !any {
                    let suggestions: Vec<String> = emulators::KNOWN.iter().filter(|e| e.machines.contains(machine)).map(|e| format!("{} ({})", e.name, e.website)).collect();
                    let words = if suggestions.is_empty() { String::new() } else { format!(" Emulators that run them: {}.", suggestions.join(", ")) };
                    ui.add(egui::Label::new(RichText::new(format!("Nothing set up runs {} programs yet. Find emulators, under Devices and emulators, adds any that are installed.{words}", machine.name())).color(theme::text_dim())).wrap());
                }
                ui.add_space(6.0);
                ui.checkbox(&mut keep, format!("Always run {} programs there (F9)", machine.name()));
                ui.horizontal(|ui| {
                    manage = ui.button("Devices and emulators\u{2026}").clicked();
                    close |= ui.button("Cancel").clicked();
                });
            }
            None => {
                ui.label("This isn't a program Limoniter knows how to run. Run it as a BASIC listing for:");
                ui.add_space(6.0);
                for (machine, name) in LISTING_MACHINES {
                    if ui.button(name).clicked() {
                        as_listing = Some(machine);
                    }
                }
                ui.add_space(6.0);
                close |= ui.button("Cancel").clicked();
            }
        });
        let Dialog::Run(state) = std::mem::replace(&mut self.dialog, Dialog::None) else {
            return true;
        };
        match *state {
            _ if manage => self.open_devices_box(),
            RunBox::Which { doc_id } if as_listing.is_some() => {
                if let Some(index) = self.docs.iter().position(|doc| doc.id == doc_id) {
                    let doc = &mut self.docs[index];
                    let stem = doc.path.as_deref().and_then(Path::file_stem).map_or_else(|| doc.name(), |s| s.to_string_lossy().into_owned());
                    doc.listing_of = as_listing.map(|machine| (machine, stem));
                    self.active = index;
                    self.run_on_machine();
                }
            }
            RunBox::Where { runnable, .. } if go.is_some() => {
                let target = go.expect("checked");
                if keep {
                    let name = self.target_name(target);
                    self.running.set_default(runnable.machine, &name);
                    self.save_settings_if_changed();
                }
                self.begin_run(target);
                self.pending_focus = true;
            }
            _ if close => self.pending_focus = true,
            RunBox::Where { runnable, .. } => self.dialog = Dialog::Run(Box::new(RunBox::Where { runnable, remember: keep })),
            which => self.dialog = Dialog::Run(Box::new(which)),
        }
        true
    }
}

/// What the debugger's buttons ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Debug {
    CarryOn,
    Step,
    StepOver,
}

impl From<crate::debug_panel::Action> for Debug {
    fn from(action: crate::debug_panel::Action) -> Self {
        match action {
            crate::debug_panel::Action::CarryOn => Debug::CarryOn,
            crate::debug_panel::Action::Step => Debug::Step,
            crate::debug_panel::Action::StepOver => Debug::StepOver,
        }
    }
}

/// While a BASIC program is stopped before a statement: where, the buttons, and its variables.
fn basic_debugger(ui: &mut Ui, line: u16, statement: u8, variables: &[crate::zx_basic_debug::Variable]) -> Option<Debug> {
    let mut action = None;
    ui.add_space(4.0);
    egui::Frame::new().fill(ui.visuals().faint_bg_color).inner_margin(egui::Margin::same(6)).corner_radius(4.0).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Stopped at line {line}, statement {statement}")).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Next statement").on_hover_text("Runs this statement and stops before the next").clicked() {
                    action = Some(Debug::Step);
                }
                if ui.small_button("Carry on").on_hover_text("Runs on until the next breakpoint").clicked() {
                    action = Some(Debug::CarryOn);
                }
            });
        });
        egui::CollapsingHeader::new(format!("Variables ({})", variables.len())).id_salt("basic-variables").default_open(true).show(ui, |ui| {
            if variables.is_empty() {
                ui.label(RichText::new("None yet.").color(theme::text_faint()).size(11.5));
            }
            egui::Grid::new("basic-variables-grid").num_columns(2).spacing([14.0, 2.0]).show(ui, |ui| {
                for variable in variables {
                    ui.label(RichText::new(&variable.name).monospace().size(12.0).strong());
                    ui.label(RichText::new(&variable.value).monospace().size(12.0));
                    ui.end_row();
                }
            });
        });
    });
    action
}

/// Keys pressed on the computer, played on the Spectrum. Letters, digits, Space and Enter are held as long as
/// they're held here, as games need; capitals and symbols are typed, with the shift the Spectrum needs for each.
fn spectrum_keys(machine: &mut crate::zx_machine::BuiltIn, events: &[egui::Event], symbol_shift: bool) {
    use rustzx_core::zx::keys::{CompoundKey, ZXKey};
    machine.key(ZXKey::SymShift, symbol_shift);
    for event in events {
        match event {
            egui::Event::Key { key, pressed, repeat: false, modifiers, .. } => {
                let compound = match key {
                    Key::Backspace => Some(CompoundKey::Delete),
                    Key::ArrowLeft => Some(CompoundKey::ArrowLeft),
                    Key::ArrowRight => Some(CompoundKey::ArrowRight),
                    Key::ArrowUp => Some(CompoundKey::ArrowUp),
                    Key::ArrowDown => Some(CompoundKey::ArrowDown),
                    Key::Escape => Some(CompoundKey::Break),
                    _ => None,
                };
                if let Some(compound) = compound {
                    machine.compound(compound, *pressed);
                    continue;
                }
                let plain = match key {
                    Key::Enter => Some(ZXKey::Enter),
                    Key::Space => Some(ZXKey::Space),
                    // With Shift, a letter or digit comes as the character it makes, typed below.
                    _ if modifiers.shift => None,
                    _ => key.name().chars().next().filter(|_| key.name().len() == 1).and_then(|c| crate::zx_machine::keys_for(c.to_ascii_lowercase())).and_then(|keys| keys.first().copied()),
                };
                if let Some(zx) = plain {
                    machine.key(zx, *pressed);
                }
            }
            egui::Event::Text(text) => {
                for c in text.chars() {
                    if c.is_ascii_uppercase() || (c.is_ascii_punctuation()) {
                        machine.type_char(c);
                    }
                }
            }
            _ => {}
        }
    }
}

/// A port nothing is listening on, for an emulator to be reached on.
fn free_port() -> Option<u16> {
    std::net::TcpListener::bind(("127.0.0.1", 0)).ok().and_then(|listener| listener.local_addr().ok()).map(|address| address.port())
}
