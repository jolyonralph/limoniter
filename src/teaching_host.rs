//! The window's side of the machines built in for teaching: the Manchester Baby (see `ssem`), EDSAC (`edsac`) and
//! the PDP-8 (`pdp8`, with `pal8`). Each runs in a panel beside the source it was run from (F9), showing what the
//! machine itself showed: the Baby's store as dots and dashes on its tube, EDSAC's monitoring tubes and teleprinter,
//! and the PDP-8's front panel of lights and switches with its Teletype. Each can be stopped, stepped an
//! instruction at a time and run to a breakpoint set in the source, and runs at its own speed or as fast as it goes.

use super::run_host::{Session, SessionKind};
use super::*;
use crate::running::{Machine, Runnable};
use std::time::Instant;

pub(super) const BABY_NAME: &str = "Built-in Manchester Baby";
pub(super) const EDSAC_NAME: &str = "Built-in EDSAC";
pub(super) const PDP8_NAME: &str = "Built-in PDP-8";

/// The teaching machines, with the name each built-in one goes by.
pub(super) const MACHINES: [(Machine, &str); 3] = [(Machine::Ssem, BABY_NAME), (Machine::Edsac, EDSAC_NAME), (Machine::Pdp8, PDP8_NAME)];

/// The machine a language's source is for.
pub(super) fn machine_for_language(language: &str) -> Option<Machine> {
    match language {
        highlight::SSEM => Some(Machine::Ssem),
        highlight::EDSAC => Some(Machine::Edsac),
        highlight::PAL8 => Some(Machine::Pdp8),
        _ => None,
    }
}

/// Whether the built-in machine takes this; if not, why not.
pub(super) fn takes(machine: Machine, run: &Runnable) -> Result<(), String> {
    if run.machine == machine {
        return Ok(());
    }
    Err(match machine {
        Machine::Ssem => crate::i18n::tr("The built-in Manchester Baby runs its own source (.ssem)."),
        Machine::Edsac => crate::i18n::tr("The built-in EDSAC runs its own tapes (.edsac)."),
        _ => crate::i18n::tr("The built-in PDP-8 runs PAL source (.pal) and RIM tapes."),
    }
    .into())
}

/// Problems found in a program, as words to show: the first few, with their lines.
fn problems_text(problems: &[(usize, String)]) -> String {
    let mut text: Vec<String> = problems.iter().take(8).map(|(line, message)| crate::i18n::tf!("Line {}: {message}", line + 1)).collect();
    if problems.len() > 8 {
        text.push(crate::i18n::tf!("...and {} more.", problems.len() - 8));
    }
    text.join("\n")
}

/// One of the machines, with what it was made from.
pub(super) enum Kind {
    Baby { baby: Box<crate::ssem::Baby>, program: Box<crate::ssem::Program> },
    Edsac { edsac: Box<crate::edsac::Edsac>, tank: usize },
    Pdp8 { pdp: Box<crate::pdp8::Pdp8>, assembly: Option<crate::pal8::Assembly> },
}

/// A teaching machine in its panel.
pub(super) struct Teaching {
    pub kind: Kind,
    /// Whether it's running (the PDP-8 keeps its own RUN), and whether as fast as it goes.
    running: bool,
    pub fast: bool,
    /// When it last ran, and the machine's time owed since (microseconds; nanoseconds for the PDP-8).
    last: Option<Instant>,
    owed: f64,
    /// Whether the next instruction is the one it stopped at, which a breakpoint there mustn't stop again.
    resumed: bool,
    /// Something to say under the machine: a tape loaded, a problem.
    pub note: Option<String>,
}

/// The time an instruction takes on the Baby, in microseconds.
const BABY_US: f64 = crate::ssem::MICROSECONDS_PER_INSTRUCTION as f64;

impl Teaching {
    /// The machine a program is for, with the program in it, started.
    pub(super) fn start(run: &Runnable) -> Result<Teaching, String> {
        let text = String::from_utf8_lossy(&run.bytes).replace("\r\n", "\n");
        let kind = match run.machine {
            Machine::Ssem => {
                let program = crate::ssem::assemble(&text).map_err(|p| problems_text(&p))?;
                Kind::Baby { baby: Box::new(crate::ssem::Baby::new(program.store)), program: Box::new(program) }
            }
            Machine::Edsac => Kind::Edsac { edsac: Box::new(crate::edsac::Edsac::from_source(&text).map_err(|p| problems_text(&p))?), tank: 0 },
            Machine::Pdp8 => {
                let mut pdp = Box::new(crate::pdp8::Pdp8::default());
                let assembly = if run.format == crate::running::Format::Rim {
                    pdp.load(&crate::pdp8::read_rim(&run.bytes)?);
                    None
                } else {
                    let assembly = crate::pal8::assemble(&text).map_err(|p| problems_text(&p))?;
                    pdp.load(&assembly.words);
                    pdp.pc = assembly.start;
                    pdp.sr = assembly.start;
                    Some(assembly)
                };
                pdp.start();
                Kind::Pdp8 { pdp, assembly }
            }
            other => return Err(crate::i18n::tf!("{} programs don't run on a built-in teaching machine.", other.name())),
        };
        let running = !matches!(kind, Kind::Pdp8 { .. });
        Ok(Teaching { kind, running, fast: false, last: None, owed: 0.0, resumed: false, note: None })
    }

