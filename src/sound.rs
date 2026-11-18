//! Sound files to play: MP3, WAV, FLAC, Ogg Vorbis and AIFF through Symphonia, and three older kinds read here
//! that it doesn't know: the Amiga's IFF 8SVX samples, Creative's VOC files from DOS games, and Sun's .au (.snd)
//! files from Unix workstations and the NeXT.
//!
//! Symphonia's are decoded a packet at a time as they play, so a long MP3 isn't held in memory; the older kinds
//! are small and are decoded whole.

use crate::media::InfoSection;
use crate::preview::{Block, BlockKind, Span};
use std::collections::VecDeque;
use std::io::Cursor;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;

/// The endings sound files go by.
pub const ENDINGS: [&str; 13] = ["wav", "mp3", "mp2", "flac", "ogg", "oga", "aif", "aiff", "aifc", "8svx", "voc", "au", "snd"];

/// Whether a file's name or first bytes say it's a sound file.
pub fn recognised(head: &[u8], name: &str) -> bool {
    let ending = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    if ENDINGS.contains(&ending.as_str()) && ending != "snd" {
        return true;
    }
    let form = |kind: &[u8]| head.starts_with(b"FORM") && head.get(8..12) == Some(kind);
    (head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WAVE"))
        || head.starts_with(b"fLaC")
        || head.starts_with(b"OggS")
        || head.starts_with(b"ID3")
        || form(b"AIFF")
        || form(b"AIFC")
        || form(b"8SVX")
        || head.starts_with(b"Creative Voice File\x1A")
        || head.starts_with(b".snd")
}

/// What a sound file says about itself.
#[derive(Clone, Debug, Default)]
pub struct Info {
    pub format: String,
    pub codec: String,
    pub rate: u32,
    pub channels: usize,
    pub bits: Option<u32>,
    pub seconds: Option<f64>,
    /// Title, artist and the rest, as the file's tags give them.
    pub tags: Vec<(String, String)>,
}

impl Info {
    fn rows(&self) -> Vec<(String, String)> {
        let mut rows = vec![("Format".to_string(), self.format.clone())];
        if !self.codec.is_empty() && self.codec != self.format {
            rows.push(("Encoding".into(), self.codec.clone()));
        }
        rows.push((crate::i18n::tr("Sample rate").into(), format!("{} Hz", self.rate)));
        rows.push(("Channels".into(), match self.channels {
            1 => "1 (mono)".to_string(),
            2 => "2 (stereo)".to_string(),
            n => n.to_string(),
        }));
        if let Some(bits) = self.bits {
            rows.push(("Bits".into(), bits.to_string()));
        }
        if let Some(seconds) = self.seconds {
            rows.push(("Length".into(), clock(seconds)));
        }
        rows.extend(self.tags.iter().cloned());
        rows
    }

    pub fn sections(&self) -> Vec<InfoSection> {
        vec![InfoSection { title: "Sound".into(), rows: self.rows() }]
    }

    pub fn blocks(&self) -> Vec<Block> {
        let title = self.tags.iter().find(|(k, _)| k == "Title").map_or_else(|| self.format.clone(), |(_, v)| v.clone());
        let mut blocks = vec![Block { kind: BlockKind::Heading(2), spans: vec![Span { text: title, ..Span::default() }], indent: 0, quote: 0, marker: None }];
        for (label, value) in self.rows() {
            blocks.push(Block { kind: BlockKind::Paragraph, spans: vec![Span { text: format!("{label}: "), strong: true, ..Span::default() }, Span { text: value, ..Span::default() }], indent: 0, quote: 0, marker: None });
        }
        blocks
    }
}

/// Minutes and seconds, as 3:07.
pub fn clock(seconds: f64) -> String {
    let whole = seconds.max(0.0).round() as u64;
    if whole >= 3600 { format!("{}:{:02}:{:02}", whole / 3600, whole / 60 % 60, whole % 60) } else { format!("{}:{:02}", whole / 60, whole % 60) }
}

/// Sound decoded whole: samples one channel after another for each frame.
#[derive(Clone, Debug, Default)]
pub struct Pcm {
    pub rate: u32,
    pub channels: usize,
    pub samples: Vec<f32>,
    pub info: Info,
}

fn be16(b: &[u8], at: usize) -> u16 {
    b.get(at..at + 2).map_or(0, |b| u16::from_be_bytes([b[0], b[1]]))
}

fn be32(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4).map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// An Amiga IFF 8SVX sample: its VHDR, its BODY of signed 8-bit samples (or Fibonacci-delta packed), and
/// its NAME, AUTH and ANNO if it has them.
pub fn read_8svx(bytes: &[u8]) -> Result<Pcm, String> {
    if !(bytes.starts_with(b"FORM") && bytes.get(8..12) == Some(b"8SVX")) {
        return Err(crate::i18n::tr("This isn't an IFF 8SVX sound.").into());
    }
    let mut at = 12;
    let (mut rate, mut packed, mut body, mut stereo) = (8363u32, false, None, false);
    let mut tags = Vec::new();
    let end = (8 + be32(bytes, 4) as usize).min(bytes.len());
    while at + 8 <= end {
        let id = &bytes[at..at + 4];
        let size = be32(bytes, at + 4) as usize;
        let data = &bytes[at + 8..(at + 8 + size).min(bytes.len())];
        match id {
            b"VHDR" if data.len() >= 16 => {
                rate = u32::from(be16(data, 12)).max(1);
                packed = data[15] == 1;
            }
            b"CHAN" if data.len() >= 4 => stereo = be32(data, 0) == 6,
            b"BODY" => body = Some(data),
            b"NAME" | b"AUTH" | b"ANNO" | b"(c) " => {
                let label = match id {
                    b"NAME" => "Title",
                    b"AUTH" => "Author",
                    b"ANNO" => "Note",
                    _ => "Copyright",
                };
                let text: String = data.iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
                if !text.trim().is_empty() {
                    tags.push((label.to_string(), text.trim().to_string()));
                }
            }
            _ => {}
        }
        at += 8 + size + (size & 1);
    }
    let body = body.ok_or(crate::i18n::tr("The 8SVX file has no BODY of samples."))?;
    let raw: Vec<i8> = if packed {
        // Fibonacci-delta: two 4-bit steps a byte, after a byte to start from.
        const STEPS: [i8; 16] = [-34, -21, -13, -8, -5, -3, -2, -1, 0, 1, 2, 3, 5, 8, 13, 21];
        let mut value = body.get(1).copied().unwrap_or(0) as i8;
        let mut out = Vec::with_capacity(body.len() * 2);
        for &b in body.iter().skip(2) {
            for nibble in [b >> 4, b & 15] {
                value = value.wrapping_add(STEPS[usize::from(nibble)]);
                out.push(value);
            }
        }
        out
    } else {
        body.iter().map(|&b| b as i8).collect()
    };
    let channels = if stereo { 2 } else { 1 };
    let samples: Vec<f32> = if stereo {
        // Left then right, each half the body.
        let half = raw.len() / 2;
        (0..half).flat_map(|i| [f32::from(raw[i]) / 128.0, f32::from(raw[half + i]) / 128.0]).collect()
    } else {
        raw.iter().map(|&s| f32::from(s) / 128.0).collect()
    };
    let seconds = samples.len() as f64 / channels as f64 / f64::from(rate);
    let codec = if packed { "Fibonacci-delta packed 8-bit" } else { "8-bit signed" };
    Ok(Pcm { rate, channels, info: Info { format: crate::i18n::tr("Amiga IFF 8SVX sample").into(), codec: codec.into(), rate, channels, bits: Some(8), seconds: Some(seconds), tags }, samples })
}

/// A Creative Voice File: blocks of sound data, 8-bit unsigned (and, from version 1.20, 16-bit) at the rate
/// each block gives.
pub fn read_voc(bytes: &[u8]) -> Result<Pcm, String> {
    if !bytes.starts_with(b"Creative Voice File\x1A") {
        return Err(crate::i18n::tr("This isn't a Creative Voice File.").into());
    }
    let mut at = usize::from(u16::from_le_bytes([bytes[20], bytes[21]]));
    let (mut rate, mut channels, mut bits) = (0u32, 1usize, 8u32);
    let mut samples = Vec::new();
    let mut codec = "8-bit unsigned";
    let mut tags = Vec::new();
    let mut pending: Option<(u32, usize)> = None;
    while at < bytes.len() {
        let kind = bytes[at];
        if kind == 0 || at + 4 > bytes.len() {
            break;
        }
        let size = usize::from(bytes[at + 1]) | usize::from(bytes[at + 2]) << 8 | usize::from(bytes[at + 3]) << 16;
        let data = &bytes[at + 4..(at + 4 + size).min(bytes.len())];
        match kind {
            1 if data.len() >= 2 => {
                let (block_rate, block_channels) = pending.take().unwrap_or((1_000_000 / (256 - u32::from(data[0])).max(1), 1));
                if data[1] != 0 {
                    codec = "packed (not played)";
                } else {
                    rate = block_rate;
                    channels = block_channels;
                    samples.extend(data[2..].iter().map(|&b| (f32::from(b) - 128.0) / 128.0));
                }
            }
            2 => samples.extend(data.iter().map(|&b| (f32::from(b) - 128.0) / 128.0)),
            3 if data.len() >= 3 => {
                let length = usize::from(u16::from_le_bytes([data[0], data[1]])) + 1;
                samples.extend(std::iter::repeat_n(0.0, length * channels));
            }
            5 => {
                let text: String = data.iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
                if !text.trim().is_empty() {
                    tags.push(("Note".to_string(), text.trim().to_string()));
                }
            }
            8 if data.len() >= 4 => {
                let tc = u32::from(u16::from_le_bytes([data[0], data[1]]));
                let stereo = data[3] == 1;
                let ch = if stereo { 2 } else { 1 };
                pending = Some((256_000_000 / (ch as u32 * (65_536 - tc).max(1)), ch));
            }
            9 if data.len() >= 12 => {
                rate = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                bits = u32::from(data[4]);
                channels = usize::from(data[5]).max(1);
                let format = u16::from_le_bytes([data[6], data[7]]);
                let pcm = &data[12..];
                match (format, bits) {
                    (0, 8) => samples.extend(pcm.iter().map(|&b| (f32::from(b) - 128.0) / 128.0)),
                    (4, 16) => {
                        codec = "16-bit signed";
                        samples.extend(pcm.as_chunks::<2>().0.iter().map(|p| f32::from(i16::from_le_bytes([p[0], p[1]])) / 32768.0));
                    }
                    _ => codec = "packed (not played)",
                }
            }
            _ => {}
        }
        at += 4 + size;
    }
    if rate == 0 || samples.is_empty() {
        return Err(crate::i18n::tr("The VOC file has no sound in a form Limoniter plays.").into());
    }
    let seconds = samples.len() as f64 / channels as f64 / f64::from(rate);
    Ok(Pcm { rate, channels, info: Info { format: crate::i18n::tr("Creative Voice File (VOC)").into(), codec: codec.into(), rate, channels, bits: Some(bits), seconds: Some(seconds), tags }, samples })
}

/// A Sun/NeXT .au file: a big-endian header, then samples in μ-law, A-law, linear 8, 16, 24 or 32-bit, or float.
pub fn read_au(bytes: &[u8]) -> Result<Pcm, String> {
    if !bytes.starts_with(b".snd") || bytes.len() < 24 {
        return Err(crate::i18n::tr("This isn't a Sun or NeXT sound file.").into());
    }
    let offset = be32(bytes, 4) as usize;
    let size = be32(bytes, 8) as usize;
    let encoding = be32(bytes, 12);
    let rate = be32(bytes, 16).max(1);
    let channels = (be32(bytes, 20) as usize).max(1);
    let end = if size == 0xFFFF_FFFF { bytes.len() } else { (offset + size).min(bytes.len()) };
    let data = bytes.get(offset.min(bytes.len())..end).unwrap_or(&[]);
    let (codec, bits, samples): (&str, u32, Vec<f32>) = match encoding {
        1 => ("\u{3BC}-law", 8, data.iter().map(|&b| ulaw(b)).collect()),
        2 => ("8-bit linear", 8, data.iter().map(|&b| f32::from(b as i8) / 128.0).collect()),
        3 => ("16-bit linear", 16, data.as_chunks::<2>().0.iter().map(|p| f32::from(i16::from_be_bytes([p[0], p[1]])) / 32768.0).collect()),
        4 => ("24-bit linear", 24, data.as_chunks::<3>().0.iter().map(|p| (i32::from_be_bytes([p[0], p[1], p[2], 0]) >> 8) as f32 / 8_388_608.0).collect()),
        5 => ("32-bit linear", 32, data.as_chunks::<4>().0.iter().map(|p| i32::from_be_bytes([p[0], p[1], p[2], p[3]]) as f32 / 2_147_483_648.0).collect()),
        6 => ("32-bit float", 32, data.as_chunks::<4>().0.iter().map(|p| f32::from_be_bytes([p[0], p[1], p[2], p[3]])).collect()),
        27 => ("A-law", 8, data.iter().map(|&b| alaw(b)).collect()),
        other => return Err(crate::i18n::tf!("This .au file's samples are in encoding {other}, which Limoniter doesn't play.")),
    };
    let mut tags = Vec::new();
    if let Some(note) = bytes.get(24..offset.min(bytes.len())) {
        let text: String = note.iter().take_while(|&&b| b != 0).map(|&b| b as char).collect();
        if !text.trim().is_empty() {
            tags.push(("Note".to_string(), text.trim().to_string()));
        }
    }
    let seconds = samples.len() as f64 / channels as f64 / f64::from(rate);
    Ok(Pcm { rate, channels, info: Info { format: crate::i18n::tr("Sun/NeXT audio (.au)").into(), codec: codec.into(), rate, channels, bits: Some(bits), seconds: Some(seconds), tags }, samples })
}

fn ulaw(b: u8) -> f32 {
    let b = !b;
    let sign = b & 0x80;
    let exponent = (b >> 4) & 7;
    let mantissa = i32::from(b & 15);
    let magnitude = (((mantissa << 3) + 0x84) << exponent) - 0x84;
    (if sign != 0 { -magnitude } else { magnitude }) as f32 / 32768.0
}

fn alaw(b: u8) -> f32 {
    let b = b ^ 0x55;
    let sign = b & 0x80;
    let exponent = (b >> 4) & 7;
    let mantissa = i32::from(b & 15);
    let magnitude = if exponent == 0 { (mantissa << 4) + 8 } else { ((mantissa << 4) + 0x108) << (exponent - 1) };
    (if sign != 0 { magnitude } else { -magnitude }) as f32 / 32768.0
}

/// The old kinds read here, by their first bytes.
fn read_own(bytes: &[u8]) -> Option<Result<Pcm, String>> {
    if bytes.starts_with(b"FORM") && bytes.get(8..12) == Some(b"8SVX") {
        Some(read_8svx(bytes))
    } else if bytes.starts_with(b"Creative Voice File") {
        Some(read_voc(bytes))
    } else if bytes.starts_with(b".snd") {
        Some(read_au(bytes))
    } else {
        None
    }
}

fn open_symphonia(bytes: std::sync::Arc<[u8]>, name: &str) -> Result<Box<dyn FormatReader>, String> {
    let mut hint = Hint::new();
    if let Some((_, ending)) = name.rsplit_once('.') {
        hint.with_extension(&ending.to_ascii_lowercase());
    }
    let stream = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    symphonia::default::get_probe().probe(&hint, stream, FormatOptions::default(), MetadataOptions::default()).map_err(|e| crate::i18n::tf!("This isn't a sound file Limoniter can play ({e})."))
}

fn symphonia_info(reader: &mut Box<dyn FormatReader>) -> Result<(Info, u32), String> {
    let track = reader.first_track_known_codec(TrackType::Audio).ok_or(crate::i18n::tr("There's no sound in the file that Limoniter can play."))?.clone();
    let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or(crate::i18n::tr("There's no sound in the file that Limoniter can play."))?.clone();
    let codec = symphonia::default::get_codecs().get_audio_decoder(params.codec).map(|c| c.codec.info.long_name.to_string()).unwrap_or_default();
    let rate = params.sample_rate.unwrap_or(44_100);
    let seconds = match (track.time_base, track.duration) {
        (Some(base), Some(duration)) => base.calc_duration(duration).map(|t| t.as_secs_f64()),
        _ => track.num_frames.map(|frames| frames as f64 / f64::from(rate)),
    };
    let mut tags = Vec::new();
    if let Some(revision) = reader.metadata().skip_to_latest() {
        for tag in revision.media.tags.iter().chain(revision.per_track.iter().flat_map(|t| t.metadata.tags.iter())) {
            use symphonia::core::meta::StandardTag as T;
            let named = match &tag.std {
                Some(T::TrackTitle(v)) => Some(("Title", v.to_string())),
                Some(T::Artist(v)) => Some(("Artist", v.to_string())),
                Some(T::Album(v)) => Some(("Album", v.to_string())),
                Some(T::Composer(v)) => Some(("Composer", v.to_string())),
                Some(T::Genre(v)) => Some(("Genre", v.to_string())),
                Some(T::RecordingDate(v)) | Some(T::ReleaseDate(v)) => Some(("Date", v.to_string())),
                Some(T::Comment(v)) => Some(("Comment", v.to_string())),
                Some(T::Copyright(v)) => Some(("Copyright", v.to_string())),
                _ => None,
            };
            if let Some((label, value)) = named
                && !value.trim().is_empty()
                && !tags.iter().any(|(l, _): &(String, String)| l == label)
            {
                tags.push((label.to_string(), value.trim().to_string()));
            }
        }
    }
    let about = reader.format_info();
    let format = match about.short_name {
        "mp1" => "MPEG audio, layer 1 (MP1)",
        "mp2" => "MPEG audio, layer 2 (MP2)",
        "mp3" => "MPEG audio, layer 3 (MP3)",
        "wave" => "WAV",
        "aiff" => "AIFF",
        "flac" => "FLAC",
        "ogg" => "Ogg",
        _ => about.long_name,
    };
    // Where the format says it all (an MP3's codec is MP3), the encoding isn't said again.
    let codec = if about.short_name.starts_with("mp") || about.short_name == "flac" { String::new() } else { codec };
    let info = Info { format: format.to_string(), codec, rate, channels: params.channels.as_ref().map_or(2, |c| c.count()), bits: params.bits_per_sample, seconds, tags };
    Ok((info, track.id))
}

/// What a sound file says about itself, without playing it.
pub fn info(bytes: &[u8], name: &str) -> Result<Info, String> {
    if let Some(pcm) = read_own(bytes) {
        return pcm.map(|p| p.info);
    }
    let mut reader = open_symphonia(bytes.into(), name)?;
    symphonia_info(&mut reader).map(|(info, _)| info)
}

/// Where a sound's frames come from as it plays.
enum Feed {
    Whole(Pcm, usize),
    Decoding {
        reader: Box<dyn FormatReader>,
        decoder: Box<dyn AudioDecoder>,
        track: u32,
        channels: usize,
        rate: u32,
        done: bool,
        time_base: Option<symphonia::core::units::TimeBase>,
        /// A packet read to check a seek worked, to be decoded first.
        pending: Option<symphonia::core::packet::Packet>,
        /// The file, to open again when a seek doesn't work.
        bytes: std::sync::Arc<[u8]>,
        name: String,
    },
}

/// A sound file playing, at the rate the speakers ask for.
pub struct SoundPlayer {
    feed: Feed,
    pub info: Info,
    out_rate: f64,
    /// Decoded stereo frames waiting, and how far into the first one the output has got.
    waiting: VecDeque<[f32; 2]>,
    along: f64,
    /// Frames of the file's own rate taken so far, which says where it is.
    taken: u64,
    /// Frames still to be dropped after a seek, which lands on the packet before the time asked for.
    skip: u64,
    scratch: Vec<f32>,
}

impl SoundPlayer {
    pub fn new(bytes: Vec<u8>, name: &str, out_rate: u32) -> Result<SoundPlayer, String> {
        let feed = if let Some(pcm) = read_own(&bytes) {
            Feed::Whole(pcm?, 0)
        } else {
            let bytes: std::sync::Arc<[u8]> = bytes.into();
            let mut reader = open_symphonia(bytes.clone(), name)?;
            let (info, track) = symphonia_info(&mut reader)?;
            let params = reader.tracks().iter().find(|t| t.id == track).and_then(|t| t.codec_params.as_ref()).and_then(|p| p.audio()).cloned().ok_or(crate::i18n::tr("The sound's track has gone."))?;
            let time_base = reader.tracks().iter().find(|t| t.id == track).and_then(|t| t.time_base);
            let decoder = symphonia::default::get_codecs().make_audio_decoder(&params, &AudioDecoderOptions::default()).map_err(|e| crate::i18n::tf!("Limoniter can't decode this sound ({e})."))?;
            return Ok(SoundPlayer::with(Feed::Decoding { reader, decoder, track, channels: info.channels.max(1), rate: info.rate, done: false, time_base, pending: None, bytes, name: name.to_string() }, info, out_rate));
        };
        let info = match &feed {
            Feed::Whole(pcm, _) => pcm.info.clone(),
            Feed::Decoding { .. } => Info::default(),
        };
        Ok(SoundPlayer::with(feed, info, out_rate))
    }

    fn with(feed: Feed, info: Info, out_rate: u32) -> SoundPlayer {
        SoundPlayer { feed, info, out_rate: f64::from(out_rate.max(1000)), waiting: VecDeque::new(), along: 0.0, taken: 0, skip: 0, scratch: Vec::new() }
    }

    fn rate(&self) -> u32 {
        match &self.feed {
            Feed::Whole(pcm, _) => pcm.rate,
            Feed::Decoding { rate, .. } => *rate,
        }
        .max(1)
    }

    /// Seconds played.
    pub fn time(&self) -> f64 {
        self.taken as f64 / f64::from(self.rate())
    }

    pub fn ended(&self) -> bool {
        self.waiting.len() < 2
            && match &self.feed {
                Feed::Whole(pcm, at) => *at >= pcm.samples.len(),
                Feed::Decoding { done, .. } => *done,
            }
    }

    /// More frames decoded into the queue; false when there are no more.
    fn fill(&mut self) -> bool {
        match &mut self.feed {
            Feed::Whole(pcm, at) => {
                let channels = pcm.channels.max(1);
                let end = (*at + 4096 * channels).min(pcm.samples.len());
                if *at >= end {
                    return false;
                }
                for frame in pcm.samples[*at..end].chunks(channels) {
                    let left = frame[0];
                    let right = if channels > 1 { frame[1] } else { left };
                    self.waiting.push_back([left, right]);
                }
                *at = end;
                true
            }
            Feed::Decoding { reader, decoder, track, channels, done, pending, .. } => {
                let skip = &mut self.skip;
                if *done {
                    return false;
                }
                loop {
                    let packet = match pending.take().map_or_else(|| reader.next_packet(), |p| Ok(Some(p))) {
                        Ok(Some(packet)) => packet,
                        Ok(None) | Err(_) => {
                            *done = true;
                            return false;
                        }
                    };
                    if packet.track_id != *track {
                        continue;
                    }
                    match decoder.decode(&packet) {
                        Ok(buffer) => {
                            let count = buffer.spec().channels().count().max(1);
                            *channels = count;
                            self.scratch.clear();
                            buffer.copy_to_vec_interleaved(&mut self.scratch);
                            for frame in self.scratch.chunks(count) {
                                if *skip > 0 {
                                    *skip -= 1;
                                    continue;
                                }
                                let left = frame[0];
                                let right = if count > 1 { frame[1] } else { left };
                                self.waiting.push_back([left, right]);
                            }
                            return true;
                        }
                        // A damaged packet is skipped, as players do.
                        Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                        Err(_) => {
                            *done = true;
                            return false;
                        }
                    }
                }
            }
        }
    }

    /// Plays into `out`, left and right one after the other, at the speakers' rate.
    pub fn render(&mut self, out: &mut [f32]) {
        let step = f64::from(self.rate()) / self.out_rate;
        for frame in out.as_chunks_mut::<2>().0 {
            while self.waiting.len() < 2 {
                if !self.fill() {
                    break;
                }
            }
            if self.waiting.len() < 2 {
                frame[0] = 0.0;
                frame[1] = 0.0;
                continue;
            }
            let (a, b) = (self.waiting[0], self.waiting[1]);
            let t = self.along as f32;
            frame[0] = a[0] + (b[0] - a[0]) * t;
            frame[1] = a[1] + (b[1] - a[1]) * t;
            self.along += step;
            while self.along >= 1.0 && !self.waiting.is_empty() {
                self.along -= 1.0;
                self.waiting.pop_front();
                self.taken += 1;
            }
        }
    }

    /// Goes to a time, in seconds from the start.
    pub fn seek(&mut self, seconds: f64) {
        self.waiting.clear();
        self.along = 0.0;
        let rate = self.rate();
        match &mut self.feed {
            Feed::Whole(pcm, at) => {
                let frame = (seconds.max(0.0) * f64::from(rate)) as usize;
                *at = (frame * pcm.channels.max(1)).min(pcm.samples.len());
                self.taken = frame as u64;
            }
            Feed::Decoding { reader, decoder, track, done, time_base, pending, bytes, name, .. } => {
                let seconds = seconds.max(0.0);
                let Some(time) = Time::try_from_secs_f64(seconds) else { return };
                self.taken = (seconds * f64::from(rate)) as u64;
                *done = false;
                *pending = None;
                decoder.reset();
                if let Ok(to) = reader.seek(SeekMode::Accurate, SeekTo::Time { time, track_id: Some(*track) }) {
                    let at = |ts| time_base.and_then(|base| base.calc_time(ts)).map_or(0.0, |t| t.as_secs_f64());
                    let early = (at(to.required_ts) - at(to.actual_ts)).max(0.0);
                    self.skip = (early * f64::from(rate)).round() as u64;
                    // Symphonia's WAV reader finds nothing after seeking ADPCM; then it's read again from the start.
                    if let Ok(Some(packet)) = reader.next_packet() {
                        *pending = Some(packet);
                        return;
                    }
                }
                match open_symphonia(bytes.clone(), name) {
                    Ok(fresh) => {
                        *reader = fresh;
                        self.skip = self.taken;
                    }
                    Err(_) => *done = true,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A WAV file of a 441 Hz sine at 44.1 kHz, 16-bit mono.
    fn wav(seconds: f64) -> Vec<u8> {
        let frames = (44_100.0 * seconds) as usize;
        let mut data = Vec::new();
        for i in 0..frames {
            let v = ((i as f64 * 441.0 / 44_100.0 * std::f64::consts::TAU).sin() * 16_000.0) as i16;
            data.extend_from_slice(&v.to_le_bytes());
        }
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&[16, 0, 0, 0, 1, 0, 1, 0]);
        b.extend_from_slice(&44_100u32.to_le_bytes());
        b.extend_from_slice(&88_200u32.to_le_bytes());
        b.extend_from_slice(&[2, 0, 16, 0]);
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(data.len() as u32).to_le_bytes());
        b.extend_from_slice(&data);
        b
    }

    fn rises(out: &[f32]) -> usize {
        out.as_chunks::<2>().0.iter().map(|f| f[0]).collect::<Vec<_>>().windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count()
    }

    #[test]
    fn a_wav_file_plays_at_its_pitch_at_any_rate() {
        let bytes = wav(1.0);
        assert!(recognised(&bytes, "x.bin"));
        let info = info(&bytes, "tone.wav").unwrap();
        assert_eq!((info.rate, info.channels, info.bits), (44_100, 1, Some(16)));
        assert!((info.seconds.unwrap() - 1.0).abs() < 0.01);
        for rate in [44_100, 48_000] {
            let mut player = SoundPlayer::new(bytes.clone(), "tone.wav", rate).unwrap();
            let mut out = vec![0.0; rate as usize];
            player.render(&mut out);
            assert!((219..=222).contains(&rises(&out)), "{rate}: {}", rises(&out));
            assert!((player.time() - 0.5).abs() < 0.01);
            assert!(!player.ended());
            player.seek(0.9);
            let mut rest = vec![0.0; rate as usize];
            player.render(&mut rest);
            assert!(player.ended());
        }
    }

    #[test]
    fn old_kinds_are_read_here() {
        // An 8SVX sample with a name.
        let mut svx = b"FORM\0\0\0\08SVXVHDR\0\0\0\x14".to_vec();
        svx.extend_from_slice(&[0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0x1F, 0x40, 1, 0, 0, 1, 0, 0]);
        svx.extend_from_slice(b"NAME\0\0\0\x04bell");
        svx.extend_from_slice(b"BODY\0\0\0\x04");
        svx.extend_from_slice(&[0x40, 0xC0, 0x7F, 0x80]);
        let size = (svx.len() - 8) as u32;
        svx[4..8].copy_from_slice(&size.to_be_bytes());
        let pcm = read_8svx(&svx).unwrap();
        assert_eq!((pcm.rate, pcm.channels, pcm.samples.len()), (8000, 1, 4));
        assert_eq!(pcm.samples, [0.5, -0.5, 127.0 / 128.0, -1.0]);
        assert_eq!(pcm.info.tags, [("Title".to_string(), "bell".to_string())]);
        // A VOC with one block of 8-bit sound at 1,000,000 / (256 - 156) = 10 kHz.
        let mut voc = b"Creative Voice File\x1A\x1A\x00\x0A\x01\x29\x11".to_vec();
        voc.extend_from_slice(&[1, 6, 0, 0, 156, 0, 128, 255, 0, 128, 0]);
        let pcm = read_voc(&voc).unwrap();
        assert_eq!((pcm.rate, pcm.samples.len()), (10_000, 4));
        assert_eq!(pcm.samples[1], 127.0 / 128.0);
        // A μ-law .au: 0xFF is silence, 0x80 the loudest.
        let mut au = b".snd".to_vec();
        for v in [24u32, 3, 1, 8000, 1] {
            au.extend_from_slice(&v.to_be_bytes());
        }
        au.extend_from_slice(&[0xFF, 0x80, 0x00]);
        let pcm = read_au(&au).unwrap();
        assert_eq!(pcm.samples[0], 0.0);
        assert!(pcm.samples[1] > 0.9 && pcm.samples[2] < -0.9);
        assert!(recognised(&au, "beep") && recognised(&svx, "x") && recognised(&voc, "x"));
        assert!(!recognised(b"hello", "notes.txt"));
    }
}

/// Tones made by ffmpeg in ../testmedia/sounds, if they're there: three seconds of 441 Hz in each of Symphonia's
/// formats, with a title and an artist.
#[cfg(test)]
#[test]
fn real_files_of_each_kind_play() {
    let folder = std::path::Path::new("../testmedia/sounds");
    if !folder.exists() {
        return;
    }
    for name in ["tone.mp3", "tone.flac", "tone.ogg", "tone.aiff", "tone-adpcm.wav"] {
        let bytes = std::fs::read(folder.join(name)).unwrap();
        assert!(recognised(&bytes, name), "{name}");
        let about = info(&bytes, name).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!((about.seconds.unwrap_or(0.0) - 3.0).abs() < 0.1, "{name}: {:?}", about.seconds);
        assert_eq!(about.rate, 44_100, "{name}");
        if name != "tone.aiff" && name != "tone-adpcm.wav" {
            assert!(about.tags.contains(&("Title".to_string(), "Test tone".to_string())), "{name}: {:?}", about.tags);
        }
        let mut player = SoundPlayer::new(bytes, name, 48_000).unwrap();
        player.seek(1.0);
        let mut out = vec![0.0; 96_000];
        player.render(&mut out);
        let rises = out.as_chunks::<2>().0.iter().map(|f| f[0]).collect::<Vec<_>>().windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        assert!((438..=444).contains(&rises), "{name}: {rises}");
        assert!((player.time() - 2.0).abs() < 0.1, "{name}: {}", player.time());
    }
}
