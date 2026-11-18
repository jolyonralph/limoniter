//! Tracker music, read to be played: ProTracker modules from the Amiga, and the PC's Scream Tracker 3 (.s3m),
//! FastTracker 2 (.xm) and Impulse Tracker (.it) modules. Each is read into one shape, a [`Song`] of patterns,
//! samples and instruments, which [`play::Player`] plays with each tracker's own effects.
//!
//! What a ProTracker module says about itself is still shown by `protracker`; the PC formats' own details come
//! from here.

mod load_it;
mod load_mod;
mod load_s3m;
mod load_xm;
pub mod play;

use crate::media::InfoSection;
use crate::preview::{Block, BlockKind, Span};

/// A note that's let go of: the instrument's envelope is released and it fades.
pub const NOTE_OFF: u8 = 255;
/// A note that stops at once.
pub const NOTE_CUT: u8 = 254;
/// Impulse Tracker's note fade: the note fades out without being let go of.
pub const NOTE_FADE: u8 = 253;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Mod,
    S3m,
    Xm,
    It,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Mod => "ProTracker module",
            Format::S3m => "Scream Tracker 3 module",
            Format::Xm => "FastTracker 2 module",
            Format::It => "Impulse Tracker module",
        }
    }
}

/// What a volume column does: FastTracker 2's and Impulse Tracker's, which do more than set a volume.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Vol {
    #[default]
    None,
    Set(u8),
    SlideUp(u8),
    SlideDown(u8),
    FineUp(u8),
    FineDown(u8),
    VibratoSpeed(u8),
    VibratoDepth(u8),
    /// 0 to 255.
    Pan(u8),
    PanLeft(u8),
    PanRight(u8),
    /// The speed itself, 0 meaning the last one.
    TonePorta(u8),
    /// In the same steps as the effect column's slides.
    PortaUp(u8),
    PortaDown(u8),
}

/// An effect, whichever tracker's letter or number it was written as. Where trackers differ in what the same
/// parameter means, there's one for each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fx {
    #[default]
    None,
    Arpeggio(u8),
    /// ProTracker's and FastTracker's slides: every tick but the first.
    PortaUp(u8),
    PortaDown(u8),
    FinePortaUp(u8),
    FinePortaDown(u8),
    ExtraFinePortaUp(u8),
    ExtraFinePortaDown(u8),
    /// Scream Tracker's and Impulse Tracker's, where Fx and Ex parameters are the fine and extra fine slides.
    StPortaUp(u8),
    StPortaDown(u8),
    TonePorta(u8),
    Vibrato(u8),
    FineVibrato(u8),
    TonePortaVolSlide(u8),
    VibratoVolSlide(u8),
    Tremolo(u8),
    Tremor(u8),
    /// 0 to 255.
    Pan(u8),
    Offset(u8),
    HighOffset(u8),
    /// ProTracker's and FastTracker's: x0 up, 0y down.
    VolSlide(u8),
    /// Scream Tracker's and Impulse Tracker's, with DxF and DFx the fine slides.
    StVolSlide(u8),
    Jump(u8),
    Break(u8),
    SetVolume(u8),
    Speed(u8),
    Tempo(u8),
    /// Impulse Tracker's T0x and T1x.
    TempoSlide(u8),
    PatternLoop(u8),
    NoteCut(u8),
    NoteDelay(u8),
    PatternDelay(u8),
    FinePatternDelay(u8),
    Retrig(u8),
    MultiRetrig(u8),
    FineVolUp(u8),
    FineVolDown(u8),
    VibratoWave(u8),
    TremoloWave(u8),
    /// The sample's finetune, in ProTracker's sixteen steps (8 to 15 below, 0 to 7 above).
    Finetune(u8),
    /// 0 to 128.
    GlobalVolume(u8),
    GlobalVolSlide(u8),
    KeyOff(u8),
    EnvelopePos(u8),
    PanSlide(u8),
    ChannelVolume(u8),
    ChannelVolSlide(u8),
    /// Impulse Tracker's S7x: what happens to notes left playing in the background.
    PastNotes(u8),
    /// Surround: one side played upside down, which sounds as if it's all round. Setting a panning ends it.
    Surround,
}

/// One channel on one row. `note` is 1 to 120 (C-0 to B-9), or one of the NOTE_ values, or 0 for none;
/// `instrument` counts from 1. `shown` is the effect as the tracker writes it, for the pattern view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cell {
    pub note: u8,
    pub instrument: u8,
    pub vol: Vol,
    pub fx: Fx,
    pub shown: [u8; 3],
}

