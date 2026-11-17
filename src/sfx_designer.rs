//! The sound effects designer's window: a bank of effects for one machine, each drawn as a graph of its frames
//! (the note above, the volume below), its frames' values to type, a shape with presets to make one from, and the
//! sound played through the speakers as it's changed.

use crate::sfx::{self, Bank, Chip, Effect, Frame, Machine, Noise, Preset, Shape, Wave};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, vec2};
use std::sync::{Arc, Mutex, PoisonError};

#[cfg(test)]
#[path = "sfx_designer/tests.rs"]
mod tests;

/// What the window asks of the app.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Asked {
    Open,
    FromSelection,
    Insert,
    SaveSource,
    SaveBank,
    OpenProgram,
    Close,
}

/// A sound playing: its samples, and how far through them the speakers have got.
struct Shared {
    samples: Vec<f32>,
    at: usize,
    looping: bool,
}

pub struct Playing {
    _stream: cpal::Stream,
    shared: Arc<Mutex<Shared>>,
    rate: u32,
    /// The effect it's of, and the frames a second.
    effect: usize,
    frame_rate: f64,
}

impl Playing {
    fn start(samples: Vec<f32>, looping: bool, effect: usize, frame_rate: f64) -> Result<Playing, String> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let device = cpal::default_host().default_output_device().ok_or(crate::i18n::tr("There's no sound device to play through."))?;
        let config = device.default_output_config().map_err(|e| crate::i18n::tf!("The sound device can't be used ({e})."))?;
        let rate = config.sample_rate();
        let channels = usize::from(config.channels()).max(1);
        let shared = Arc::new(Mutex::new(Shared { samples, at: 0, looping }));
        let feed = shared.clone();
        let format = config.sample_format();
        let config: cpal::StreamConfig = config.config();
        macro_rules! build {
            ($t:ty) => {
                device.build_output_stream::<$t, _, _>(
                    config,
                    move |data: &mut [$t], _| {
                        let mut shared = feed.lock().unwrap_or_else(PoisonError::into_inner);
                        for frame in data.chunks_mut(channels) {
                            if shared.at >= shared.samples.len() && shared.looping && !shared.samples.is_empty() {
                                shared.at = 0;
                            }
                            let value = shared.samples.get(shared.at).copied().unwrap_or(0.0);
                            shared.at += 1;
                            for out in frame.iter_mut() {
                                *out = <$t as cpal::FromSample<f32>>::from_sample_(value);
                            }
                        }
                    },
                    |_| {},
                    None,
                )
            };
        }
        let stream = match format {
            cpal::SampleFormat::F32 => build!(f32),
            cpal::SampleFormat::I16 => build!(i16),
            cpal::SampleFormat::U16 => build!(u16),
            cpal::SampleFormat::I32 => build!(i32),
            other => return Err(crate::i18n::tf!("The sound device wants samples as {other}, which Limoniter doesn't make.")),
        }
        .map_err(|e| crate::i18n::tf!("The sound device wouldn't play ({e})."))?;
        stream.play().map_err(|e| crate::i18n::tf!("The sound device wouldn't start ({e})."))?;
        Ok(Playing { _stream: stream, shared, rate, effect, frame_rate })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The frame being heard, while it plays.
    fn frame(&self) -> Option<usize> {
        let shared = self.lock();
        let at = if shared.looping && !shared.samples.is_empty() { shared.at % shared.samples.len() } else { shared.at };
        (at < shared.samples.len()).then(|| (at as f64 / f64::from(self.rate) * self.frame_rate) as usize)
    }
}

/// Which way drawing on the graph goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Drawing {
    Pitch,
    Volume,
}

pub struct Editor {
    pub bank: Bank,
    pub chosen: usize,
    /// The frame whose values are shown to type.
    pub frame: usize,
    pub shape: Shape,
    seed: u64,
    pub label: String,
    /// Each change played as it's made.
    pub play_changes: bool,
    pub looping: bool,
    undo: Vec<(Bank, usize)>,
    redo: Vec<(Bank, usize)>,
    /// Where a drag on the graph has been, so one drag is one undo and fills every frame it crossed.
    drag: Option<(Drawing, usize)>,
    playing: Option<Playing>,
    /// Why there's no sound, when there isn't.
    pub note: Option<String>,
    /// A change waiting to be heard, once the pointer's let go of.
    changed: bool,
    /// Where the graph was drawn, and a frame's width on it.
    graph_rect: Rect,
    column: f32,
    /// The SID's envelope at the end of each frame, for the effect it was worked out for.
    levels: Option<(u64, Vec<f32>)>,
}

