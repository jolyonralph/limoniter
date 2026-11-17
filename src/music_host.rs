//! The preview's player for music and sound: tracker modules (ProTracker, Scream Tracker 3, FastTracker 2 and
//! Impulse Tracker) played by Limoniter's own player, and sound files (MP3, WAV, FLAC, Ogg, AIFF, 8SVX, VOC, AU).
//! Play and pause, stop, loop, and a slider to move through it; for a module, the pattern as it plays, with each
//! channel's number to switch it off and on. It goes on playing while other files are worked on, until it's
//! stopped, another is played, or its file is closed.

use super::{Limoniter, theme};
use crate::sound::{SoundPlayer, clock};
use crate::tracker::{Song, play::Player};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use eframe::egui::{self, Button, RichText, Ui};
use std::sync::{Arc, Mutex, PoisonError};

enum Source {
    Tracker(Box<Player>),
    Sound(Box<SoundPlayer>),
}

impl Source {
    fn render(&mut self, out: &mut [f32]) {
        match self {
            Source::Tracker(player) => player.render(out),
            Source::Sound(player) => player.render(out),
        }
    }

    fn time(&self) -> f64 {
        match self {
            Source::Tracker(player) => player.time,
            Source::Sound(player) => player.time(),
        }
    }

    fn ended(&self) -> bool {
        match self {
            Source::Tracker(player) => player.ended,
            Source::Sound(player) => player.ended(),
        }
    }
}

/// What the speakers' thread and the window share.
struct Shared {
    source: Source,
    paused: bool,
    looping: bool,
}

pub struct Music {
    /// The document it's playing, and the version of it.
    doc: u64,
    shared: Arc<Mutex<Shared>>,
    _stream: cpal::Stream,
    rate: u32,
    length: Option<f64>,
    song: Option<Arc<Song>>,
    /// Channels switched off, for a module.
    silenced: Vec<bool>,
    /// Where the slider was let go, to seek to once it's let go of.
    dragging: Option<f64>,
}