    pub(super) fn is_running(&self) -> bool {
        match &self.kind {
            Kind::Pdp8 { pdp, .. } => pdp.run,
            _ => self.running,
        }
    }

    fn set_running(&mut self, on: bool) {
        self.resumed = on;
        self.last = None;
        self.owed = 0.0;
        match &mut self.kind {
            Kind::Pdp8 { pdp, .. } if on => pdp.carry_on(),
            Kind::Pdp8 { pdp, .. } => pdp.run = false,
            Kind::Baby { baby, .. } if on && baby.stopped => {}
            Kind::Edsac { edsac, .. } if on && edsac.stopped.is_some() => {}
            _ => self.running = on,
        }
    }

    /// Does one instruction.
    pub(super) fn step(&mut self) {
        self.set_running(false);
        match &mut self.kind {
            Kind::Baby { baby, .. } => baby.step(),
            Kind::Edsac { edsac, .. } => edsac.step(),
            Kind::Pdp8 { pdp, .. } => {
                pdp.halted = false;
                pdp.step();
            }
        }
    }

    /// The source line the next instruction came from, if it's known.
    pub(super) fn line_now(&self) -> Option<usize> {
        match &self.kind {
            // Stopped, it's at the STP it stopped at.
            Kind::Baby { baby, program } if baby.stopped => program.line_at(crate::ssem::line_of(baby.ci)),
            Kind::Baby { baby, program } => program.line_at(baby.next_line()),
            Kind::Edsac { edsac, .. } => edsac.line_now(),
            Kind::Pdp8 { pdp, assembly } => assembly.as_ref().and_then(|a| a.line_at(pdp.pc)),
        }
    }

    /// The addresses of the source lines with breakpoints.
    fn breakpoint_addresses(&self, lines: &std::collections::BTreeSet<usize>) -> Vec<usize> {
        match &self.kind {
            Kind::Baby { program, .. } => lines.iter().filter_map(|l| program.address_of(*l)).collect(),
            Kind::Edsac { edsac, .. } => edsac.placed.iter().filter(|(_, l)| lines.contains(l)).map(|(a, _)| *a).collect(),
            Kind::Pdp8 { assembly, .. } => assembly.as_ref().map(|a| lines.iter().filter_map(|l| a.address_of(*l)).map(usize::from).collect()).unwrap_or_default(),
        }
    }

    /// Runs for the time since it last ran (or a good deal more, as fast as it goes), stopping at a breakpoint.
    pub(super) fn tick(&mut self, now: Instant, lines: &std::collections::BTreeSet<usize>) {
        let breakpoints = self.breakpoint_addresses(lines);
        if !self.is_running() {
            self.last = None;
            return;
        }
        let elapsed = self.last.map_or(0.0, |t| now.duration_since(t).as_secs_f64().min(0.25));
        self.last = Some(now);
        let fast = self.fast;
        match &mut self.kind {
            Kind::Baby { baby, .. } => {
                let count = if fast {
                    400_000
                } else {
                    self.owed += elapsed * 1e6;
                    let n = (self.owed / BABY_US) as u64;
                    self.owed -= n as f64 * BABY_US;
                    n
                };
                for _ in 0..count {
                    if !self.resumed && breakpoints.contains(&baby.next_line()) {
                        self.running = false;
                        break;
                    }
                    self.resumed = false;
                    baby.step();
                    if baby.stopped {
                        self.running = false;
                        break;
                    }
                }
            }
            Kind::Edsac { edsac, .. } => {
                // As fast as it goes, the tape reader and teleprinter wait for nothing either.
                let until = if fast { u64::MAX } else { edsac.time + (elapsed * 1e6) as u64 };
                let mut count = 0;
                while edsac.time < until && count < 300_000 {
                    // EDSAC's words learn their lines as the tape is read, so they're looked up as it goes.
                    if !self.resumed && edsac.loaded && edsac.placed.get(&edsac.sct).is_some_and(|line| lines.contains(line)) {
                        self.running = false;
                        break;
                    }
                    self.resumed = false;
                    edsac.step();
                    count += 1;
                    if edsac.stopped.is_some() {
                        self.running = false;
                        break;
                    }
                }
            }
            Kind::Pdp8 { pdp, .. } => {
                let ns = if fast { 20_000_000_000 } else { (elapsed * 1e9) as u64 };
                let points: Vec<u16> = breakpoints.iter().map(|a| *a as u16).collect();
                pdp.run_for(ns, if fast { 3_000_000 } else { 2_000_000 }, &points);
                self.resumed = false;
            }
        }
    }

    /// Starts it again from the beginning, with the same program.
    fn restart(&mut self, run: &Runnable) -> Result<(), String> {
        let fast = self.fast;
        let tank = match &self.kind {
            Kind::Edsac { tank, .. } => *tank,
            _ => 0,
        };
        let sr = match &self.kind {
            Kind::Pdp8 { pdp, .. } => Some(pdp.sr),
            _ => None,
        };
        *self = Teaching::start(run)?;
        self.fast = fast;
        if let Kind::Edsac { tank: t, .. } = &mut self.kind {
            *t = tank;
        }
        if let (Kind::Pdp8 { pdp, .. }, Some(sr)) = (&mut self.kind, sr) {
            pdp.sr = sr;
        }
        Ok(())
    }
}

