//! The built-in ZX80, ZX81 and Jupiter Ace in the window: a panel beside the file a program was run from, as the built-in
//! Spectrum has, with the screen (which takes the keyboard when clicked), pause, restart and switch off, and the
//! debugger for machine code assembled from the file. A machine whose ROM hasn't been chosen asks for it there.

use super::run_host::{Session, SessionKind};
use super::*;
use crate::micro_roms::Needs;
use crate::running::{Format, Machine, Runnable};
use crate::z80_micro::{Hardware, Key as MatrixKey};
use std::time::{Duration, Instant};

/// A machine in the panel: running, or waiting for its ROM.
pub(super) enum Micro {
    Zx80(Box<crate::zx80::Zx80>),
    Zx81(Box<crate::zx81::Zx81>),
    Ace(Box<crate::jupiter_ace::Ace>),
    NeedsRom(Needs),
}

/// The same for each machine's core.
macro_rules! with_core {
    ($micro:expr, $core:ident => $then:expr, $otherwise:expr) => {
        match $micro {
            Micro::Zx80(m) => {
                let $core = &mut m.core;
                $then
            }
            Micro::Zx81(m) => {
                let $core = &mut m.core;
                $then
            }
            Micro::Ace(m) => {
                let $core = &mut m.core;
                $then
            }
            Micro::NeedsRom(_) => $otherwise,
        }
    };
}

/// Where the trainer's view of these machines' memory starts: the ZX81's and the Ace's RAM are all above it.
pub(super) const TRAINER_BASE: usize = 0x2000;

pub(super) const ZX80_NAME: &str = "Built-in ZX80";
pub(super) const ZX81_NAME: &str = "Built-in ZX81";
pub(super) const ACE_NAME: &str = "Built-in Jupiter Ace";

/// Which ROM a machine needs.
pub(super) fn needs(machine: Machine) -> Option<Needs> {
    match machine {
        Machine::Zx80 => Some(Needs::Zx80),
        Machine::Zx81 => Some(Needs::Zx81),
        Machine::JupiterAce => Some(Needs::Ace),
        _ => None,
    }
}

/// Whether the built-in machine takes this; if not, why not.
pub(super) fn takes(needs: Needs, run: &Runnable) -> Result<(), String> {
    match (needs, run.machine, run.format) {
        (Needs::Zx80, Machine::Zx80, Format::O) => Ok(()),
        (Needs::Zx81, Machine::Zx81, Format::P | Format::Tzx) => Ok(()),
        (Needs::Ace, Machine::JupiterAce, Format::Tap | Format::Tzx | Format::Forth) => Ok(()),
        (Needs::Zx80, ..) => Err(crate::i18n::tr("The built-in ZX80 runs ZX80 programs (.o files).").into()),
        (Needs::Zx81, ..) => Err(crate::i18n::tr("The built-in ZX81 runs ZX81 programs (.p files and ZX81 tapes).").into()),
        (Needs::Ace, ..) => Err(crate::i18n::tr("The built-in Jupiter Ace runs Ace tapes and Forth source.").into()),
        // The Vectrex has a panel of its own (see `vectrex_run_host`).
        (Needs::Vectrex, ..) => Err(crate::i18n::tr("The built-in Vectrex runs Vectrex cartridges.").into()),
    }
}