fn lock(shared: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The speakers, and the rate they play at.
fn speakers() -> Result<(cpal::Device, cpal::SupportedStreamConfig), String> {
    let device = cpal::default_host().default_output_device().ok_or(crate::i18n::tr("There's no sound device to play through."))?;
    let config = device.default_output_config().map_err(|e| crate::i18n::tf!("The sound device can't be used ({e})."))?;
    Ok((device, config))
}

fn stream<T: SizedSample + FromSample<f32>>(device: &cpal::Device, config: &cpal::StreamConfig, shared: Arc<Mutex<Shared>>) -> Result<cpal::Stream, String> {
    let channels = usize::from(config.channels).max(1);
    let mut scratch: Vec<f32> = Vec::new();
    device
        .build_output_stream::<T, _, _>(
            *config,
            move |data: &mut [T], _| {
                let frames = data.len() / channels;
                scratch.resize(frames * 2, 0.0);
                {
                    let mut shared = lock(&shared);
                    if shared.paused {
                        scratch.fill(0.0);
                    } else {
                        shared.source.render(&mut scratch);
                        // A sound file that's to loop starts again as it ends; a module loops by itself.
                        if shared.looping
                            && let Source::Sound(player) = &mut shared.source
                            && player.ended()
                        {
                            player.seek(0.0);
                        }
                    }
                }
                for (frame, sample) in data.chunks_mut(channels).zip(scratch.as_chunks::<2>().0.iter()) {
                    for (channel, out) in frame.iter_mut().enumerate() {
                        *out = T::from_sample(if channel % 2 == 0 { sample[0] } else { sample[1] });
                    }
                }
            },
            |_| {},
            None,
        )
        .map_err(|e| crate::i18n::tf!("The sound device wouldn't play ({e})."))
}

/// What a file is to be played as: a module, or a sound file.
pub fn playable(head: &[u8], name: &str) -> bool {
    crate::tracker::pc_format(head).is_some() || crate::protracker::has_module_tag(head) || crate::protracker::named_as_module(name) || crate::sound::recognised(head, name)
}

impl Music {
    fn start(doc: u64, bytes: Vec<u8>, name: &str) -> Result<Music, String> {
        let (device, config) = speakers()?;
        let rate = config.sample_rate();
        let tracker = crate::tracker::pc_format(&bytes).is_some() || (!crate::sound::recognised(&bytes, name) && (crate::protracker::has_module_tag(&bytes) || crate::protracker::named_as_module(name)));
        let (source, length, song) = if tracker {
            let song = Arc::new(crate::tracker::load(&bytes, name)?);
            let length = Player::duration(&song);
            (Source::Tracker(Box::new(Player::new(song.clone(), rate))), length, Some(song))
        } else {
            let player = SoundPlayer::new(bytes, name, rate)?;
            let length = player.info.seconds;
            (Source::Sound(Box::new(player)), length, None)
        };
        let channels = song.as_ref().map_or(0, |s| s.channels);
        let shared = Arc::new(Mutex::new(Shared { source, paused: false, looping: false }));
        let config: cpal::StreamConfig = config.config();
        let stream = match device.default_output_config().map(|c| c.sample_format()).unwrap_or(SampleFormat::F32) {
            SampleFormat::F32 => stream::<f32>(&device, &config, shared.clone()),
            SampleFormat::I16 => stream::<i16>(&device, &config, shared.clone()),
            SampleFormat::U16 => stream::<u16>(&device, &config, shared.clone()),
            SampleFormat::I32 => stream::<i32>(&device, &config, shared.clone()),
            other => Err(crate::i18n::tf!("The sound device wants samples as {other}, which Limoniter doesn't make.")),
        }?;
        stream.play().map_err(|e| crate::i18n::tf!("The sound device wouldn't start ({e})."))?;
        Ok(Music { doc, shared, _stream: stream, rate, length, song, silenced: vec![false; channels], dragging: None })
    }

    /// Goes to a time: a module is played again from the start without a sound up to it.
    fn seek(&mut self, seconds: f64) {
        let mut shared = lock(&self.shared);
        let looping = shared.looping;
        match &mut shared.source {
            Source::Tracker(player) => {
                let mut fresh = Player::new(player.song.clone(), self.rate);
                fresh.looping = looping;
                fresh.silenced = self.silenced.clone();
                fresh.seek(seconds);
                **player = fresh;
            }
            Source::Sound(player) => player.seek(seconds),
        }
    }
}

impl Limoniter {
    /// Stops the music when its file is closed.
    pub(super) fn music_housekeeping(&mut self) {
        if let Some(music) = &self.music
            && !self.docs.iter().any(|d| d.id == music.doc)
        {
            self.music = None;
        }
    }

    /// Starts the file in front playing, paused at a time: for pictures of the player.
    pub(super) fn play_music_at(&mut self, seconds: f64) {
        let doc = &self.docs[self.active];
        let bytes = if doc.hex_mode { doc.hex.read(0, doc.hex.len()) } else { doc.text.encode(&doc.text.format, crate::codec::Lossy::Omit).unwrap_or_default() };
        match Music::start(doc.id, bytes, &doc.name()) {
            Ok(mut music) => {
                music.seek(seconds);
                lock(&music.shared).paused = true;
                self.music = Some(music);
            }
            Err(why) => self.music_note = Some(why),
        }
    }

    /// The player above a module's or sound file's preview.
    pub(super) fn music_bar(&mut self, ui: &mut Ui) {
        let doc = &self.docs[self.active];
        let (id, name) = (doc.id, doc.name());
        let ours = self.music.as_ref().is_some_and(|m| m.doc == id);
        if !ours {
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("\u{25B6} Play")).on_hover_text(crate::i18n::tr("Play it through the computer's speakers")).clicked() {
                    let doc = &self.docs[self.active];
                    let bytes = if doc.hex_mode { doc.hex.read(0, doc.hex.len()) } else { doc.text.encode(&doc.text.format, crate::codec::Lossy::Omit).unwrap_or_default() };
                    match Music::start(id, bytes, &name) {
                        Ok(music) => {
                            self.music = Some(music);
                            self.music_note = None;
                        }
                        Err(why) => self.music_note = Some(why),
                    }
                }
                if let Some(note) = &self.music_note {
                    ui.label(RichText::new(note).color(theme::text_faint()));
                } else if self.music.is_some() {
                    ui.label(RichText::new(crate::i18n::tr("Another file is playing; this stops it.")).color(theme::text_faint()));
                }
            });
            ui.add_space(4.0);
            return;
        }
        let Some(music) = &mut self.music else { return };
        let (paused, looping, time, ended) = {
            let shared = lock(&music.shared);
            (shared.paused, shared.looping, shared.source.time(), shared.source.ended())
        };
        let mut stop = false;
        ui.horizontal(|ui| {
            let (label, hint) = if paused || ended { ("\u{25B6} Play", "Carry on playing") } else { ("\u{23F8} Pause", "Pause, to carry on from here") };
            if ui.button(crate::i18n::tr(label)).on_hover_text(crate::i18n::tr(hint)).clicked() {
                if ended {
                    music.seek(0.0);
                }
                let mut shared = lock(&music.shared);
                shared.paused = !(paused || ended);
            }
            if ui.button(crate::i18n::tr("\u{25A0} Stop")).on_hover_text(crate::i18n::tr("Stop playing")).clicked() {
                stop = true;
            }
            let mut looped = looping;
            if ui.checkbox(&mut looped, crate::i18n::tr("Loop")).on_hover_text(crate::i18n::tr("Start again from where the song loops back to, or from the start, when it ends")).changed() {
                let mut shared = lock(&music.shared);
                shared.looping = looped;
                if let Source::Tracker(player) = &mut shared.source {
                    player.looping = looped;
                }
            }
            let shown = music.dragging.unwrap_or(time);
            let total = music.length.map_or_else(|| "?".to_string(), clock);
            ui.label(RichText::new(crate::i18n::tf!("{} / {total}", clock(shown))).monospace());
        });
        if let Some(length) = music.length.filter(|l| *l > 0.0) {
            let mut at = music.dragging.unwrap_or(time).min(length);
            let width = ui.available_width();
            ui.spacing_mut().slider_width = (width - 16.0).max(80.0);
            let response = ui.add(egui::Slider::new(&mut at, 0.0..=length).show_value(false));
            if response.dragged() || response.changed() {
                music.dragging = Some(at);
            }
            if response.drag_stopped() || (response.changed() && !response.dragged()) {
                music.dragging = None;
                music.seek(at);
            }
        }
        if let Some(song) = music.song.clone() {
            let place = match &lock(&music.shared).source {
                Source::Tracker(player) => player.place(),
                Source::Sound(_) => Default::default(),
            };
            pattern_view(ui, &song, place, &mut music.silenced, &music.shared);
        }
        if !paused && !ended {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
        }
        if stop {
            self.music = None;
        }
        ui.add_space(4.0);
    }
}