impl Limoniter {
    /// Starts a built-in teaching machine on the source in front, in place of anything it was running for it.
    pub(super) fn run_teaching(&mut self, run: Runnable) -> Result<(), String> {
        let doc_id = self.docs[self.active].id;
        let machine = match Teaching::start(&run) {
            Ok(machine) => machine,
            Err(problem) => {
                // The source's problems are marked at their lines too.
                let text = String::from_utf8_lossy(&run.bytes).replace("\r\n", "\n");
                let problems = match run.machine {
                    Machine::Ssem => crate::ssem::assemble(&text).err(),
                    Machine::Edsac => crate::edsac::punch(&text).err(),
                    _ => crate::pal8::assemble(&text).err(),
                };
                if let (Some(problems), Some(code)) = (problems, self.docs[self.active].text.code.as_mut()) {
                    code.problems = problems;
                }
                return Err(problem);
            }
        };
        if let Some(code) = self.docs[self.active].text.code.as_mut() {
            code.problems.clear();
        }
        let target = MACHINES.iter().find(|(m, _)| *m == run.machine).map_or(BABY_NAME, |(_, name)| name);
        self.running.sessions.retain(|s| !matches!(s.kind, SessionKind::Teaching { doc_id: d, .. } if d == doc_id));
        self.running.sessions.push(Session { target: target.to_string(), machine: run.machine, file: run.name.clone(), kind: SessionKind::Teaching { machine: Box::new(machine), doc_id, run: Box::new(run) }, started: Instant::now() });
        Ok(())
    }

    /// What the document in front runs as, if it's source for a teaching machine.
    pub(super) fn teaching_runnable(&self) -> Option<Runnable> {
        let doc = &self.docs[self.active];
        if doc.hex_mode || doc.browser.is_some() {
            return None;
        }
        let machine = machine_for_language(&doc.language_name())?;
        let format = match machine {
            Machine::Ssem => crate::running::Format::Ssem,
            Machine::Edsac => crate::running::Format::EdsacTape,
            _ => crate::running::Format::Pal,
        };
        let stem = doc.path.as_deref().and_then(Path::file_stem).map_or_else(|| doc.name(), |s| s.to_string_lossy().into_owned());
        let text = doc.text.all_lines().join("\n");
        Some(Runnable { machine, format, name: format!("{stem}.{}", format.ending()), bytes: std::sync::Arc::new(text.into_bytes()) })
    }

    /// Runs the teaching machine beside the document in front, for the screenshots: as fast as it goes, until it
    /// stops or for a number of frames.
    pub(super) fn run_teaching_for_screenshot(&mut self, frames: usize, fast: bool) {
        let Some(doc_id) = self.docs.get(self.active).map(|d| d.id) else {
            return;
        };
        let breakpoints = self.docs[self.active].text.code.as_ref().map(|c| c.breakpoints.clone()).unwrap_or_default();
        for session in &mut self.running.sessions {
            if let SessionKind::Teaching { machine, doc_id: d, .. } = &mut session.kind
                && *d == doc_id
            {
                machine.fast = fast;
                let mut now = Instant::now();
                for _ in 0..frames {
                    now += Duration::from_millis(20);
                    machine.tick(now, &breakpoints);
                    if !machine.is_running() {
                        break;
                    }
                }
                machine.fast = false;
            }
        }
    }

