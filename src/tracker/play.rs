//! Plays a [`Song`]: the rows read tick by tick with each tracker's effects, and the samples mixed into stereo.
//!
//! Channels hold what the effects work on (the note's pitch, its volume, the effects' memories); voices are the
//! samples sounding. A channel has one voice in front, and Impulse Tracker's instruments can leave notes playing
//! behind it, which is why the two are kept apart.

use super::{Cell, Format, Fx, Loop, NOTE_CUT, NOTE_FADE, NOTE_OFF, NewNote, Song, Vol};
use std::sync::Arc;

/// ProTracker's vibrato and tremolo sine, a whole cycle in 64 steps, from -255 to 255.
fn wave(kind: u8, position: u8) -> i32 {
    let p = i32::from(position & 63);
    match kind & 3 {
        0 => ((p as f32) * std::f32::consts::TAU / 64.0).sin().mul_add(255.0, 0.0).round() as i32,
        1 => 255 - p * 8,
        2 => {
            if p < 32 {
                255
            } else {
                -255
            }
        }
        // Random, as close as matters: a fixed jumble.
        _ => ((p * 97 + 31) % 511) - 255,
    }
}

/// Leaves quiet sound alone and rounds off the loudest, so it never goes past full scale with a hard click.
fn limit(x: f32) -> f32 {
    const KNEE: f32 = 0.8;
    let size = x.abs();
    if size <= KNEE { x } else { x.signum() * (KNEE + (1.0 - KNEE) * ((size - KNEE) / (1.0 - KNEE)).tanh()) }
}

/// How long a song can run, in seconds, before it's taken to go on forever.
const LONGEST_SONG: f64 = 60.0 * 60.0;
/// How many notes left behind can sound at once before the quietest are let go.
const MOST_VOICES: usize = 96;
/// Samples over which a change of volume is spread, so it doesn't click.
const RAMP: usize = 64;

#[derive(Clone, Default)]
struct Voice {
    sample: usize,
    instrument: Option<usize>,
    position: f64,
    backwards: bool,
    on: bool,
    /// Samples a second, before the arpeggio and vibrato.
    rate: f64,
    /// From the channel: 0 to 1 before envelopes; panning 0 to 1.
    volume: f32,
    pan: f32,
    surround: bool,
    released: bool,
    fading: bool,
    fade: f32,
    volume_tick: u16,
    pan_tick: u16,
    /// Set for notes left behind, and for those being quickly faded out to stop a click.
    background: bool,
    vanishing: bool,
    /// What each side was mixed at last, for the ramp to the next.
    left: f32,
    right: f32,
    target_left: f32,
    target_right: f32,
    ramp: usize,
}

#[derive(Clone, Default)]
struct Channel {
    voice: Option<usize>,
    instrument: usize,
    sample: usize,
    note: u8,
    period: f64,
    target: f64,
    porta_speed: u8,
    volume: i32,
    pan: i32,
    channel_volume: i32,
    finetune: Option<i16>,
    vibrato_position: u8,
    vibrato_speed: u8,
    vibrato_depth: u8,
    vibrato_wave: u8,
    tremolo_position: u8,
    tremolo_speed: u8,
    tremolo_depth: u8,
    tremolo_wave: u8,
    vibrato_now: f64,
    tremolo_now: i32,
    arpeggio_now: f64,
    tremor_count: u8,
    tremor_off: bool,
    retrig_count: u8,
    loop_row: usize,
    loop_count: u8,
    high_offset: u8,
    /// Each effect's last non-zero parameter.
    memory: [u8; 24],
    cell: Cell,
    /// The note delay's tick, while the row's note waits for it.
    delayed: Option<u8>,
    muted: bool,
    surround: bool,
}

/// The effects' memory slots.
mod slot {
    pub const VOLSLIDE: usize = 0;
    pub const PORTA_UP: usize = 1;
    pub const PORTA_DOWN: usize = 2;
    pub const TONE: usize = 3;
    pub const VIBRATO: usize = 4;
    pub const TREMOLO: usize = 5;
    pub const OFFSET: usize = 6;
    pub const RETRIG: usize = 7;
    pub const TREMOR: usize = 8;
    pub const ARPEGGIO: usize = 9;
    pub const GLOBAL_SLIDE: usize = 10;
    pub const PAN_SLIDE: usize = 11;
    pub const CHANNEL_SLIDE: usize = 12;
    pub const FINE_UP: usize = 13;
    pub const FINE_DOWN: usize = 14;
    pub const FINE_VOL_UP: usize = 15;
    pub const FINE_VOL_DOWN: usize = 16;
    pub const EXTRA_UP: usize = 17;
    pub const EXTRA_DOWN: usize = 18;
    pub const TEMPO: usize = 19;
    /// Scream Tracker's one memory for all of them.
    pub const SHARED: usize = 20;
}

/// Where the song is: its position in the order, the pattern there, and the row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Place {
    pub position: usize,
    pub pattern: u8,
    pub row: usize,
}

pub struct Player {
    pub song: Arc<Song>,
    rate: f64,
    channels: Vec<Channel>,
    voices: Vec<Voice>,
    position: usize,
    row: usize,
    tick: u32,
    speed: u32,
    tempo: u32,
    global_volume: i32,
    pattern_delay: u32,
    fine_delay: u32,
    next: Option<(usize, usize)>,
    /// Samples left in this tick.
    left_in_tick: usize,
    /// Rows played already (by position and row), to tell when the song comes round again.
    visited: Vec<Vec<bool>>,
    pub ended: bool,
    /// Whether it starts again from its restart position when it ends.
    pub looping: bool,
    /// Seconds played.
    pub time: f64,
    /// Channels switched off by whoever's listening.
    pub silenced: Vec<bool>,
}