impl Default for Editor {
    fn default() -> Editor {
        Editor { bank: Bank::default(), chosen: 0, frame: 0, shape: Shape::preset(Preset::Laser), seed: 1, label: "sfx".into(), play_changes: true, looping: false, undo: Vec::new(), redo: Vec::new(), drag: None, playing: None, note: None, changed: false, graph_rect: Rect::NOTHING, column: 1.0, levels: None }
    }
}

const GRAPH_HEIGHT: f32 = 300.0;
/// Of the graph, the share at the top for the note.
const PITCH_SHARE: f32 = 0.62;
const LOWEST_HZ: f64 = 30.0;
const HIGHEST_HZ: f64 = 12_000.0;

impl Editor {
    pub fn chip(&self) -> Chip {
        self.bank.machine.chip()
    }

    pub fn effect(&self) -> &Effect {
        &self.bank.effects[self.chosen.min(self.bank.effects.len() - 1)]
    }

    fn effect_mut(&mut self) -> &mut Effect {
        let at = self.chosen.min(self.bank.effects.len() - 1);
        &mut self.bank.effects[at]
    }

    pub fn remember(&mut self) {
        self.undo.push((self.bank.clone(), self.chosen));
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.changed = true;
    }

    pub fn undo(&mut self) {
        if let Some((bank, chosen)) = self.undo.pop() {
            self.redo.push((std::mem::replace(&mut self.bank, bank), self.chosen));
            self.chosen = chosen;
            self.changed = true;
        }
    }

    pub fn redo(&mut self) {
        if let Some((bank, chosen)) = self.redo.pop() {
            self.undo.push((std::mem::replace(&mut self.bank, bank), self.chosen));
            self.chosen = chosen;
            self.changed = true;
        }
    }

    /// The bank for another machine: its effects' notes kept as near as that chip can play them.
    pub fn set_machine(&mut self, machine: Machine) {
        if machine == self.bank.machine {
            return;
        }
        self.remember();
        let from = self.bank.machine;
        // Each frame's note and loudness go across, as near as the other chip has them.
        self.bank = Bank { machine, effects: self.bank.effects.iter().map(|e| e.for_machine(from, machine)).collect() };
    }

    /// The chosen effect made again from the shape.
    pub fn make_from_shape(&mut self) {
        self.remember();
        let machine = self.bank.machine;
        let name = self.effect().name.clone();
        *self.effect_mut() = Effect::from_shape(&name, machine, &self.shape);
        self.frame = self.frame.min(self.effect().frames.len().saturating_sub(1));
    }

    pub fn add_effect(&mut self, effect: Effect) {
        self.remember();
        self.bank.effects.insert(self.chosen + 1, effect);
        self.chosen += 1;
        self.frame = 0;
    }

    pub fn source(&self) -> Result<String, String> {
        self.bank.source(&self.label, false)
    }

    pub fn program(&self) -> Result<String, String> {
        self.bank.source(&self.label, self.bank.has_program())
    }

    /// Reads effects back from text; whether there were any.
    pub fn read_source(&mut self, text: &str) -> Result<(), String> {
        let bank = sfx::from_source(text, self.bank.machine)?;
        self.remember();
        self.bank = bank;
        self.chosen = 0;
        self.frame = 0;
        Ok(())
    }

    /// The chosen effect played through the speakers.
    pub fn play(&mut self) {
        self.changed = false;
        let machine = self.bank.machine;
        let rate = cpal_rate();
        let samples = sfx::sound::render(machine, self.effect(), rate);
        // A new player each time, at the device's own rate.
        self.playing = None;
        match Playing::start(samples, self.looping, self.chosen, machine.frame_rate()) {
            Ok(playing) => {
                if playing.rate != rate {
                    let samples = sfx::sound::render(machine, self.effect(), playing.rate);
                    playing.lock().samples = samples;
                }
                self.playing = Some(playing);
                self.note = None;
            }
            Err(why) => self.note = Some(why),
        }
    }

    pub fn stop(&mut self) {
        self.playing = None;
    }

    fn playing_frame(&self) -> Option<usize> {
        self.playing.as_ref().filter(|p| p.effect == self.chosen).and_then(Playing::frame)
    }