#[derive(Clone, Debug, Default)]
pub struct Pattern {
    pub rows: usize,
    pub cells: Vec<Cell>,
}

impl Pattern {
    pub fn empty(rows: usize, channels: usize) -> Pattern {
        Pattern { rows, cells: vec![Cell::default(); rows * channels] }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Loop {
    #[default]
    None,
    Forward,
    PingPong,
}

#[derive(Clone, Debug, Default)]
pub struct Sample {
    pub name: String,
    /// 16-bit, whatever it was saved as.
    pub data: Vec<i16>,
    pub loop_kind: Loop,
    pub loop_start: usize,
    pub loop_end: usize,
    /// Impulse Tracker's sustain loop, played until the note is let go of.
    pub sustain: Option<(usize, usize, Loop)>,
    /// 0 to 64.
    pub volume: u8,
    /// Impulse Tracker's sample global volume, 0 to 64.
    pub global_volume: u8,
    /// 0 to 255, if the sample sets one.
    pub pan: Option<u8>,
    /// How fast it plays at the song's middle note, in samples a second.
    pub base_rate: f32,
    /// FastTracker's relative note and ProTracker's and FastTracker's finetune, in 128ths of a semitone.
    pub relative: i8,
    pub finetune: i16,
    /// Bits as saved, for the list of samples.
    pub bits: u8,
}

#[derive(Clone, Debug, Default)]
pub struct Envelope {
    /// (tick, value): 0 to 64.
    pub points: Vec<(u16, u8)>,
    pub on: bool,
    pub sustain: Option<(usize, usize)>,
    pub looped: Option<(usize, usize)>,
}

impl Envelope {
    pub fn value(&self, tick: u16) -> f32 {
        let Some(first) = self.points.first() else { return 64.0 };
        if tick <= first.0 {
            return f32::from(first.1);
        }
        for pair in self.points.windows(2) {
            let ((t0, v0), (t1, v1)) = (pair[0], pair[1]);
            if tick < t1 {
                if t1 <= t0 {
                    return f32::from(v1);
                }
                let along = f32::from(tick - t0) / f32::from(t1 - t0);
                return f32::from(v0) + (f32::from(v1) - f32::from(v0)) * along;
            }
        }
        f32::from(self.points.last().map_or(64, |p| p.1))
    }

    fn tick_of(&self, point: usize) -> u16 {
        self.points.get(point).map_or(0, |p| p.0)
    }

    /// The tick after this one: held at the sustain until let go of, and round the loop.
    pub fn next(&self, tick: u16, released: bool) -> u16 {
        let next = tick.saturating_add(1);
        if let Some((start, end)) = self.sustain.filter(|_| !released)
            && next > self.tick_of(end)
        {
            return self.tick_of(start);
        }
        if let Some((start, end)) = self.looped
            && next > self.tick_of(end)
        {
            return self.tick_of(start);
        }
        next.min(self.points.last().map_or(0, |p| p.0))
    }

    /// Whether it's played to its end and stays there.
    pub fn ended(&self, tick: u16, released: bool) -> bool {
        self.looped.is_none() && (released || self.sustain.is_none()) && self.points.last().is_some_and(|p| tick >= p.0)
    }
}

/// What Impulse Tracker does with a note still playing when another starts in its channel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NewNote {
    #[default]
    Cut,
    Continue,
    Off,
    Fade,
}

#[derive(Clone, Debug, Default)]
pub struct Instrument {
    pub name: String,
    /// For each note, the note played and the sample (from 1; 0 for none).
    pub keymap: Vec<(u8, u16)>,
    pub volume: Envelope,
    pub panning: Envelope,
    /// How much it fades each tick once let go of, as a fraction of full volume.
    pub fadeout: f32,
    /// 0 to 128.
    pub global_volume: u8,
    pub pan: Option<u8>,
    pub new_note: NewNote,
}

#[derive(Clone, Debug)]
pub struct Song {
    pub format: Format,
    pub title: String,
    /// The tracker that saved it, where the file says.
    pub tracker: String,
    pub channels: usize,
    /// Pattern numbers; 254 is skipped and 255 ends the song, as Scream Tracker and Impulse Tracker mark them.
    pub orders: Vec<u8>,
    pub restart: usize,
    pub patterns: Vec<Pattern>,
    pub samples: Vec<Sample>,
    pub instruments: Vec<Instrument>,
    pub speed: u8,
    pub tempo: u8,
    /// 0 to 128.
    pub global_volume: u8,
    /// Each channel's panning (0 to 255), volume (0 to 64) and whether it's switched off.
    pub pans: Vec<u8>,
    pub channel_volumes: Vec<u8>,
    pub muted: Vec<bool>,
    pub surround: Vec<bool>,
    /// FastTracker's and Impulse Tracker's linear pitch; otherwise Amiga periods.
    pub linear: bool,
    /// ProTracker's limits on its periods, B-3 to C-1.
    pub amiga_limits: bool,
    /// The note (from 0, C-0) a sample plays at its base rate.
    pub middle: u8,
    /// Scream Tracker 3.00's volume slides, which slide on the first tick as well.
    pub fast_slides: bool,
    /// Scream Tracker's effects share one memory.
    pub shared_memory: bool,
    /// The song's message, as Impulse Tracker keeps one.
    pub message: String,
    /// Octaves the pattern view shows notes higher than they're numbered here: ProTracker's C-1 is the middle C-4.
    pub octave_shift: i8,
}

impl Song {
    fn new(format: Format, channels: usize) -> Song {
        Song {
            format,
            title: String::new(),
            tracker: String::new(),
            channels,
            orders: Vec::new(),
            restart: 0,
            patterns: Vec::new(),
            samples: Vec::new(),
            instruments: Vec::new(),
            speed: 6,
            tempo: 125,
            global_volume: 128,
            pans: vec![128; channels],
            channel_volumes: vec![64; channels],
            muted: vec![false; channels],
            surround: vec![false; channels],
            linear: false,
            amiga_limits: false,
            middle: 48,
            fast_slides: false,
            shared_memory: false,
            message: String::new(),
            octave_shift: 0,
        }
    }