impl Player {
    pub fn new(song: Arc<Song>, rate: u32) -> Player {
        let channels = (0..song.channels)
            .map(|c| Channel { pan: i32::from(song.pans[c]), channel_volume: i32::from(song.channel_volumes[c]), muted: song.muted[c], surround: song.surround[c], ..Channel::default() })
            .collect();
        let visited = song.orders.iter().map(|&p| vec![false; song.pattern(p).map_or(0, |p| p.rows)]).collect();
        let mut player = Player {
            rate: f64::from(rate.max(1000)),
            channels,
            voices: Vec::new(),
            position: 0,
            row: 0,
            tick: 0,
            speed: u32::from(song.speed.max(1)),
            tempo: u32::from(song.tempo.max(32)),
            global_volume: i32::from(song.global_volume),
            pattern_delay: 0,
            fine_delay: 0,
            next: None,
            left_in_tick: 0,
            visited,
            ended: false,
            looping: false,
            time: 0.0,
            silenced: vec![false; song.channels],
            song,
        };
        player.skip_markers();
        player.mark_visited();
        player
    }

    pub fn place(&self) -> Place {
        let pattern = self.song.orders.get(self.position).copied().unwrap_or(0);
        Place { position: self.position, pattern, row: self.row }
    }

    fn tick_samples(&self) -> usize {
        (self.rate * 2.5 / f64::from(self.tempo)).round().max(1.0) as usize
    }

    /// Plays on without mixing until `seconds` in, as when the slider is moved. Returns false if the song ends first.
    pub fn seek(&mut self, seconds: f64) -> bool {
        while self.time < seconds && !self.ended {
            self.process_tick();
            self.time += 2.5 / f64::from(self.tempo);
        }
        for voice in &mut self.voices {
            voice.ramp = 0;
            voice.left = voice.target_left;
            voice.right = voice.target_right;
        }
        !self.ended
    }

    /// How long the song plays once through, worked out by playing it without mixing; None for one that never
    /// ends (it's taken to stop after an hour).
    pub fn duration(song: &Arc<Song>) -> Option<f64> {
        let mut player = Player::new(song.clone(), 8000);
        while !player.ended && player.time < LONGEST_SONG {
            player.process_tick();
            player.time += 2.5 / f64::from(player.tempo);
        }
        player.ended.then_some(player.time)
    }

    /// Mixes into `out`, left and right one after the other. After the end, it's silence.
    pub fn render(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let mut done = 0;
        let frames = out.len() / 2;
        while done < frames {
            if self.left_in_tick == 0 {
                if self.ended {
                    return;
                }
                self.process_tick();
                self.left_in_tick = self.tick_samples();
                self.time += self.left_in_tick as f64 / self.rate;
            }
            let count = self.left_in_tick.min(frames - done);
            self.mix(&mut out[done * 2..(done + count) * 2]);
            done += count;
            self.left_in_tick -= count;
        }
    }

    fn mix(&mut self, out: &mut [f32]) {
        let song = &self.song;
        // Each voice at the same level however many channels there are, as libopenmpt mixes them; a busy song
        // is kept from clipping by the limiter below rather than turned down.
        let gain = if song.format == Format::Mod && song.channels == 4 { 0.34 } else { 0.28 };
        for voice in self.voices.iter_mut().filter(|v| v.on) {
            let Some(sample) = song.samples.get(voice.sample) else {
                voice.on = false;
                continue;
            };
            let data = &sample.data;
            if data.is_empty() {
                voice.on = false;
                continue;
            }
            let (loop_kind, start, end) = match sample.sustain {
                Some((s, e, kind)) if !voice.released => (kind, s, e),
                _ => (sample.loop_kind, sample.loop_start, sample.loop_end),
            };
            let looping = loop_kind != Loop::None && end > start;
            let length = if looping { end } else { data.len() };
            let step = voice.rate / self.rate;
            for frame in out.as_chunks_mut::<2>().0 {
                let at = voice.position as usize;
                if at >= length && !looping {
                    voice.on = false;
                    break;
                }
                let along = (voice.position - at as f64) as f32;
                let a = f32::from(data[at.min(data.len() - 1)]);
                let next = if at + 1 < length {
                    at + 1
                } else if looping && loop_kind == Loop::Forward {
                    start
                } else {
                    at
                };
                let b = f32::from(data[next.min(data.len() - 1)]);
                let value = (a + (b - a) * along) / 32768.0;
                if voice.ramp > 0 {
                    voice.left += (voice.target_left - voice.left) / voice.ramp as f32;
                    voice.right += (voice.target_right - voice.right) / voice.ramp as f32;
                    voice.ramp -= 1;
                }
                frame[0] += value * voice.left * gain;
                frame[1] += value * voice.right * gain;
                if voice.backwards {
                    voice.position -= step;
                    if voice.position < start as f64 {
                        voice.position = 2.0 * start as f64 - voice.position;
                        voice.backwards = false;
                    }
                } else {
                    voice.position += step;
                    if looping && voice.position >= end as f64 {
                        if loop_kind == Loop::PingPong {
                            voice.position = (2.0 * end as f64 - voice.position - 1.0).max(start as f64);
                            voice.backwards = true;
                        } else {
                            let span = (end - start) as f64;
                            voice.position = start as f64 + (voice.position - end as f64) % span;
                        }
                    }
                }
            }
            if voice.vanishing && voice.ramp == 0 {
                voice.on = false;
            }
        }
        for sample in out.iter_mut() {
            *sample = limit(*sample);
        }
    }