    /// The panel of the teaching machine running for the document in front.
    pub(super) fn teaching_panel(&mut self, ui: &mut Ui) {
        let Some(doc_id) = self.docs.get(self.active).map(|doc| doc.id) else {
            return;
        };
        let Some(index) = self.running.sessions.iter().position(|s| matches!(&s.kind, SessionKind::Teaching { doc_id: d, .. } if *d == doc_id)) else {
            return;
        };
        let doc_index = self.docs.iter().position(|d| d.id == doc_id);
        let breakpoint_lines = doc_index.and_then(|i| self.docs[i].text.code.as_ref()).map(|c| c.breakpoints.clone()).unwrap_or_default();
        let ctx = ui.ctx().clone();
        let session = &mut self.running.sessions[index];
        let title = format!("{}: {}", session.machine.name(), session.file);
        let SessionKind::Teaching { machine, run, .. } = &mut session.kind else {
            return;
        };
        machine.tick(Instant::now(), &breakpoint_lines);
        let (mut stop, mut restart) = (false, false);
        let mut save: Option<Vec<u8>> = None;
        let mut load: Option<Vec<u8>> = None;
        let teletype_id = egui::Id::unique("built-in-teletype");
        Panel::right("teaching-machine-panel").default_size(500.0).size_range(300.0..=1400.0).resizable(true).frame(egui::Frame::new().fill(theme::field()).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&title).color(theme::text_dim()).size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    stop = crate::access::called(ui.add(Button::new(RichText::new(crate::i18n::tr("\u{D7}")).color(theme::text_dim())).frame(false)), "Switch it off").on_hover_text(crate::i18n::tr("Switch it off")).clicked();
                    restart = ui.small_button(crate::i18n::tr("Restart")).on_hover_text(crate::i18n::tr("Loads the program again and starts it from the beginning")).clicked();
                });
            });
            ui.horizontal_wrapped(|ui| {
                let running = machine.is_running();
                let label = if running { crate::i18n::tr("Stop") } else { crate::i18n::tr("Run") };
                if ui.small_button(label).on_hover_text(crate::i18n::tr("Runs on from here, until it stops or reaches a breakpoint")).clicked() {
                    machine.set_running(!running);
                }
                if ui.small_button(crate::i18n::tr("Step")).on_hover_text(crate::i18n::tr("Does one instruction")).clicked() {
                    machine.step();
                }
                let speed = if machine.fast { crate::i18n::tr("As fast as it goes") } else { crate::i18n::tr("At its own speed") };
                if ui.small_button(speed).on_hover_text(crate::i18n::tr("Runs at the speed the machine itself ran, or as fast as this computer can")).clicked() {
                    machine.fast = !machine.fast;
                    machine.owed = 0.0;
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let editable = !machine.is_running();
                match &mut machine.kind {
                    Kind::Baby { baby, .. } => baby_panel(ui, baby, editable),
                    Kind::Edsac { edsac, tank } => edsac_panel(ui, edsac, tank),
                    Kind::Pdp8 { pdp, assembly } => {
                        let (loaded, punched) = pdp8_panel(ui, pdp, assembly.as_ref(), teletype_id);
                        load = loaded;
                        save = punched;
                    }
                }
                if let Some(note) = &machine.note {
                    ui.add(egui::Label::new(RichText::new(note).color(theme::text_faint()).size(12.0)).wrap());
                }
            });
        });
        // The Teletype's keyboard, while its paper has the keyboard.
        if let Kind::Pdp8 { pdp, .. } = &mut machine.kind
            && ctx.memory(|m| m.has_focus(teletype_id))
        {
            for event in ctx.input(|i| i.events.clone()) {
                match event {
                    egui::Event::Text(text) => text.chars().filter(char::is_ascii).for_each(|c| pdp.type_key(c.to_ascii_uppercase() as u8)),
                    egui::Event::Key { key: Key::Enter, pressed: true, .. } => pdp.type_key(0o15),
                    egui::Event::Key { key: Key::Backspace, pressed: true, .. } => pdp.type_key(0o177),
                    egui::Event::Key { key: Key::Escape, pressed: true, .. } => pdp.type_key(0o33),
                    _ => {}
                }
            }
        }
        if let Some(bytes) = load
            && let Kind::Pdp8 { pdp, .. } = &mut machine.kind
        {
            match crate::pdp8::read_tape(&bytes) {
                Ok(words) => {
                    pdp.load(&words);
                    machine.note = Some(crate::i18n::tf!("{} words read from the tape into memory. Set the switches to where it starts, then Load address and Start.", words.len()));
                }
                Err(problem) => machine.note = Some(problem),
            }
        }
        if let Some(bytes) = save
            && let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Punch a BIN tape")).add_filter(crate::i18n::tr("PDP-8 BIN tapes"), &["bin"]).set_file_name(format!("{}.bin", run.name.rsplit_once('.').map_or(run.name.as_str(), |(stem, _)| stem))).save_file()
        {
            machine.note = Some(match std::fs::write(&path, bytes) {
                Ok(()) => crate::i18n::tf!("Punched to {}.", path.display()),
                Err(e) => e.to_string(),
            });
        }
        let stopped_line = if machine.is_running() { None } else { machine.line_now() };
        if machine.is_running() {
            ctx.request_repaint_after(Duration::from_millis(15));
        }
        if restart {
            let run = (**run).clone();
            if let Err(problem) = machine.restart(&run) {
                machine.note = Some(problem);
            }
        }
        if let Some(i) = doc_index
            && let Some(code) = &mut self.docs[i].text.code
            && code.stopped != stopped_line
        {
            code.stopped = stopped_line;
            if let Some(line) = stopped_line {
                self.docs[i].text.move_caret(line, 0, false);
            }
        }
        if stop {
            self.running.sessions.remove(index);
            if let Some(code) = doc_index.and_then(|i| self.docs[i].text.code.as_mut()) {
                code.stopped = None;
            }
            self.pending_focus = true;
        }
    }
}

/// A phosphor's colours: the glow and the screen behind it.
const GREEN: egui::Color32 = egui::Color32::from_rgb(120, 255, 140);
const GREEN_DIM: egui::Color32 = egui::Color32::from_rgb(40, 110, 55);
const SCREEN: egui::Color32 = egui::Color32::from_rgb(8, 18, 10);

