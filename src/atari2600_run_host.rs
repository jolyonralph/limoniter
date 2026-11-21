//! The window's side of the built-in Atari 2600: its panel beside the source or cartridge it's running, with the
//! screen (which takes the keyboard as the joysticks when clicked), the console's switches, and the debugger
//! when it stops at a source line's breakpoint.

use super::*;
use crate::atari2600_machine::Atari2600;
use run_host::{Session, SessionKind, BUILT_IN_2600_NAME};
use crate::control_hints::{ControlKeys, Pad};
use std::time::Instant;

impl Limoniter {
    /// Starts the built-in 2600 on a cartridge, in place of anything it was running for the same file.
    pub(super) fn run_built_in_2600(&mut self, run: crate::running::Runnable) -> Result<(), String> {
        let machine = Atari2600::start(&run, self.speaker())?;
        let doc_id = self.docs[self.active].id;
        self.running.sessions.retain(|s| !matches!(s.kind, SessionKind::Atari2600 { doc_id: d, .. } if d == doc_id));
        self.running.sessions.push(Session { target: BUILT_IN_2600_NAME.into(), machine: run.machine, file: run.name.clone(), kind: SessionKind::Atari2600 { machine: Box::new(machine), doc_id, run: Box::new(run), texture: None, error: None }, started: Instant::now() });
        Ok(())
    }