    fn skip_markers(&mut self) {
        let mut guard = 0;
        while guard < 512 {
            match self.song.orders.get(self.position) {
                Some(&254) => self.position += 1,
                Some(&p) if p != 255 && self.song.pattern(p).is_some() => return,
                Some(&p) if p != 255 => self.position += 1,
                _ => {
                    self.end_or_restart();
                    if self.ended {
                        return;
                    }
                }
            }
            guard += 1;
        }
        self.ended = true;
    }

    fn end_or_restart(&mut self) {
        if self.looping {
            self.position = self.song.restart;
            self.row = 0;
        } else {
            self.ended = true;
        }
    }

    fn mark_visited(&mut self) {
        let (position, row) = (self.position, self.row);
        if let Some(seen) = self.visited.get_mut(position).and_then(|rows| rows.get_mut(row)) {
            if *seen && !self.looping {
                self.ended = true;
            }
            *seen = true;
        }
    }

    fn rows_in(&self, position: usize) -> usize {
        self.song.orders.get(position).and_then(|&p| self.song.pattern(p)).map_or(64, |p| p.rows)
    }

    fn next_row(&mut self) {
        self.tick = 0;
        self.pattern_delay = 0;
        self.fine_delay = 0;
        if let Some((position, row)) = self.next.take() {
            self.position = position;
            self.row = row;
            self.skip_markers();
            if self.row >= self.rows_in(self.position) {
                self.row = 0;
            }
        } else {
            self.row += 1;
            if self.row >= self.rows_in(self.position) {
                self.row = 0;
                self.position += 1;
                self.skip_markers();
            }
        }
        if !self.ended {
            self.mark_visited();
        }
    }

    fn process_tick(&mut self) {
        if self.ended {
            return;
        }
        if self.tick == 0 {
            self.start_row();
        } else {
            for c in 0..self.channels.len() {
                self.channel_tick(c);
            }
        }
        self.update_voices();
        self.tick += 1;
        if self.tick >= self.speed * (1 + self.pattern_delay) + self.fine_delay {
            self.next_row();
        }
    }

    fn start_row(&mut self) {
        let song = self.song.clone();
        let Some(pattern) = song.orders.get(self.position).and_then(|&p| song.pattern(p)) else {
            return;
        };
        for c in 0..self.channels.len() {
            let cell = song.cell(pattern, self.row, c);
            let channel = &mut self.channels[c];
            channel.cell = cell;
            channel.vibrato_now = 0.0;
            channel.tremolo_now = 0;
            channel.arpeggio_now = 0.0;
            channel.delayed = None;
            if let Fx::NoteDelay(d) = cell.fx
                && d > 0
            {
                channel.delayed = Some(d);
                continue;
            }
            self.start_cell(c);
        }
    }

    /// The memory an effect uses: its own, or Scream Tracker's shared one.
    fn remember(&mut self, c: usize, which: usize, param: u8) -> u8 {
        let which = if self.song.shared_memory && which <= slot::TREMOR { slot::SHARED } else { which };
        let memory = &mut self.channels[c].memory[which];
        if param != 0 {
            *memory = param;
        }
        *memory
    }

    fn note_period(&self, note: u8, sample: usize, finetune: Option<i16>) -> f64 {
        let song = &self.song;
        let s = song.samples.get(sample);
        let relative = s.map_or(0, |s| i32::from(s.relative));
        let finetune = finetune.or(s.map(|s| s.finetune)).unwrap_or(0);
        let semitones = f64::from(i32::from(note) - 1 + relative - i32::from(song.middle)) + f64::from(finetune) / 128.0;
        if song.linear {
            -semitones * 64.0
        } else {
            let base = s.map_or(8363.0, |s| f64::from(s.base_rate)).max(1.0);
            let period = 14_317_056.0 / (base * 2f64.powf(semitones / 12.0));
            if song.amiga_limits { period.round().clamp(113.0 * 4.0, 856.0 * 4.0) } else { period }
        }
    }

    fn period_rate(&self, period: f64, sample: usize) -> f64 {
        let song = &self.song;
        if song.linear {
            let base = song.samples.get(sample).map_or(8363.0, |s| f64::from(s.base_rate));
            base * 2f64.powf(-period / 768.0)
        } else if period <= 0.0 {
            0.0
        } else {
            14_317_056.0 / period
        }
    }

    fn clamp_period(&self, period: f64) -> f64 {
        if self.song.linear {
            period.clamp(-6000.0, 8000.0)
        } else if self.song.amiga_limits {
            period.clamp(113.0 * 4.0, 856.0 * 4.0)
        } else {
            period.clamp(16.0, 65_535.0 * 4.0)
        }
    }

    /// The instrument and sample a note plays, by the instrument's note map; and the note it's played as.
    fn resolve(&self, instrument: usize, note: u8) -> (Option<usize>, u8) {
        if self.song.instruments.is_empty() {
            return (instrument.checked_sub(1), note);
        }
        let Some(i) = instrument.checked_sub(1).and_then(|i| self.song.instruments.get(i)) else { return (None, note) };
        match i.keymap.get(usize::from(note.max(1) - 1)) {
            Some(&(mapped, sample)) if sample > 0 => (Some(usize::from(sample) - 1), if self.song.format == Format::It { mapped } else { note }),
            _ => (None, note),
        }
    }