    pub fn pattern(&self, number: u8) -> Option<&Pattern> {
        self.patterns.get(usize::from(number))
    }

    pub fn cell(&self, pattern: &Pattern, row: usize, channel: usize) -> Cell {
        pattern.cells.get(row * self.channels + channel).copied().unwrap_or_default()
    }

    /// A note as the tracker shows it: C-4, C#4, === (let go of), ^^^ (cut), or dots.
    pub fn note_name(&self, note: u8) -> String {
        const NAMES: [&str; 12] = ["C-", "C#", "D-", "D#", "E-", "F-", "F#", "G-", "G#", "A-", "A#", "B-"];
        match note {
            0 => "\u{B7}\u{B7}\u{B7}".into(),
            NOTE_OFF => "===".into(),
            NOTE_CUT => "^^^".into(),
            NOTE_FADE => "~~~".into(),
            n if n <= 120 => {
                let n = i32::from(n - 1);
                format!("{}{}", NAMES[(n % 12) as usize], n / 12 - i32::from(self.octave_shift))
            }
            _ => "???".into(),
        }
    }

    /// A cell as the pattern view shows it.
    pub fn cell_text(&self, cell: &Cell) -> String {
        let instrument = if cell.instrument == 0 { "\u{B7}\u{B7}".to_string() } else { format!("{:02}", cell.instrument) };
        let volume = match cell.vol {
            Vol::None => "\u{B7}\u{B7}".to_string(),
            Vol::Set(v) => format!("{v:02}"),
            Vol::Pan(p) => format!("p{:X}", p >> 4),
            _ => "v~".into(),
        };
        let effect = if cell.shown == [0; 3] { "\u{B7}\u{B7}\u{B7}".to_string() } else { cell.shown.iter().map(|&b| b as char).collect() };
        if self.format == Format::Mod { format!("{} {instrument} {effect}", self.note_name(cell.note)) } else { format!("{} {instrument} {volume} {effect}", self.note_name(cell.note)) }
    }

    fn used_samples(&self) -> impl Iterator<Item = (usize, &Sample)> {
        self.samples.iter().enumerate().filter(|(_, s)| !s.data.is_empty()).map(|(i, s)| (i + 1, s))
    }