    /// The window's contents; what it asks for.
    pub fn draw(&mut self, ui: &mut Ui) -> Option<Asked> {
        let mut asked = None;
        let chip = self.chip();
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Chip"));
            let mut chosen_chip = chip;
            egui::ComboBox::from_id_salt("sfx-chip").selected_text(crate::i18n::tr(chip.name())).show_ui(ui, |ui| {
                for c in Chip::ALL {
                    ui.selectable_value(&mut chosen_chip, c, crate::i18n::tr(c.name()));
                }
            });
            if chosen_chip != chip {
                self.set_machine(chosen_chip.machines()[0]);
            }
            let mut machine = self.bank.machine;
            egui::ComboBox::from_id_salt("sfx-machine").selected_text(crate::i18n::tr(machine.name())).show_ui(ui, |ui| {
                for &m in self.chip().machines() {
                    ui.selectable_value(&mut machine, m, crate::i18n::tr(m.name()));
                }
            });
            self.set_machine(machine);
            ui.label(crate::i18n::tr("Label"));
            ui.add(egui::TextEdit::singleline(&mut self.label).desired_width(90.0).font(egui::TextStyle::Monospace));
            ui.add_space(8.0);
            if ui.add_enabled(!self.undo.is_empty(), egui::Button::new(crate::i18n::tr("Undo"))).clicked() {
                self.undo();
            }
            if ui.add_enabled(!self.redo.is_empty(), egui::Button::new(crate::i18n::tr("Redo"))).clicked() {
                self.redo();
            }
        });
        ui.separator();
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(170.0);
                self.effects_list(ui);
            });
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.set_width(600.0);
                self.graph(ui);
                ui.add_space(4.0);
                self.frame_values(ui);
            });
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.set_width(230.0);
                let most = (ui.ctx().content_rect().height() - 260.0).max(240.0);
                egui::ScrollArea::vertical().id_salt("sfx-shape").max_height(most).show(ui, |ui| self.shape_controls(ui));
            });
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            let playing = self.playing_frame().is_some();
            if ui.button(if playing { crate::i18n::tr("\u{25A0} Stop") } else { crate::i18n::tr("\u{25B6} Play") }).on_hover_text(crate::i18n::tr("Hear the effect")).clicked() {
                if playing { self.stop() } else { self.play() }
            }
            ui.checkbox(&mut self.looping, crate::i18n::tr("Loop"));
            ui.checkbox(&mut self.play_changes, crate::i18n::tr("Play each change")).on_hover_text(crate::i18n::tr("Hear the effect again whenever it's changed"));
            ui.add_space(8.0);
            let program_tip = if self.bank.has_program() { "The effects with a program that plays each in turn, in a new tab: F9 runs it" } else { "The effects and their player, in a new tab" };
            let buttons: [(&str, &str, Asked); 6] = [
                ("Open\u{2026}", "Source this saved, or for the AY an AYFX bank (.afb) or effect (.afx)", Asked::Open),
                ("Read the selection", "Effects in the source selected in the document", Asked::FromSelection),
                ("Put in the source", "The effects and their player at the document's cursor", Asked::Insert),
                ("Save source\u{2026}", "The effects and their player", Asked::SaveSource),
                (if chip == Chip::Ay { crate::i18n::tr("Save bank\u{2026}") } else { crate::i18n::tr("Save bytes\u{2026}") }, if chip == Chip::Ay { crate::i18n::tr("An AYFX bank (.afb), as AYFX Editor saves it") } else { crate::i18n::tr("The effects' data alone, for INCBIN") }, Asked::SaveBank),
                ("Program to run", program_tip, Asked::OpenProgram),
            ];
            for (name, tip, what) in buttons {
                if ui.button(crate::i18n::tr(name)).on_hover_text(crate::i18n::tr(tip)).clicked() {
                    asked = Some(what);
                }
            }
        });
        if let Some(note) = &self.note {
            ui.label(RichText::new(note.as_str()).color(crate::theme::text_dim()));
        }
        if self.playing_frame().is_some() {
            ui.ctx().request_repaint();
        }
        // A change heard once the pointer's let go of, not at every step of a drag.
        if self.changed && self.play_changes && self.drag.is_none() && !ui.input(|i| i.pointer.any_down()) {
            self.play();
        }
        asked
    }

    fn effects_list(&mut self, ui: &mut Ui) {
        ui.label(RichText::new(crate::i18n::tr("Effects")).strong());
        egui::ScrollArea::vertical().id_salt("sfx-list").max_height(230.0).show(ui, |ui| {
            for i in 0..self.bank.effects.len() {
                let e = &self.bank.effects[i];
                let text = format!("{i}  {}  ({})", e.name, e.frames.len());
                if ui.selectable_label(i == self.chosen, text).clicked() && i != self.chosen {
                    self.chosen = i;
                    self.frame = 0;
                    if self.play_changes {
                        self.changed = true;
                    }
                }
            }
        });
        ui.add_space(4.0);
        let mut name = self.effect().name.clone();
        if ui.add(egui::TextEdit::singleline(&mut name).desired_width(160.0).hint_text(crate::i18n::tr("Its name"))).changed() {
            self.remember();
            self.changed = false;
            self.effect_mut().name = name;
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button(crate::i18n::tr("New")).on_hover_text(crate::i18n::tr("A new effect, from the shape")).clicked() {
                let effect = Effect::from_shape(&format!("effect {}", self.bank.effects.len()), self.bank.machine, &self.shape);
                self.add_effect(effect);
            }
            if ui.button(crate::i18n::tr("Copy")).clicked() {
                let mut copy = self.effect().clone();
                copy.name = format!("{} copy", copy.name);
                self.add_effect(copy);
            }
            if ui.add_enabled(self.bank.effects.len() > 1, egui::Button::new(crate::i18n::tr("Delete"))).clicked() {
                self.remember();
                self.bank.effects.remove(self.chosen);
                self.chosen = self.chosen.min(self.bank.effects.len() - 1);
                self.changed = false;
            }
            if ui.add_enabled(self.chosen > 0, egui::Button::new(crate::i18n::tr("\u{2191}"))).on_hover_text(crate::i18n::tr("Earlier: a lower number")).clicked() {
                self.remember();
                self.bank.effects.swap(self.chosen, self.chosen - 1);
                self.chosen -= 1;
                self.changed = false;
            }
            if ui.add_enabled(self.chosen + 1 < self.bank.effects.len(), egui::Button::new(crate::i18n::tr("\u{2193}"))).on_hover_text(crate::i18n::tr("Later: a higher number")).clicked() {
                self.remember();
                self.bank.effects.swap(self.chosen, self.chosen + 1);
                self.chosen += 1;
                self.changed = false;
            }
        });
        ui.add_space(6.0);
        let seconds = self.effect().frames.len() as f64 / self.bank.machine.frame_rate();
        ui.label(RichText::new(crate::i18n::tf!("{} frames, {seconds:.2} seconds at {:.1} frames a second", self.effect().frames.len(), self.bank.machine.frame_rate())).size(11.0).color(crate::theme::text_dim()));
        let bytes = self.bank.bytes().map_or(0, |b| b.len());
        ui.label(RichText::new(crate::i18n::tf!("The bank's data: {bytes} bytes")).size(11.0).color(crate::theme::text_dim()));
    }

    /// The graph: the notes above (on a scale of octaves), the volume below, a column a frame; drawing on it sets them.
    fn graph(&mut self, ui: &mut Ui) {
        let machine = self.bank.machine;
        let chip = self.chip();
        let frames = self.effect().frames.len();
        let width = ui.available_width().min(600.0);
        let column = (width / frames.max(1) as f32).clamp(5.0, 40.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(crate::i18n::tr("Draw the note above and the volume below; click a frame for its values.")).size(11.0).color(crate::theme::text_dim()));
        });
        egui::ScrollArea::horizontal().id_salt("sfx-graph").show(ui, |ui| {
            let (rect, response) = ui.allocate_exact_size(vec2((column * frames as f32).max(width), GRAPH_HEIGHT), Sense::click_and_drag());
            self.graph_rect = rect;
            self.column = column;
            let levels = self.sid_levels();
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 2.0, crate::theme::field());
            let split = rect.top() + rect.height() * PITCH_SHARE;
            let pitch_rect = Rect::from_min_max(rect.min, Pos2::new(rect.right(), split - 2.0));
            let volume_rect = Rect::from_min_max(Pos2::new(rect.left(), split + 2.0), rect.max);
            let dim = crate::theme::text_dim();
            // Octaves, labelled at each C.
            let y_of = |hz: f64| -> f32 {
                let t = ((hz.clamp(LOWEST_HZ, HIGHEST_HZ) / LOWEST_HZ).log2() / (HIGHEST_HZ / LOWEST_HZ).log2()) as f32;
                pitch_rect.bottom() - t * pitch_rect.height()
            };
            for octave in 1..=9 {
                let c = 16.3516 * 2f64.powi(octave);
                if (LOWEST_HZ..=HIGHEST_HZ).contains(&c) {
                    let y = y_of(c);
                    painter.hline(rect.x_range(), y, Stroke::new(1.0, dim.gamma_multiply(0.25)));
                    painter.text(Pos2::new(rect.left() + 2.0, y - 1.0), egui::Align2::LEFT_BOTTOM, format!("C{octave}"), egui::FontId::proportional(9.0), dim.gamma_multiply(0.7));
                }
            }
            painter.hline(rect.x_range(), split, Stroke::new(1.0, dim.gamma_multiply(0.5)));
            let effect = self.effect();
            let accent = crate::theme::accent();
            let noise_colour = Color32::from_rgb(200, 120, 60);
            let mut last: Option<Pos2> = None;
            for (i, f) in effect.frames.iter().enumerate() {
                let x0 = rect.left() + i as f32 * column;
                let middle = x0 + column / 2.0;
                if i == self.frame {
                    painter.rect_filled(Rect::from_min_max(Pos2::new(x0, rect.top()), Pos2::new(x0 + column, rect.bottom())), 0.0, accent.gamma_multiply(0.12));
                }
                let loud = if chip == Chip::Sid { levels.get(i).copied().unwrap_or(0.0) } else { sfx::loudness(chip, f, effect.adsr) as f32 };
                let bar = Rect::from_min_max(Pos2::new(x0 + 1.0, volume_rect.bottom() - loud * volume_rect.height()), Pos2::new(x0 + column - 1.0, volume_rect.bottom()));
                painter.rect_filled(bar, 0.0, if f.noise || (chip == Chip::Sid && f.wave & 0x80 != 0) || (chip == Chip::Tia && !matches!(f.wave, 4 | 5 | 12 | 13 | 6 | 10 | 14)) { noise_colour } else { accent }.gamma_multiply(0.8));
                let audible = if chip == Chip::Sid { loud > 0.01 && f.wave & 0xF0 != 0 } else { f.sounding(chip) };
                match sfx::hz(machine, f) {
                    Some(h) => {
                        let point = Pos2::new(middle, y_of(h));
                        let colour = if audible { accent } else { dim.gamma_multiply(0.5) };
                        if let Some(p) = last {
                            painter.line_segment([p, point], Stroke::new(1.5, colour.gamma_multiply(0.6)));
                        }
                        painter.circle_filled(point, (column / 4.0).clamp(1.5, 3.5), colour);
                        last = Some(point);
                    }
                    None => last = None,
                }
                // Noise: a band where its pitch is.
                let noise_at = match chip {
                    Chip::Ay if f.noise => Some(1.0 - f64::from(f.noise_period) / 31.0),
                    Chip::Sid if f.wave & 0x80 != 0 => Some((f64::from(f.period.max(1)).log2() / 16.0).clamp(0.0, 1.0)),
                    Chip::Beeper if f.noise && f.tone => Some(1.0 - (f64::from(f.period) / 500.0).clamp(0.0, 1.0)),
                    Chip::Tia if matches!(f.wave, 8 | 2 | 3 | 7 | 9 | 15 | 1) => Some(1.0 - f64::from(f.period) / 31.0),
                    _ => None,
                };
                if let Some(n) = noise_at.filter(|_| audible || chip == Chip::Sid) {
                    let y = pitch_rect.bottom() - n as f32 * pitch_rect.height();
                    painter.rect_filled(Rect::from_center_size(Pos2::new(middle, y), vec2((column - 2.0).max(2.0), 4.0)), 1.0, noise_colour);
                }
            }
            if let Some(at) = self.playing_frame() {
                let x = rect.left() + at as f32 * column;
                painter.vline(x, rect.y_range(), Stroke::new(2.0, Color32::from_rgb(230, 70, 70)));
            }
            // Drawing.
            let pointer = response.interact_pointer_pos();
            if response.drag_stopped() || (!response.dragged() && !response.is_pointer_button_down_on()) {
                self.drag = None;
            }
            if let Some(p) = pointer.filter(|_| response.is_pointer_button_down_on() || response.clicked()) {
                let i = (((p.x - rect.left()) / column).floor().max(0.0) as usize).min(frames.saturating_sub(1));
                if response.dragged() || response.drag_started() || response.clicked() {
                    let (drawing, from) = match self.drag {
                        Some(d) => d,
                        None => {
                            // From where the button went down: a drag is only seen once the pointer's moved a little.
                            let origin = ui.input(|input| input.pointer.press_origin()).unwrap_or(p);
                            let start = (((origin.x - rect.left()) / column).floor().max(0.0) as usize).min(frames.saturating_sub(1));
                            self.remember();
                            self.frame = i;
                            (if origin.y < split { Drawing::Pitch } else { Drawing::Volume }, start)
                        }
                    };
                    let (low, high) = (from.min(i), from.max(i));
                    let t = ((p.y - pitch_rect.top()) / pitch_rect.height()).clamp(0.0, 1.0) as f64;
                    let hz = LOWEST_HZ * (HIGHEST_HZ / LOWEST_HZ).powf(1.0 - t);
                    let level = 1.0 - ((p.y - volume_rect.top()) / volume_rect.height()).clamp(0.0, 1.0);
                    for j in low..=high {
                        let frame = &mut self.effect_mut().frames[j];
                        match drawing {
                            Drawing::Pitch => set_note(machine, frame, hz),
                            Drawing::Volume => set_level(chip, frame, level),
                        }
                    }
                    self.drag = if response.clicked() { None } else { Some((drawing, i)) };
                    self.changed = true;
                }
            }
            if let Some(p) = response.hover_pos() {
                let i = (((p.x - rect.left()) / column).floor().max(0.0) as usize).min(frames.saturating_sub(1));
                let f = self.effect().frames[i];
                let note = sfx::hz(machine, &f).map_or("no note".into(), |h| format!("{h:.0} Hz, {}", sfx::note_name(h)));
                response.on_hover_text(crate::i18n::tf!("Frame {i}: {note}"));
            }
        });
    }

    /// For the SID, its envelope at the end of each frame, from 0 to 1, worked out once for each version of the effect.
    fn sid_levels(&mut self) -> Vec<f32> {
        if self.chip() != Chip::Sid {
            return Vec::new();
        }
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (self.bank.machine, self.effect().adsr, self.effect().frames.iter().map(|f| f.tone).collect::<Vec<_>>()).hash(&mut hasher);
        let key = hasher.finish();
        if let Some((k, levels)) = &self.levels
            && *k == key
        {
            return levels.clone();
        }
        let levels = sfx::sound::sid_levels(self.bank.machine, self.effect());
        self.levels = Some((key, levels.clone()));
        levels
    }

    /// The chosen frame's values to type, and the effect's length.
    fn frame_values(&mut self, ui: &mut Ui) {
        let chip = self.chip();
        let machine = self.bank.machine;
        let count = self.effect().frames.len();
        self.frame = self.frame.min(count.saturating_sub(1));
        let mut frame = self.effect().frames[self.frame];
        let mut adsr = self.effect().adsr;
        let before = (frame, adsr);
        ui.horizontal(|ui| {
            ui.label(RichText::new(crate::i18n::tf!("Frame {}", self.frame)).strong());
            if ui.small_button(crate::i18n::tr("\u{25C0}")).clicked() {
                self.frame = self.frame.saturating_sub(1);
            }
            if ui.small_button(crate::i18n::tr("\u{25B6}")).clicked() {
                self.frame = (self.frame + 1).min(count - 1);
            }
            let note = sfx::hz(machine, &frame).map_or("no note".into(), |h| format!("{h:.1} Hz, {}", sfx::note_name(h)));
            ui.label(RichText::new(note).monospace());
        });
        ui.horizontal_wrapped(|ui| match chip {
            Chip::Ay => {
                ui.checkbox(&mut frame.tone, crate::i18n::tr("Tone"));
                ui.checkbox(&mut frame.noise, crate::i18n::tr("Noise"));
                ui.label(crate::i18n::tr("Period"));
                ui.add(egui::DragValue::new(&mut frame.period).range(1..=4095).hexadecimal(3, false, true).prefix("$"));
                ui.label(crate::i18n::tr("Noise"));
                ui.add(egui::DragValue::new(&mut frame.noise_period).range(0..=31));
                ui.label(crate::i18n::tr("Volume"));
                ui.add(egui::DragValue::new(&mut frame.volume).range(0..=15));
            }
            Chip::Sid => {
                ui.checkbox(&mut frame.tone, crate::i18n::tr("Gate"));
                for (bit, name) in [(0x10u8, "Tri"), (0x20, "Saw"), (0x40, "Pulse"), (0x80, "Noise")] {
                    let mut on = frame.wave & bit != 0;
                    if ui.checkbox(&mut on, crate::i18n::tr(name)).changed() {
                        frame.wave = if on { frame.wave | bit } else { frame.wave & !bit };
                    }
                }
                ui.label(crate::i18n::tr("Frequency"));
                ui.add(egui::DragValue::new(&mut frame.period).hexadecimal(4, false, true).prefix("$"));
                ui.label(crate::i18n::tr("Pulse"));
                ui.add(egui::DragValue::new(&mut frame.width).range(0..=4095).hexadecimal(3, false, true).prefix("$"));
            }
            Chip::Tia => {
                ui.label(crate::i18n::tr("AUDC"));
                egui::ComboBox::from_id_salt("sfx-audc").width(150.0).selected_text(crate::i18n::tf!("{}: {}", frame.wave, tia_kind(frame.wave))).show_ui(ui, |ui| {
                    for c in 0..16u8 {
                        ui.selectable_value(&mut frame.wave, c, crate::i18n::tf!("{c}: {}", tia_kind(c)));
                    }
                });
                ui.label(crate::i18n::tr("AUDF"));
                ui.add(egui::DragValue::new(&mut frame.period).range(0..=31));
                ui.label(crate::i18n::tr("AUDV"));
                ui.add(egui::DragValue::new(&mut frame.volume).range(0..=15));
            }
            Chip::Beeper => {
                ui.checkbox(&mut frame.tone, crate::i18n::tr("Sounding"));
                ui.checkbox(&mut frame.noise, crate::i18n::tr("Noise"));
                ui.label(crate::i18n::tr("Wave"));
                ui.add(egui::DragValue::new(&mut frame.period).range(2..=60_000).suffix(" \u{D7} 26 T"));
                ui.label(crate::i18n::tr("Width"));
                ui.add(egui::DragValue::new(&mut frame.volume).range(0..=15)).on_hover_text(crate::i18n::tr("How wide the high half of each wave is: 15 is square, the loudest; less is thinner and quieter"));
            }
        });
        if chip == Chip::Sid {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Envelope"));
                let mut parts = [adsr[0] >> 4, adsr[0] & 15, adsr[1] >> 4, adsr[1] & 15];
                for (part, name) in parts.iter_mut().zip(["A", "D", "S", "R"]) {
                    ui.label(crate::i18n::tr(name));
                    ui.add(egui::DragValue::new(part).range(0..=15));
                }
                adsr = [parts[0] << 4 | parts[1], parts[2] << 4 | parts[3]];
                ui.label(RichText::new(crate::i18n::tf!("attack {} ms, release {} ms", sfx::SID_ATTACK_MS[usize::from(parts[0])], sfx::SID_ATTACK_MS[usize::from(parts[3])] * 3.0)).size(11.0).color(crate::theme::text_dim()));
            });
        }
        if (frame, adsr) != before {
            self.remember();
            let at = self.frame;
            let effect = self.effect_mut();
            effect.frames[at] = frame.clamped(chip);
            effect.adsr = adsr;
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button(crate::i18n::tr("Add a frame")).on_hover_text(crate::i18n::tr("A copy of this frame after it")).clicked() && count < sfx::MOST_FRAMES {
                self.remember();
                let at = self.frame;
                let copy = self.effect().frames[at];
                self.effect_mut().frames.insert(at + 1, copy);
                self.frame += 1;
            }
            if ui.add_enabled(count > 1, egui::Button::new(crate::i18n::tr("Delete the frame"))).clicked() {
                self.remember();
                let at = self.frame;
                self.effect_mut().frames.remove(at);
                self.frame = self.frame.min(count - 2);
            }
            if ui.button(crate::i18n::tr("Quiet")).on_hover_text(crate::i18n::tr("This frame makes no sound")).clicked() {
                self.remember();
                let at = self.frame;
                let quiet = Frame { period: self.effect().frames[at].period, ..Frame::quiet(chip) };
                self.effect_mut().frames[at] = quiet;
            }
            let mut length = count;
            ui.label(crate::i18n::tr("Frames"));
            if ui.add(egui::DragValue::new(&mut length).range(1..=sfx::MOST_FRAMES)).changed() && length != count {
                self.remember();
                let last = *self.effect().frames.last().unwrap_or(&Frame::quiet(chip));
                self.effect_mut().frames.resize(length, Frame { volume: 0, tone: chip != Chip::Sid && last.tone, ..last });
            }
            if ui.button(crate::i18n::tr("Backwards")).on_hover_text(crate::i18n::tr("The frames in the other order")).clicked() {
                self.remember();
                self.effect_mut().frames.reverse();
            }
        });
    }

    /// The shape, with its presets: changing it makes the chosen effect again.
    fn shape_controls(&mut self, ui: &mut Ui) {
        ui.label(RichText::new(crate::i18n::tr("Make it from a shape")).strong());
        ui.label(RichText::new(crate::i18n::tr("Changing these makes the chosen effect again.")).size(11.0).color(crate::theme::text_dim()));
        let before = self.shape;
        egui::Grid::new("sfx-presets").num_columns(3).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
            for (i, preset) in Preset::ALL.iter().enumerate() {
                if ui.button(crate::i18n::tr(preset.name())).clicked() {
                    self.shape = Shape::preset(*preset);
                }
                if i % 3 == 2 {
                    ui.end_row();
                }
            }
            if ui.button(crate::i18n::tr("Random")).on_hover_text(crate::i18n::tr("Something new, by chance")).clicked() {
                self.seed = self.seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                self.shape = Shape::random(self.seed);
            }
            if ui.button(crate::i18n::tr("Vary")).on_hover_text(crate::i18n::tr("This shape, changed a little")).clicked() {
                self.seed = self.seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                self.shape = self.shape.vary(self.seed, 0.3);
            }
        });
        ui.add_space(6.0);
        let s = &mut self.shape;
        ui.add(egui::Slider::new(&mut s.frames, 1..=150).text(crate::i18n::tr("frames")));
        ui.add(egui::Slider::new(&mut s.pitch, 40.0..=6000.0).logarithmic(true).text(crate::i18n::tr("Hz to start")));
        ui.add(egui::Slider::new(&mut s.slide, -6.0..=6.0).text(crate::i18n::tr("slide"))).on_hover_text(crate::i18n::tr("Semitones a frame: up when more than 0"));
        ui.add(egui::Slider::new(&mut s.curve, -0.5..=0.5).text(crate::i18n::tr("curve"))).on_hover_text(crate::i18n::tr("Added to the slide each frame"));
        ui.add(egui::Slider::new(&mut s.vibrato, 0.0..=6.0).text(crate::i18n::tr("vibrato"))).on_hover_text(crate::i18n::tr("Semitones either way"));
        ui.add(egui::Slider::new(&mut s.vibrato_frames, 2.0..=30.0).text(crate::i18n::tr("frames a wobble")));
        ui.add(egui::Slider::new(&mut s.jump, -12.0..=12.0).step_by(1.0).text(crate::i18n::tr("jump"))).on_hover_text(crate::i18n::tr("Semitones, once"));
        ui.add(egui::Slider::new(&mut s.jump_at, 0..=60).text(crate::i18n::tr("jump at frame"))).on_hover_text(crate::i18n::tr("0 for no jump"));
        ui.add(egui::Slider::new(&mut s.repeat, 0..=30).text(crate::i18n::tr("repeat every"))).on_hover_text(crate::i18n::tr("Frames: the slide starts again, for arpeggios and sirens"));
        ui.add(egui::Slider::new(&mut s.attack, 0..=40).text(crate::i18n::tr("attack")));
        ui.add(egui::Slider::new(&mut s.hold, 0..=150).text(crate::i18n::tr("hold")));
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Noise"));
            egui::ComboBox::from_id_salt("sfx-noise").selected_text(crate::i18n::tr(s.noise.name())).show_ui(ui, |ui| {
                for n in Noise::ALL {
                    ui.selectable_value(&mut s.noise, n, crate::i18n::tr(n.name()));
                }
            });
        });
        ui.add(egui::Slider::new(&mut s.noise_pitch, 0.0..=1.0).text(crate::i18n::tr("noise pitch")));
        ui.add(egui::Slider::new(&mut s.noise_slide, -0.05..=0.05).text(crate::i18n::tr("noise slide")));
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Wave"));
            egui::ComboBox::from_id_salt("sfx-wave").selected_text(crate::i18n::tr(s.wave.name())).show_ui(ui, |ui| {
                for w in Wave::ALL {
                    ui.selectable_value(&mut s.wave, w, crate::i18n::tr(w.name()));
                }
            });
        })
        .response
        .on_hover_text(crate::i18n::tr("For the SID and the TIA"));
        ui.add(egui::Slider::new(&mut s.width, 0.05..=0.5).text(crate::i18n::tr("pulse width"))).on_hover_text(crate::i18n::tr("The SID's pulse and the beeper's width: 0.5 is square"));
        if self.shape != before {
            self.make_from_shape();
        }
        ui.add_space(4.0);
        if ui.button(crate::i18n::tr("Make it again")).on_hover_text(crate::i18n::tr("The chosen effect, made from this shape")).clicked() {
            self.make_from_shape();
        }
    }
}