impl Micro {
    /// Starts the machine a program is for, with the program in it, if its ROM has been chosen; else it waits for one.
    pub(super) fn start(run: &Runnable) -> Result<Micro, String> {
        let needs = needs(run.machine).ok_or_else(|| crate::i18n::tf!("{} programs don't run on a built-in machine.", run.machine.name()))?;
        takes(needs, run)?;
        let rom = match crate::micro_roms::rom(needs) {
            Some(Ok(rom)) => rom,
            // A ROM that's gone, or changed, is asked for again; the panel says why.
            _ => return Ok(Micro::NeedsRom(needs)),
        };
        match needs {
            Needs::Zx80 => {
                let programs = crate::zx80::programs_in(&run.name, &run.bytes)?;
                Ok(Micro::Zx80(Box::new(crate::zx80::Zx80::start(rom, programs)?)))
            }
            Needs::Zx81 => {
                let programs = crate::zx81::programs_in(&run.name, &run.bytes)?;
                Ok(Micro::Zx81(Box::new(crate::zx81::Zx81::start(rom, 16, programs)?)))
            }
            Needs::Ace => {
                let mut ace = crate::jupiter_ace::Ace::start(rom, true)?;
                if run.format == Format::Forth {
                    let text = String::from_utf8_lossy(&run.bytes).replace("\r\n", "\n");
                    let mut text = text.trim_end().to_string();
                    text.push('\n');
                    ace.type_text(&text)?;
                } else {
                    let blocks = crate::jupiter_ace::tape_blocks(&run.name, &run.bytes)?;
                    let files = crate::jupiter_ace::tape_files(&blocks);
                    let Some((name, dictionary)) = files.first().cloned() else {
                        return Err(crate::i18n::tr("There's no Jupiter Ace file on this tape.").into());
                    };
                    ace.insert_tape(blocks);
                    // A dictionary is loaded by name; bytes go back where they were saved from.
                    let command = if dictionary { format!("LOAD {name}\n") } else { format!("0 0 BLOAD {name}\n") };
                    ace.type_text(&command)?;
                }
                Ok(Micro::Ace(Box::new(ace)))
            }
            Needs::Vectrex => Err(crate::i18n::tr("The built-in Vectrex runs Vectrex cartridges.").into()),
        }
    }

    pub(super) fn frame(&mut self) {
        with_core!(self, core => core.frame(), ())
    }

