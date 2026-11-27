//! The window's side of emulators in Limoniter's window (see `retro`): the panel beside the file a program came
//! from, showing the core's pictures, with the keyboard (a computer's keys, or a console's joypad) and the mouse
//! sent while the screen has the keyboard; Pause, Reset, Save state and Load state; the core's options; and the
//! machine's memory. And the part of Get an emulator that downloads the cores and the helper.

use super::*;
use crate::retro::{self, Helper};
use crate::running::Machine;
use run_host::{Session, SessionKind};
use std::time::Instant;

/// How often the memory shown is read again while the machine runs.
const MEMORY_EVERY: Duration = Duration::from_millis(150);
/// The memory view's window on memory, as `debug_panel::memory_section` shows it.
const MEMORY_WINDOW: usize = 128;
/// The most of a core's memory the trainer reads at once (the helper's own limit is the same).
const TRAINER_MOST: usize = 1 << 20;

/// A machine running in a core, and what the panel keeps about it.
pub(super) struct RetroRun {
    helper: Option<Helper>,
    pub core: &'static retro::Core,
    pub machine: Machine,
    /// The newest frame shown, and its size.
    frame: u64,
    size: [usize; 2],
    pub paused: bool,
    pub muted: bool,
    /// Libretro key codes held down, and the joypad's buttons.
    keys: Vec<u32>,
    joypad: u16,
    modifiers: egui::Modifiers,
    mouse_buttons: u8,
    /// The memory view: whether it's open, the address typed, the bytes read and where they start, those
    /// read before them, the read waiting for its reply, and when memory was last read.
    show_memory: bool,
    show_options: bool,
    typed: String,
    memory: Option<(usize, Vec<u8>)>,
    memory_before: Option<(usize, Vec<u8>)>,
    reading: Option<u64>,
    read_at: Option<Instant>,
    /// Commands whose replies are waited for, and what to say when each comes.
    waiting: Vec<(u64, String)>,
    pub note: Option<String>,
    /// The folder this run's program and frames are in, tidied away when it stops.
    folder: PathBuf,
    /// Where Save state keeps this program's state.
    state_file: PathBuf,
    /// The trainer's reads and writes of the machine's memory, which go through the helper like the memory view's.
    trainer: Trainer,
}

/// What the trainer has of a core's memory: the last whole read, the read waiting for its reply, and the writes
/// whose replies are still to be thrown away.
#[derive(Default)]
struct Trainer {
    ram: Option<Vec<u8>>,
    reading: Option<u64>,
    writes: Vec<u64>,
}