/// The rate the speakers are likely to play at, before they're asked.
fn cpal_rate() -> u32 {
    48_000
}

/// What each of the TIA's sixteen AUDC settings sounds like.
pub fn tia_kind(audc: u8) -> &'static str {
    match audc & 15 {
        0 | 11 => "silent (set to 1)",
        1 => "buzzy (4-bit poly)",
        2 => "low rumble (div 31 \u{2192} poly4)",
        3 => "rumble (poly5 \u{2192} poly4)",
        4 | 5 => "pure, high",
        6 | 10 => "pure, low (div 31)",
        7 | 9 => "rattle (5-bit poly)",
        8 => "white noise (9-bit poly)",
        12 | 13 => "pure, middle (div 6)",
        14 => "pure, lowest (div 93)",
        _ => "low rattle (poly5 div 6)",
    }
}

/// A frame given a note: tone on, and the period that comes nearest.
fn set_note(machine: Machine, frame: &mut Frame, hz: f64) {
    let chip = machine.chip();
    match chip {
        Chip::Tia => {
            if sfx::tia_ticks(frame.wave).is_none() {
                frame.wave = 4;
            }
        }
        Chip::Sid => {
            if frame.wave & 0xF0 == 0 || frame.wave & 0x80 != 0 {
                frame.wave = 0x40;
            }
        }
        _ => {
            frame.tone = true;
            if chip == Chip::Beeper {
                frame.noise = false;
            }
        }
    }
    frame.period = sfx::period_for(machine, hz, frame);
    if frame.volume == 0 && chip != Chip::Sid {
        frame.volume = if chip == Chip::Beeper { 15 } else { 12 };
    }
    *frame = frame.clamped(chip);
}

/// A frame given a loudness from 0 to 1 (for the SID, its gate on or off).
fn set_level(chip: Chip, frame: &mut Frame, level: f32) {
    match chip {
        Chip::Sid => frame.tone = level > 0.5,
        _ => {
            frame.volume = (level * 15.0).round() as u8;
            if (chip == Chip::Beeper || chip == Chip::Ay) && frame.volume > 0 && !frame.tone && !frame.noise {
                frame.tone = true;
            }
        }
    }
}