    /// The machine's name as the debug console's lines give it.
    pub(super) fn console_name(&self) -> &'static str {
        match self {
            Micro::Zx80(_) => "ZX80",
            Micro::Zx81(_) => "ZX81",
            Micro::Ace(_) => "Jupiter Ace",
            Micro::NeedsRom(_) => "",
        }
    }

    /// The lines the program has written to the debug console port since last asked.
    pub(super) fn take_console(&mut self) -> Vec<String> {
        with_core!(self, core => core.hw.take_console(), Vec::new())
    }

    /// For the trainer: memory from $2000 up (all the RAM any of them has), and writing to it.
    pub(super) fn trainer_ram(&mut self) -> Option<Vec<u8>> {
        with_core!(self, core => Some(core.memory()[TRAINER_BASE..].to_vec()), None)
    }

    pub(super) fn trainer_poke(&mut self, address: u16, bytes: &[u8]) {
        with_core!(self, core => {
            for (i, byte) in bytes.iter().enumerate() {
                core.hw.poke(address.wrapping_add(i as u16), *byte);
            }
        }, ())
    }

    pub(super) fn trainer_block(&mut self, addresses: Vec<u16>) {
        with_core!(self, core => core.set_blocked(addresses), ())
    }

    fn catch_up(&mut self) {
        with_core!(self, core => core.catch_up(), ())
    }

    fn picture(&self) -> Option<&[u8]> {
        match self {
            Micro::Zx80(m) => Some(m.core.hw.picture()),
            Micro::Zx81(m) => Some(m.core.hw.picture()),
            Micro::Ace(m) => Some(m.core.hw.picture()),
            Micro::NeedsRom(_) => None,
        }
    }

    fn paused(&self) -> bool {
        match self {
            Micro::Zx80(m) => m.core.paused,
            Micro::Zx81(m) => m.core.paused,
            Micro::Ace(m) => m.core.paused,
            Micro::NeedsRom(_) => false,
        }
    }

    /// Which machine's controls it has, for the control hints.
    fn pad(&self) -> crate::control_hints::Pad {
        use crate::control_hints::Pad;
        match self {
            Micro::Zx80(_) | Micro::NeedsRom(Needs::Zx80) => Pad::Zx80,
            Micro::Zx81(_) | Micro::NeedsRom(Needs::Zx81) => Pad::Zx81,
            _ => Pad::JupiterAce,
        }
    }

    /// Keys pressed on the computer, played on the machine: letters, digits, Space and Enter held as long as they're
    /// held here (as games need), Shift as its SHIFT (and Ctrl or Alt as the Ace's SYMBOL SHIFT), the editing keys
    /// as the machine's own, and other signs typed with the keys that make them.
    fn keys(&mut self, events: &[egui::Event], modifiers: egui::Modifiers, controls: &crate::control_hints::ControlKeys) {
        let ace = matches!(self, Micro::Ace(_));
        let pad = self.pad();
        let named = |name: &str| if ace { crate::jupiter_ace::key(name) } else { crate::zx81::key(name) };
        let shift = named("SHIFT").expect("a key");
        let mut hold: Vec<(Vec<MatrixKey>, bool)> = vec![(vec![shift], modifiers.shift)];
        if ace {
            hold.push((vec![crate::jupiter_ace::SYMBOL_SHIFT], modifiers.ctrl || modifiers.alt));
        }
        let mut typed: Vec<Vec<MatrixKey>> = Vec::new();
        for event in events {
            match event {
                egui::Event::Key { key, pressed, repeat: false, .. } => {
                    // A key given to the joystick (see `control_hints`) presses the key its games read instead.
                    if let Some(control) = controls.control_for(pad, *key) {
                        let digit = match control {
                            "up" => "7",
                            "down" => "6",
                            "left" => "5",
                            "right" => "8",
                            _ => "0",
                        };
                        hold.push((vec![named(digit).expect("a key")], *pressed));
                        continue;
                    }
                    // The editing keys: SHIFT and 0 to rub out, SHIFT and 5 to 8 for the arrows; Esc is BREAK.
                    let with_shift = match key {
                        Key::Backspace => Some("0"),
                        Key::ArrowLeft => Some("5"),
                        Key::ArrowDown => Some("6"),
                        Key::ArrowUp => Some("7"),
                        Key::ArrowRight => Some("8"),
                        Key::Escape if ace => Some("SPACE"),
                        _ => None,
                    };
                    if let Some(name) = with_shift {
                        hold.push((vec![shift, named(name).expect("a key")], *pressed));
                        continue;
                    }
                    let name = match key {
                        Key::Enter => Some(if ace { "ENTER" } else { "NEWLINE" }.to_string()),
                        Key::Space | Key::Escape => Some("SPACE".to_string()),
                        Key::Period if !ace => Some(".".to_string()),
                        _ => key.name().chars().next().filter(|c| key.name().len() == 1 && c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase().to_string()),
                    };
                    if let Some(found) = name.and_then(|n| named(&n)) {
                        hold.push((vec![found], *pressed));
                    }
                }
                egui::Event::Text(text) => {
                    for c in text.chars().filter(|c| !c.is_ascii_alphanumeric() && *c != ' ' && !(*c == '.' && !ace)) {
                        let keys = match self {
                            Micro::Zx80(_) => crate::zx80::keys_for(c),
                            Micro::Zx81(_) => crate::zx81::keys_for(c),
                            Micro::Ace(m) => m.keys_for(c),
                            Micro::NeedsRom(_) => None,
                        };
                        typed.extend(keys);
                    }
                }
                _ => {}
            }
        }
        with_core!(self, core => {
            for (keys, down) in hold {
                core.hold(&keys, down);
            }
            for keys in typed {
                core.type_keys(keys, 4, 2);
            }
        }, ())
    }

    fn release_all(&mut self) {
        with_core!(self, core => core.release_all(), ())
    }
}

/// What the debugger's buttons ask for.
enum Asked {
    CarryOn,
    Step,
    StepOver,
}

impl Limoniter {
    /// Starts the built-in ZX80, ZX81 or Jupiter Ace on this, in place of anything it was running for the same file.
    pub(super) fn run_micro(&mut self, run: Runnable) -> Result<(), String> {
        let machine = Micro::start(&run)?;
        let doc_id = self.docs[self.active].id;
        let target = match run.machine {
            Machine::Zx80 => ZX80_NAME,
            Machine::Zx81 => ZX81_NAME,
            _ => ACE_NAME,
        };
        self.running.sessions.retain(|s| !matches!(s.kind, SessionKind::Micro { doc_id: d, .. } if d == doc_id));
        self.running.sessions.push(Session { target: target.into(), machine: run.machine, file: run.name.clone(), kind: SessionKind::Micro { machine: Box::new(machine), doc_id, run: Box::new(run), texture: None, error: None }, started: Instant::now() });
        Ok(())
    }