    /// The built-in 2600's panel, beside the file it's running.
    pub(super) fn atari2600_panel(&mut self, ui: &mut Ui) {
        let Some(doc_id) = self.docs.get(self.active).map(|doc| doc.id) else {
            return;
        };
        let Some(index) = self.running.sessions.iter().position(|s| matches!(&s.kind, SessionKind::Atari2600 { doc_id: d, .. } if *d == doc_id)) else {
            return;
        };
        let focus_id = egui::Id::unique("built-in-atari-2600");
        let (mut stop, mut restart) = (false, false);
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
        let (keys, hint, choosing) = (self.control_keys.clone(), self.control_keys.hint(Pad::Atari2600), self.choosing_a_control_key());
        let mut open_controls = false;
        let session = &mut self.running.sessions[index];
        let title = format!("{}: {}", session.machine.name(), session.file);
        let SessionKind::Atari2600 { machine, texture, error, .. } = &mut session.kind else {
            return;
        };
        machine.set_breakpoints(breakpoints);
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
        let size = machine.size();
        let image = egui::ColorImage::from_rgba_unmultiplied(size, machine.picture());
        match texture {
            Some(texture) => texture.set(image, options),
            None => *texture = Some(ctx.load_texture("built-in-atari-2600", image, options)),
        }
        let focused = ctx.memory(|m| m.has_focus(focus_id));
        // A 2600's pixels are wider than its lines are tall.
        let ratio = size[1] as f32 / (size[0] as f32 * machine.tv.aspect());
        Panel::right("atari-2600-panel").default_size(480.0).size_range(240.0..=1400.0).resizable(true).frame(egui::Frame::new().fill(theme::field()).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).color(theme::text_dim()).size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    stop = crate::access::called(ui.add(Button::new(RichText::new(crate::i18n::tr("\u{D7}")).color(theme::text_dim())).frame(false)), "Switch it off").on_hover_text(crate::i18n::tr("Switch it off")).clicked();
                    restart = ui.small_button(crate::i18n::tr("Restart")).on_hover_text(crate::i18n::tr("Runs the cartridge again from the start")).clicked();
                    open_controls = ui.small_button(crate::i18n::tr("Controls\u{2026}")).on_hover_text(crate::i18n::tr("Which keys are its joysticks and switches, and changing them")).clicked();
                    let pause = if machine.paused { "Carry on" } else { "Pause" };
                    if ui.small_button(pause).clicked() {
                        if machine.paused {
                            machine.carry_on();
                        } else {
                            machine.paused = true;
                        }
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
            // The console's switches: Reset and Select held while the button is, the others switched.
            ui.horizontal_wrapped(|ui| {
                let reset = ui.add(Button::new(crate::i18n::tr("Game Reset")).small().sense(egui::Sense::click_and_drag())).on_hover_text(crate::i18n::tr("The console's Game Reset switch, held while the button is (or Enter, while the screen has the keyboard)"));
                let select = ui.add(Button::new(crate::i18n::tr("Game Select")).small().sense(egui::Sense::click_and_drag())).on_hover_text(crate::i18n::tr("The console's Game Select switch, held while the button is (or Backspace, while the screen has the keyboard)"));
                let controls = machine.controls();
                if !focused {
                    controls.reset = reset.is_pointer_button_down_on();
                    controls.select = select.is_pointer_button_down_on();
                } else {
                    controls.reset |= reset.is_pointer_button_down_on();
                    controls.select |= select.is_pointer_button_down_on();
                }
                let tv = if controls.colour { crate::i18n::tr("Colour") } else { crate::i18n::tr("Black and white") };
                if ui.small_button(tv).on_hover_text(crate::i18n::tr("The TV type switch: colour, or black and white")).clicked() {
                    controls.colour = !controls.colour;
                }
                for (player, name) in [(0, crate::i18n::tr("Left difficulty")), (1, crate::i18n::tr("Right difficulty"))] {
                    let letter = if controls.difficulty_a[player] { "A" } else { "B" };
                    if ui.small_button(format!("{name} {letter}")).on_hover_text(crate::i18n::tr("A difficulty switch: A is the harder")).clicked() {
                        controls.difficulty_a[player] = !controls.difficulty_a[player];
                    }
                }
            });
            ui.separator();
            let room = (ui.available_height() - 60.0).max(120.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let width = ui.available_width().min(room / ratio);
                let screen = egui::vec2(width, width * ratio);
                let (rect, _) = ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - width).max(0.0) / 2.0);
                    ui.allocate_exact_size(screen, egui::Sense::hover())
                }).inner;
                let response = ui.interact(rect, focus_id, egui::Sense::click());
                crate::access::named(&response, egui::Role::Canvas, "The 2600's screen: the keys go to its joysticks while it has the keyboard");
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
                if machine.paused && error.is_none() && machine.stopped.is_some() {
                    let mut stop = machine.stop_words();
                    stop.reason = machine.stop_reason.clone();
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
                    crate::debug_panel::breakpoints(ui, code, &|line| line_texts.get(&line).cloned().unwrap_or_default(), true, true);
                }
            });
        });
        if focused && !choosing {
            let events = ctx.input(|i| i.events.clone());
            joystick_keys(machine, &events, &keys);
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
        let stopped_line = match (machine.paused && machine.stopped.is_some(), &assembly) {
            (true, Some(a)) => a.line_at(i64::from(machine.cpu().pc)).filter(|(file, _)| *file == 0).map(|(_, line)| line),
            _ => None,
        };
        if !machine.paused && error.is_none() {
            ctx.request_repaint_after(Duration::from_millis(15));
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
            self.control_editor = Some(crate::control_hints::Editor::new(Pad::Atari2600));
        }
        if restart {
            let SessionKind::Atari2600 { run, machine, .. } = &self.running.sessions[index].kind else {
                return;
            };
            let (run, muted) = ((**run).clone(), machine.muted);
            match Atari2600::start(&run, self.speaker()) {
                Ok(mut fresh) => {
                    fresh.set_muted(muted);
                    if let SessionKind::Atari2600 { machine, error, .. } = &mut self.running.sessions[index].kind {
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

/// Keys pressed on the computer, as the joysticks and the console's Reset and Select: the arrow keys and Space the
/// left joystick, Y, G, H and J with F the right one, as Stella has them, unless they've been moved (see
/// `control_hints`).
fn joystick_keys(machine: &mut Atari2600, events: &[egui::Event], keys: &ControlKeys) {
    for event in events {
        let egui::Event::Key { key, pressed, repeat: false, .. } = event else {
            continue;
        };
        let pressed = *pressed;
        let Some(control) = keys.control_for(Pad::Atari2600, *key) else {
            continue;
        };
        let controls = machine.controls();
        match control {
            "up" => controls.sticks[0][0] = pressed,
            "down" => controls.sticks[0][1] = pressed,
            "left" => controls.sticks[0][2] = pressed,
            "right" => controls.sticks[0][3] = pressed,
            "up2" => controls.sticks[1][0] = pressed,
            "down2" => controls.sticks[1][1] = pressed,
            "left2" => controls.sticks[1][2] = pressed,
            "right2" => controls.sticks[1][3] = pressed,
            "reset" => controls.reset = pressed,
            "select" => controls.select = pressed,
            "fire" => machine.set_fire(0, pressed),
            "fire2" => machine.set_fire(1, pressed),
            _ => {}
        }
    }
}