    fn start_cell(&mut self, c: usize) {
        let song = self.song.clone();
        let cell = self.channels[c].cell;
        let porta = matches!(cell.fx, Fx::TonePorta(_) | Fx::TonePortaVolSlide(_)) || matches!(cell.vol, Vol::TonePorta(_));
        let has_note = (1..=120).contains(&cell.note);
        // The instrument: its sample's volume and panning, and a note to come.
        if cell.instrument > 0 {
            let channel = &mut self.channels[c];
            channel.instrument = usize::from(cell.instrument);
            let note = if has_note { cell.note } else { channel.note.max(1) };
            let (sample, _) = self.resolve(usize::from(cell.instrument), note);
            if let Some(sample) = sample.and_then(|s| song.samples.get(s).map(|data| (s, data))) {
                let channel = &mut self.channels[c];
                channel.volume = i32::from(sample.1.volume);
                if let Some(pan) = sample.1.pan {
                    channel.pan = i32::from(pan);
                } else if let Some(pan) = song.instruments.get(usize::from(cell.instrument) - 1).and_then(|i| i.pan) {
                    channel.pan = i32::from(pan);
                }
                channel.finetune = None;
                if !has_note || porta {
                    channel.sample = sample.0;
                }
            }
            // FastTracker starts the envelopes again on an instrument number, with or without a note.
            if song.format == Format::Xm
                && let Some(v) = self.channels[c].voice
                && let Some(voice) = self.voices.get_mut(v)
            {
                voice.volume_tick = 0;
                voice.pan_tick = 0;
                voice.released = false;
                voice.fading = false;
                voice.fade = 1.0;
            }
        }
        if let Fx::Finetune(f) = cell.fx {
            let f = if f > 7 { i16::from(f) - 16 } else { i16::from(f) };
            self.channels[c].finetune = Some(f * 16);
        }
        if has_note {
            let instrument = self.channels[c].instrument;
            let (sample, played) = self.resolve(instrument, cell.note);
            if let Some(sample) = sample.filter(|&s| song.samples.get(s).is_some_and(|s| !s.data.is_empty())) {
                let period = self.note_period(played, sample, self.channels[c].finetune);
                let playing = self.channels[c].voice.is_some_and(|v| self.voices[v].on);
                if porta && playing {
                    self.channels[c].target = period;
                } else {
                    let channel = &mut self.channels[c];
                    channel.note = played;
                    channel.sample = sample;
                    channel.period = period;
                    channel.target = period;
                    if channel.vibrato_wave < 4 {
                        channel.vibrato_position = 0;
                    }
                    if channel.tremolo_wave < 4 {
                        channel.tremolo_position = 0;
                    }
                    channel.retrig_count = 0;
                    channel.tremor_count = 0;
                    let offset = match cell.fx {
                        Fx::Offset(o) => {
                            let o = self.remember(c, slot::OFFSET, o);
                            usize::from(o) * 256 + usize::from(self.channels[c].high_offset) * 65536
                        }
                        _ => 0,
                    };
                    self.trigger(c, sample, offset);
                }
            } else if !porta && song.format != Format::Mod {
                // A note with no sample under it silences the channel.
                self.cut(c);
            }
        } else if cell.note == NOTE_OFF {
            self.key_off(c);
        } else if cell.note == NOTE_CUT {
            self.cut(c);
        } else if cell.note == NOTE_FADE
            && let Some(v) = self.channels[c].voice
        {
            self.voices[v].fading = true;
        }
        self.volume_column_start(c);
        self.effect_start(c);
    }

    /// Starts a sample in a channel; the note it had goes on behind or is quickly faded out.
    fn trigger(&mut self, c: usize, sample: usize, offset: usize) {
        let song = self.song.clone();
        let instrument = self.channels[c].instrument.checked_sub(1).filter(|_| !song.instruments.is_empty());
        if let Some(old) = self.channels[c].voice
            && self.voices[old].on
        {
            let past = instrument.and_then(|i| song.instruments.get(i)).map_or(NewNote::Cut, |i| i.new_note);
            let voice = &mut self.voices[old];
            voice.background = true;
            match past {
                NewNote::Cut => {
                    voice.vanishing = true;
                    voice.target_left = 0.0;
                    voice.target_right = 0.0;
                    voice.ramp = RAMP;
                }
                NewNote::Continue => {}
                NewNote::Off => voice.released = true,
                NewNote::Fade => voice.fading = true,
            }
        }
        let length = song.samples[sample].data.len();
        let voice = Voice {
            sample,
            instrument,
            position: if offset < length { offset as f64 } else if song.format == Format::It || song.samples[sample].loop_kind != Loop::None { 0.0 } else { length as f64 },
            on: offset < length || song.format == Format::It,
            fade: 1.0,
            ..Voice::default()
        };
        let slot = match self.voices.iter().position(|v| !v.on) {
            Some(free) => {
                self.voices[free] = voice;
                free
            }
            None => {
                self.voices.push(voice);
                self.voices.len() - 1
            }
        };
        // A channel whose sample came to its end still points at the slot it had, which may be this one.
        for channel in &mut self.channels {
            if channel.voice == Some(slot) {
                channel.voice = None;
            }
        }
        self.channels[c].voice = Some(slot);
        self.trim_voices();
    }

    /// Lets the quietest notes left behind go when there are too many.
    fn trim_voices(&mut self) {
        let mut count = self.voices.iter().filter(|v| v.on).count();
        while count > MOST_VOICES {
            let quietest = self.voices.iter().enumerate().filter(|(_, v)| v.on && v.background).min_by(|a, b| (a.1.target_left + a.1.target_right).total_cmp(&(b.1.target_left + b.1.target_right))).map(|(i, _)| i);
            match quietest {
                Some(i) => self.voices[i].on = false,
                None => break,
            }
            count -= 1;
        }
    }

    fn cut(&mut self, c: usize) {
        if let Some(v) = self.channels[c].voice {
            let voice = &mut self.voices[v];
            voice.vanishing = true;
            voice.target_left = 0.0;
            voice.target_right = 0.0;
            voice.ramp = RAMP;
        }
        self.channels[c].volume = 0;
    }