impl Drop for RetroRun {
    fn drop(&mut self) {
        // The helper first, so its frames file is let go before the folder goes.
        self.helper = None;
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

impl RetroRun {
    pub(super) fn start(machine: Machine, run: &crate::running::Runnable) -> Result<RetroRun, String> {
        let core = retro::takes(machine, run)?;
        let helper = retro::find_helper().ok_or(crate::i18n::tr("The helper, limoniter-retro, isn't here: Get an emulator gets it, or choose it by hand."))?;
        let core_file = retro::installed_core(core.core).ok_or_else(|| crate::i18n::tf!("The {} core isn't downloaded: Get an emulator gets it.", core.name))?;
        let (Some(saves), Some(system)) = (retro::saves_folder(), retro::system_folder()) else {
            return Err(crate::i18n::tr("There's nowhere to keep it: Limoniter's settings folder couldn't be found.").into());
        };
        static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let folder = saves.join(format!("run-{}-{}", std::process::id(), RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let _ = std::fs::create_dir_all(&system);
        let (program, run_options) = retro::write_game(run, &folder)?;
        let frames = folder.join("frames");
        let options = retro::with_options(retro::Prefs::load().options_for(core.core), run_options);
        let start = retro::Start { helper: &helper, core: &core_file, frames: &frames, system: &system, saves: &saves, game: Some(&program), options: &options, silent: false };
        let helper = Helper::start(&start)?;
        let stem = Path::new(&run.name).file_stem().map_or_else(|| run.name.clone(), |s| s.to_string_lossy().into_owned());
        let state_file = saves.join("states").join(format!("{stem}.{}.state", core.core));
        Ok(RetroRun {
            helper: Some(helper),
            core,
            machine,
            frame: 0,
            size: [0, 0],
            paused: false,
            muted: false,
            keys: Vec::new(),
            joypad: 0,
            modifiers: egui::Modifiers::default(),
            mouse_buttons: 0,
            show_memory: false,
            show_options: false,
            typed: String::new(),
            memory: None,
            memory_before: None,
            reading: None,
            read_at: None,
            waiting: Vec::new(),
            note: None,
            folder,
            state_file,
            trainer: Trainer::default(),
        })
    }

    fn send(&mut self, command: serde_json::Value) -> Option<u64> {
        self.helper.as_mut().map(|helper| helper.send(command))
    }

    /// Lets go of every key, button and mouse button held, as when the screen loses the keyboard.
    fn release_all(&mut self) {
        for code in std::mem::take(&mut self.keys) {
            self.send(serde_json::json!({"cmd": "key", "code": code, "down": false}));
        }
        if self.joypad != 0 {
            self.joypad = 0;
            self.send(serde_json::json!({"cmd": "joypad", "port": 0, "buttons": 0}));
        }
        if self.mouse_buttons != 0 {
            self.mouse_buttons = 0;
            self.send(serde_json::json!({"cmd": "mouse", "dx": 0, "dy": 0, "buttons": 0}));
        }
        self.modifiers = egui::Modifiers::default();
    }

    fn key(&mut self, code: u32, down: bool) {
        let held = self.keys.contains(&code);
        if held == down {
            return;
        }
        if down {
            self.keys.push(code);
        } else {
            self.keys.retain(|k| *k != code);
        }
        // The character, for cores that want it: the letter or digit itself.
        let character = if (32..127).contains(&code) { code } else { 0 };
        let modifiers = u32::from(self.modifiers.shift) | u32::from(self.modifiers.ctrl) << 1 | u32::from(self.modifiers.alt) << 2;
        self.send(serde_json::json!({"cmd": "key", "code": code, "down": down, "character": if down { character } else { 0 }, "modifiers": modifiers}));
    }

    /// The keys pressed while the screen has the keyboard: a computer's keys, or a console's joypad.
    fn keys_from(&mut self, events: &[egui::Event], modifiers: egui::Modifiers) {
        let joypad_before = self.joypad;
        if self.core.keyboard {
            // egui tells of Shift, Ctrl and Alt as modifiers, not keys: they're pressed and let go here as keys.
            for (now, before, code) in [(modifiers.shift, self.modifiers.shift, retro::KEY_LEFT_SHIFT), (modifiers.ctrl, self.modifiers.ctrl, retro::KEY_LEFT_CTRL), (modifiers.alt, self.modifiers.alt, retro::KEY_LEFT_ALT)] {
                if now != before {
                    self.key(code, now);
                }
            }
        } else if modifiers.shift != self.modifiers.shift {
            self.joypad = if modifiers.shift { self.joypad | 1 << retro::JOYPAD_SELECT } else { self.joypad & !(1 << retro::JOYPAD_SELECT) };
        }
        self.modifiers = modifiers;
        for event in events {
            let egui::Event::Key { key, pressed, repeat: false, .. } = event else {
                continue;
            };
            if self.core.keyboard {
                if let Some(code) = retro::retro_key(*key) {
                    self.key(code, *pressed);
                }
            } else if let Some(bit) = retro::joypad_bit(*key) {
                self.joypad = if *pressed { self.joypad | 1 << bit } else { self.joypad & !(1 << bit) };
            }
        }
        if self.joypad != joypad_before {
            let buttons = self.joypad;
            self.send(serde_json::json!({"cmd": "joypad", "port": 0, "buttons": buttons}));
        }
    }

    /// The core's newest picture, if there's a new one.
    fn new_frame(&mut self) -> Option<retro::Frame> {
        let frame = self.helper.as_mut()?.frame(self.frame)?;
        self.frame = frame.number;
        self.size = [frame.width, frame.height];
        Some(frame)
    }

    /// Takes in replies that have come: memory read, and states saved or loaded.
    fn replies(&mut self) {
        let Some(helper) = self.helper.as_mut() else { return };
        helper.pump();
        if let Some(id) = self.reading
            && let Some(reply) = helper.take_reply(id)
        {
            self.reading = None;
            match reply {
                Ok(value) => {
                    let start = value["offset"].as_u64().unwrap_or(0) as usize;
                    if let Some(bytes) = value["data"].as_str().and_then(retro::unhex) {
                        let before = self.memory.replace((start, bytes));
                        if before.as_ref().is_some_and(|(s, _)| *s == start) {
                            self.memory_before = before;
                        }
                    }
                }
                Err(problem) => self.note = Some(problem),
            }
        }
        let mut done = Vec::new();
        for (id, said) in &self.waiting {
            if let Some(reply) = helper.take_reply(*id) {
                done.push(*id);
                self.note = Some(match reply {
                    Ok(_) => said.clone(),
                    Err(problem) => problem,
                });
            }
        }
        self.waiting.retain(|(id, _)| !done.contains(id));
    }

    /// Asks for the memory shown, when it's time.
    fn read_memory(&mut self, start: usize) {
        let Some((region, size)) = self.helper.as_ref().and_then(|h| h.ready.as_ref()).and_then(retro::Ready::memory) else {
            return;
        };
        if self.reading.is_some() {
            return;
        }
        let stale = self.memory.as_ref().is_none_or(|(s, _)| *s != start);
        let due = self.read_at.is_none_or(|at| at.elapsed() >= MEMORY_EVERY) && !self.paused;
        if stale || due {
            self.read_at = Some(Instant::now());
            self.reading = self.send(serde_json::json!({"cmd": "read_memory", "region": region, "offset": start.min(size), "length": MEMORY_WINDOW}));
        }
    }

    /// The core's memory as the trainer last read it, asking for it again when none is on its way (so it's a
    /// moment old, as the memory view's is). Nothing if the core shows no memory.
    pub(super) fn trainer_ram(&mut self) -> Option<&[u8]> {
        let (region, size) = self.helper.as_ref().and_then(|h| h.ready.as_ref()).and_then(retro::Ready::memory)?;
        let helper = self.helper.as_mut()?;
        if let Some(id) = self.trainer.reading
            && let Some(reply) = helper.take_reply(id)
        {
            self.trainer.reading = None;
            if let Ok(value) = reply
                && let Some(bytes) = value["data"].as_str().and_then(retro::unhex)
            {
                self.trainer.ram = Some(bytes);
            }
        }
        if self.trainer.reading.is_none() {
            let length = size.min(TRAINER_MOST);
            self.trainer.reading = Some(helper.send(serde_json::json!({"cmd": "read_memory", "region": region, "offset": 0, "length": length})));
        }
        self.trainer.ram.as_deref()
    }

    /// Writes bytes into the core's memory (the helper's write_memory), whose reply is let go of when it comes.
    pub(super) fn trainer_poke(&mut self, address: u32, bytes: &[u8]) {
        let Some((region, _)) = self.helper.as_ref().and_then(|h| h.ready.as_ref()).and_then(retro::Ready::memory) else { return };
        let Some(helper) = self.helper.as_mut() else { return };
        let writes = std::mem::take(&mut self.trainer.writes);
        self.trainer.writes = writes.into_iter().filter(|id| helper.take_reply(*id).is_none()).collect();
        // A few thousand frames' writes unanswered means the helper isn't answering: don't pile them up.
        if self.trainer.writes.len() < 64 {
            let id = helper.send(serde_json::json!({"cmd": "write_memory", "region": region, "offset": address, "data": retro::hex(bytes)}));
            self.trainer.writes.push(id);
        }
    }

    fn peek(memory: &Option<(usize, Vec<u8>)>, at: u32) -> Option<u8> {
        let (start, bytes) = memory.as_ref()?;
        bytes.get((at as usize).checked_sub(*start)?).copied()
    }
}

/// An address typed in the memory view: hex, with or without $ or 0x before it.
fn address(typed: &str, size: usize) -> Result<i64, String> {
    let text = typed.trim().trim_start_matches('$').trim_start_matches("0x").trim_start_matches("0X");
    if text.is_empty() {
        return Ok(0);
    }
    let value = i64::from_str_radix(text, 16).map_err(|_| crate::i18n::tr("Type an address in hex, such as 4000 or $C000.").to_string())?;
    if value as usize >= size {
        return Err(crate::i18n::tf!("The machine's memory is only {size:X} (hex) bytes long."));
    }
    Ok(value)
}

/// The picture's size on the screen: whole multiples of the machine's pixels where they fit, kept to the
/// machine's shape (`aspect` is its width over its height, as it's meant to be seen).
pub(super) fn screen_size(picture: [usize; 2], aspect: f32, room: egui::Vec2) -> egui::Vec2 {
    let height = picture[1].max(1) as f32;
    let aspect = if aspect > 0.1 { aspect } else { picture[0].max(1) as f32 / height };
    let natural = egui::vec2(height * aspect, height);
    let fits = (room.x / natural.x).min(room.y / natural.y);
    let scale = if fits >= 1.0 { fits.floor() } else { fits.max(0.1) };
    natural * scale
}

impl Limoniter {
    /// Starts a machine in Limoniter's window on a program, in place of anything it was running for the same file.
    pub(super) fn run_in_window(&mut self, machine: Machine, run: crate::running::Runnable) -> Result<(), String> {
        let started = RetroRun::start(machine, &run)?;
        let doc_id = self.docs[self.active].id;
        self.running.sessions.retain(|s| !matches!(s.kind, SessionKind::Retro { doc_id: d, .. } if d == doc_id));
        self.running.sessions.push(Session { target: retro::target_name(machine), machine: run.machine, file: run.name.clone(), kind: SessionKind::Retro { machine: Box::new(started), doc_id, run: Box::new(run), texture: None }, started: Instant::now() });
        Ok(())
    }

    /// The panel of a machine in Limoniter's window, beside the file it's running.
    pub(super) fn retro_panel(&mut self, ui: &mut Ui) {
        let Some(doc_id) = self.docs.get(self.active).map(|doc| doc.id) else {
            return;
        };
        let Some(index) = self.running.sessions.iter().position(|s| matches!(&s.kind, SessionKind::Retro { doc_id: d, .. } if *d == doc_id)) else {
            return;
        };
        let focus_id = egui::Id::unique("retro-machine-screen");
        let ctx = ui.ctx().clone();
        let (mut stop, mut restart) = (false, false);
        let mut set_option: Option<(String, String)> = None;
        let choosing = self.choosing_a_control_key();
        let session = &mut self.running.sessions[index];
        let file = session.file.clone();
        let SessionKind::Retro { machine: run, texture, .. } = &mut session.kind else {
            return;
        };
        run.replies();
        let (ready, gone, log) = match run.helper.as_mut() {
            Some(helper) => {
                let running = helper.running();
                let gone = helper.gone.clone().or_else(|| (!running).then(|| crate::i18n::tr("The helper stopped.").to_string()));
                (helper.ready.clone(), gone, helper.log_path.clone())
            }
            None => (None, Some(crate::i18n::tr("The helper stopped.").to_string()), PathBuf::new()),
        };
        if let Some(frame) = run.new_frame() {
            let image = egui::ColorImage::from_rgba_unmultiplied([frame.width, frame.height], &frame.pixels);
            let options = egui::TextureOptions { magnification: egui::TextureFilter::Nearest, minification: egui::TextureFilter::Linear, ..egui::TextureOptions::default() };
            match texture {
                Some(texture) => texture.set(image, options),
                None => *texture = Some(ctx.load_texture("retro-machine", image, options)),
            }
        }
        let title = match &ready {
            Some(ready) => crate::i18n::tf!("{}: {file} ({} {})", run.machine.name(), ready.core, ready.version),
            None => crate::i18n::tf!("{}: {file} ({})", run.machine.name(), run.core.name),
        };
        let focused = ctx.memory(|m| m.has_focus(focus_id));
        Panel::right("retro-machine-panel").default_size(520.0).size_range(240.0..=1400.0).resizable(true).frame(egui::Frame::new().fill(theme::field()).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).color(theme::text_dim()).size(12.5));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    stop = crate::access::called(ui.add(Button::new(RichText::new(crate::i18n::tr("\u{D7}")).color(theme::text_dim())).frame(false)), "Switch it off").on_hover_text(crate::i18n::tr("Switch it off")).clicked();
                    restart = ui.small_button(crate::i18n::tr("Restart")).on_hover_text(crate::i18n::tr("Starts the machine again, with the program loaded afresh")).clicked();
                });
            });
            ui.horizontal_wrapped(|ui| {
                let usable = ready.is_some() && gone.is_none();
                ui.add_enabled_ui(usable, |ui| {
                    let pause = if run.paused { crate::i18n::tr("Carry on") } else { crate::i18n::tr("Pause") };
                    if ui.small_button(pause).clicked() {
                        run.paused = !run.paused;
                        let command = if run.paused { "pause" } else { "resume" };
                        run.send(serde_json::json!({"cmd": command}));
                    }
                    if ui.small_button(crate::i18n::tr("Reset")).on_hover_text(crate::i18n::tr("The machine's reset, as its own button would")).clicked() {
                        run.send(serde_json::json!({"cmd": "reset"}));
                    }
                    let sound = if run.muted { crate::i18n::tr("Sound on") } else { crate::i18n::tr("Mute") };
                    if ui.small_button(sound).clicked() {
                        run.muted = !run.muted;
                        let on = run.muted;
                        run.send(serde_json::json!({"cmd": "mute", "on": on}));
                    }
                    if ui.small_button(crate::i18n::tr("Save state")).on_hover_text(crate::i18n::tf!("Keeps the machine as it is now, in {}", run.state_file.display())).clicked() {
                        if let Some(folder) = run.state_file.parent() {
                            let _ = std::fs::create_dir_all(folder);
                        }
                        let path = run.state_file.display().to_string();
                        if let Some(id) = run.send(serde_json::json!({"cmd": "save_state", "path": path})) {
                            run.waiting.push((id, crate::i18n::tr("State saved.").to_string()));
                        }
                    }
                    let saved = run.state_file.is_file();
                    if ui.add_enabled(saved, Button::new(crate::i18n::tr("Load state")).small()).on_hover_text(crate::i18n::tr("Puts the machine back as Save state kept it")).on_disabled_hover_text(crate::i18n::tr("Save state keeps a state to come back to")).clicked() {
                        let path = run.state_file.display().to_string();
                        if let Some(id) = run.send(serde_json::json!({"cmd": "load_state", "path": path})) {
                            run.waiting.push((id, crate::i18n::tr("State loaded.").to_string()));
                        }
                    }
                    let has_memory = ready.as_ref().and_then(retro::Ready::memory).is_some();
                    if ui.add_enabled(has_memory, egui::Button::selectable(run.show_memory, crate::i18n::tr("Memory")).small()).on_hover_text(crate::i18n::tr("The machine's memory, as it runs")).on_disabled_hover_text(crate::i18n::tr("This core doesn't let its memory be seen")).clicked() {
                        run.show_memory = !run.show_memory;
                    }
                    let has_options = ready.as_ref().is_some_and(|r| !r.options.is_empty());
                    if ui.add_enabled(has_options, egui::Button::selectable(run.show_options, crate::i18n::tr("Options")).small()).on_hover_text(crate::i18n::tr("The core's own options, kept for next time")).clicked() {
                        run.show_options = !run.show_options;
                    }
                });
            });
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let room = egui::vec2(ui.available_width(), (ui.available_height() - 60.0).max(120.0));
                let aspect = ready.as_ref().map_or(4.0 / 3.0, |r| r.aspect);
                let picture = if run.size[0] > 0 { run.size } else { ready.as_ref().map_or([320, 240], |r| [r.width as usize, r.height as usize]) };
                let screen = screen_size(picture, aspect, room);
                let (rect, _) = ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - screen.x).max(0.0) / 2.0);
                    ui.allocate_exact_size(screen, egui::Sense::hover())
                }).inner;
                let response = ui.interact(rect, focus_id, egui::Sense::click());
                crate::access::named(&response, egui::Role::Canvas, "The machine's screen: the keys go to it while it has the keyboard");
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
                // The mouse, for machines that have one: its movement over the screen, in the machine's pixels.
                if run.core.mouse && focused {
                    let (delta, primary, secondary, middle, over) = ctx.input(|i| (i.pointer.delta(), i.pointer.primary_down(), i.pointer.secondary_down(), i.pointer.middle_down(), i.pointer.hover_pos().is_some_and(|p| rect.contains(p))));
                    let buttons = if over { u8::from(primary) | u8::from(secondary) << 1 | u8::from(middle) << 2 } else { 0 };
                    let per_pixel = picture[1].max(1) as f32 / rect.height().max(1.0);
                    let (dx, dy) = ((delta.x * per_pixel).round() as i64, (delta.y * per_pixel).round() as i64);
                    if (over && (dx != 0 || dy != 0)) || buttons != run.mouse_buttons {
                        run.mouse_buttons = buttons;
                        run.send(serde_json::json!({"cmd": "mouse", "dx": if over { dx } else { 0 }, "dy": if over { dy } else { 0 }, "buttons": buttons}));
                    }
                }
                let words = match (&gone, &ready) {
                    (Some(problem), _) => problem.clone(),
                    (None, None) => crate::i18n::tf!("Starting {}\u{2026}", run.core.name),
                    (None, Some(ready)) if ready.error.is_some() => ready.error.clone().unwrap_or_default(),
                    (None, Some(_)) if focused && run.core.keyboard => crate::i18n::tr("The keys go to the machine. Click the text to type there again.").into(),
                    (None, Some(_)) if focused => crate::i18n::tr("The arrow keys are the joypad, Z and X its B and A, A and S its Y and X, Enter Start and Shift Select. Click the text to type there again.").into(),
                    (None, Some(_)) => crate::i18n::tr("Click the screen to give the machine the keyboard.").into(),
                };
                ui.add(egui::Label::new(RichText::new(words).color(theme::text_faint()).size(12.0)).wrap());
                if gone.is_some() && log.is_file() {
                    ui.label(RichText::new(crate::i18n::tf!("The helper's log is in {}", log.display())).color(theme::text_faint()).size(11.5));
                }
                if let Some(needs) = run.core.needs {
                    let system = retro::system_folder().map(|f| f.display().to_string()).unwrap_or_default();
                    ui.label(RichText::new(crate::i18n::tf!("{} needs {needs}, in {system}.", run.core.name)).color(theme::text_faint()).size(11.5));
                }
                if let Some(note) = &run.note {
                    ui.label(RichText::new(note.as_str()).color(theme::text_dim()).size(12.0));
                }
                if run.show_options
                    && let Some(ready) = &ready
                {
                    egui::CollapsingHeader::new(crate::i18n::tr("Options")).id_salt("retro-options").default_open(true).show(ui, |ui| {
                        ui.label(RichText::new(crate::i18n::tr("Some take effect only when the machine is restarted.")).color(theme::text_faint()).size(11.5));
                        egui::Grid::new("retro-options-grid").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                            for option in &ready.options {
                                ui.label(option.description.as_str()).on_hover_text(option.key.as_str());
                                egui::ComboBox::from_id_salt(("retro-option", option.key.as_str())).selected_text(option.value.as_str()).show_ui(ui, |ui| {
                                    for value in &option.values {
                                        if ui.selectable_label(*value == option.value, value.as_str()).clicked() {
                                            set_option = Some((option.key.clone(), value.clone()));
                                        }
                                    }
                                });
                                ui.end_row();
                            }
                        });
                    });
                }
                if run.show_memory
                    && let Some((region, size)) = ready.as_ref().and_then(retro::Ready::memory)
                {
                    let start = address(&run.typed, size).unwrap_or(0) as usize & !0xF;
                    run.read_memory(start);
                    let hint = crate::i18n::tr("hex, from 0");
                    let digits = if size > 0x10000 { 6 } else { 4 };
                    let (memory, before) = (&run.memory, &run.memory_before);
                    crate::debug_panel::memory_section(ui, &mut run.typed, hint, &|typed| address(typed, size), digits, &|at| RetroRun::peek(memory, at).filter(|_| (at as usize) < size), &|at| RetroRun::peek(before, at));
                    let words = if region == "memory_map" {
                        crate::i18n::tr("The machine's memory, by its own addresses, as the core maps it. Addresses with nothing there read as 00.").to_string()
                    } else {
                        crate::i18n::tf!("The core's system RAM: {size} bytes, numbered from 0 as the core keeps it, which may not be the machine's own addresses.")
                    };
                    ui.label(RichText::new(words).color(theme::text_faint()).size(11.5));
                }
            });
        });
        if focused && !choosing {
            let (events, modifiers) = ctx.input(|i| (i.events.clone(), i.modifiers));
            run.keys_from(&events, modifiers);
        } else {
            run.release_all();
        }
        if let Some((key, value)) = set_option {
            run.send(serde_json::json!({"cmd": "option", "key": key, "value": value}));
            if let Some(helper) = run.helper.as_mut()
                && let Some(ready) = helper.ready.as_mut()
                && let Some(option) = ready.options.iter_mut().find(|o| o.key == key)
            {
                option.value = value.clone();
            }
            let mut prefs = retro::Prefs::load();
            prefs.set_option(run.core.core, &key, &value);
            prefs.save();
        }
        if gone.is_none() {
            ctx.request_repaint_after(Duration::from_millis(if run.paused { 100 } else { 15 }));
        }
        if restart {
            let SessionKind::Retro { run: program, machine, .. } = &self.running.sessions[index].kind else {
                return;
            };
            let (program, which) = ((**program).clone(), machine.machine);
            // The old helper goes first, so two don't play at once.
            if let SessionKind::Retro { machine, .. } = &mut self.running.sessions[index].kind {
                machine.helper = None;
            }
            match RetroRun::start(which, &program) {
                Ok(fresh) => {
                    if let SessionKind::Retro { machine, texture, .. } = &mut self.running.sessions[index].kind {
                        **machine = fresh;
                        *texture = None;
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

    /// Choose helper: the helper program picked by hand, and kept.
    pub(super) fn choose_retro_helper(&mut self) -> Option<String> {
        let program = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tr("Choose the helper, limoniter-retro")).pick_file()?;
        let mut prefs = retro::Prefs::load();
        prefs.helper = Some(program.clone());
        prefs.save();
        Some(crate::i18n::tf!("The helper is {}.", program.display()))
    }
}

/// For a screenshot: the memory view of the machine just started, opened if asked for.
pub(super) fn open_memory_for_screenshot(app: &mut Limoniter, memory: bool) {
    if let Some(SessionKind::Retro { machine, .. }) = app.running.sessions.last_mut().map(|s| &mut s.kind) {
        machine.show_memory = memory;
    }
}

/// Get an emulator's part for the cores: a download going on, and what's said about the last one.
#[derive(Default)]
pub(super) struct Getting {
    download: Option<(String, std::sync::mpsc::Receiver<Result<PathBuf, String>>)>,
    pub note: String,
}

/// Installing what's missing from Run on machine's box, off the UI thread, and what's said about it.
#[derive(Default)]
pub(super) struct Install {
    going: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    pub note: String,
}

/// Downloads the helper if it's missing, then the core if that is, stopping at the first thing that goes wrong.
/// The two downloads are given in, so that a test can stand in for them.
pub(super) fn install_job(missing: retro::Missing, helper: impl FnOnce() -> Result<PathBuf, String>, core: impl FnOnce(&str) -> Result<PathBuf, String>) -> Result<(), String> {
    if missing.helper {
        helper()?;
    }
    if let Some(wanted) = missing.core {
        core(wanted.core)?;
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn install_job_for_tests(missing: retro::Missing, helper: impl FnOnce() -> Result<PathBuf, String>, core: impl FnOnce(&str) -> Result<PathBuf, String>) -> Result<(), String> {
    install_job(missing, helper, core)
}

impl Install {
    #[cfg(test)]
    pub fn start_for_tests(&mut self, job: impl FnOnce() -> Result<(), String> + Send + 'static) {
        self.start_with(job);
    }

    pub fn busy(&self) -> bool {
        self.going.is_some()
    }

    /// Starts downloading what's missing, using the same downloads as Get an emulator.
    pub fn start(&mut self, missing: retro::Missing) {
        self.start_with(move || install_job(missing, retro::download_helper, retro::download_core));
    }

    fn start_with(&mut self, job: impl FnOnce() -> Result<(), String> + Send + 'static) {
        if self.busy() {
            return;
        }
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(job());
        });
        self.going = Some(receive);
        self.note.clear();
    }

    /// Takes in a download that has finished, if one has. Whether one is still going.
    pub fn poll(&mut self) -> bool {
        if let Some(result) = self.going.as_ref().and_then(|r| r.try_recv().ok()) {
            self.going = None;
            self.note = match result {
                Ok(()) => crate::i18n::tr("It's installed. Its choice can be used now.").to_string(),
                Err(problem) => problem,
            };
        }
        self.busy()
    }
}

/// What the person asked for in Get an emulator's cores.
pub(super) enum GetAsk {
    ChooseHelper,
    SetUpAmiga,
}

/// The helper and the cores, in Get an emulator: each with whether it's here, and a button to download it.
pub(super) fn getting_section(ui: &mut Ui, state: &mut Getting) -> Option<GetAsk> {
    let mut ask = None;
    if let Some((what, result)) = state.download.as_ref().and_then(|(w, r)| r.try_recv().ok().map(|result| (w.clone(), result))) {
        state.download = None;
        state.note = match result {
            Ok(path) => crate::i18n::tf!("{what} is downloaded, to {}.", path.display()),
            Err(problem) => problem,
        };
    }
    if state.download.is_some() {
        ui.ctx().request_repaint_after(Duration::from_millis(250));
    }
    ui.separator();
    ui.label(RichText::new(crate::i18n::tr("In Limoniter's window")).strong());
    ui.label(RichText::new(crate::i18n::tr("Machines run in a panel beside the file, by libretro cores, which the helper limoniter-retro runs. F9's Run on offers each machine whose core is here.")).color(theme::text_faint()).size(12.0));
    let busy = state.download.is_some();
    let mut wanted: Option<(String, Box<dyn FnOnce() -> Result<PathBuf, String> + Send>)> = None;
    egui::Grid::new("get-retro").num_columns(3).spacing([12.0, 4.0]).show(ui, |ui| {
        ui.label(crate::i18n::tr("The helper (limoniter-retro)")).on_hover_text(crate::i18n::tf!("From github.com/{}", retro::HELPER_REPOSITORY));
        match retro::find_helper() {
            Some(path) => {
                ui.label(RichText::new(crate::i18n::tr("Installed")).color(theme::text_dim())).on_hover_text(path.display().to_string());
            }
            None => {
                ui.label(RichText::new(crate::i18n::tr("Not installed")).color(theme::text_faint()));
            }
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, Button::new(crate::i18n::tr("Download"))).on_hover_text(crate::i18n::tf!("Its latest release, from github.com/{}", retro::HELPER_REPOSITORY)).clicked() {
                wanted = Some((crate::i18n::tr("The helper").into(), Box::new(retro::download_helper)));
            }
            if ui.button(crate::i18n::tr("Choose helper\u{2026}")).on_hover_text(crate::i18n::tr("A helper you've built or downloaded yourself")).clicked() {
                ask = Some(GetAsk::ChooseHelper);
            }
        });
        ui.end_row();
        for core in retro::CORES {
            let machines: Vec<&str> = core.machines.iter().map(|m| m.name()).collect();
            let hover = match core.needs {
                Some(needs) => crate::i18n::tf!("{}. It needs {needs}, in the system folder: {}", machines.join(", "), retro::system_folder().map(|f| f.display().to_string()).unwrap_or_default()),
                None => machines.join(", "),
            };
            ui.label(crate::i18n::tf!("{} core", core.name)).on_hover_text(hover);
            match retro::installed_core(core.core) {
                Some(path) => {
                    ui.label(RichText::new(crate::i18n::tr("Installed")).color(theme::text_dim())).on_hover_text(path.display().to_string());
                    ui.label("");
                }
                None => {
                    ui.label(RichText::new(crate::i18n::tr("Not installed")).color(theme::text_faint()));
                    let address = retro::core_address(core.core, std::env::consts::OS, std::env::consts::ARCH);
                    let button = ui.add_enabled(!busy && address.is_some(), Button::new(crate::i18n::tr("Download")));
                    let button = match &address {
                        Some(address) => button.on_hover_text(crate::i18n::tf!("Its latest nightly build, from {address}")),
                        None => button.on_disabled_hover_text(crate::i18n::tr("The libretro buildbot has no cores for this kind of computer.")),
                    };
                    if button.clicked() {
                        let name = core.core;
                        wanted = Some((crate::i18n::tf!("The {} core", core.name), Box::new(move || retro::download_core(name))));
                    }
                }
            }
            ui.end_row();
        }
    });
    if ui.button(crate::i18n::tr("Set up the Amiga\u{2026}")).on_hover_text(crate::i18n::tr("Installs what F9 needs to run Amiga programs in Limoniter's window, Kickstart included, in one go")).clicked() {
        ask = Some(GetAsk::SetUpAmiga);
    }
    if let Some((what, download)) = wanted {
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(download());
        });
        state.download = Some((what, receive));
        state.note.clear();
    }
    if let Some((what, _)) = &state.download {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(crate::i18n::tf!("Downloading {what}\u{2026}"));
        });
    }
    if !state.note.is_empty() {
        ui.label(RichText::new(&state.note).color(theme::text_dim()));
    }
    ask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picture_is_scaled_by_whole_numbers_where_it_fits() {
        // A Spectrum's 320 × 240 at 4:3 in room for three and a bit times it: three times.
        assert_eq!(screen_size([320, 240], 4.0 / 3.0, egui::vec2(1000.0, 800.0)), egui::vec2(960.0, 720.0));
        // Too little room for even once: as big as fits, still its shape.
        let small = screen_size([320, 240], 4.0 / 3.0, egui::vec2(160.0, 800.0));
        assert!((small.x - 160.0).abs() < 0.01 && (small.y - 120.0).abs() < 0.01);
        // A 2600's 160 × 210 is shown 4:3, its pixels wider than tall.
        let wide = screen_size([160, 210], 4.0 / 3.0, egui::vec2(600.0, 500.0));
        assert!((wide.x / wide.y - 4.0 / 3.0).abs() < 0.01);
        assert_eq!(wide.y, 420.0);
    }

    #[test]
    fn addresses_are_read_as_hex() {
        assert_eq!(address("", 0x10000), Ok(0));
        assert_eq!(address("$C000", 0x10000), Ok(0xC000));
        assert_eq!(address("0x10", 0x10000), Ok(16));
        assert!(address("10000", 0x10000).is_err());
        assert!(address("zz", 0x10000).is_err());
    }

    #[test]
    fn memory_is_peeked_from_what_was_read() {
        let memory = Some((0x100, vec![1, 2, 3]));
        assert_eq!(RetroRun::peek(&memory, 0x101), Some(2));
        assert_eq!(RetroRun::peek(&memory, 0xFF), None);
        assert_eq!(RetroRun::peek(&memory, 0x103), None);
        assert_eq!(RetroRun::peek(&None, 0), None);
    }
}