/// Draws rows of bits on a tube, a dash (or a big dot) for each one: each row's bits from `bits(row)`, the first
/// at the left. What's clicked, as (row, bit).
fn tube(ui: &mut Ui, rows: usize, columns: usize, width: f32, dashes: bool, bits: &dyn Fn(usize, usize) -> bool, marked: Option<usize>, sense: bool) -> Option<(usize, usize)> {
    let cell = (width / (columns as f32 + 2.0)).clamp(4.0, 18.0);
    let size = egui::vec2(cell * (columns as f32 + 2.0), cell * (rows as f32 + 1.0));
    let (rect, response) = ui.allocate_exact_size(size, if sense { egui::Sense::click() } else { egui::Sense::hover() });
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, cell * 0.8, SCREEN);
    let origin = rect.min + egui::vec2(cell * 1.5, cell);
    for row in 0..rows {
        let y = origin.y + row as f32 * cell;
        if marked == Some(row) {
            painter.circle_filled(egui::pos2(rect.min.x + cell * 0.6, y), cell * 0.18, GREEN_DIM);
        }
        for column in 0..columns {
            let x = origin.x + column as f32 * cell;
            let on = bits(row, column);
            if on && dashes {
                painter.line_segment([egui::pos2(x - cell * 0.32, y), egui::pos2(x + cell * 0.32, y)], egui::Stroke::new(cell * 0.22, GREEN));
            } else if on {
                painter.circle_filled(egui::pos2(x, y), cell * 0.3, GREEN);
            } else {
                painter.circle_filled(egui::pos2(x, y), cell * 0.11, GREEN);
            }
        }
    }
    let clicked = response.clicked().then(|| response.interact_pointer_pos()).flatten()?;
    let column = ((clicked.x - origin.x) / cell + 0.5).floor();
    let row = ((clicked.y - origin.y) / cell + 0.5).floor();
    (column >= 0.0 && row >= 0.0 && (column as usize) < columns && (row as usize) < rows).then_some((row as usize, column as usize))
}

fn caption(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::text_dim()).size(12.0));
}

/// The Baby's tubes: the store (the screen is the memory), the accumulator, and the control tube's CI and PI.
fn baby_panel(ui: &mut Ui, baby: &mut crate::ssem::Baby, editable: bool) {
    let width = ui.available_width().min(420.0);
    caption(ui, crate::i18n::tr("The store: 32 lines of 32 bits, a dash for a one, the least significant bit at the left"));
    let store = baby.store;
    let next = baby.next_line();
    if let Some((row, bit)) = tube(ui, 32, 32, width, true, &|row, bit| store[row] >> bit & 1 == 1, Some(next), editable) {
        baby.flip(row, bit as u32);
    }
    caption(ui, crate::i18n::tr("The accumulator (A), and the control: CI, then PI"));
    let (a, ci, pi) = (baby.a, baby.ci, baby.pi);
    let registers = [a, ci, pi];
    tube(ui, 3, 32, width, true, &|row, bit| registers[row] >> bit & 1 == 1, None, false);
    let state = if baby.stopped { crate::i18n::tr("Stopped: the stop lamp is lit.") } else { "" };
    ui.label(RichText::new(crate::i18n::tf!("A = {}   CI = {}   instructions done: {}", a as i32, ci as i32, baby.done)).monospace().size(12.0));
    ui.label(RichText::new(crate::i18n::tf!("Next: line {next}, {}   {}", crate::ssem::describe(store[next]), crate::ssem::binary(store[next]))).monospace().size(12.0));
    if !state.is_empty() {
        ui.label(RichText::new(state).color(theme::accent()));
    }
    if editable {
        ui.label(RichText::new(crate::i18n::tr("Click a dot to set or clear that bit, as the Baby's typewriter buttons did.")).color(theme::text_faint()).size(12.0));
    }
}

/// EDSAC's monitoring tubes (a long tank of the store, the accumulator, the multiplier, the sequence control and
/// the order) and the teleprinter's paper.
fn edsac_panel(ui: &mut Ui, edsac: &mut crate::edsac::Edsac, tank: &mut usize) {
    let width = ui.available_width().min(560.0);
    ui.horizontal(|ui| {
        caption(ui, &crate::i18n::tf!("Long tank {tank}: words {} to {}, the first at the bottom", *tank * 32, *tank * 32 + 31));
        if ui.small_button("\u{25BC}").on_hover_text(crate::i18n::tr("The tank before")).clicked() {
            *tank = (*tank + 31) % 32;
        }
        if ui.small_button("\u{25B2}").on_hover_text(crate::i18n::tr("The next tank")).clicked() {
            *tank = (*tank + 1) % 32;
        }
    });
    let base = *tank * 32;
    // Each row is a long word: the odd word (senior) at the left, then the sandwich bit and the even word.
    let words: Vec<u64> = (0..16).map(|row| edsac.long(base + 2 * (15 - row))).collect();
    let marked = (edsac.sct >= base && edsac.sct < base + 32).then(|| 15 - (edsac.sct - base) / 2);
    tube(ui, 16, 35, width, false, &|row, bit| words[row] >> (34 - bit) & 1 == 1, marked, false);
    caption(ui, crate::i18n::tr("The accumulator (71 bits), the multiplier (35), the sequence control and the order"));
    let abc = edsac.abc();
    tube(ui, 1, 71, width, false, &|_, bit| abc >> (70 - bit) & 1 == 1, None, false);
    let rs = edsac.rs as u64;
    tube(ui, 1, 35, width / 2.0, false, &|_, bit| rs >> (34 - bit) & 1 == 1, None, false);
    let (sct, order) = (edsac.sct as u32, edsac.order);
    ui.horizontal(|ui| {
        tube(ui, 1, 10, width / 4.0, false, &|_, bit| sct >> (9 - bit) & 1 == 1, None, false);
        tube(ui, 1, 17, width / 2.5, false, &|_, bit| order >> (16 - bit) & 1 == 1, None, false);
    });
    let state = match &edsac.stopped {
        Some(crate::edsac::Stop::Finished) => crate::i18n::tr("Stopped at a Z order: the program has finished, and the bell rang.").to_string(),
        Some(crate::edsac::Stop::TapeEnded) => crate::i18n::tr("Stopped: the tape ran out.").to_string(),
        Some(crate::edsac::Stop::NotAnOrder(at)) => crate::i18n::tf!("Stopped: the word at {at} isn't an order."),
        None if !edsac.loaded => crate::i18n::tf!("{} reading the tape: {} of {} characters", edsac.orders.name(), edsac.read, edsac.tape.codes.len()),
        None => String::new(),
    };
    let at = edsac.at();
    ui.label(RichText::new(crate::i18n::tf!("Order: {} at {at}   A = {}   orders done: {}   time: {:.1} s", crate::edsac::describe(edsac.short(at), edsac.orders), crate::edsac::Edsac::integer(edsac.a()), edsac.done, edsac.time as f64 / 1e6)).monospace().size(12.0));
    if !state.is_empty() {
        ui.label(RichText::new(state).color(theme::accent()));
    }
    caption(ui, crate::i18n::tr("The teleprinter"));
    paper(ui, &edsac.printer.text(), None);
}