    fn summary(&self) -> Vec<(String, String)> {
        let mut rows = vec![("Title".to_string(), if self.title.is_empty() { "(none)".into() } else { self.title.clone() }), ("Format".into(), self.format.name().to_string())];
        if !self.tracker.is_empty() {
            rows.push((crate::i18n::tr("Made with").into(), self.tracker.clone()));
        }
        rows.push(("Channels".into(), self.channels.to_string()));
        rows.push((crate::i18n::tr("Song length").into(), format!("{} positions", self.orders.iter().filter(|&&p| p < 254).count())));
        rows.push(("Patterns".into(), self.patterns.len().to_string()));
        if !self.instruments.is_empty() {
            rows.push(("Instruments".into(), self.instruments.len().to_string()));
        }
        let bytes: usize = self.samples.iter().map(|s| s.data.len() * usize::from(s.bits.max(8) / 8)).sum();
        rows.push(("Samples".into(), format!("{} used, {} bytes", self.used_samples().count(), group(bytes as u64))));
        rows.push(("Speed".into(), crate::i18n::tf!("{} ticks a row at {} BPM", self.speed, self.tempo)));
        rows.push(("Pitch".into(), if self.linear { "Linear".into() } else { crate::i18n::tr("Amiga periods").into() }));
        rows
    }

    fn sample_line(sample: &Sample) -> String {
        let looped = match sample.loop_kind {
            Loop::None => "no loop".to_string(),
            Loop::Forward => format!("loops {} to {}", sample.loop_start, sample.loop_end),
            Loop::PingPong => format!("loops back and forth {} to {}", sample.loop_start, sample.loop_end),
        };
        crate::i18n::tf!("{} samples ({}-bit), {looped}, volume {}, {} Hz", group(sample.data.len() as u64), sample.bits, sample.volume, sample.base_rate.round())
    }

    /// What File properties shows: the song, its instruments or samples, and its order.
    pub fn sections(&self) -> Vec<InfoSection> {
        let mut sections = vec![InfoSection { title: crate::i18n::tr("Music module").into(), rows: self.summary() }];
        if !self.instruments.is_empty() {
            let rows = self.instruments.iter().enumerate().filter(|(_, i)| !i.name.trim().is_empty() || i.keymap.iter().any(|k| k.1 > 0)).map(|(n, i)| (format!("{:02}", n + 1), i.name.clone())).collect();
            sections.push(InfoSection { title: "Instruments".into(), rows });
        }
        let samples = self.used_samples().map(|(n, s)| (format!("{n:02} {}", s.name), Self::sample_line(s))).collect();
        sections.push(InfoSection { title: "Samples".into(), rows: samples });
        sections.push(InfoSection { title: crate::i18n::tr("Song order").into(), rows: vec![(String::new(), self.order_text())] });
        if !self.message.trim().is_empty() {
            sections.push(InfoSection { title: "Message".into(), rows: vec![(String::new(), self.message.clone())] });
        }
        sections
    }

    fn order_text(&self) -> String {
        self.orders.iter().map(|&p| if p == 254 { "++".to_string() } else if p == 255 { "--".to_string() } else { format!("{p:02X}") }).collect::<Vec<_>>().join(" ")
    }