/// The rows around the one playing, with the channels' numbers to switch them off and on.
fn pattern_view(ui: &mut Ui, song: &Song, place: crate::tracker::play::Place, silenced: &mut [bool], shared: &Mutex<Shared>) {
    let Some(pattern) = song.pattern(place.pattern) else { return };
    let playing = song.orders.iter().filter(|&&p| p < 254).count();
    ui.label(RichText::new(crate::i18n::tf!("Position {} of {playing} \u{B7} pattern {} \u{B7} row {}", place.position + 1, place.pattern, place.row)).color(theme::text_dim()).size(12.5));
    const AROUND: usize = 6;
    let font = egui::FontId::monospace(12.0);
    egui::ScrollArea::horizontal().id_salt("music-pattern").show(ui, |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            // Each channel's number sits over its column: the cells are all the same number of characters.
            let char_width = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
            let cell = song.cell_text(&Default::default()).chars().count() + 1;
            let height = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.add_space(char_width * 3.0);
                for (c, off) in silenced.iter_mut().enumerate() {
                    let label = format!("{:^width$}", c + 1, width = cell - 1);
                    let text = if *off { RichText::new(label).font(font.clone()).color(theme::text_faint()).strikethrough() } else { RichText::new(label).font(font.clone()).color(theme::text_dim()) };
                    let button = Button::new(text).frame(false);
                    if ui.add_sized([char_width * cell as f32, height], button).on_hover_text(if *off { crate::i18n::tr("Switch this channel back on") } else { crate::i18n::tr("Switch this channel off") }).clicked() {
                        *off = !*off;
                        changed = true;
                    }
                }
            });
            if changed && let Source::Tracker(player) = &mut lock(shared).source {
                player.silenced = silenced.to_vec();
            }
            for row in place.row.saturating_sub(AROUND)..(place.row + AROUND + 1).min(pattern.rows) {
                let cells: Vec<String> = (0..song.channels).map(|c| song.cell_text(&song.cell(pattern, row, c))).collect();
                let line = format!("{row:02X} {}", cells.join(" "));
                let text = RichText::new(line).font(font.clone());
                let text = if row == place.row { text.background_color(theme::selection()).color(theme::text()) } else { text.color(theme::text_dim()) };
                ui.add(egui::Label::new(text).extend());
            }
        });
    });
    ui.add_space(4.0);
}