/// A teleprinter's paper: what it has printed, the latest at the bottom. With an id, it takes the keyboard when
/// clicked.
fn paper(ui: &mut Ui, text: &str, id: Option<egui::Id>) {
    let paper = egui::Color32::from_rgb(246, 241, 222);
    let ink = egui::Color32::from_rgb(40, 36, 30);
    let focused = id.is_some_and(|id| ui.memory(|m| m.has_focus(id)));
    let frame = egui::Frame::new().fill(paper).inner_margin(egui::Margin::same(6)).stroke(if focused { egui::Stroke::new(2.0, theme::accent()) } else { egui::Stroke::NONE });
    let response = frame
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::ScrollArea::vertical().id_salt(("paper", id)).max_height(220.0).stick_to_bottom(true).show(ui, |ui| {
                let lines: Vec<&str> = text.lines().collect();
                let shown = lines[lines.len().saturating_sub(400)..].join("\n");
                ui.label(RichText::new(if shown.is_empty() { " ".to_string() } else { shown }).monospace().color(ink).size(12.5));
            });
        })
        .response;
    if let Some(id) = id {
        let response = ui.interact(response.rect, id, egui::Sense::click());
        crate::access::named(&response, egui::Role::Canvas, "The Teletype: keys typed go to its keyboard while it has the keyboard");
        if response.clicked() {
            response.request_focus();
        }
        if focused {
            ui.memory_mut(|m| m.set_focus_lock_filter(id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true }));
        }
    }
}