    fn key_off(&mut self, c: usize) {
        let song = self.song.clone();
        let Some(v) = self.channels[c].voice else { return };
        let envelope_on = self.voices[v].instrument.and_then(|i| song.instruments.get(i)).is_some_and(|i| i.volume.on);
        let voice = &mut self.voices[v];
        voice.released = true;
        match song.format {
            Format::Xm if !envelope_on => self.channels[c].volume = 0,
            Format::Xm => voice.fading = true,
            Format::It => {
                if !envelope_on {
                    voice.fading = true;
                }
            }
            // Scream Tracker and ProTracker have no note off of their own, only a cut.
            _ => self.cut(c),
        }
    }

    fn volume_column_start(&mut self, c: usize) {
        let channel = &mut self.channels[c];
        match channel.cell.vol {
            Vol::Set(v) => channel.volume = i32::from(v.min(64)),
            Vol::FineUp(v) => channel.volume = (channel.volume + i32::from(v)).min(64),
            Vol::FineDown(v) => channel.volume = (channel.volume - i32::from(v)).max(0),
            Vol::VibratoSpeed(s) if s > 0 => channel.vibrato_speed = s,
            Vol::VibratoDepth(d) if d > 0 => channel.vibrato_depth = d,
            Vol::Pan(p) => {
                channel.pan = i32::from(p);
                channel.surround = false;
            }
            Vol::TonePorta(s) if s > 0 => channel.porta_speed = s,
            _ => {}
        }
    }

    fn volume_column_tick(&mut self, c: usize) {
        let cell = self.channels[c].cell;
        match cell.vol {
            Vol::SlideUp(v) => self.slide_volume(c, i32::from(v)),
            Vol::SlideDown(v) => self.slide_volume(c, -i32::from(v)),
            Vol::PanLeft(p) => self.channels[c].pan = (self.channels[c].pan - i32::from(p)).max(0),
            Vol::PanRight(p) => self.channels[c].pan = (self.channels[c].pan + i32::from(p)).min(255),
            Vol::VibratoDepth(_) => self.vibrato(c, 5),
            Vol::TonePorta(_) => self.tone_porta(c),
            Vol::PortaUp(p) => self.slide_period(c, -f64::from(p) * 4.0),
            Vol::PortaDown(p) => self.slide_period(c, f64::from(p) * 4.0),
            _ => {}
        }
    }

    fn slide_volume(&mut self, c: usize, by: i32) {
        let channel = &mut self.channels[c];
        channel.volume = (channel.volume + by).clamp(0, 64);
    }

    fn slide_period(&mut self, c: usize, by: f64) {
        let period = self.clamp_period(self.channels[c].period + by);
        self.channels[c].period = period;
    }

    /// A Scream Tracker volume slide on the first tick (true) or the others: DxF and DFx are fine slides.
    fn st_volume_slide(&mut self, c: usize, param: u8, first: bool) {
        let param = self.remember(c, slot::VOLSLIDE, param);
        let (x, y) = (param >> 4, param & 15);
        if y == 15 && x > 0 {
            if first {
                self.slide_volume(c, i32::from(x));
            }
        } else if x == 15 && y > 0 {
            if first {
                self.slide_volume(c, -i32::from(y));
            }
        } else if !first || self.song.fast_slides {
            if y == 0 {
                self.slide_volume(c, i32::from(x));
            } else if x == 0 || self.song.format == Format::S3m {
                self.slide_volume(c, -i32::from(y));
            }
        }
    }

    fn st_porta(&mut self, c: usize, param: u8, up: bool, first: bool) {
        let param = self.remember(c, if up { slot::PORTA_UP } else { slot::PORTA_DOWN }, param);
        // Impulse Tracker's E and F share one memory.
        if self.song.format == Format::It {
            let other = if up { slot::PORTA_DOWN } else { slot::PORTA_UP };
            self.channels[c].memory[other] = param;
        }
        let sign = if up { -1.0 } else { 1.0 };
        let (x, y) = (param >> 4, param & 15);
        let by = match x {
            15 if first => f64::from(y) * 4.0,
            14 if first => f64::from(y),
            15 | 14 => 0.0,
            _ if !first => f64::from(param) * 4.0,
            _ => 0.0,
        };
        self.slide_period(c, sign * by);
    }

    fn mod_volume_slide(&mut self, c: usize, param: u8) {
        let param = if self.song.format == Format::Xm { self.remember(c, slot::VOLSLIDE, param) } else { param };
        let (x, y) = (param >> 4, param & 15);
        if x > 0 {
            self.slide_volume(c, i32::from(x));
        } else {
            self.slide_volume(c, -i32::from(y));
        }
    }

    fn tone_porta(&mut self, c: usize) {
        let channel = &mut self.channels[c];
        let speed = f64::from(channel.porta_speed) * 4.0;
        if channel.period < channel.target {
            channel.period = (channel.period + speed).min(channel.target);
        } else if channel.period > channel.target {
            channel.period = (channel.period - speed).max(channel.target);
        }
    }

    /// Vibrato for this tick; `shift` is how far the depth is scaled down (fine vibrato's is 2 more).
    fn vibrato(&mut self, c: usize, shift: u32) {
        let channel = &mut self.channels[c];
        let delta = wave(channel.vibrato_wave, channel.vibrato_position) * i32::from(channel.vibrato_depth);
        channel.vibrato_now = f64::from(delta) / f64::from(1u32 << shift);
        channel.vibrato_position = channel.vibrato_position.wrapping_add(channel.vibrato_speed) & 63;
    }

