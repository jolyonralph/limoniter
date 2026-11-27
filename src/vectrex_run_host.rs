//! The window's side of the built-in Vectrex: its panel beside the source or cartridge it's running, with the vector
//! screen (which takes the keyboard as the controller when clicked), and the debugger when it stops at a source
//! line's breakpoint. Without the BIOS chosen, the panel asks for it, or runs the cartridge without one.

use super::*;
use crate::micro_roms::Needs;
use crate::running::Runnable;
use crate::vectrex_machine::Vectrex;
use crate::vectrex_screen::{HEIGHT, WIDTH};
use run_host::{BUILT_IN_VECTREX_NAME, Session, SessionKind};
use crate::control_hints::Pad;
use std::time::Instant;

/// The machine in the panel: running, or waiting for its BIOS.
pub(super) enum VectrexRun {
    Running(Box<Vectrex>),
    NeedsBios,
}

impl VectrexRun {
    /// The cartridge started, with the BIOS if it's been chosen (or without, if asked); else it waits for one.
    fn start(run: &Runnable, speaker: Option<crate::speaker::Speaker>, without_bios: bool) -> Result<VectrexRun, String> {
        Vectrex::takes(run).map_err(|why| crate::i18n::tf!("The built-in Vectrex {why}"))?;
        let bios = match crate::micro_roms::rom(Needs::Vectrex) {
            _ if without_bios => None,
            Some(Ok(bios)) => Some(bios),
            // A BIOS that's gone, or changed, is asked for again; the panel says why.
            _ => return Ok(VectrexRun::NeedsBios),
        };
        Ok(VectrexRun::Running(Box::new(Vectrex::start(run, bios, speaker)?)))
    }
}

impl Limoniter {
    /// Starts the built-in Vectrex on a cartridge, in place of anything it was running for the same file.
    pub(super) fn run_built_in_vectrex(&mut self, run: Runnable) -> Result<(), String> {
        let machine = VectrexRun::start(&run, self.speaker(), false)?;
        let doc_id = self.docs[self.active].id;
        self.running.sessions.retain(|s| !matches!(s.kind, SessionKind::Vectrex { doc_id: d, .. } if d == doc_id));
        self.running.sessions.push(Session { target: BUILT_IN_VECTREX_NAME.into(), machine: run.machine, file: run.name.clone(), kind: SessionKind::Vectrex { machine: Box::new(machine), doc_id, run: Box::new(run), texture: None, error: None }, started: Instant::now() });
        Ok(())
    }

    /// The built-in Vectrex for the file in front, started again without the BIOS.
    pub(super) fn vectrex_without_bios(&mut self) {
        let Some(doc_id) = self.docs.get(self.active).map(|doc| doc.id) else {
            return;
        };
        let speaker = self.speaker();
        if let Some(session) = self.running.sessions.iter_mut().find(|s| matches!(&s.kind, SessionKind::Vectrex { doc_id: d, .. } if *d == doc_id))
            && let SessionKind::Vectrex { machine, run, error, .. } = &mut session.kind
        {
            match VectrexRun::start(run, speaker, true) {
                Ok(fresh) => {
                    **machine = fresh;
                    *error = None;
                }
                Err(problem) => *error = Some(problem),
            }
        }
    }