/// The PDP-8's front panel and Teletype. A tape to read in, or one to punch, if asked for.
fn pdp8_panel(ui: &mut Ui, pdp: &mut crate::pdp8::Pdp8, assembly: Option<&crate::pal8::Assembly>, teletype: egui::Id) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let lamp_on = egui::Color32::from_rgb(255, 190, 70);
    let lamp_off = egui::Color32::from_rgb(70, 52, 30);
    let panel = egui::Color32::from_rgb(38, 34, 40);
    let label = egui::Color32::from_rgb(225, 220, 210);
    let width = ui.available_width().min(560.0);
    let cell = (width / 16.0).clamp(14.0, 30.0);
    let rows: [(&str, Option<bool>, u16, usize); 4] = [(crate::i18n::tr("Program counter"), None, pdp.pc, 12), (crate::i18n::tr("Memory address"), None, pdp.ma, 12), (crate::i18n::tr("Memory buffer"), None, pdp.mb, 12), (crate::i18n::tr("Link, accumulator"), Some(pdp.link), pdp.ac, 12)];
    let names = ["AND", "TAD", "ISZ", "DCA", "JMS", "JMP", "IOT", "OPR"];
    let height = cell * 10.6;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(cell * 16.0, height), egui::Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, panel);
    let font = egui::FontId::proportional((cell * 0.42).max(9.0));
    let left = rect.min.x + cell * 5.0;
    let spacing = cell * 0.8;
    let lamp = |painter: &egui::Painter, x: f32, y: f32, on: bool| painter.circle_filled(egui::pos2(x, y), cell * 0.27, if on { lamp_on } else { lamp_off });
    for (i, (name, link, value, bits)) in rows.iter().enumerate() {
        let y = rect.min.y + cell * (0.8 + i as f32 * 1.1);
        painter.text(egui::pos2(rect.min.x + cell * 0.3, y), egui::Align2::LEFT_CENTER, *name, font.clone(), label);
        if let Some(link) = link {
            lamp(&painter, left - cell * 0.8, y, *link);
        }
        for bit in 0..*bits {
            let gap = (bit / 3) as f32 * cell * 0.25;
            lamp(&painter, left + bit as f32 * spacing + gap, y, value >> (11 - bit) & 1 == 1);
        }
    }
    // The instruction, lit by name, and the major states, RUN and the interrupt.
    let y = rect.min.y + cell * 5.3;
    for (i, name) in names.iter().enumerate() {
        let x = rect.min.x + cell * (0.9 + i as f32 * 1.4);
        lamp(&painter, x, y, usize::from(pdp.ir) == i);
        painter.text(egui::pos2(x, y + cell * 0.55), egui::Align2::CENTER_CENTER, *name, font.clone(), label);
    }
    let y = rect.min.y + cell * 6.6;
    let states = [(crate::i18n::tr("Fetch"), pdp.state == crate::pdp8::State::Fetch), (crate::i18n::tr("Defer"), pdp.state == crate::pdp8::State::Defer), (crate::i18n::tr("Execute"), pdp.state == crate::pdp8::State::Execute), ("RUN", pdp.run), ("ION", pdp.ion)];
    for (i, (name, on)) in states.iter().enumerate() {
        let x = rect.min.x + cell * (0.9 + i as f32 * 2.2);
        lamp(&painter, x, y, *on);
        painter.text(egui::pos2(x, y + cell * 0.55), egui::Align2::CENTER_CENTER, *name, font.clone(), label);
    }
    // The switch register: a switch up for a one, in threes as octal digits are read.
    let y = rect.min.y + cell * 8.6;
    painter.text(egui::pos2(rect.min.x + cell * 0.3, y), egui::Align2::LEFT_CENTER, crate::i18n::tr("Switch register"), font.clone(), label);
    let mut switch_at = Vec::new();
    for bit in 0..12 {
        let gap = (bit / 3) as f32 * cell * 0.25;
        let x = left + bit as f32 * spacing + gap;
        let up = pdp.sr >> (11 - bit) & 1 == 1;
        let colour = if (bit / 3) % 2 == 0 { egui::Color32::from_rgb(230, 140, 60) } else { egui::Color32::from_rgb(240, 210, 110) };
        let body = egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(cell * 0.6, cell * 1.4));
        painter.rect_filled(body, 3.0, colour.gamma_multiply(0.55));
        let handle = egui::Rect::from_center_size(egui::pos2(x, if up { y - cell * 0.35 } else { y + cell * 0.35 }), egui::vec2(cell * 0.55, cell * 0.6));
        painter.rect_filled(handle, 3.0, colour);
        switch_at.push(body);
    }
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && let Some(bit) = switch_at.iter().position(|r| r.expand(2.0).contains(pos))
    {
        pdp.sr ^= 1 << (11 - bit);
    }
    crate::access::named(&response, egui::Role::Canvas, "The PDP-8's front panel: click a switch of the switch register to flip it");
    // The keys.
    ui.horizontal_wrapped(|ui| {
        if ui.small_button(crate::i18n::tr("Load address")).on_hover_text(crate::i18n::tr("The switches into the program counter")).clicked() {
            pdp.load_address();
        }
        if ui.small_button(crate::i18n::tr("Start")).on_hover_text(crate::i18n::tr("Clears the accumulator, the link and the flags, and runs from the program counter")).clicked() {
            pdp.start();
        }
        if ui.small_button(crate::i18n::tr("Deposit")).on_hover_text(crate::i18n::tr("The switches into the word at the program counter, which then moves on")).clicked() {
            pdp.deposit();
        }
        if ui.small_button(crate::i18n::tr("Examine")).on_hover_text(crate::i18n::tr("Shows the word at the program counter, which then moves on")).clicked() {
            pdp.examine();
        }
        if ui.small_button(crate::i18n::tr("Continue")).clicked() {
            pdp.carry_on();
        }
        if ui.small_button(crate::i18n::tr("Halt")).on_hover_text(crate::i18n::tr("Stops it after the instruction it's doing")).clicked() {
            pdp.run = false;
        }
    });
    let next = pdp.memory[usize::from(pdp.pc)];
    let source = assembly.and_then(|a| a.symbols.iter().filter(|(_, v)| **v <= pdp.pc).max_by_key(|(_, v)| **v).map(|(name, v)| if *v == pdp.pc { name.clone() } else { format!("{name}+{}", crate::pdp8::octal(pdp.pc - v).trim_start_matches('0')) }));
    ui.label(RichText::new(crate::i18n::tf!("PC {} ({})   next {}   AC {}   L {}", crate::pdp8::octal(pdp.pc), source.unwrap_or_default(), crate::pdp8::octal(next), crate::pdp8::octal(pdp.ac), u8::from(pdp.link))).monospace().size(12.0));
    if pdp.halted {
        ui.label(RichText::new(crate::i18n::tr("Halted (HLT). Continue carries on after it.")).color(theme::accent()));
    }
    caption(ui, crate::i18n::tr("The Teletype: click its paper to type on its keyboard"));
    paper(ui, &pdp.teletype_text(), Some(teletype));
    let mut load = None;
    let mut punch = None;
    ui.horizontal_wrapped(|ui| {
        if ui.small_button(crate::i18n::tr("Read a paper tape\u{2026}")).on_hover_text(crate::i18n::tr("Reads a RIM or BIN tape into memory, as DEC's loaders did: your own, or FOCAL or BASIC if you have them")).clicked()
            && let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Read a paper tape")).add_filter(crate::i18n::tr("PDP-8 paper tapes"), &["bin", "rim", "bn", "pt"]).add_filter(crate::i18n::tr("Any file"), &["*"]).pick_file()
        {
            load = std::fs::read(path).ok();
        }
        if let Some(assembly) = assembly
            && ui.small_button(crate::i18n::tr("Punch a BIN tape\u{2026}")).on_hover_text(crate::i18n::tr("Saves the assembled program as a BIN paper tape, for other PDP-8s and simulators")).clicked()
        {
            punch = Some(crate::pdp8::bin_tape(&assembly.words));
        }
    });
    (load, punch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runnable(machine: Machine, format: crate::running::Format, source: &str) -> Runnable {
        Runnable { machine, format, name: "t".into(), bytes: std::sync::Arc::new(source.as_bytes().to_vec()) }
    }

    #[test]
    fn each_machine_starts_on_its_source_and_stops_at_a_breakpoint() {
        // The Baby, stopping before line 13's STP is reached... at its first visit to line 6.
        let mut baby = Teaching::start(&runnable(Machine::Ssem, crate::running::Format::Ssem, crate::ssem::KILBURN)).unwrap();
        baby.fast = true;
        let line_6 = crate::ssem::KILBURN.lines().position(|l| l.starts_with("06:")).unwrap();
        let lines: std::collections::BTreeSet<usize> = [line_6].into_iter().collect();
        assert_eq!(baby.breakpoint_addresses(&lines), [6]);
        baby.tick(Instant::now(), &lines);
        assert!(!baby.is_running());
        assert_eq!(baby.line_now(), Some(line_6));
        // Run on from there: the breakpoint it's at doesn't stop it again at once.
        baby.set_running(true);
        let now = Instant::now();
        baby.tick(now, &lines);
        let Kind::Baby { baby: b, .. } = &baby.kind else { panic!() };
        assert!(b.done > 6, "{}", b.done);
        // Stepping does one instruction.
        let done = b.done;
        baby.step();
        let Kind::Baby { baby: b, .. } = &baby.kind else { panic!() };
        assert_eq!(b.done, done + 1);

        // EDSAC reads its tape, then stops at a breakpoint on the main program's first order.
        let mut edsac = Teaching::start(&runnable(Machine::Edsac, crate::running::Format::EdsacTape, crate::edsac::SQUARES)).unwrap();
        edsac.fast = true;
        let line = crate::edsac::SQUARES.lines().position(|l| l.starts_with("O41S  [84")).unwrap();
        // Until the tape is read, the line isn't known: it's learnt as the tape is read.
        let lines: std::collections::BTreeSet<usize> = [line].into_iter().collect();
        for _ in 0..50 {
            edsac.tick(Instant::now(), &lines);
            if !edsac.is_running() {
                break;
            }
        }
        assert_eq!(edsac.line_now(), Some(line));

        // The PDP-8 runs its program and prints.
        let mut pdp = Teaching::start(&runnable(Machine::Pdp8, crate::running::Format::Pal, crate::pal8::HELLO)).unwrap();
        assert!(pdp.is_running());
        pdp.fast = true;
        for _ in 0..20 {
            pdp.tick(Instant::now(), &Default::default());
        }
        let Kind::Pdp8 { pdp: p, .. } = &pdp.kind else { panic!() };
        assert!(p.halted);
        assert_eq!(p.teletype_text(), "HELLO, WORLD\n");
    }

    #[test]
    fn at_its_own_speed_the_baby_does_about_seven_hundred_instructions_a_second() {
        let mut baby = Teaching::start(&runnable(Machine::Ssem, crate::running::Format::Ssem, crate::ssem::KILBURN)).unwrap();
        let start = Instant::now();
        baby.tick(start, &Default::default());
        for tenth in 1..=10 {
            baby.tick(start + Duration::from_millis(100 * tenth), &Default::default());
        }
        let Kind::Baby { baby: b, .. } = &baby.kind else { panic!() };
        assert!((690..=700).contains(&b.done), "{}", b.done);
    }

    #[test]
    fn a_finished_program_is_marked_where_it_stopped() {
        let mut baby = Teaching::start(&runnable(Machine::Ssem, crate::running::Format::Ssem, crate::ssem::KILBURN)).unwrap();
        baby.fast = true;
        while baby.is_running() {
            baby.tick(Instant::now(), &Default::default());
        }
        assert_eq!(baby.line_now(), crate::ssem::KILBURN.lines().position(|l| l.starts_with("13: STP")));
        let mut edsac = Teaching::start(&runnable(Machine::Edsac, crate::running::Format::EdsacTape, crate::edsac::SQUARES)).unwrap();
        edsac.fast = true;
        while edsac.is_running() {
            edsac.tick(Instant::now(), &Default::default());
        }
        assert_eq!(edsac.line_now(), crate::edsac::SQUARES.lines().position(|l| l.starts_with("ZS")));
    }

    #[test]
    fn problems_in_the_source_are_said_with_their_lines() {
        let problem = Teaching::start(&runnable(Machine::Pdp8, crate::running::Format::Pal, "*200\nTAD NOWHERE")).err().unwrap();
        assert!(problem.starts_with("Line 2:"), "{problem}");
        assert!(Teaching::start(&runnable(Machine::Ssem, crate::running::Format::Ssem, "1: FRED")).is_err());
        assert!(takes(Machine::Edsac, &runnable(Machine::Ssem, crate::running::Format::Ssem, "")).is_err());
    }

    #[test]
    fn languages_lead_to_their_machines() {
        assert_eq!(machine_for_language(highlight::SSEM), Some(Machine::Ssem));
        assert_eq!(machine_for_language(highlight::EDSAC), Some(Machine::Edsac));
        assert_eq!(machine_for_language(highlight::PAL8), Some(Machine::Pdp8));
        assert_eq!(machine_for_language("Assembly Z80"), None);
    }
}