    fn tremolo(&mut self, c: usize) {
        let channel = &mut self.channels[c];
        channel.tremolo_now = wave(channel.tremolo_wave, channel.tremolo_position) * i32::from(channel.tremolo_depth) / 64;
        channel.tremolo_position = channel.tremolo_position.wrapping_add(channel.tremolo_speed) & 63;
    }

    fn set_vibrato(&mut self, c: usize, param: u8) {
        let channel = &mut self.channels[c];
        if param >> 4 != 0 {
            channel.vibrato_speed = param >> 4;
        }
        if param & 15 != 0 {
            channel.vibrato_depth = param & 15;
        }
    }

    fn retrigger(&mut self, c: usize) {
        let sample = self.channels[c].sample;
        if self.song.samples.get(sample).is_some_and(|s| !s.data.is_empty()) {
            self.trigger(c, sample, 0);
        }
    }

    fn effect_start(&mut self, c: usize) {
        let cell = self.channels[c].cell;
        let it = self.song.format == Format::It;
        match cell.fx {
            Fx::Speed(s) if s > 0 => self.speed = u32::from(s),
            Fx::Tempo(t) => self.tempo = u32::from(t.max(32)),
            Fx::TempoSlide(t) => {
                let t = self.remember(c, slot::TEMPO, t);
                self.channels[c].memory[slot::TEMPO] = t;
            }
            Fx::Jump(p) => {
                // A break in the same row says which row of the new position to start at.
                let row = self.next.map_or(0, |n| n.1);
                self.next = Some((usize::from(p), row));
            }
            Fx::Break(r) => {
                let position = self.find_jump().unwrap_or(self.position + 1);
                self.next = Some((position, usize::from(r)));
            }
            Fx::SetVolume(v) => self.channels[c].volume = i32::from(v.min(64)),
            Fx::Pan(p) => {
                self.channels[c].pan = i32::from(p);
                self.channels[c].surround = false;
            }
            Fx::Surround => self.channels[c].surround = true,
            Fx::FinePortaUp(p) => {
                let p = if self.song.format == Format::Xm { self.remember(c, slot::FINE_UP, p) } else { p };
                self.slide_period(c, -f64::from(p) * 4.0);
            }
            Fx::FinePortaDown(p) => {
                let p = if self.song.format == Format::Xm { self.remember(c, slot::FINE_DOWN, p) } else { p };
                self.slide_period(c, f64::from(p) * 4.0);
            }
            Fx::ExtraFinePortaUp(p) => {
                let p = self.remember(c, slot::EXTRA_UP, p);
                self.slide_period(c, -f64::from(p));
            }
            Fx::ExtraFinePortaDown(p) => {
                let p = self.remember(c, slot::EXTRA_DOWN, p);
                self.slide_period(c, f64::from(p));
            }
            Fx::StPortaUp(p) => self.st_porta(c, p, true, true),
            Fx::StPortaDown(p) => self.st_porta(c, p, false, true),
            Fx::StVolSlide(p) => self.st_volume_slide(c, p, true),
            Fx::VibratoVolSlide(p) | Fx::TonePortaVolSlide(p) if matches!(self.song.format, Format::S3m | Format::It) => self.st_volume_slide(c, p, true),
            Fx::FineVolUp(v) => {
                let v = if self.song.format == Format::Xm { self.remember(c, slot::FINE_VOL_UP, v) } else { v };
                self.slide_volume(c, i32::from(v));
            }
            Fx::FineVolDown(v) => {
                let v = if self.song.format == Format::Xm { self.remember(c, slot::FINE_VOL_DOWN, v) } else { v };
                self.slide_volume(c, -i32::from(v));
            }
            Fx::TonePorta(p) => {
                let p = self.remember(c, slot::TONE, p);
                self.channels[c].porta_speed = p;
            }
            Fx::Vibrato(p) | Fx::FineVibrato(p) => {
                let p = self.remember(c, slot::VIBRATO, p);
                self.set_vibrato(c, p);
                // Impulse Tracker's vibrato moves on the first tick too.
                if it {
                    self.vibrato(c, if matches!(cell.fx, Fx::FineVibrato(_)) { 7 } else { 5 });
                }
            }
            Fx::Tremolo(p) => {
                let p = self.remember(c, slot::TREMOLO, p);
                let channel = &mut self.channels[c];
                if p >> 4 != 0 {
                    channel.tremolo_speed = p >> 4;
                }
                if p & 15 != 0 {
                    channel.tremolo_depth = p & 15;
                }
            }
            Fx::Arpeggio(p) => {
                self.remember(c, slot::ARPEGGIO, p);
            }
            Fx::VibratoWave(w) => self.channels[c].vibrato_wave = w,
            Fx::TremoloWave(w) => self.channels[c].tremolo_wave = w,
            Fx::PatternLoop(x) => {
                let row = self.row;
                let channel = &mut self.channels[c];
                if x == 0 {
                    channel.loop_row = row;
                } else {
                    let again = if channel.loop_count == 0 {
                        channel.loop_count = x;
                        true
                    } else {
                        channel.loop_count -= 1;
                        channel.loop_count > 0
                    };
                    if again {
                        let back = channel.loop_row;
                        self.next = Some((self.position, back));
                        if let Some(rows) = self.visited.get_mut(self.position) {
                            for seen in rows.iter_mut().skip(back) {
                                *seen = false;
                            }
                        }
                    }
                }
            }
            Fx::PatternDelay(d) if self.pattern_delay == 0 => self.pattern_delay = u32::from(d),
            Fx::FinePatternDelay(d) => self.fine_delay += u32::from(d),
            Fx::GlobalVolume(v) => self.global_volume = i32::from(v.min(128)),
            Fx::KeyOff(0) => self.key_off(c),
            Fx::EnvelopePos(p) => {
                if let Some(v) = self.channels[c].voice {
                    self.voices[v].volume_tick = u16::from(p);
                    self.voices[v].pan_tick = u16::from(p);
                }
            }
            Fx::ChannelVolume(v) => self.channels[c].channel_volume = i32::from(v.min(64)),
            Fx::HighOffset(h) => self.channels[c].high_offset = h,
            Fx::Retrig(_) | Fx::MultiRetrig(_) => {
                if let Fx::MultiRetrig(p) = cell.fx {
                    self.remember(c, slot::RETRIG, p);
                }
            }
            Fx::Tremor(p) => {
                self.remember(c, slot::TREMOR, p);
            }
            Fx::GlobalVolSlide(p) => {
                let p = self.remember(c, slot::GLOBAL_SLIDE, p);
                if it {
                    let (x, y) = (p >> 4, p & 15);
                    if y == 15 && x > 0 {
                        self.global_volume = (self.global_volume + i32::from(x)).min(128);
                    } else if x == 15 && y > 0 {
                        self.global_volume = (self.global_volume - i32::from(y)).max(0);
                    }
                }
            }
            Fx::ChannelVolSlide(p) => {
                let p = self.remember(c, slot::CHANNEL_SLIDE, p);
                let (x, y) = (p >> 4, p & 15);
                let channel = &mut self.channels[c];
                if y == 15 && x > 0 {
                    channel.channel_volume = (channel.channel_volume + i32::from(x)).min(64);
                } else if x == 15 && y > 0 {
                    channel.channel_volume = (channel.channel_volume - i32::from(y)).max(0);
                }
            }
            Fx::PanSlide(p) => {
                self.remember(c, slot::PAN_SLIDE, p);
            }
            Fx::NoteCut(0) => self.cut(c),
            _ => {}
        }
    }