    /// The preview: the summary, the instruments, the samples and the order.
    pub fn blocks(&self) -> Vec<Block> {
        let plain = |text: String| Span { text, ..Span::default() };
        let strong = |text: String| Span { text, strong: true, ..Span::default() };
        let block = |kind: BlockKind, spans: Vec<Span>| Block { kind, spans, indent: 0, quote: 0, marker: None };
        let mut blocks = vec![block(BlockKind::Heading(2), vec![plain(if self.title.is_empty() { crate::i18n::tr("Untitled module").into() } else { self.title.clone() })])];
        for (label, value) in self.summary().into_iter().skip(1) {
            blocks.push(block(BlockKind::Paragraph, vec![strong(format!("{label}: ")), plain(value)]));
        }
        if !self.message.trim().is_empty() {
            blocks.push(block(BlockKind::Heading(3), vec![plain("Message".into())]));
            blocks.push(block(BlockKind::Code, vec![Span { text: self.message.clone(), code: true, ..Span::default() }]));
        }
        if !self.instruments.is_empty() {
            blocks.push(block(BlockKind::Heading(3), vec![plain("Instruments".into())]));
            let mut rows = vec![["#", "Name"].map(|h| vec![strong(h.into())]).to_vec()];
            for (n, instrument) in self.instruments.iter().enumerate() {
                if instrument.name.trim().is_empty() && instrument.keymap.iter().all(|k| k.1 == 0) {
                    continue;
                }
                rows.push([format!("{:02}", n + 1), instrument.name.clone()].map(|c| vec![plain(c)]).to_vec());
            }
            blocks.push(block(BlockKind::Table(rows), Vec::new()));
        }
        blocks.push(block(BlockKind::Heading(3), vec![plain("Samples".into())]));
        let mut rows = vec![["#", "Name", "Length", "Loop", "Volume", "Rate"].map(|h| vec![strong(h.into())]).to_vec()];
        for (n, sample) in self.samples.iter().enumerate() {
            // Empty slots with names are kept: trackers' users wrote messages and credits in them.
            if sample.data.is_empty() && sample.name.trim().is_empty() {
                continue;
            }
            let has = !sample.data.is_empty();
            let looped = if sample.loop_kind == Loop::None { String::new() } else { format!("{}\u{2013}{}", sample.loop_start, sample.loop_end) };
            let cells = [format!("{:02}", n + 1), sample.name.clone(), if has { group(sample.data.len() as u64) } else { String::new() }, looped, if has { sample.volume.to_string() } else { String::new() }, if has { format!("{} Hz", sample.base_rate.round()) } else { String::new() }];
            rows.push(cells.map(|c| vec![plain(c)]).to_vec());
        }
        blocks.push(block(BlockKind::Table(rows), Vec::new()));
        blocks.push(block(BlockKind::Heading(3), vec![plain(crate::i18n::tr("Song order").into())]));
        blocks.push(block(BlockKind::Code, vec![Span { text: self.order_text(), code: true, ..Span::default() }]));
        blocks
    }
}

/// A number with its thousands marked.
fn group(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Text from a fixed-length field: up to the first zero, anything unprintable as a space.
fn text(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    bytes[..end].iter().map(|&b| if (0x20..0x7F).contains(&b) || b >= 0xA0 { b as char } else { ' ' }).collect::<String>().trim_end().to_string()
}

fn u16_le(bytes: &[u8], at: usize) -> u16 {
    bytes.get(at..at + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}

fn u32_le(bytes: &[u8], at: usize) -> u32 {
    bytes.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Which PC tracker's module the bytes are, by the mark each one puts in its header.
pub fn pc_format(head: &[u8]) -> Option<Format> {
    if head.starts_with(b"Extended Module: ") {
        Some(Format::Xm)
    } else if head.starts_with(b"IMPM") {
        Some(Format::It)
    } else if head.get(44..48) == Some(b"SCRM") {
        Some(Format::S3m)
    } else {
        None
    }
}

/// Whether a file's name is one a PC tracker's module goes by.
pub fn named_as_pc_module(name: &str) -> bool {
    let lower = name.rsplit(['/', '\\']).next().unwrap_or(name).to_ascii_lowercase();
    [".s3m", ".xm", ".it"].iter().any(|e| lower.ends_with(e))
}

/// Reads a module of any of the kinds, to play.
pub fn load(bytes: &[u8], name: &str) -> Result<Song, String> {
    match pc_format(bytes) {
        Some(Format::Xm) => load_xm::load(bytes),
        Some(Format::It) => load_it::load(bytes),
        Some(Format::S3m) => load_s3m::load(bytes),
        _ => load_mod::load(bytes, name),
    }
}

/// ProTracker's and FastTracker's numbered effects as one effect.
fn mod_fx(effect: u8, param: u8, xm: bool) -> Fx {
    let (x, y) = (param >> 4, param & 15);
    match effect {
        0 if param != 0 => Fx::Arpeggio(param),
        0 => Fx::None,
        1 => Fx::PortaUp(param),
        2 => Fx::PortaDown(param),
        3 => Fx::TonePorta(param),
        4 => Fx::Vibrato(param),
        5 => Fx::TonePortaVolSlide(param),
        6 => Fx::VibratoVolSlide(param),
        7 => Fx::Tremolo(param),
        8 => Fx::Pan(param),
        9 => Fx::Offset(param),
        10 => Fx::VolSlide(param),
        11 => Fx::Jump(param),
        12 => Fx::SetVolume(param),
        13 => Fx::Break(x * 10 + y),
        14 => match x {
            1 => Fx::FinePortaUp(y),
            2 => Fx::FinePortaDown(y),
            4 => Fx::VibratoWave(y),
            5 => Fx::Finetune(y),
            6 => Fx::PatternLoop(y),
            7 => Fx::TremoloWave(y),
            8 => Fx::Pan(y * 17),
            9 => Fx::Retrig(y),
            10 => Fx::FineVolUp(y),
            11 => Fx::FineVolDown(y),
            12 => Fx::NoteCut(y),
            13 => Fx::NoteDelay(y),
            14 => Fx::PatternDelay(y),
            _ => Fx::None,
        },
        15 if param == 0 => Fx::None,
        15 if param < 32 => Fx::Speed(param),
        15 => Fx::Tempo(param),
        16 if xm => Fx::GlobalVolume(param.min(64) * 2),
        17 if xm => Fx::GlobalVolSlide(param),
        20 if xm => Fx::KeyOff(param),
        21 if xm => Fx::EnvelopePos(param),
        25 if xm => Fx::PanSlide(param),
        27 if xm => Fx::MultiRetrig(param),
        29 if xm => Fx::Tremor(param),
        33 if xm && x == 1 => Fx::ExtraFinePortaUp(y),
        33 if xm && x == 2 => Fx::ExtraFinePortaDown(y),
        _ => Fx::None,
    }
}

/// How ProTracker and FastTracker write an effect: its number in base 36, then the parameter.
fn mod_shown(effect: u8, param: u8) -> [u8; 3] {
    if effect == 0 && param == 0 {
        return [0; 3];
    }
    let digit = |d: u8| if d < 10 { b'0' + d } else { b'A' + d - 10 };
    let hex = |d: u8| if d < 10 { b'0' + d } else { b'A' + d - 10 };
    [digit(effect.min(35)), hex(param >> 4), hex(param & 15)]
}

/// Scream Tracker's and Impulse Tracker's lettered effects as one effect. `it` is Impulse Tracker's meanings
/// where they differ.
fn st_fx(command: u8, param: u8, it: bool) -> Fx {
    let (x, y) = (param >> 4, param & 15);
    match command {
        1 => Fx::Speed(param),
        2 => Fx::Jump(param),
        3 => Fx::Break(if it { param } else { x * 10 + y }),
        4 => Fx::StVolSlide(param),
        5 => Fx::StPortaDown(param),
        6 => Fx::StPortaUp(param),
        7 => Fx::TonePorta(param),
        8 => Fx::Vibrato(param),
        9 => Fx::Tremor(param),
        10 => Fx::Arpeggio(param),
        11 => Fx::VibratoVolSlide(param),
        12 => Fx::TonePortaVolSlide(param),
        13 if it => Fx::ChannelVolume(param),
        14 if it => Fx::ChannelVolSlide(param),
        15 => Fx::Offset(param),
        16 if it => Fx::PanSlide(param),
        17 => Fx::MultiRetrig(param),
        18 => Fx::Tremolo(param),
        19 => match x {
            2 => Fx::Finetune(y),
            3 => Fx::VibratoWave(y),
            4 => Fx::TremoloWave(y),
            6 if it => Fx::FinePatternDelay(y),
            7 if it => Fx::PastNotes(y),
            8 => Fx::Pan(y * 17),
            9 if y == 1 => Fx::Surround,
            10 if it => Fx::HighOffset(y),
            11 => Fx::PatternLoop(y),
            12 => Fx::NoteCut(y),
            13 => Fx::NoteDelay(y),
            14 => Fx::PatternDelay(y),
            _ => Fx::None,
        },
        20 if it && param < 0x20 => Fx::TempoSlide(param),
        20 if param >= 0x20 => Fx::Tempo(param),
        21 => Fx::FineVibrato(param),
        22 => Fx::GlobalVolume(if it { param.min(128) } else { param.min(64) * 2 }),
        23 if it => Fx::GlobalVolSlide(param),
        24 if it => Fx::Pan(param),
        24 if param == 0xA4 => Fx::Surround,
        24 => Fx::Pan(if param <= 0x80 { (u16::from(param) * 2).min(255) as u8 } else { 128 }),
        _ => Fx::None,
    }
}

fn st_shown(command: u8, param: u8) -> [u8; 3] {
    if command == 0 || command > 26 {
        return [0; 3];
    }
    let hex = |d: u8| if d < 10 { b'0' + d } else { b'A' + d - 10 };
    [b'A' + command - 1, hex(param >> 4), hex(param & 15)]
}

/// 8-bit samples made 16-bit.
fn widen(bytes: &[u8], signed: bool) -> Vec<i16> {
    bytes.iter().map(|&b| if signed { i16::from(b as i8) << 8 } else { (i16::from(b) - 128) << 8 }).collect()
}

fn widen16(bytes: &[u8], signed: bool) -> Vec<i16> {
    bytes.as_chunks::<2>().0.iter().map(|p| {
        let v = u16::from_le_bytes([p[0], p[1]]);
        if signed { v as i16 } else { (i32::from(v) - 32768) as i16 }
    }).collect()
}

#[cfg(test)]
mod tests;