    /// The built-in Vectrex's panel, beside the file it's running.
    pub(super) fn vectrex_panel(&mut self, ui: &mut Ui) {
        let Some(doc_id) = self.docs.get(self.active).map(|doc| doc.id) else {
            return;
        };
        let Some(index) = self.running.sessions.iter().position(|s| matches!(&s.kind, SessionKind::Vectrex { doc_id: d, .. } if *d == doc_id)) else {
            return;
        };
        let focus_id = egui::Id::unique("built-in-vectrex");
        let (mut stop, mut restart, mut chose_bios, mut without_bios) = (false, false, false, false);
        // A cartridge assembled from this document can be debugged: its breakpoints set at their lines' addresses.
        let assembly = self.running.assembled.as_ref().filter(|c| c.doc_id == doc_id).map(|c| c.assembly.clone());
        let doc_index = self.docs.iter().position(|d| d.id == doc_id);
        let mut code = doc_index.and_then(|i| self.docs[i].text.code.take());
        let breakpoints: Vec<u16> = match (&assembly, &code) {
            (Some(assembly), Some(code)) => code.breakpoints.iter().filter_map(|line| assembly.address_of(0, *line)).map(|a| a as u16).collect(),
            _ => Vec::new(),
        };
        let line_texts: std::collections::BTreeMap<usize, String> = match (doc_index, &code) {
            (Some(i), Some(code)) => code.breakpoints.iter().filter(|l| **l < self.docs[i].text.line_count()).map(|l| (*l, self.docs[i].text.line(*l).to_string())).collect(),
            _ => Default::default(),
        };
        let mut debug: Option<crate::debug_panel::Action> = None;
        let ctx = ui.ctx().clone();
        let (keys, hint, choosing) = (self.control_keys.clone(), self.control_keys.hint(Pad::Vectrex), self.choosing_a_control_key());
        let mut open_controls = false;
        let session = &mut self.running.sessions[index];
        let title = format!("{}: {}", session.machine.name(), session.file);
        let SessionKind::Vectrex { machine: run_state, texture, error, .. } = &mut session.kind else {
            return;
        };
        if let VectrexRun::Running(machine) = run_state.as_mut() {
            machine.set_breakpoints(breakpoints);
            if let (Some(assembly), Some(code)) = (&assembly, &mut code) {
                machine.set_conditions(crate::debug_panel::conditions_by_address(code, assembly), Some(std::sync::Arc::new(assembly.symbols.clone())));
                machine.set_watchpoints(crate::debug_panel::watchpoints_for(code, Some(&assembly.symbols)));
                machine.set_hit_rules(crate::debug_panel::hit_rules_by_address(code, assembly));
                crate::debug_panel::show_hit_counts(code, assembly, &|a| machine.hit_count(a));
            }
            if error.is_none()
                && let Err(problem) = machine.catch_up()
            {
                *error = Some(problem);
            }
            // Drawn smoothly: the glow is soft, and the picture is scaled to the panel.
            let options = egui::TextureOptions { magnification: egui::TextureFilter::Linear, minification: egui::TextureFilter::Linear, ..egui::TextureOptions::default() };
            let image = egui::ColorImage::from_rgba_unmultiplied([WIDTH, HEIGHT], machine.picture());
            match texture {
                Some(texture) => texture.set(image, options),
                None => *texture = Some(ctx.load_texture("built-in-vectrex", image, options)),
            }
        }
        let focused = ctx.memory(|m| m.has_focus(focus_id));
        let ratio = HEIGHT as f32 / WIDTH as f32;
        Panel::right("vectrex-panel").default_size(400.0).size_range(240.0..=1400.0).resizable(true).frame(egui::Frame::new().fill(theme::field()).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).color(theme::text_dim()).size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    stop = crate::access::called(ui.add(Button::new(RichText::new(crate::i18n::tr("\u{D7}")).color(theme::text_dim())).frame(false)), "Switch it off").on_hover_text(crate::i18n::tr("Switch it off")).clicked();
                    if let VectrexRun::Running(machine) = run_state.as_mut() {
                        restart = ui.small_button(crate::i18n::tr("Restart")).on_hover_text(crate::i18n::tr("Runs the cartridge again from the start")).clicked();
                        open_controls = ui.small_button(crate::i18n::tr("Controls\u{2026}")).on_hover_text(crate::i18n::tr("Which keys are its joystick and buttons, and changing them")).clicked();
                        let pause = if machine.paused { "Carry on" } else { "Pause" };
                        if ui.small_button(crate::i18n::tr(pause)).clicked() {
                            if machine.paused {
                                machine.carry_on();
                            } else {
                                machine.paused = true;
                            }
                        }
                        if machine.has_speaker() {
                            let sound = if machine.muted { "Sound on" } else { "Mute" };
                            if ui.small_button(crate::i18n::tr(sound)).clicked() {
                                let muted = !machine.muted;
                                machine.set_muted(muted);
                            }
                        }
                    }
                });
            });
            ui.separator();
            let VectrexRun::Running(machine) = run_state.as_mut() else {
                ui.add(egui::Label::new(RichText::new(Needs::Vectrex.where_to_get()).color(theme::text_dim())).wrap());
                if let Some(problem) = error.as_ref() {
                    ui.add(egui::Label::new(RichText::new(problem.as_str()).color(theme::rainbow_mistake())).wrap());
                } else if let Some(Err(problem)) = crate::micro_roms::rom(Needs::Vectrex) {
                    ui.add(egui::Label::new(RichText::new(problem).color(theme::rainbow_mistake())).wrap());
                }
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button(crate::i18n::tr("Choose the BIOS file\u{2026}")).clicked()
                        && let Some(path) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("The Vectrex BIOS")).add_filter(crate::i18n::tr("ROM"), crate::micro_roms::Needs::Vectrex.file_endings()).pick_file()
                    {
                        match crate::micro_roms::choose_file(Needs::Vectrex, &path) {
                            Ok(_) => chose_bios = true,
                            Err(problem) => *error = Some(problem),
                        }
                    }
                    without_bios = ui.button(crate::i18n::tr("Run without the BIOS")).on_hover_text(crate::i18n::tr("Starts the cartridge where its header ends, with nothing in the BIOS's place: for cartridges that drive the hardware themselves")).clicked();
                });
                return;
            };
            let room = (ui.available_height() - 60.0).max(120.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let width = ui.available_width().min(room / ratio);
                let screen = egui::vec2(width, width * ratio);
                let (rect, _) = ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - width).max(0.0) / 2.0);
                    ui.allocate_exact_size(screen, egui::Sense::hover())
                }).inner;
                let response = ui.interact(rect, focus_id, egui::Sense::click());
                crate::access::named(&response, egui::Role::Canvas, "The Vectrex's screen: the keys go to its controller while it has the keyboard");
                ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);
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
                let words = match error {
                    Some(problem) => problem.clone(),
                    None if focused => crate::i18n::tf!("{hint} Click the text to type there again."),
                    None => crate::i18n::tr("Click the screen to play with the keyboard.").into(),
                };
                ui.add(egui::Label::new(RichText::new(words).color(theme::text_faint()).size(12.0)).wrap());
                if !machine.has_bios() {
                    ui.add(egui::Label::new(RichText::new(crate::i18n::tr("Running without the BIOS: the cartridge starts where its header ends, and can't call the BIOS's routines.")).color(theme::text_faint()).size(12.0)).wrap());
                }
                if machine.paused && error.is_none() && machine.stopped.is_some() {
                    let mut stop = machine.stop_words();
                    stop.reason = machine.stop_reason.clone();
                    stop.counts = Some((machine.frames, machine.instructions));
                    let memory = machine.memory();
                    match code.as_mut() {
                        Some(code) => debug = crate::debug_panel::stopped(ui, &stop, Some(&memory), machine.memory_before.as_deref().map(|m| m.as_slice()), assembly.as_deref(), code),
                        None => {
                            let mut scratch = crate::text_buffer::CodeMarks::default();
                            debug = crate::debug_panel::stopped(ui, &stop, Some(&memory), None, None, &mut scratch);
                        }
                    }
                }
                if let (Some(code), true) = (code.as_mut(), assembly.is_some()) {
                    crate::debug_panel::breakpoints(ui, code, &|line| line_texts.get(&line).cloned().unwrap_or_default(), &crate::debug_panel::Support::writes_only(crate::debug_panel::reads_unseen("6809")));
                }
            });
        });
        let mut stopped_line = None;
        if let VectrexRun::Running(machine) = run_state.as_mut() {
            if focused && !choosing {
                controller_keys(machine, &ctx, &keys);
            } else {
                machine.release_all();
            }
            if let Some(action) = debug {
                machine.memory_before = Some(std::sync::Arc::new(machine.memory()));
                let done = match action {
                    crate::debug_panel::Action::CarryOn => {
                        machine.carry_on();
                        Ok(())
                    }
                    crate::debug_panel::Action::Step => machine.step(),
                    crate::debug_panel::Action::StepOver => machine.step_over(),
                };
                if let Err(problem) = done {
                    *error = Some(problem);
                }
            }
            // Where it's stopped, marked in the source, which is brought to that line when it first stops there.
            stopped_line = match (machine.paused && machine.stopped.is_some(), &assembly) {
                (true, Some(a)) => a.line_at(i64::from(machine.cpu.pc)).filter(|(file, _)| *file == 0).map(|(_, line)| line),
                _ => None,
            };
            if !machine.paused && error.is_none() {
                ctx.request_repaint_after(Duration::from_millis(15));
            }
        }
        if let (Some(i), Some(code)) = (doc_index, code.take()) {
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
        if open_controls {
            self.control_editor = Some(crate::control_hints::Editor::new(Pad::Vectrex));
        }
        if restart || chose_bios || without_bios {
            let SessionKind::Vectrex { run, machine, .. } = &self.running.sessions[index].kind else {
                return;
            };
            let muted = matches!(machine.as_ref(), VectrexRun::Running(m) if m.muted);
            // A restart keeps to how it was running: with the BIOS, or without.
            let without = without_bios || (restart && matches!(machine.as_ref(), VectrexRun::Running(m) if !m.has_bios()));
            let run = (**run).clone();
            match VectrexRun::start(&run, self.speaker(), without) {
                Ok(mut fresh) => {
                    if let VectrexRun::Running(m) = &mut fresh {
                        m.set_muted(muted);
                    }
                    if let SessionKind::Vectrex { machine, error, .. } = &mut self.running.sessions[index].kind {
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
}

/// The keys held, as the first controller: the arrow keys its joystick (all the way over, as a digital joystick
/// would be), and A, S, D and F its buttons 1 to 4, as vecx and ParaJVE have them, unless they've been moved (see
/// `control_hints`).
fn controller_keys(machine: &mut Vectrex, ctx: &egui::Context, keys: &crate::control_hints::ControlKeys) {
    let held = |i: &egui::InputState, control: &str| keys.key(Pad::Vectrex, control).is_some_and(|k| i.key_down(k));
    let (left, right, up, down, buttons) = ctx.input(|i| (held(i, "left"), held(i, "right"), held(i, "up"), held(i, "down"), ["button1", "button2", "button3", "button4"].map(|b| held(i, b))));
    let axis = |less: bool, more: bool| match (less, more) {
        (true, false) => -127,
        (false, true) => 127,
        _ => 0,
    };
    let controls = machine.controls();
    controls.sticks[0] = [axis(left, right), axis(down, up)];
    controls.buttons[0] = buttons;
}

#[cfg(test)]
mod tests {
    use super::super::tests::app_with;
    use super::*;

    /// File, New as, Vectrex cartridge, and F9: assembled to a cartridge, the built-in Vectrex is offered first,
    /// and it runs (with the BIOS, if it's here) or asks for the BIOS.
    #[test]
    fn the_vectrex_starter_runs_on_the_built_in_vectrex() {
        let (mut app, ctx) = app_with("");
        let at = app.new_kinds.iter().position(|k| k.name == "Vectrex cartridge (6809)").expect("New as has the Vectrex cartridge");
        app.new_of_kind(at);
        app.run_on_machine();
        let Dialog::Run(boxed) = &app.dialog else { panic!("it asks where") };
        let run_host::RunBox::Where { runnable, .. } = boxed.as_ref() else { panic!("it asks where") };
        assert_eq!(runnable.machine, crate::running::Machine::Vectrex);
        let targets = app.targets_for(runnable);
        assert_eq!(targets[0].1, BUILT_IN_VECTREX_NAME, "the built-in Vectrex is offered first");
        assert!(targets[0].2.is_ok());
        assert!(targets.iter().any(|t| t.1 == run_host::BUILT_IN_2600_NAME && t.2.is_err()), "the 2600 says it can't");
        app.close_dialog();
        app.run_on_target(run_host::Target::BuiltInVectrex);
        let id = app.docs[app.active].id;
        let session = app.running.sessions.iter_mut().find(|s| s.target == BUILT_IN_VECTREX_NAME && matches!(s.kind, SessionKind::Vectrex { doc_id, .. } if doc_id == id)).expect("running");
        let SessionKind::Vectrex { machine, .. } = &mut session.kind else { unreachable!() };
        match machine.as_mut() {
            VectrexRun::Running(vectrex) => {
                assert!(vectrex.has_bios());
                for _ in 0..50 {
                    vectrex.frame().unwrap();
                }
            }
            VectrexRun::NeedsBios => assert!(crate::micro_roms::rom(Needs::Vectrex).is_none(), "it asks for the BIOS only when there's none"),
        }
        for _ in 0..3 {
            ctx.run_ui(Default::default(), |ui| app.vectrex_panel(ui)).textures_delta.clear();
        }
        // Without the BIOS, the starter's first call to it is said to need it.
        app.vectrex_without_bios();
        app.run_frames_for_screenshot(2);
        let session = app.running.sessions.iter().find(|s| s.target == BUILT_IN_VECTREX_NAME).expect("still there");
        let SessionKind::Vectrex { error, .. } = &session.kind else { unreachable!() };
        assert!(error.as_deref().is_some_and(|e| e.contains("Wait_Recal")), "{error:?}");
        for _ in 0..2 {
            ctx.run_ui(Default::default(), |ui| app.vectrex_panel(ui)).textures_delta.clear();
        }
        app.stop_all_sessions();
        assert!(app.running.sessions.is_empty());
    }

    /// A cartridge that drives the hardware itself runs without the BIOS, and draws.
    #[test]
    fn a_cartridge_needing_no_bios_runs_without_it() {
        let (mut app, ctx) = app_with("");
        let at = app.new_kinds.iter().position(|k| k.name == "Vectrex cartridge (6809)").expect("New as has the Vectrex cartridge");
        app.new_of_kind(at);
        app.doc().text.set_content(include_str!("vectrex_machine/square.asm").lines().map(String::from).collect());
        app.run_on_target(run_host::Target::BuiltInVectrex);
        app.vectrex_without_bios();
        app.run_frames_for_screenshot(5);
        let session = app.running.sessions.iter_mut().find(|s| s.target == BUILT_IN_VECTREX_NAME).expect("running");
        let SessionKind::Vectrex { machine, error, .. } = &mut session.kind else { unreachable!() };
        assert!(error.is_none(), "{error:?}");
        let VectrexRun::Running(vectrex) = machine.as_mut() else { panic!("running without the BIOS") };
        assert!(!vectrex.has_bios() && vectrex.frames >= 5);
        assert!(vectrex.picture().chunks(4).any(|p| p[2] > 200), "something drawn");
        ctx.run_ui(Default::default(), |ui| app.vectrex_panel(ui)).textures_delta.clear();
        app.stop_all_sessions();
    }
}