    /// Where a jump in this row goes, if there's one.
    fn find_jump(&self) -> Option<usize> {
        self.channels.iter().find_map(|ch| match ch.cell.fx {
            Fx::Jump(p) => Some(usize::from(p)),
            _ => None,
        })
    }

    fn channel_tick(&mut self, c: usize) {
        let tick = self.tick;
        if let Some(delay) = self.channels[c].delayed {
            if tick == u32::from(delay) {
                self.channels[c].delayed = None;
                self.start_cell(c);
            }
            return;
        }
        let cell = self.channels[c].cell;
        let channel = &mut self.channels[c];
        channel.vibrato_now = 0.0;
        channel.tremolo_now = 0;
        self.volume_column_tick(c);
        let xm = self.song.format == Format::Xm;
        let st = matches!(self.song.format, Format::S3m | Format::It);
        match cell.fx {
            Fx::Arpeggio(p) => {
                let p = if st { self.channels[c].memory[if self.song.shared_memory { slot::SHARED } else { slot::ARPEGGIO }] } else { p };
                let semitones = match tick % 3 {
                    1 => p >> 4,
                    2 => p & 15,
                    _ => 0,
                };
                self.channels[c].arpeggio_now = f64::from(semitones);
            }
            Fx::PortaUp(p) => {
                let p = if xm { self.remember(c, slot::PORTA_UP, p) } else { p };
                self.slide_period(c, -f64::from(p) * 4.0);
            }
            Fx::PortaDown(p) => {
                let p = if xm { self.remember(c, slot::PORTA_DOWN, p) } else { p };
                self.slide_period(c, f64::from(p) * 4.0);
            }
            Fx::StPortaUp(p) => self.st_porta(c, p, true, false),
            Fx::StPortaDown(p) => self.st_porta(c, p, false, false),
            Fx::TonePorta(_) => self.tone_porta(c),
            Fx::Vibrato(_) => self.vibrato(c, 5),
            Fx::FineVibrato(_) => self.vibrato(c, 7),
            Fx::TonePortaVolSlide(p) => {
                self.tone_porta(c);
                if st { self.st_volume_slide(c, p, false) } else { self.mod_volume_slide(c, p) }
            }
            Fx::VibratoVolSlide(p) => {
                self.vibrato(c, 5);
                if st { self.st_volume_slide(c, p, false) } else { self.mod_volume_slide(c, p) }
            }
            Fx::Tremolo(_) => self.tremolo(c),
            Fx::VolSlide(p) => self.mod_volume_slide(c, p),
            Fx::StVolSlide(p) => self.st_volume_slide(c, p, false),
            Fx::Retrig(x) => {
                if x > 0 && tick.is_multiple_of(u32::from(x)) {
                    self.retrigger(c);
                }
            }
            Fx::MultiRetrig(_) => {
                let p = self.channels[c].memory[if self.song.shared_memory { slot::SHARED } else { slot::RETRIG }];
                let (x, y) = (p >> 4, p & 15);
                let channel = &mut self.channels[c];
                channel.retrig_count += 1;
                if y > 0 && channel.retrig_count >= y {
                    channel.retrig_count = 0;
                    let v = channel.volume;
                    channel.volume = match x {
                        1..=5 => v - (1 << (x - 1)),
                        6 => v * 2 / 3,
                        7 => v / 2,
                        9..=13 => v + (1 << (x - 9)),
                        14 => v * 3 / 2,
                        15 => v * 2,
                        _ => v,
                    }
                    .clamp(0, 64);
                    self.retrigger(c);
                }
            }
            Fx::Tremor(_) => {
                let p = self.channels[c].memory[if self.song.shared_memory { slot::SHARED } else { slot::TREMOR }];
                let (on, off) = ((p >> 4) + 1, (p & 15) + 1);
                let channel = &mut self.channels[c];
                channel.tremor_count = (channel.tremor_count + 1) % (on + off);
                channel.tremor_off = channel.tremor_count >= on;
            }
            Fx::NoteCut(t) if tick == u32::from(t) => self.cut(c),
            Fx::KeyOff(t) if tick == u32::from(t) => self.key_off(c),
            Fx::GlobalVolSlide(_) => {
                let p = self.channels[c].memory[slot::GLOBAL_SLIDE];
                let (x, y) = (p >> 4, p & 15);
                let step = if self.song.format == Format::It { 1 } else { 2 };
                if self.song.format != Format::It || (x != 15 && y != 15) {
                    if x > 0 {
                        self.global_volume = (self.global_volume + i32::from(x) * step).min(128);
                    } else {
                        self.global_volume = (self.global_volume - i32::from(y) * step).max(0);
                    }
                }
            }
            Fx::ChannelVolSlide(_) => {
                let p = self.channels[c].memory[slot::CHANNEL_SLIDE];
                let (x, y) = (p >> 4, p & 15);
                let channel = &mut self.channels[c];
                if x != 15 && y != 15 {
                    if y == 0 {
                        channel.channel_volume = (channel.channel_volume + i32::from(x)).min(64);
                    } else if x == 0 {
                        channel.channel_volume = (channel.channel_volume - i32::from(y)).max(0);
                    }
                }
            }
            Fx::PanSlide(_) => {
                let p = self.channels[c].memory[slot::PAN_SLIDE];
                let (x, y) = (p >> 4, p & 15);
                let channel = &mut self.channels[c];
                // FastTracker's x slides right; Impulse Tracker's x slides left, four steps at a time.
                let (right, left) = if self.song.format == Format::It { (i32::from(y) * 4, i32::from(x) * 4) } else { (i32::from(x), i32::from(y)) };
                if (self.song.format != Format::It) || (x != 15 && y != 15) {
                    channel.pan = (channel.pan + if right > 0 { right } else { -left }).clamp(0, 255);
                }
            }
            Fx::TempoSlide(_) => {
                let t = self.channels[c].memory[slot::TEMPO];
                let tempo = self.tempo as i32 + if t >> 4 == 1 { i32::from(t & 15) } else { -i32::from(t & 15) };
                self.tempo = tempo.clamp(32, 255) as u32;
            }
            _ => {}
        }
    }

