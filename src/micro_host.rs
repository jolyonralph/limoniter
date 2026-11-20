//! The built-in ZX81 and Jupiter Ace in the window: a panel beside the file a program was run from, as the built-in
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
    Zx81(Box<crate::zx81::Zx81>),
    Ace(Box<crate::jupiter_ace::Ace>),
    NeedsRom(Needs),
}

/// The same for each machine's core.
macro_rules! with_core {
    ($micro:expr, $core:ident => $then:expr, $otherwise:expr) => {
        match $micro {
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

pub(super) const ZX81_NAME: &str = "Built-in ZX81";
pub(super) const ACE_NAME: &str = "Built-in Jupiter Ace";

/// Which ROM a machine needs.
pub(super) fn needs(machine: Machine) -> Option<Needs> {
    match machine {
        Machine::Zx81 => Some(Needs::Zx81),
        Machine::JupiterAce => Some(Needs::Ace),
        _ => None,
    }
}

/// Whether the built-in machine takes this; if not, why not.
pub(super) fn takes(needs: Needs, run: &Runnable) -> Result<(), String> {
    match (needs, run.machine, run.format) {
        (Needs::Zx81, Machine::Zx81, Format::P | Format::Tzx) => Ok(()),
        (Needs::Ace, Machine::JupiterAce, Format::Tap | Format::Tzx | Format::Forth) => Ok(()),
        (Needs::Zx81, ..) => Err(crate::i18n::tr("The built-in ZX81 runs ZX81 programs (.p files and ZX81 tapes).").into()),
        (Needs::Ace, ..) => Err(crate::i18n::tr("The built-in Jupiter Ace runs Ace tapes and Forth source.").into()),
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
        }
    }

    pub(super) fn frame(&mut self) {
        with_core!(self, core => core.frame(), ())
    }

    fn catch_up(&mut self) {
        with_core!(self, core => core.catch_up(), ())
    }

    fn picture(&self) -> Option<&[u8]> {
        match self {
            Micro::Zx81(m) => Some(m.core.hw.picture()),
            Micro::Ace(m) => Some(m.core.hw.picture()),
            Micro::NeedsRom(_) => None,
        }
    }

    fn paused(&self) -> bool {
        match self {
            Micro::Zx81(m) => m.core.paused,
            Micro::Ace(m) => m.core.paused,
            Micro::NeedsRom(_) => false,
        }
    }

    /// Keys pressed on the computer, played on the machine: letters, digits, Space and Enter held as long as they're
    /// held here (as games need), Shift as its SHIFT (and Ctrl or Alt as the Ace's SYMBOL SHIFT), the editing keys
    /// as the machine's own, and other signs typed with the keys that make them.
    fn keys(&mut self, events: &[egui::Event], modifiers: egui::Modifiers) {
        let ace = matches!(self, Micro::Ace(_));
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
    /// Starts the built-in ZX81 or Jupiter Ace on this, in place of anything it was running for the same file.
    pub(super) fn run_micro(&mut self, run: Runnable) -> Result<(), String> {
        let machine = Micro::start(&run)?;
        let doc_id = self.docs[self.active].id;
        let target = if run.machine == Machine::Zx81 { ZX81_NAME } else { ACE_NAME };
        self.running.sessions.retain(|s| !matches!(s.kind, SessionKind::Micro { doc_id: d, .. } if d == doc_id));
        self.running.sessions.push(Session { target: target.into(), machine: run.machine, file: run.name.clone(), kind: SessionKind::Micro { machine: Box::new(machine), doc_id, run: Box::new(run), texture: None, error: None }, started: Instant::now() });
        Ok(())
    }

    /// The panel for the built-in ZX81 or Ace running a program from the file in front.
    pub(super) fn micro_panel(&mut self, ui: &mut Ui) {
        let Some(doc_id) = self.docs.get(self.active).map(|doc| doc.id) else {
            return;
        };
        let Some(index) = self.running.sessions.iter().position(|s| matches!(&s.kind, SessionKind::Micro { doc_id: d, .. } if *d == doc_id)) else {
            return;
        };
        let focus_id = egui::Id::unique("built-in-micro-screen");
        let (mut stop, mut restart, mut chose_rom) = (false, false, false);
        let assembly = self.running.assembled.as_ref().filter(|c| c.doc_id == doc_id).map(|c| c.assembly.clone());
        let doc_index = self.docs.iter().position(|d| d.id == doc_id);
        let breakpoints: Vec<u16> = match (&assembly, doc_index.and_then(|i| self.docs[i].text.code.as_ref())) {
            (Some(assembly), Some(code)) => code.breakpoints.iter().filter_map(|line| assembly.address_of(0, *line)).map(|a| a as u16).collect(),
            _ => Vec::new(),
        };
        let mut code = doc_index.and_then(|i| self.docs[i].text.code.take());
        let mut asked: Option<Asked> = None;
        let ctx = ui.ctx().clone();
        let session = &mut self.running.sessions[index];
        let title = format!("{}: {}", session.machine.name(), session.file);
        let SessionKind::Micro { machine, texture, error, .. } = &mut session.kind else {
            return;
        };
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
            Micro::Zx81(_) | Micro::NeedsRom(Needs::Zx81) => "ZX81",
            _ => "Jupiter Ace",
        };
        Panel::right("micro-panel").default_size(crate::z80_micro::WIDTH as f32 * 2.0 + 16.0).size_range(240.0..=1400.0).resizable(true).frame(egui::Frame::new().fill(theme::field()).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).color(theme::text_dim()).size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    stop = crate::access::called(ui.add(Button::new(RichText::new(crate::i18n::tr("\u{D7}")).color(theme::text_dim())).frame(false)), "Switch it off").on_hover_text(crate::i18n::tr("Switch it off")).clicked();
                    if !matches!(machine.as_ref(), Micro::NeedsRom(_)) {
                        restart = ui.small_button(crate::i18n::tr("Restart")).on_hover_text(crate::i18n::tr("Runs the program again from the start")).clicked();
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
                    && let Some(path) = rfd::FileDialog::new().set_title(crate::i18n::tf!("The {} ROM", needs.machine())).add_filter(crate::i18n::tr("ROM"), &["rom", "bin"]).pick_file()
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
                let words = match (error.as_ref(), machine.as_ref()) {
                    (Some(problem), _) => problem.clone(),
                    (None, Micro::Zx81(_)) if focused => crate::i18n::tr("Typing goes to the ZX81, whose keys type keywords as its own do: Shift is its SHIFT, Backspace rubs out, the arrows move the cursor, and Esc or Space is BREAK. Click the text to type there again.").into(),
                    (None, Micro::Ace(_)) if focused => crate::i18n::tr("Typing goes to the Jupiter Ace: Shift is its SHIFT, Ctrl or Alt its SYMBOL SHIFT, Backspace deletes, the arrows move the cursor and Esc is BREAK. Click the text to type there again.").into(),
                    (None, Micro::Ace(m)) if m.core.typing() => crate::i18n::tr("Typing the program in\u{2026}").into(),
                    _ => crate::i18n::tf!("Click the screen to type on the {name}."),
                };
                ui.add(egui::Label::new(RichText::new(words).color(theme::text_faint()).size(12.0)).wrap());
                // Stopped at a breakpoint or a step: the registers, memory and buttons, as for the built-in Spectrum.
                let stopped = match machine.as_ref() {
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
        if focused {
            let events = ctx.input(|i| i.events.clone());
            let modifiers = ctx.input(|i| i.modifiers);
            machine.keys(&events, modifiers);
        } else {
            machine.release_all();
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

    /// For a screenshot: the built-in ZX81 and Ace run on for a number of frames.
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