    /// The panel for the built-in ZX80, ZX81 or Ace running a program from the file in front.
    pub(super) fn micro_panel(&mut self, ui: &mut Ui) {
        let Some(doc_id) = self.docs.get(self.active).map(|doc| doc.id) else {
            return;
        };
        let Some(index) = self.running.sessions.iter().position(|s| matches!(&s.kind, SessionKind::Micro { doc_id: d, .. } if *d == doc_id)) else {
            return;
        };
        let focus_id = egui::Id::unique("built-in-micro-screen");
        let (mut stop, mut restart, mut chose_rom, mut save_tape) = (false, false, false, false);
        let assembly = self.running.assembled.as_ref().filter(|c| c.doc_id == doc_id).map(|c| c.assembly.clone());
        let doc_index = self.docs.iter().position(|d| d.id == doc_id);
        let breakpoints: Vec<u16> = match (&assembly, doc_index.and_then(|i| self.docs[i].text.code.as_ref())) {
            (Some(assembly), Some(code)) => code.breakpoints.iter().filter_map(|line| assembly.address_of(0, *line)).map(|a| a as u16).collect(),
            _ => Vec::new(),
        };
        let mut code = doc_index.and_then(|i| self.docs[i].text.code.take());
        let mut asked: Option<Asked> = None;
        let ctx = ui.ctx().clone();
        let (keys, choosing) = (self.control_keys.clone(), self.choosing_a_control_key());
        let mut open_controls = false;
        let session = &mut self.running.sessions[index];
        let title = format!("{}: {}", session.machine.name(), session.file);
        let SessionKind::Micro { machine, texture, error, .. } = &mut session.kind else {
            return;
        };
        let pad = machine.pad();
        let hint = keys.hint(pad);
        with_core!(machine.as_mut(), core => core.set_breakpoints(breakpoints), ());
        if error.is_none() {
            machine.catch_up();
        }
        if let Some(picture) = machine.picture() {
            let options = egui::TextureOptions { magnification: egui::TextureFilter::Nearest, minification: egui::TextureFilter::Linear, ..egui::TextureOptions::default() };
            let image = egui::ColorImage::from_rgba_unmultiplied([crate::z80_micro::WIDTH, crate::z80_micro::HEIGHT], picture);
            match texture {
                Some(texture) => texture.set(image, options),
                None => *texture = Some(ctx.load_texture("built-in-micro", image, options)),
            }
        }
        let focused = ctx.memory(|m| m.has_focus(focus_id));
        let name = match machine.as_ref() {
            Micro::Zx80(_) | Micro::NeedsRom(Needs::Zx80) => "ZX80",
            Micro::Zx81(_) | Micro::NeedsRom(Needs::Zx81) => "ZX81",
            _ => "Jupiter Ace",
        };
        Panel::right("micro-panel").default_size(crate::z80_micro::WIDTH as f32 * 2.0 + 16.0).size_range(240.0..=1400.0).resizable(true).frame(egui::Frame::new().fill(theme::field()).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).color(theme::text_dim()).size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    stop = crate::access::called(ui.add(Button::new(RichText::new(crate::i18n::tr("\u{D7}")).color(theme::text_dim())).frame(false)), "Switch it off").on_hover_text(crate::i18n::tr("Switch it off")).clicked();
                    // What the ZX80's own SAVE saved, kept to be saved as a file.
                    if let Micro::Zx80(m) = machine.as_ref()
                        && !m.saved().is_empty()
                    {
                        save_tape = ui.small_button(crate::i18n::tr("Save its tape\u{2026}")).on_hover_text(crate::i18n::tr("Saves the program the ZX80's SAVE saved, as a .o file")).clicked();
                    }
                    if !matches!(machine.as_ref(), Micro::NeedsRom(_)) {
                        restart = ui.small_button(crate::i18n::tr("Restart")).on_hover_text(crate::i18n::tr("Runs the program again from the start")).clicked();
                        open_controls = ui.small_button(crate::i18n::tr("Controls\u{2026}")).on_hover_text(crate::i18n::tr("Which keys are its joystick, and changing them")).clicked();
                        let paused = machine.paused();
                        if ui.small_button(crate::i18n::tr(if paused { "Carry on" } else { "Pause" })).clicked() {
                            if paused {
                                asked = Some(Asked::CarryOn);
                            } else {
                                with_core!(machine.as_mut(), core => core.paused = true, ());
                            }
                        }
                    }
                });
            });
            ui.separator();
            if let Micro::NeedsRom(needs) = machine.as_ref() {
                let needs = *needs;
                ui.add(egui::Label::new(RichText::new(needs.where_to_get()).color(theme::text_dim())).wrap());
                if let Some(problem) = error.as_ref() {
                    ui.add(egui::Label::new(RichText::new(problem.as_str()).color(theme::rainbow_mistake())).wrap());
                } else if let Some(Err(problem)) = crate::micro_roms::rom(needs) {
                    ui.add(egui::Label::new(RichText::new(problem).color(theme::rainbow_mistake())).wrap());
                }
                ui.add_space(6.0);
                if ui.button(crate::i18n::tr("Choose the ROM file\u{2026}")).clicked()
                    && let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tf!("The {} ROM", needs.machine())).add_filter(crate::i18n::tr("ROM"), needs.file_endings()).pick_file()
                {
                    match crate::micro_roms::choose_file(needs, &path) {
                        Ok(_) => chose_rom = true,
                        Err(problem) => *error = Some(problem),
                    }
                }
                return;
            }
            let under = 40.0;
            let room = (ui.available_height() - under).max(120.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let ratio = crate::z80_micro::HEIGHT as f32 / crate::z80_micro::WIDTH as f32;
                let width = ui.available_width().min(room / ratio);
                let size = egui::vec2(width, width * ratio);
                let (rect, _) = ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - width).max(0.0) / 2.0);
                    ui.allocate_exact_size(size, egui::Sense::hover())
                }).inner;
                let response = ui.interact(rect, focus_id, egui::Sense::click());
                crate::access::named(&response, egui::Role::Canvas, &crate::i18n::tf!("The {name}'s screen: typing goes to the {name} while it has the keyboard"));
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
                control_hints_host::screen_hints(ui, &response, focused, &hint);
                let words = match (error.as_ref(), machine.as_ref()) {
                    (Some(problem), _) => problem.clone(),
                    (None, Micro::Zx80(_)) if focused => crate::i18n::tf!("{hint} The picture goes while a program runs, as it did on the ZX80. Click the text to type there again."),
                    (None, Micro::Zx81(_) | Micro::Ace(_)) if focused => crate::i18n::tf!("{hint} Click the text to type there again."),
                    (None, Micro::Ace(m)) if m.core.typing() => crate::i18n::tr("Typing the program in\u{2026}").into(),
                    _ => crate::i18n::tf!("Click the screen to type on the {name}."),
                };
                ui.add(egui::Label::new(RichText::new(words).color(theme::text_faint()).size(12.0)).wrap());
                // Stopped at a breakpoint or a step: the registers, memory and buttons, as for the built-in Spectrum.
                let stopped = match machine.as_ref() {
                    Micro::Zx80(m) => m.core.paused.then(|| (m.core.registers(), m.core.memory(), m.core.memory_before.clone())),
                    Micro::Zx81(m) => m.core.paused.then(|| (m.core.registers(), m.core.memory(), m.core.memory_before.clone())),
                    Micro::Ace(m) => m.core.paused.then(|| (m.core.registers(), m.core.memory(), m.core.memory_before.clone())),
                    Micro::NeedsRom(_) => None,
                };
                if let (Some((registers, memory, before)), None) = (stopped, error.as_ref()) {
                    let stop = crate::remote_debug::Stop::z80(registers);
                    let action = match code.as_mut() {
                        Some(code) => crate::debug_panel::stopped(ui, &stop, Some(&memory), before.as_deref().map(|m| m.as_slice()), assembly.as_deref(), code),
                        None => crate::debug_panel::stopped(ui, &stop, Some(&memory), None, None, &mut crate::text_buffer::CodeMarks::default()),
                    };
                    match action {
                        Some(crate::debug_panel::Action::CarryOn) => asked = Some(Asked::CarryOn),
                        Some(crate::debug_panel::Action::Step) => asked = Some(Asked::Step),
                        Some(crate::debug_panel::Action::StepOver) => asked = Some(Asked::StepOver),
                        None => {}
                    }
                }
            });
        });
        if focused && !choosing {
            let events = ctx.input(|i| i.events.clone());
            let modifiers = ctx.input(|i| i.modifiers);
            machine.keys(&events, modifiers, &keys);
        } else {
            machine.release_all();
        }
        if open_controls {
            self.control_editor = Some(crate::control_hints::Editor::new(pad));
        }
        if let Some(asked) = asked {
            with_core!(machine.as_mut(), core => {
                core.memory_before = Some(std::sync::Arc::new(core.memory()));
                match asked {
                    Asked::CarryOn => core.carry_on(),
                    Asked::Step => core.step(),
                    Asked::StepOver => core.step_over(),
                }
            }, ());
        }
        let paused = machine.paused();
        let pc = with_core!(machine.as_mut(), core => Some(core.cpu.regs.get_pc()), None);
        if !paused && error.is_none() {
            ctx.request_repaint_after(Duration::from_millis(15));
        }
        if let (Some(i), Some(code)) = (doc_index, code.take()) {
            self.docs[i].text.code = Some(code);
        }
        // Where it's stopped, marked in the source, which is brought to that line.
        let stopped_line = if paused { assembly.as_ref().and_then(|a| pc.and_then(|pc| a.line_at(i64::from(pc)))).filter(|(file, _)| *file == 0).map(|(_, line)| line) } else { None };
        if let Some(doc) = self.docs.iter_mut().find(|d| d.id == doc_id)
            && let Some(code) = &mut doc.text.code
            && code.stopped != stopped_line
        {
            code.stopped = stopped_line;
            if let Some(line) = stopped_line {
                doc.text.move_caret(line, 0, false);
            }
        }
        if save_tape
            && let SessionKind::Micro { machine, .. } = &self.running.sessions[index].kind
            && let Micro::Zx80(m) = machine.as_ref()
            && let Some(bytes) = m.saved().last().cloned()
            && let Some(target) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Save the ZX80's tape")).add_filter(crate::i18n::tr("ZX80 program"), &["o"]).set_file_name("program.o").save_file()
            && let Err(e) = crate::codec::write_file(&target, &bytes)
        {
            self.message("Save its tape", crate::i18n::tf!("Couldn't save {}.\n\n{e}", target.display()));
        }
        if restart || chose_rom {
            let SessionKind::Micro { run, .. } = &self.running.sessions[index].kind else {
                return;
            };
            let run = (**run).clone();
            match Micro::start(&run) {
                Ok(fresh) => {
                    if let SessionKind::Micro { machine, error, .. } = &mut self.running.sessions[index].kind {
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

    /// For a screenshot: the built-in ZX80, ZX81 and Ace run on for a number of frames.
    pub(super) fn run_micro_frames_for_screenshot(&mut self, frames: usize) {
        for session in &mut self.running.sessions {
            if let SessionKind::Micro { machine, .. } = &mut session.kind {
                for _ in 0..frames {
                    machine.frame();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    #[test]
    fn a_zx81_listing_runs_on_the_built_in_zx81() {
        let (mut app, ctx) = app_with("");
        app.new_document();
        app.doc().text.set_content(vec!["10 PRINT \"HI\"".into()]);
        app.doc().listing_of = Some((basic::Machine::Zx81, "hi".into()));
        app.run_on_machine();
        let Dialog::Run(boxed) = &app.dialog else { panic!("it asks where") };
        let run_host::RunBox::Where { runnable, .. } = boxed.as_ref() else { panic!("it asks where") };
        assert_eq!((runnable.machine, runnable.format), (Machine::Zx81, Format::P));
        let targets = app.targets_for(runnable);
        assert_eq!(targets[0].1, ZX81_NAME, "the built-in ZX81 is offered first");
        assert!(targets[0].2.is_ok());
        assert!(targets.iter().all(|t| t.1 != ACE_NAME), "and the Ace isn't, for a ZX81 program");
        assert!(targets.iter().any(|t| t.1 == run_host::BUILT_IN_NAME && t.2.is_err()), "the Spectrum says it can't");
        app.close_dialog();
        app.run_on_target(run_host::Target::Zx81);
        let id = app.docs[app.active].id;
        assert!(app.running.sessions.iter().any(|s| s.target == ZX81_NAME && matches!(s.kind, SessionKind::Micro { doc_id, .. } if doc_id == id)));
        // The panel is drawn: running, or asking for the ROM if none has been chosen.
        for _ in 0..3 {
            ctx.run_ui(Default::default(), |ui| app.micro_panel(ui)).textures_delta.clear();
        }
        app.stop_all_sessions();
        assert!(app.running.sessions.is_empty());
    }

    /// File, New as, ZX80 BASIC, and F9: the listing tokenised as a `.o` file and run on the built-in ZX80, which
    /// (with the real ROM, if it's here) loads it, runs it and prints.
    #[test]
    fn a_zx80_listing_from_new_as_runs_on_the_built_in_zx80() {
        let (mut app, ctx) = app_with("");
        let at = app.new_kinds.iter().position(|k| k.name == "ZX80 BASIC").expect("New as has ZX80 BASIC");
        assert_eq!(app.new_kinds[at].group(), "ZX Spectrum, ZX80, ZX81 and Jupiter Ace");
        app.new_of_kind(at);
        assert!(matches!(app.doc().listing_of, Some((basic::Machine::Zx80, _))), "F9 tokenises it for the ZX80");
        app.run_on_machine();
        let Dialog::Run(boxed) = &app.dialog else { panic!("it asks where") };
        let run_host::RunBox::Where { runnable, .. } = boxed.as_ref() else { panic!("it asks where") };
        assert_eq!((runnable.machine, runnable.format), (Machine::Zx80, Format::O));
        assert!(runnable.name.ends_with(".o"));
        let targets = app.targets_for(runnable);
        assert_eq!(targets[0].1, ZX80_NAME, "the built-in ZX80 is offered first");
        assert!(targets[0].2.is_ok());
        assert!(targets.iter().all(|t| t.1 != ZX81_NAME && t.1 != ACE_NAME), "and not the ZX81 or the Ace");
        app.close_dialog();
        let rom = crate::zx80::tests::real_rom_path();
        if let Some(path) = &rom {
            crate::micro_roms::choose(Needs::Zx80, Some(path.clone()));
        }
        app.run_on_target(run_host::Target::Zx80);
        let id = app.docs[app.active].id;
        let session = app.running.sessions.iter_mut().find(|s| s.target == ZX80_NAME && matches!(s.kind, SessionKind::Micro { doc_id, .. } if doc_id == id)).expect("running");
        let SessionKind::Micro { machine, .. } = &mut session.kind else { unreachable!() };
        if rom.is_some() {
            let Micro::Zx80(zx80) = machine.as_mut() else { panic!("the ZX80 started") };
            for _ in 0..300 {
                zx80.core.frame();
            }
            assert_eq!(zx80.screen_text()[9], "HELLO FROM LIMONITER 10", "{:?}", zx80.screen_text());
        } else {
            assert!(matches!(machine.as_ref(), Micro::NeedsRom(Needs::Zx80)), "it asks for the ROM");
        }
        // The panel is drawn: running, or asking for the ROM.
        for _ in 0..3 {
            ctx.run_ui(Default::default(), |ui| app.micro_panel(ui)).textures_delta.clear();
        }
        app.stop_all_sessions();
        assert!(app.running.sessions.is_empty());
    }

    #[test]
    fn zx80_programs_are_told_from_object_files() {
        let o = crate::zx80_program::program(&["10 PRINT 1"]).unwrap();
        assert_eq!(Runnable::from_file("game.o", o.clone()).map(|r| (r.machine, r.format)), Some((Machine::Zx80, Format::O)));
        assert_eq!(Runnable::from_file("game.80", o).map(|r| (r.machine, r.format)), Some((Machine::Zx80, Format::O)));
        assert!(Runnable::from_file("main.o", b"\x7FELF\x02\x01\x01\0\0\0\0\0\0\0\0\0\x01\0".to_vec()).is_none(), "a C compiler's object file isn't run");
    }

    #[test]
    fn without_a_rom_the_machine_waits_for_one() {
        if std::env::var_os("LIMONITER_ACE_ROM").is_some() {
            return;
        }
        let run = Runnable::from_listing(&[": HI 1 . ;"], basic::Machine::JupiterAce, "hi").unwrap();
        assert_eq!((run.machine, run.format), (Machine::JupiterAce, Format::Forth));
        assert!(matches!(Micro::start(&run), Ok(Micro::NeedsRom(Needs::Ace))));
        assert!(Needs::Ace.where_to_get().contains("ace.rom"));
    }

    #[test]
    fn files_say_which_machine_they_are_for() {
        let p = crate::zx81_program::program(&["10 PRINT 1"]).unwrap();
        assert_eq!(Runnable::from_file("game.p", p.clone()).map(|r| r.machine), Some(Machine::Zx81));
        assert_eq!(Runnable::from_file("game.81", p).map(|r| r.machine), Some(Machine::Zx81));
        assert!(Runnable::from_file("prog.p", b"program hello; begin end.".to_vec()).is_none(), "Pascal stays Pascal");
        let ace = crate::jupiter_ace::dictionary_tape("game", 0x3C51, &[1, 2], [0; 4]);
        assert_eq!(Runnable::from_file("game.tap", ace).map(|r| r.machine), Some(Machine::JupiterAce));
        let spectrum = crate::basic::spectrum_tape(&crate::basic::tokenise_spectrum(&["10 PRINT 1"], crate::basic::ZxDialect::Spectrum).unwrap(), "s", None);
        assert_eq!(Runnable::from_file("s.tap", spectrum).map(|r| r.machine), Some(Machine::Spectrum48));
        assert_eq!(Runnable::from_file("words.fth", b": HI ;".to_vec()).map(|r| r.machine), Some(Machine::JupiterAce));
    }

    #[test]
    fn z80_source_for_the_zx81_becomes_a_p_file() {
        let text = crate::zx81_program::STARTER;
        let assembled = crate::assemblers::assemble_own(crate::assemblers::Cpu::Z80, std::path::Path::new("hello.asm"), text);
        let run = super::super::asm_host::runnable_from(crate::assemblers::Cpu::Z80, &assembled, "hello", Default::default()).unwrap();
        assert_eq!((run.machine, run.format, run.name.as_str()), (Machine::Zx81, Format::P, "hello.p"));
        let listing = crate::basic_extra::list_zx81(&run.bytes);
        assert_eq!(listing.lines.last().map(String::as_str), Some("2 RAND USR 16514"));
        assert_eq!(super::super::asm_host::machine_z80(["; machine: ZX81"]), Some(Machine::Zx81));
        // Without the comment, code for a Spectrum still goes on a Spectrum tape.
        let spectrum = crate::assemblers::assemble_own(crate::assemblers::Cpu::Z80, std::path::Path::new("s.asm"), "        org 32768\n        ret\n");
        assert_eq!(super::super::asm_host::runnable_from(crate::assemblers::Cpu::Z80, &spectrum, "s", Default::default()).unwrap().machine, Machine::Spectrum48);
    }
}