    /// Hands each channel's pitch, volume and panning to its voice, and moves every voice's envelopes on.
    fn update_voices(&mut self) {
        let song = self.song.clone();
        for c in 0..self.channels.len() {
            let Some(v) = self.channels[c].voice else { continue };
            let channel = &self.channels[c];
            if self.voices[v].background {
                self.channels[c].voice = None;
                continue;
            }
            let period = channel.period + channel.vibrato_now;
            let mut rate = self.period_rate(period, channel.sample);
            if channel.arpeggio_now != 0.0 {
                rate *= 2f64.powf(channel.arpeggio_now / 12.0);
            }
            let sample = song.samples.get(channel.sample);
            let volume = if channel.tremor_off { 0 } else { (channel.volume + channel.tremolo_now).clamp(0, 64) };
            let sample_global = sample.map_or(64.0, |s| f32::from(s.global_volume)) / 64.0;
            let silenced = channel.muted || self.silenced.get(c).copied().unwrap_or(false);
            let loudness = if silenced { 0.0 } else { volume as f32 / 64.0 * channel.channel_volume as f32 / 64.0 * sample_global };
            let pan = channel.pan as f32 / 255.0;
            let surround = channel.surround;
            let voice = &mut self.voices[v];
            voice.surround = surround;
            voice.rate = rate;
            voice.volume = loudness;
            voice.pan = pan;
        }
        let global = self.global_volume as f32 / 128.0;
        for voice in self.voices.iter_mut().filter(|v| v.on) {
            let instrument = voice.instrument.and_then(|i| song.instruments.get(i));
            let mut envelope = 1.0;
            let mut pan = voice.pan;
            if let Some(instrument) = instrument {
                if instrument.volume.on {
                    envelope = instrument.volume.value(voice.volume_tick) / 64.0;
                    if song.format == Format::It && instrument.volume.ended(voice.volume_tick, voice.released) {
                        if envelope <= 0.0 {
                            voice.vanishing = true;
                        }
                        voice.fading = true;
                    }
                    voice.volume_tick = instrument.volume.next(voice.volume_tick, voice.released);
                }
                if instrument.panning.on {
                    let offset = (instrument.panning.value(voice.pan_tick) - 32.0) / 32.0;
                    pan = (pan + offset * (0.5 - (pan - 0.5).abs())).clamp(0.0, 1.0);
                    voice.pan_tick = instrument.panning.next(voice.pan_tick, voice.released);
                }
                envelope *= f32::from(instrument.global_volume) / 128.0;
                if voice.fading {
                    voice.fade = (voice.fade - instrument.fadeout).max(0.0);
                    if voice.fade <= 0.0 {
                        voice.vanishing = true;
                    }
                }
            }
            let level = voice.volume * envelope * voice.fade * global;
            let (left, right) = if voice.surround { (1.0, -1.0) } else { ((1.0 - pan).min(0.5) * 2.0, pan.min(0.5) * 2.0) };
            if voice.vanishing {
                voice.target_left = 0.0;
                voice.target_right = 0.0;
                if voice.ramp == 0 {
                    voice.ramp = RAMP;
                }
            } else {
                voice.target_left = level * left;
                voice.target_right = level * right;
                voice.ramp = RAMP;
            }
        }
    }
}
