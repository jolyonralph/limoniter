//! Atari ST chip music, read for what it says about itself: SNDH files (a replay routine and its tune behind a
//! header of tags, very often packed with Pack-Ice) and Leonard's YM files (the YM2149's registers frame by frame,
//! usually packed with LHA). Only read: nothing here plays them. The replay calls for an SNDH file are written
//! here too, for 68000 source.

use crate::media::InfoSection;
use crate::preview::{Block, BlockKind, Span};
use crate::st_packers::{self, Packer};

/// What calls an SNDH file's play routine, and how often.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timer {
    A,
    B,
    C,
    D,
    /// The vertical blank, once a frame.
    Vbl,
}

impl Timer {
    pub fn name(self) -> &'static str {
        match self {
            Timer::A => "Timer A",
            Timer::B => "Timer B",
            Timer::C => "Timer C",
            Timer::D => "Timer D",
            Timer::Vbl => "the VBL",
        }
    }

    fn tag(self) -> &'static str {
        match self {
            Timer::A => "TA",
            Timer::B => "TB",
            Timer::C => "TC",
            Timer::D => "TD",
            Timer::Vbl => "!V",
        }
    }
}

/// One of an SNDH file's tunes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tune {
    pub name: Option<String>,
    /// How long it plays, in calls of the play routine (FRMS); 0 means it loops for ever.
    pub frames: Option<u32>,
    /// How long it plays in seconds, as older files say (TIME); 0 means for ever.
    pub seconds: Option<u32>,
    /// What it needs, as its own flags say, where each tune has its own.
    pub flags: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sndh {
    pub title: Option<String>,
    pub composer: Option<String>,
    pub ripper: Option<String>,
    pub converter: Option<String>,
    pub year: Option<String>,
    pub tunes: Vec<Tune>,
    /// The tune played first, counted from 1.
    pub default_tune: Option<usize>,
    /// The timer the file asks to be played from, and its rate in Hz. Timer C at 50 Hz if it doesn't say.
    pub timer: Option<(Timer, u32)>,
    /// The flags for the whole file (FLAG~).
    pub flags: Option<String>,
    /// How long the header is, up to where it ended (HDNS, or the first tag not known).
    pub header_size: usize,
    /// Where the init, exit and play routines are, from the branches at the start.
    pub routines: [Option<usize>; 3],
    /// The packer it came packed with, and the packed size.
    pub packed: Option<(Packer, u64)>,
    pub size: usize,
    pub notes: Vec<String>,
}

/// What each flag letter says the music needs or uses, from the SNDH format's own list.
pub fn flag_meaning(flag: char) -> Option<&'static str> {
    Some(match flag {
        'a' => "uses Timer A",
        'b' => "uses Timer B",
        'c' => "uses Timer C",
        'd' => "uses Timer D",
        'e' => "needs an STE",
        'f' | 'x' => "sound effects",
        'g' => "digital sound",
        'h' => "uses the HBL",
        'j' => "jingles",
        'k' => "kills the system",
        'l' => "uses the STE's LMC1992 (volume and tone)",
        'p' => "Amiga AGA",
        's' => "needs the Falcon's DSP 56001",
        't' | 'B' => "uses the blitter",
        'y' => "plays on the YM2149",
        '0' => "STE DMA sound at 6.25 kHz",
        '1' => "STE or Falcon DMA sound at 12.5 kHz",
        '2' => "STE or Falcon DMA sound at 25 kHz",
        '3' => "STE or Falcon DMA sound at 50 kHz",
        '4' => "Falcon DMA sound at 12.3 kHz",
        '5' => "Falcon DMA sound at 14 kHz",
        '6' => "Falcon DMA sound at 16.4 kHz",
        '7' => "Falcon DMA sound at 19.7 kHz",
        '8' => "Falcon DMA sound at 24.6 kHz",
        '9' => "Falcon DMA sound at 32.8 kHz",
        'A' => "Falcon DMA sound at 49.2 kHz",
        'C' => "needs a 68020 or later",
        'F' => "filters",
        'S' => "stereo",
        _ => return None,
    })
}

/// Flags said in words: "uses Timer D, plays on the YM2149".
pub fn flags_in_words(flags: &str) -> String {
    let words: Vec<String> = flags.chars().filter(|c| !c.is_whitespace()).map(|c| flag_meaning(c).map_or_else(|| format!("'{c}'"), str::to_string)).collect();
    words.join(", ")
}

/// Text in the ST's own character set.
fn st_text(bytes: &[u8]) -> String {
    let set = crate::charset::known("atari-st");
    bytes.iter().map(|&b| if b < 0x80 { if b < 0x20 { ' ' } else { b as char } } else { set.decoded[usize::from(b)] }).collect::<String>().trim().to_string()
}

/// Where a branch at the start of the file goes, as the routines' entries are written: bra.s, bra.w or jmp(pc),
/// after any nops.
fn branch_target(bytes: &[u8], at: usize) -> Option<usize> {
    let word = |at: usize| bytes.get(at..at + 2).map(|w| u16::from_be_bytes([w[0], w[1]]));
    let w = word(at)?;
    if w == 0x4E71 {
        return branch_target(bytes, at + 2);
    }
    if w & 0xFF00 == 0x6000 && w & 0xFF != 0 {
        return usize::try_from(at as isize + 2 + isize::from((w & 0xFF) as u8 as i8)).ok();
    }
    if w == 0x6000 || w == 0x4EFA {
        let displacement = word(at + 2)? as i16;
        return usize::try_from(at as isize + 2 + displacement as isize).ok();
    }
    None
}

/// Whether the bytes are an SNDH file as they are (not packed).
pub fn is_sndh(bytes: &[u8]) -> bool {
    bytes.len() >= 20 && &bytes[12..16] == b"SNDH"
}

/// Whether a file's name says it's SNDH music (.snd alone is too common an ending to go by).
pub fn named_as_sndh(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".sndh")
}

/// Reads an SNDH file, unpacking it first if Pack-Ice or Atomik packed it.
pub fn read_sndh(bytes: &[u8]) -> Result<Sndh, String> {
    let (bytes, packed) = match st_packers::unpack(bytes) {
        Some(Ok(unpacked)) => (std::borrow::Cow::Owned(unpacked.bytes), Some((unpacked.packer, unpacked.packed_size))),
        Some(Err(why)) => return Err(why),
        None => (std::borrow::Cow::Borrowed(bytes), None),
    };
    if !is_sndh(&bytes) {
        return Err(if packed.is_some() { "It unpacks, but what it holds isn't SNDH music: there's no SNDH at byte 12.".into() } else { "This isn't SNDH music: there's no SNDH at byte 12.".into() });
    }
    let mut sndh = Sndh { packed, size: bytes.len(), ..Sndh::default() };
    sndh.routines = [branch_target(&bytes, 0), branch_target(&bytes, 4), branch_target(&bytes, 8)];
    // The tags can't go on past where any of the routines starts.
    let bound = sndh.routines.iter().flatten().copied().filter(|&a| a > 16).fold(bytes.len(), usize::min);
    read_tags(&bytes, bound, &mut sndh);
    if sndh.tunes.is_empty() {
        sndh.tunes.push(Tune::default());
    }
    if sndh.routines.iter().any(Option::is_none) {
        sndh.notes.push("The file doesn't start with three branches to its init, exit and play routines, as SNDH files should.".into());
    }
    Ok(sndh)
}

fn read_tags(bytes: &[u8], bound: usize, sndh: &mut Sndh) {
    let mut at = 16;
    let starts = |at: usize, tag: &str| bytes.get(at..at + tag.len()).is_some_and(|b| b == tag.as_bytes()) && at + tag.len() <= bound;
    // A string up to its zero: the text, and where the next tag starts.
    let string = |at: usize| -> (String, usize) {
        let end = bytes[at.min(bound)..bound].iter().position(|&b| b == 0).map_or(bound, |p| at + p);
        (st_text(&bytes[at.min(end)..end]), (end + 1).min(bound.max(at)))
    };
    let number = |text: &str| text.trim().parse::<u32>().ok();
    let mut tunes_said: Option<usize> = None;
    let mut ended = false;
    while at < bound {
        if starts(at, "HDNS") {
            at += 4;
            ended = true;
            break;
        }
        let field = |tag: &str, at: &mut usize| -> Option<String> {
            if !starts(*at, tag) {
                return None;
            }
            let (text, next) = string(*at + tag.len());
            *at = next;
            Some(text)
        };
        if let Some(t) = field("TITL", &mut at) {
            sndh.title = Some(t);
        } else if let Some(t) = field("COMM", &mut at) {
            sndh.composer = Some(t);
        } else if let Some(t) = field("RIPP", &mut at) {
            sndh.ripper = Some(t);
        } else if let Some(t) = field("CONV", &mut at) {
            sndh.converter = Some(t);
        } else if let Some(t) = field("YEAR", &mut at) {
            sndh.year = Some(t);
        } else if starts(at, "##") {
            // Two digits, then a zero that some files leave out.
            let digits = st_text(&bytes[at + 2..(at + 4).min(bound)]);
            at = (at + 4).min(bound);
            if bytes.get(at) == Some(&0) {
                at += 1;
            }
            match number(&digits) {
                Some(n) => {
                    tunes_said = Some(n as usize);
                    sndh.tunes.resize(n as usize, Tune::default());
                }
                None => sndh.notes.push(format!("The number of tunes, \"{digits}\", isn't a number.")),
            }
        } else if starts(at, "!#SN") || (starts(at, "FLAG") && !starts(at, "FLAG~")) {
            // A table of offsets, one per tune, from the tag to each tune's own string.
            let names = starts(at, "!#SN");
            let count = tunes_said.unwrap_or(1).max(1);
            let table = at + 4;
            if table + count * 2 > bound {
                sndh.notes.push(format!("The {} table runs past the header.", if names { "tune names'" } else { "flags'" }));
                break;
            }
            let mut end = table + count * 2;
            for i in 0..count {
                let offset = usize::from(u16::from_be_bytes([bytes[table + i * 2], bytes[table + i * 2 + 1]]));
                let (text, next) = string(at + offset);
                end = end.max(next);
                if sndh.tunes.len() <= i {
                    sndh.tunes.resize(i + 1, Tune::default());
                }
                if names {
                    sndh.tunes[i].name = Some(text);
                } else {
                    sndh.tunes[i].flags = Some(text);
                }
            }
            at = end;
        } else if let Some(t) = field("FLAG~", &mut at) {
            sndh.flags = Some(t);
        } else if starts(at, "!#") || starts(at, "#!") {
            let (digits, next) = string(at + 2);
            // Only digits belong to it; a file without the zero goes straight on to its next tag.
            let shown: String = digits.chars().take_while(char::is_ascii_digit).collect();
            at = if shown.len() == digits.len() { next } else { at + 2 + shown.len() };
            sndh.default_tune = number(&shown).map(|n| n as usize);
        } else if let Some(timer) = [Timer::A, Timer::B, Timer::C, Timer::D, Timer::Vbl].into_iter().find(|t| starts(at, t.tag())) {
            let (digits, next) = string(at + 2);
            at = next;
            match number(&digits) {
                Some(rate) => sndh.timer = Some((timer, rate)),
                None => sndh.notes.push(format!("{}'s rate, \"{digits}\", isn't a number.", timer.name())),
            }
        } else if starts(at, "TIME") || starts(at, "FRMS") {
            let frames = starts(at, "FRMS");
            let count = tunes_said.unwrap_or(1).max(1);
            at += 4;
            at += at & 1;
            let size = if frames { 4 } else { 2 };
            if at + count * size > bound {
                sndh.notes.push("The tunes' lengths run past the header.".into());
                break;
            }
            for i in 0..count {
                let value = if frames { u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) } else { u32::from(u16::from_be_bytes([bytes[at], bytes[at + 1]])) };
                if sndh.tunes.len() <= i {
                    sndh.tunes.resize(i + 1, Tune::default());
                }
                if frames {
                    sndh.tunes[i].frames = Some(value);
                } else {
                    sndh.tunes[i].seconds = Some(value);
                }
                at += size;
            }
        } else if bytes[at] == 0 {
            at += 1;
        } else {
            let shown = st_text(&bytes[at..(at + 4).min(bound)]);
            sndh.notes.push(format!("The header stops at byte {at}, at a tag Limoniter doesn't know (\"{shown}\")."));
            break;
        }
    }
    if !ended && sndh.notes.is_empty() {
        sndh.notes.push("The header has no HDNS to say where it ends.".into());
    }
    sndh.header_size = at;
}

/// "4:11" for 251 seconds.
fn minutes(seconds: f64) -> String {
    let whole = seconds.round() as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

impl Sndh {
    /// The timer it plays from and its rate, as the file says or the format's default.
    pub fn timer_or_default(&self) -> (Timer, u32) {
        self.timer.unwrap_or((Timer::C, 50))
    }

    /// How long a tune lasts, said plainly, or None if the file doesn't say.
    pub fn tune_length(&self, tune: &Tune) -> Option<String> {
        let (_, rate) = self.timer_or_default();
        match (tune.frames, tune.seconds) {
            (Some(0), _) | (None, Some(0)) => Some("loops for ever".into()),
            (Some(frames), _) if rate > 0 => Some(format!("{} ({} calls at {rate} Hz)", minutes(f64::from(frames) / f64::from(rate)), group(u64::from(frames)))),
            (Some(frames), _) => Some(format!("{} calls", group(u64::from(frames)))),
            (None, Some(seconds)) => Some(minutes(f64::from(seconds))),
            (None, None) => None,
        }
    }

    fn summary(&self) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        let mut put = |label: &str, value: &Option<String>| {
            if let Some(value) = value.as_ref().filter(|v| !v.is_empty()) {
                rows.push((label.to_string(), value.clone()));
            }
        };
        put("Title", &self.title);
        put("Composer", &self.composer);
        put("Ripped by", &self.ripper);
        put("Converted by", &self.converter);
        put("Year", &self.year);
        rows.push(("Tunes".into(), match self.default_tune {
            Some(first) if self.tunes.len() > 1 => format!("{} (tune {first} plays first)", self.tunes.len()),
            _ => self.tunes.len().to_string(),
        }));
        let (timer, rate) = self.timer_or_default();
        let said = if self.timer.is_some() { "" } else { " (the file doesn't say: the SNDH default)" };
        rows.push(("Played from".into(), format!("{} at {rate} Hz{said}", timer.name())));
        if let Some(flags) = &self.flags {
            rows.push(("Needs and uses".into(), format!("{} ({flags})", flags_in_words(flags))));
        }
        if let Some((packer, packed)) = self.packed {
            rows.push(("Packed".into(), format!("with {}: {} bytes, {} unpacked", packer.name(), group(packed), group(self.size as u64))));
        } else {
            rows.push(("Size".into(), format!("{} bytes", group(self.size as u64))));
        }
        rows.push(("Header".into(), format!("{} bytes", self.header_size)));
        rows
    }

    fn tune_rows(&self) -> Vec<[String; 4]> {
        self.tunes
            .iter()
            .enumerate()
            .map(|(i, tune)| {
                let flags = tune.flags.as_deref().map(|f| format!("{} ({f})", flags_in_words(f))).unwrap_or_default();
                [(i + 1).to_string(), tune.name.clone().unwrap_or_default(), self.tune_length(tune).unwrap_or_default(), flags]
            })
            .collect()
    }

    /// What File properties shows.
    pub fn sections(&self) -> Vec<InfoSection> {
        let mut first = self.summary();
        first.extend(self.notes.iter().map(|n| (String::new(), n.clone())));
        let tunes = self.tune_rows().into_iter().map(|[n, name, length, flags]| (format!("{n} {name}").trim().to_string(), [length, flags].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("; "))).collect();
        vec![InfoSection { title: "SNDH music".into(), rows: first }, InfoSection { title: "Tunes".into(), rows: tunes }]
    }

    /// The preview.
    pub fn blocks(&self) -> Vec<Block> {
        let mut blocks = vec![heading(self.title.clone().filter(|t| !t.is_empty()).unwrap_or_else(|| "Untitled SNDH music".into()))];
        for (label, value) in self.summary().into_iter().skip(usize::from(self.title.as_ref().is_some_and(|t| !t.is_empty()))) {
            blocks.push(pair(&label, value));
        }
        for note in &self.notes {
            blocks.push(note_block(note));
        }
        blocks.push(subheading("Tunes"));
        let mut rows = vec![["#", "Name", "Length", "Flags"].map(|h| vec![strong(h.into())]).to_vec()];
        rows.extend(self.tune_rows().into_iter().map(|cells| cells.map(|c| vec![plain(c)]).to_vec()));
        blocks.push(Block { kind: BlockKind::Table(rows), spans: Vec::new(), indent: 0, quote: 0, marker: None });
        blocks
    }
}

/// A YM file: the YM2149's registers frame by frame, from Leonard's ST-Sound.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ym {
    /// YM2!, YM3!, YM3b, YM5! or YM6!.
    pub format: String,
    pub name: String,
    pub author: String,
    pub comment: String,
    pub frames: u32,
    pub rate: u32,
    pub loop_frame: u32,
    pub attributes: u32,
    pub clock: u32,
    /// Each digidrum's size in bytes.
    pub drums: Vec<u32>,
    /// How many registers each frame has (14 or 16), and the bytes they take.
    pub registers: usize,
    pub stream_bytes: u64,
    /// The LHA method it was packed with, if it was.
    pub packed: Option<String>,
    pub size: u64,
    pub notes: Vec<String>,
}

/// Whether the bytes are a YM file, as they are or packed with LHA as they usually are.
pub fn is_ym(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && matches!(&bytes[..4], b"YM2!" | b"YM3!" | b"YM3b" | b"YM4!" | b"YM5!" | b"YM6!" | b"MIX1" | b"YMT1" | b"YMT2")
}

fn lha_method(bytes: &[u8]) -> Option<String> {
    (bytes.len() > 21 && &bytes[2..5] == b"-lh" && bytes[6] == b'-').then(|| String::from_utf8_lossy(&bytes[2..7]).into_owned())
}

/// The first file in an LHA archive.
fn from_lha(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let volume = crate::media::open(std::sync::Arc::new(bytes.to_vec()), "music.lha")?;
    let first = volume.list(None)?.into_iter().find(|e| !e.is_dir).ok_or(crate::i18n::tr("The LHA archive is empty."))?;
    volume.read(&first)
}

/// Whether a file's name says it's a YM file.
pub fn named_as_ym(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".ym")
}

/// Reads a YM file, unpacking it from its LHA archive first if need be.
pub fn read_ym(bytes: &[u8]) -> Result<Ym, String> {
    let packed = lha_method(bytes);
    let unpacked;
    let bytes = if packed.is_some() {
        unpacked = from_lha(bytes)?;
        unpacked.as_slice()
    } else {
        bytes
    };
    if !is_ym(bytes) {
        return Err(crate::i18n::tr("This isn't a YM file: it doesn't start with YM2!, YM3!, YM3b, YM5! or YM6!.").into());
    }
    let format = String::from_utf8_lossy(&bytes[..4]).into_owned();
    let mut ym = Ym { format: format.clone(), packed, size: bytes.len() as u64, rate: 50, clock: 2_000_000, ..Ym::default() };
    match format.as_str() {
        "YM2!" | "YM3!" | "YM3b" => {
            let tail = if format == "YM3b" { 4 } else { 0 };
            if bytes.len() < 4 + tail {
                return Err(crate::i18n::tr("The YM file is too short.").into());
            }
            let body = (bytes.len() - 4 - tail) as u64;
            ym.registers = 14;
            ym.frames = (body / 14) as u32;
            ym.stream_bytes = body;
            ym.attributes = 1;
            if tail == 4 {
                let at = bytes.len() - 4;
                // ST-Sound reads YM3b's loop frame lowest byte first.
                ym.loop_frame = u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
            }
            if !body.is_multiple_of(14) {
                ym.notes.push(format!("The registers take {body} bytes, which isn't a whole number of 14-register frames."));
            }
            if format == "YM2!" {
                ym.notes.push("YM2! is Mad Max's: its digidrums are the ones his replay has built in.".into());
            }
        }
        "YM5!" | "YM6!" => read_ym5(bytes, &mut ym)?,
        "YMT1" | "YMT2" | "MIX1" => read_other(bytes, &mut ym)?,
        other => {
            ym.notes.push(format!("{other} files are an early kind ST-Sound no longer plays; Limoniter only names it."));
        }
    }
    Ok(ym)
}

/// YM-Tracker songs (YMT1, YMT2) and remixed digital sound (MIX1): their names and lengths, but no registers.
fn read_other(bytes: &[u8], ym: &mut Ym) -> Result<(), String> {
    let short = || "The YM file stops in the middle of its header.".to_string();
    if bytes.len() < 32 || &bytes[4..12] != b"LeOnArD!" {
        return Err(crate::i18n::tr("The YM file's header is damaged: there's no LeOnArD! after its mark.").into());
    }
    let long = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(short);
    let word = |at: usize| bytes.get(at..at + 2).map(|b| u32::from(u16::from_be_bytes([b[0], b[1]]))).ok_or_else(short);
    let mut at;
    if ym.format == "MIX1" {
        ym.attributes = long(12)? & 1;
        let samples = long(16)?;
        let blocks = long(20)? as usize;
        at = 24 + blocks.checked_mul(12).ok_or_else(short)?;
        ym.notes.push(format!("MIX1 is digital sound remixed from samples: {} blocks over {} bytes of samples, with no YM2149 registers.", blocks, group(u64::from(samples))));
        ym.rate = 0;
    } else {
        let voices = word(12)?;
        ym.rate = word(14)?;
        ym.frames = long(16)?;
        ym.loop_frame = long(20)?;
        ym.drums = vec![0; word(24)? as usize];
        ym.attributes = long(26)?;
        at = 30;
        ym.notes.push(format!("{} is a YM-Tracker song for {voices} voices: its patterns are played by ST-Sound's tracker, not kept as registers.", ym.format));
    }
    let mut text = || -> Result<String, String> {
        let end = bytes.get(at..).and_then(|rest| rest.iter().position(|&b| b == 0)).ok_or_else(short)?;
        let s = st_text(&bytes[at..at + end]);
        at += end + 1;
        Ok(s)
    };
    ym.name = text()?;
    ym.author = text()?;
    ym.comment = text()?;
    Ok(())
}

fn read_ym5(bytes: &[u8], ym: &mut Ym) -> Result<(), String> {
    if bytes.len() < 34 || &bytes[4..12] != b"LeOnArD!" {
        return Err(crate::i18n::tr("The YM file's header is damaged: there's no LeOnArD! after its mark.").into());
    }
    let long = |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let word = |at: usize| u32::from(u16::from_be_bytes([bytes[at], bytes[at + 1]]));
    ym.frames = long(12);
    ym.attributes = long(16);
    let drums = word(20) as usize;
    ym.clock = long(22);
    ym.rate = word(26);
    ym.loop_frame = long(28);
    let extra = word(32) as usize;
    let mut at = 34 + extra;
    let short = || "The YM file stops in the middle of its header.".to_string();
    for _ in 0..drums {
        let size = bytes.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(short)?;
        ym.drums.push(size);
        at = at.checked_add(4 + size as usize).filter(|&a| a <= bytes.len()).ok_or_else(short)?;
    }
    let mut text = || -> Result<String, String> {
        let end = bytes[at.min(bytes.len())..].iter().position(|&b| b == 0).ok_or_else(short)?;
        let s = st_text(&bytes[at..at + end]);
        at += end + 1;
        Ok(s)
    };
    ym.name = text()?;
    ym.author = text()?;
    ym.comment = text()?;
    ym.registers = 16;
    let left = (bytes.len() - at) as u64;
    let ends = bytes.ends_with(b"End!");
    ym.stream_bytes = u64::from(ym.frames) * 16;
    let wanted = ym.stream_bytes + if ends { 4 } else { 0 };
    if !ends {
        ym.notes.push("There's no End! after the registers.".into());
    }
    if left != wanted {
        ym.notes.push(format!("The registers should take {} bytes for {} frames, but {} are there.", group(ym.stream_bytes), group(u64::from(ym.frames)), group(left.saturating_sub(if ends { 4 } else { 0 }))));
    }
    Ok(())
}

impl Ym {
    pub fn interleaved(&self) -> bool {
        self.attributes & 1 != 0
    }

    fn attributes_in_words(&self) -> String {
        let mut words = vec![if self.interleaved() { "registers interleaved (each register's stream in turn)" } else { "registers frame by frame" }];
        if self.attributes & 2 != 0 {
            words.push("signed digidrums");
        }
        if self.attributes & 4 != 0 {
            words.push("4-bit digidrums, as the ST plays them");
        }
        if self.attributes & 8 != 0 {
            words.push("time control");
        }
        if self.attributes & 16 != 0 {
            words.push("loop mode");
        }
        format!("{} (${:X})", words.join(", "), self.attributes)
    }

    fn summary(&self) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        for (label, value) in [("Song", &self.name), ("Author", &self.author), ("Comment", &self.comment)] {
            if !value.is_empty() {
                rows.push((label.to_string(), value.clone()));
            }
        }
        let seconds = if self.rate > 0 { f64::from(self.frames) / f64::from(self.rate) } else { 0.0 };
        rows.push(("Format".into(), if self.registers > 0 { format!("{} ({} registers a frame)", self.format, self.registers) } else { self.format.clone() }));
        if self.rate > 0 {
            rows.push(("Frames".into(), format!("{} at {} Hz, {}", group(u64::from(self.frames)), self.rate, minutes(seconds))));
            rows.push(("Loops to".into(), if self.loop_frame == 0 { "the start".into() } else { format!("frame {}", group(u64::from(self.loop_frame))) }));
        }
        if self.registers == 0 {
            match &self.packed {
                Some(method) => rows.push(("Packed".into(), format!("with LHA ({method}), {} bytes unpacked", group(self.size)))),
                None => rows.push(("Size".into(), format!("{} bytes", group(self.size)))),
            }
            return rows;
        }
        rows.push(("Chip clock".into(), format!("{} Hz{}", group(u64::from(self.clock)), if self.clock == 2_000_000 { " (the Atari ST's)" } else if self.clock == 1_773_400 { " (the ZX Spectrum's)" } else if self.clock == 1_000_000 { " (the Amstrad CPC's)" } else { "" })));
        rows.push(("Attributes".into(), self.attributes_in_words()));
        if !self.drums.is_empty() {
            let total: u64 = self.drums.iter().map(|&d| u64::from(d)).sum();
            rows.push(("Digidrums".into(), format!("{}, {} bytes ({})", self.drums.len(), group(total), self.drums.iter().map(|d| group(u64::from(*d))).collect::<Vec<_>>().join(", "))));
        }
        rows.push(("Register streams".into(), format!("{} bytes", group(self.stream_bytes))));
        match &self.packed {
            Some(method) => rows.push(("Packed".into(), format!("with LHA ({method}), {} bytes unpacked", group(self.size)))),
            None => rows.push(("Size".into(), format!("{} bytes", group(self.size)))),
        }
        rows
    }

    pub fn sections(&self) -> Vec<InfoSection> {
        let mut rows = self.summary();
        rows.extend(self.notes.iter().map(|n| (String::new(), n.clone())));
        vec![InfoSection { title: "YM music".into(), rows }]
    }

    pub fn blocks(&self) -> Vec<Block> {
        let mut blocks = vec![heading(if self.name.is_empty() { "Untitled YM music".into() } else { self.name.clone() })];
        for (label, value) in self.summary().into_iter().skip(usize::from(!self.name.is_empty())) {
            blocks.push(pair(&label, value));
        }
        for note in &self.notes {
            blocks.push(note_block(note));
        }
        blocks
    }
}

fn plain(text: String) -> Span {
    Span { text, ..Span::default() }
}

fn strong(text: String) -> Span {
    Span { text, strong: true, ..Span::default() }
}

fn heading(text: String) -> Block {
    Block { kind: BlockKind::Heading(2), spans: vec![plain(text)], indent: 0, quote: 0, marker: None }
}

fn subheading(text: &str) -> Block {
    Block { kind: BlockKind::Heading(3), spans: vec![plain(text.into())], indent: 0, quote: 0, marker: None }
}

fn pair(label: &str, value: String) -> Block {
    Block { kind: BlockKind::Paragraph, spans: vec![strong(format!("{label}: ")), plain(value)], indent: 0, quote: 0, marker: None }
}

fn note_block(text: &str) -> Block {
    Block { kind: BlockKind::Paragraph, spans: vec![Span { text: text.into(), italic: true, ..Span::default() }], indent: 0, quote: 0, marker: None }
}

pub use crate::amiga_tools::group;

/// ST music of either kind, read from a file's bytes.
pub enum Music {
    Sndh(Sndh),
    Ym(Ym),
}

impl Music {
    pub fn sections(&self) -> Vec<InfoSection> {
        match self {
            Music::Sndh(s) => s.sections(),
            Music::Ym(y) => y.sections(),
        }
    }

    pub fn blocks(&self) -> Vec<Block> {
        match self {
            Music::Sndh(s) => s.blocks(),
            Music::Ym(y) => y.blocks(),
        }
    }
}

/// Whether the start of a file says it's ST music (with what it holds unpacked if need be), or its name does.
pub fn recognised(head: &[u8], name: &str) -> bool {
    if named_as_sndh(name) || named_as_ym(name) {
        return true;
    }
    if is_sndh(head) || is_ym(head) {
        return true;
    }
    // SNDH music was often kept packed as .snd on ST disks.
    if st_packers::packer(head).is_some() && name.to_ascii_lowercase().ends_with(".snd") {
        return true;
    }
    // A packed file: its first bytes, unpacked, can't be had from the start alone, so the name decides, except
    // for an LHA archive holding a single .ym file, whose name is in its header.
    if lha_method(head).is_some() {
        let name_length = usize::from(head[21]);
        return head.get(22..22 + name_length).is_some_and(|n| named_as_ym(&String::from_utf8_lossy(n)));
    }
    false
}

/// Reads ST music, packed or not.
pub fn read(bytes: &[u8], name: &str) -> Result<Music, String> {
    if lha_method(bytes).is_some() || is_ym(bytes) {
        return read_ym(bytes).map(Music::Ym);
    }
    let contents = st_packers::contents(bytes);
    if is_sndh(&contents) {
        return read_sndh(bytes).map(Music::Sndh);
    }
    if is_ym(&contents) {
        return read_ym(&contents).map(Music::Ym);
    }
    if let Some(Err(why)) = st_packers::unpack(bytes) {
        return Err(why);
    }
    Err(if named_as_ym(name) { "This isn't a YM file Limoniter can read.".into() } else { "This isn't SNDH or YM music.".into() })
}

/// The MFP's prescaler that gives a timer closest to `rate` Hz: its control value, divisor and data register.
pub fn mfp_timer(rate: u32) -> (u8, u32, u8) {
    const PRESCALERS: [(u8, u32); 7] = [(1, 4), (2, 10), (3, 16), (4, 50), (5, 64), (6, 100), (7, 200)];
    let mut best = (7, 200, 255, f64::MAX);
    for (control, divisor) in PRESCALERS {
        let data = (2_457_600.0 / (f64::from(divisor) * f64::from(rate.max(1)))).round();
        if (1.0..=256.0).contains(&data) {
            let error = (2_457_600.0 / (f64::from(divisor) * data) - f64::from(rate)).abs();
            if error < best.3 - 1e-9 {
                best = (control, divisor, data as u32, error);
            }
        }
    }
    (best.0, best.1, if best.2 == 256 { 0 } else { best.2 as u8 })
}

/// The 68000 replay calls for an SNDH file: init with the tune in d0, play from the timer it asks for, and exit.
/// Written here for any SNDH file; the file itself is included as it is (unpacked).
pub fn replay_snippet(sndh: &Sndh, file_name: &str, tune: usize, timer: (Timer, u32)) -> String {
    let (timer, rate) = timer;
    let mut s = String::new();
    let mut line = |text: &str| {
        s.push_str(text);
        s.push('\n');
    };
    let title = sndh.title.clone().filter(|t| !t.is_empty()).unwrap_or_else(|| file_name.to_string());
    let by = sndh.composer.as_deref().filter(|c| !c.is_empty()).map(|c| format!(" by {c}")).unwrap_or_default();
    line(&format!("; --- SNDH: {title}{by}, tune {tune}, played from {} at {rate} Hz ---", timer.name()));
    line("; In supervisor mode. The SNDH file starts with three branches: +0 init (the tune in d0), +4 exit,");
    line("; +8 play. Its code is position independent, so it runs where it's included, at an even address.");
    if sndh.packed.is_some() {
        line(&format!("; {file_name} is packed with {}: save what it holds (Limoniter opens it unpacked) and include", sndh.packed.map_or("", |p| p.0.name())));
        line("; that, or depack it at run time with the packer's own depacker first.");
    }
    if let Some(flags) = &sndh.flags {
        line(&format!("; Its flags say: {}.", flags_in_words(flags)));
    }
    let used: Vec<char> = sndh.flags.iter().chain(sndh.tunes.iter().filter_map(|t| t.flags.as_ref())).flat_map(|f| f.chars()).collect();
    let letter = match timer {
        Timer::A => Some('a'),
        Timer::B => Some('b'),
        Timer::C => Some('c'),
        Timer::D => Some('d'),
        Timer::Vbl => None,
    };
    if letter.is_some_and(|l| used.contains(&l)) && sndh.timer.is_none_or(|(t, _)| t != timer) {
        line(&format!("; Careful: the music itself uses {}, so play it from another.", timer.name()));
    }
    line("");
    // Each way: the vector, and how the interrupt is turned on and off.
    let (vector, enable, mask_register, bit, data_register, control_register) = match timer {
        Timer::A => ("$134", "$fffffa07", "$fffffa13", 5, "$fffffa1f", "$fffffa19"),
        Timer::B => ("$120", "$fffffa07", "$fffffa13", 0, "$fffffa21", "$fffffa1b"),
        Timer::D => ("$110", "$fffffa09", "$fffffa15", 4, "$fffffa25", "$fffffa1d"),
        Timer::C => ("$114", "", "", 5, "", ""),
        Timer::Vbl => ("$70", "", "", 0, "", ""),
    };
    line("music_init:");
    line(&format!("\t\tmoveq\t#{tune},d0\t\t; the tune, from 1"));
    line("\t\tbsr\tsndh_music\t\t; init");
    line("\t\tmove.w\tsr,-(sp)");
    line("\t\tmove.w\t#$2700,sr\t\t; no interrupts while the vector changes");
    line(&format!("\t\tmove.l\t{vector}.w,old_vector"));
    match timer {
        Timer::A | Timer::B | Timer::D => {
            let (control, divisor, data) = mfp_timer(rate);
            let actual = 2_457_600.0 / (f64::from(divisor) * if data == 0 { 256.0 } else { f64::from(data) });
            if timer == Timer::D {
                line("\t\tand.b\t#$f0,$fffffa1d\t; Timer D stopped (Timer C, in the high bits, kept)");
            } else {
                line(&format!("\t\tclr.b\t{control_register}\t\t; {} stopped", timer.name()));
            }
            line(&format!("\t\tmove.l\t#music_timer,{vector}.w"));
            line(&format!("\t\tmove.b\t#{data},{data_register}\t; 2457600 / {divisor} / {data} = {actual:.2} Hz"));
            if timer == Timer::D {
                line(&format!("\t\tor.b\t#{control},$fffffa1d\t\t; started, dividing by {divisor}"));
            } else {
                line(&format!("\t\tmove.b\t#{control},{control_register}\t\t; started, dividing by {divisor}"));
            }
            line(&format!("\t\tbset\t#{bit},{enable}\t\t; enabled"));
            line(&format!("\t\tbset\t#{bit},{mask_register}\t\t; and not masked"));
            line("\t\tbclr\t#3,$fffffa17\t\t; the MFP ends each interrupt by itself");
        }
        Timer::C => {
            line("\t\tmove.w\t#200,timer_count\t; TOS's Timer C runs at 200 Hz: play every 200/rate ticks");
            line(&format!("\t\tmove.l\t#music_timer,{vector}.w"));
        }
        Timer::Vbl => {
            line(&format!("\t\tmove.l\t#music_timer,{vector}.w\t\t; once a frame: {rate} Hz on a {rate} Hz screen"));
        }
    }
    line("\t\tmove.w\t(sp)+,sr");
    line("\t\trts");
    line("");
    line("music_exit:");
    line("\t\tmove.w\tsr,-(sp)");
    line("\t\tmove.w\t#$2700,sr");
    match timer {
        Timer::A | Timer::B => {
            line(&format!("\t\tclr.b\t{control_register}\t\t; {} stopped", timer.name()));
            line(&format!("\t\tbclr\t#{bit},{enable}"));
            line(&format!("\t\tbclr\t#{bit},{mask_register}"));
        }
        Timer::D => {
            line("\t\tand.b\t#$f0,$fffffa1d\t; Timer D stopped");
            line(&format!("\t\tbclr\t#{bit},{enable}"));
            line(&format!("\t\tbclr\t#{bit},{mask_register}"));
        }
        Timer::C | Timer::Vbl => {}
    }
    line(&format!("\t\tmove.l\told_vector,{vector}.w"));
    line("\t\tmove.w\t(sp)+,sr");
    line("\t\tbra\tsndh_music+4\t\t; exit: the music silences the YM2149");
    line("");
    line("music_timer:");
    match timer {
        Timer::C => {
            line(&format!("\t\tsub.w\t#{rate},timer_count"));
            line("\t\tbgt.s\t.not_yet");
            line("\t\tadd.w\t#200,timer_count");
            line("\t\tmovem.l\td0-a6,-(sp)");
            line("\t\tbsr\tsndh_music+8\t\t; play");
            line("\t\tmovem.l\t(sp)+,d0-a6");
            line(".not_yet:\tmove.l\told_vector,-(sp)\t; then on to TOS's own Timer C");
            line("\t\trts");
        }
        Timer::Vbl => {
            line("\t\tmovem.l\td0-a6,-(sp)");
            line("\t\tbsr\tsndh_music+8\t\t; play");
            line("\t\tmovem.l\t(sp)+,d0-a6");
            line("\t\tmove.l\told_vector,-(sp)\t; then on to the VBL as it was");
            line("\t\trts");
        }
        _ => {
            line("\t\tmovem.l\td0-a6,-(sp)");
            line("\t\tbsr\tsndh_music+8\t\t; play");
            line("\t\tmovem.l\t(sp)+,d0-a6");
            line("\t\trte");
        }
    }
    line("");
    line("old_vector:\tdc.l\t0");
    if timer == Timer::C {
        line("timer_count:\tdc.w\t0");
    }
    line("\t\teven");
    line(&format!("sndh_music:\tincbin\t\"{file_name}\""));
    line("\t\teven");
    s
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// An SNDH file made up here: three branches, the header's tags, HDNS, and routines that only return.
    pub fn synthetic(tags: &[u8]) -> Vec<u8> {
        let mut header = b"SNDH".to_vec();
        header.extend_from_slice(tags);
        if header.len() % 2 == 1 {
            header.push(0);
        }
        header.extend_from_slice(b"HDNS");
        let code = 12 + header.len();
        let mut bytes = Vec::new();
        for i in 0..3u16 {
            bytes.extend([0x60, 0x00]);
            bytes.extend((code as u16 - (i * 4 + 2)).to_be_bytes());
        }
        bytes.extend(header);
        bytes.extend([0x4E, 0x75]);
        bytes
    }

    #[test]
    fn an_sndh_header_says_its_title_composer_tunes_and_timer() {
        let mut tags = Vec::new();
        tags.extend(b"TITL\x8eberlauf\0COMM5 x Mad Max\0RIPPGrazey\0CONVsomeone\0YEAR1990\0");
        tags.extend(b"##03TB120\0!#02");
        tags.extend(b"FLAG~bey\0");
        // Names: offsets from the tag.
        let at = tags.len() + 4; // where the !#SN tag will be, after "SNDH"
        let _ = at;
        let mut names = b"!#SN".to_vec();
        let strings = [&b"Intro\0"[..], b"Main\0", b"End\0"];
        let mut offset = 4 + 6;
        for s in strings {
            names.extend((offset as u16).to_be_bytes());
            offset += s.len();
        }
        for s in strings {
            names.extend(s);
        }
        tags.extend(&names);
        tags.extend(b"FRMS");
        if (16 + tags.len()) % 2 == 1 {
            tags.push(0);
        }
        for frames in [6000u32, 0, 1200] {
            tags.extend(frames.to_be_bytes());
        }
        let bytes = synthetic(&tags);
        let sndh = read_sndh(&bytes).unwrap();
        assert_eq!(sndh.title.as_deref(), Some("\u{c4}berlauf"), "the ST's own character set");
        assert_eq!(sndh.composer.as_deref(), Some("5 x Mad Max"));
        assert_eq!((sndh.ripper.as_deref(), sndh.converter.as_deref(), sndh.year.as_deref()), (Some("Grazey"), Some("someone"), Some("1990")));
        assert_eq!(sndh.tunes.len(), 3);
        assert_eq!(sndh.default_tune, Some(2));
        assert_eq!(sndh.timer, Some((Timer::B, 120)));
        assert_eq!(sndh.flags.as_deref(), Some("bey"));
        assert_eq!(sndh.tunes.iter().map(|t| t.name.clone().unwrap()).collect::<Vec<_>>(), ["Intro", "Main", "End"]);
        assert_eq!(sndh.tunes.iter().map(|t| t.frames.unwrap()).collect::<Vec<_>>(), [6000, 0, 1200]);
        assert!(sndh.notes.is_empty(), "{:?}", sndh.notes);
        assert_eq!(sndh.routines[0], Some(sndh.header_size), "init starts right after the header");
        assert_eq!(sndh.tune_length(&sndh.tunes[0]).unwrap(), "0:50 (6,000 calls at 120 Hz)");
        assert_eq!(sndh.tune_length(&sndh.tunes[1]).unwrap(), "loops for ever");
        assert_eq!(flags_in_words("bey"), "uses Timer B, needs an STE, plays on the YM2149");
        let sections = sndh.sections();
        assert!(sections[0].rows.iter().any(|(k, v)| k == "Played from" && v == "Timer B at 120 Hz"));
        assert_eq!(sections[1].rows[0], ("1 Intro".to_string(), "0:50 (6,000 calls at 120 Hz)".to_string()));
        let blocks = sndh.blocks();
        assert_eq!(blocks[0].spans[0].text, "\u{c4}berlauf");
    }

    #[test]
    fn older_headers_with_times_flags_per_tune_and_no_end_are_read() {
        let mut tags = b"COMMJochen Hippel\0##02TC200\0".to_vec();
        tags.extend(b"FLAG");
        tags.extend(8u16.to_be_bytes());
        tags.extend(11u16.to_be_bytes());
        tags.extend(b"ay\0dy\0");
        tags.extend(b"TIME");
        if (16 + tags.len()) % 2 == 1 {
            tags.push(0);
        }
        tags.extend(95u16.to_be_bytes());
        tags.extend(0u16.to_be_bytes());
        let sndh = read_sndh(&synthetic(&tags)).unwrap();
        assert_eq!(sndh.title, None);
        assert_eq!(sndh.tunes.iter().map(|t| (t.flags.clone().unwrap(), t.seconds.unwrap())).collect::<Vec<_>>(), [("ay".to_string(), 95), ("dy".to_string(), 0)]);
        assert_eq!(sndh.tune_length(&sndh.tunes[0]).unwrap(), "1:35");
        assert!(sndh.notes.is_empty(), "{:?}", sndh.notes);
        // A file with no timer tag plays from Timer C at 50 Hz, the format's default.
        let plain = read_sndh(&synthetic(b"TITLx\0")).unwrap();
        assert_eq!(plain.timer_or_default(), (Timer::C, 50));
        assert_eq!(plain.tunes.len(), 1);
        // An unknown tag ends the header, with a note.
        let odd = read_sndh(&synthetic(b"TITLx\0ZZZZ")).unwrap();
        assert!(odd.notes[0].contains("ZZZZ"), "{:?}", odd.notes);
        assert!(read_sndh(b"not music at all, not at all").is_err());
    }

    #[test]
    fn packed_sndh_is_unpacked_first() {
        let bytes = synthetic(b"TITLPacked tune\0##01TC50\0");
        let packed = crate::st_packers::tests::ice_pack(&bytes, 1);
        assert!(!is_sndh(&packed));
        let sndh = read_sndh(&packed).unwrap();
        assert_eq!(sndh.title.as_deref(), Some("Packed tune"));
        assert_eq!(sndh.packed.map(|p| p.0), Some(Packer::Ice24));
        assert!(sndh.summary().iter().any(|(k, v)| k == "Packed" && v.starts_with("with Pack-Ice 2.4")));
        assert!(matches!(read(&packed, "tune.sndh"), Ok(Music::Sndh(_))));
        assert!(recognised(&packed[..packed.len().min(64)], "tune.sndh"));
    }

    fn ym5(name: &str, frames: u32, drums: &[u32], format: &[u8; 4]) -> Vec<u8> {
        let mut b = format.to_vec();
        b.extend(b"LeOnArD!");
        b.extend(frames.to_be_bytes());
        b.extend(1u32.to_be_bytes());
        b.extend((drums.len() as u16).to_be_bytes());
        b.extend(2_000_000u32.to_be_bytes());
        b.extend(50u16.to_be_bytes());
        b.extend(25u32.to_be_bytes());
        b.extend(0u16.to_be_bytes());
        for d in drums {
            b.extend(d.to_be_bytes());
            b.extend(std::iter::repeat_n(0x80, *d as usize));
        }
        b.extend(name.as_bytes());
        b.push(0);
        b.extend(b"Someone\0A comment\0");
        b.extend(std::iter::repeat_n(0, frames as usize * 16));
        b.extend(b"End!");
        b
    }

    #[test]
    fn a_ym_file_says_its_name_frames_and_streams() {
        let ym = read_ym(&ym5("Tune", 500, &[100, 40], b"YM6!")).unwrap();
        assert_eq!((ym.name.as_str(), ym.author.as_str(), ym.comment.as_str()), ("Tune", "Someone", "A comment"));
        assert_eq!((ym.frames, ym.rate, ym.loop_frame, ym.clock, ym.registers), (500, 50, 25, 2_000_000, 16));
        assert_eq!(ym.drums, [100, 40]);
        assert_eq!(ym.stream_bytes, 8000);
        assert!(ym.interleaved());
        assert!(ym.notes.is_empty(), "{:?}", ym.notes);
        let rows = ym.sections().remove(0).rows;
        assert!(rows.iter().any(|(k, v)| k == "Frames" && v == "500 at 50 Hz, 0:10"), "{rows:?}");
        assert!(rows.iter().any(|(k, v)| k == "Digidrums" && v.starts_with("2, 140 bytes")), "{rows:?}");
        // YM3b: 14 registers, with its loop at the end lowest byte first.
        let mut ym3 = b"YM3b".to_vec();
        ym3.extend(vec![0u8; 14 * 100]);
        ym3.extend(7u32.to_le_bytes());
        let ym = read_ym(&ym3).unwrap();
        assert_eq!((ym.frames, ym.loop_frame, ym.registers, ym.stream_bytes), (100, 7, 14, 1400));
        // A cut-short file says so.
        let mut cut = ym5("Tune", 500, &[], b"YM5!");
        cut.truncate(cut.len() - 100);
        assert!(!read_ym(&cut).unwrap().notes.is_empty());
        assert!(read_ym(b"YM5!LeOnArD").is_err());
    }

    #[test]
    fn mfp_timers_are_set_for_the_rate_asked() {
        // 50 Hz: 2457600 / 200 / 246 = 49.95 Hz.
        assert_eq!(mfp_timer(50), (7, 200, 246));
        let (_, divisor, data) = mfp_timer(200);
        assert!((2_457_600.0 / f64::from(divisor * u32::from(data)) - 200.0).abs() < 0.5);
        let (_, divisor, data) = mfp_timer(1000);
        assert!((2_457_600.0 / f64::from(divisor * u32::from(data)) - 1000.0).abs() < 5.0);
    }

    #[test]
    fn replay_calls_assemble_for_every_timer() {
        let dir = std::env::temp_dir().join(format!("limoniter-sndh-snippet-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bytes = synthetic(b"TITLTest\0COMMMe\0##02TA200\0FLAG~ay\0");
        std::fs::write(dir.join("tune.sndh"), &bytes).unwrap();
        let sndh = read_sndh(&bytes).unwrap();
        for timer in [(Timer::A, 200), (Timer::B, 60), (Timer::C, 50), (Timer::D, 100), (Timer::Vbl, 50)] {
            let snippet = replay_snippet(&sndh, "tune.sndh", 2, timer);
            assert!(snippet.contains("moveq\t#2,d0") && snippet.contains("bsr\tsndh_music+8") && snippet.contains("bra\tsndh_music+4"), "{snippet}");
            let source = format!("start:\t\tbsr\tmusic_init\n\t\tbsr\tmusic_exit\n\t\trts\n{snippet}");
            let path = dir.join("main.s");
            std::fs::write(&path, &source).unwrap();
            let assembled = crate::assemblers::assemble_own(crate::assemblers::Cpu::M68k, &path, &source);
            assert!(assembled.assembly.problems.is_empty(), "{:?}: {:#?}\n{source}", timer.0, assembled.assembly.problems);
        }
        // Played from a timer the music itself uses, it says to be careful.
        assert!(!replay_snippet(&sndh, "tune.sndh", 1, (Timer::A, 50)).contains("Careful"), "the file asks for Timer A itself");
        let other = read_sndh(&synthetic(b"TC50\0FLAG~ay\0")).unwrap();
        assert!(replay_snippet(&other, "x.sndh", 1, (Timer::A, 50)).contains("Careful"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Real SNDH files, from a folder named by LIMONITER_TEST_SNDH, with PSG play's -i output for each in the
    /// folder named by LIMONITER_TEST_SNDH_INFO (as name.sndh.info at the same place): every field it shows must
    /// read the same here. The files aren't Limoniter's to include.
    #[test]
    fn real_sndh_files_read_as_psg_play_reads_them() {
        let (Some(folder), Some(info)) = (std::env::var_os("LIMONITER_TEST_SNDH"), std::env::var_os("LIMONITER_TEST_SNDH_INFO")) else {
            eprintln!("LIMONITER_TEST_SNDH and LIMONITER_TEST_SNDH_INFO aren't set, so no real SNDH files were read.");
            return;
        };
        let folder = std::path::PathBuf::from(folder);
        let mut files = Vec::new();
        let mut stack = vec![folder.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("sndh")) {
                    files.push(path);
                }
            }
        }
        files.sort();
        let (mut same, mut differences, mut fields) = (0, Vec::new(), 0usize);
        for path in &files {
            let relative = path.strip_prefix(&folder).unwrap();
            let mut info_path = std::path::Path::new(&info).join(relative).into_os_string();
            info_path.push(".info");
            let Ok(theirs) = std::fs::read_to_string(&info_path) else {
                continue;
            };
            let bytes = std::fs::read(path).unwrap();
            let sndh = read_sndh(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let mut ours: Vec<String> = Vec::new();
            let mut want: Vec<String> = Vec::new();
            for line in theirs.lines() {
                let Some(rest) = line.strip_prefix("tag field ") else {
                    if let Some(size) = line.strip_prefix("header size ") {
                        want.push(format!("header {size}"));
                        ours.push(format!("header {}", sndh.header_size));
                    }
                    continue;
                };
                let (tag, value) = rest.split_once(' ').unwrap_or((rest, ""));
                let value = value.trim();
                let n = |v: &str| v.split_whitespace().next().and_then(|n| n.parse::<usize>().ok()).unwrap_or(0);
                let ours_for = match tag {
                    "TITL" => sndh.title.clone(),
                    "COMM" => sndh.composer.clone(),
                    "RIPP" => sndh.ripper.clone(),
                    "CONV" => sndh.converter.clone(),
                    "YEAR" => sndh.year.clone(),
                    "##" => Some(sndh.tunes.len().to_string()),
                    "!#" => sndh.default_tune.map(|t| t.to_string()),
                    "TA" | "TB" | "TC" | "TD" | "!V" => sndh.timer.filter(|(t, _)| t.tag() == tag).map(|(_, r)| r.to_string()),
                    "FRMS" => sndh.tunes.get(n(value).wrapping_sub(1)).and_then(|t| t.frames).map(|f| format!("{} {f}", n(value))),
                    "TIME" => sndh.tunes.get(n(value).wrapping_sub(1)).and_then(|t| t.seconds).map(|f| format!("{} {f}", n(value))),
                    "!#SN" => sndh.tunes.get(n(value).wrapping_sub(1)).and_then(|t| t.name.clone()).map(|name| format!("{} {name}", n(value))),
                    "FLAG" if value.starts_with('~') => sndh.flags.clone().map(|f| format!("~ {f}")),
                    "FLAG" => sndh.tunes.get(n(value).wrapping_sub(1)).and_then(|t| t.flags.clone()).map(|f| format!("{} {f}", n(value))),
                    _ => continue,
                };
                // PSG play shows the value, then for some tags what it means: only the value is compared.
                let theirs_value = match tag {
                    "FRMS" | "TIME" => value.split_whitespace().take(2).collect::<Vec<_>>().join(" "),
                    "FLAG" => {
                        let mut words = value.split_whitespace();
                        let first = words.next().unwrap_or("");
                        format!("{first} {}", words.next().filter(|w| w.chars().all(|c| flag_meaning(c).is_some() || c == '~') && !w.contains('_')).unwrap_or(""))
                    }
                    // PSG play quotes a title that could be taken for a number.
                    "TITL" if ours_for.as_ref().is_some_and(|t| format!("\"{t}\"") == value) => ours_for.clone().unwrap_or_default(),
                    _ => value.to_string(),
                };
                fields += 1;
                want.push(format!("{tag} {}", theirs_value.trim()));
                ours.push(format!("{tag} {}", ours_for.unwrap_or_default().trim()));
            }
            if ours == want {
                same += 1;
            } else {
                let diff: Vec<String> = ours.iter().zip(&want).filter(|(a, b)| a != b).map(|(a, b)| format!("ours {a:?} theirs {b:?}")).collect();
                differences.push(format!("{}: {}", relative.display(), diff.join("; ")));
            }
        }
        eprintln!("{same} of {} SNDH files read the same as PSG play, {fields} fields compared.", same + differences.len());
        for d in differences.iter().take(40) {
            eprintln!("{d}");
        }
        assert!(differences.is_empty(), "{} files differ", differences.len());
    }

    /// Real YM files from a folder named by LIMONITER_TEST_YM: each must read whole, with its registers all there.
    /// With LIMONITER_TEST_YM_INFO naming what ST-Sound read from them (lines of file|type|name|author|comment|ms),
    /// the names and lengths must be the same.
    #[test]
    fn real_ym_files_read_whole() {
        let Some(folder) = std::env::var_os("LIMONITER_TEST_YM") else {
            eprintln!("LIMONITER_TEST_YM isn't set, so no real YM files were read.");
            return;
        };
        let reference: std::collections::HashMap<String, Vec<String>> = std::env::var_os("LIMONITER_TEST_YM_INFO")
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default()
            .lines()
            .map(|l| l.split('|').map(str::to_string).collect::<Vec<_>>())
            .filter(|f| f.len() == 6)
            .map(|f| (std::path::Path::new(&f[0]).file_name().unwrap().to_string_lossy().into_owned(), f))
            .collect();
        let mut agreed = 0;
        for entry in std::fs::read_dir(folder).unwrap().flatten() {
            let path = entry.path();
            let bytes = std::fs::read(&path).unwrap();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            assert!(recognised(&bytes[..bytes.len().min(64)], &name), "{name}");
            let ym = read_ym(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(ym.frames > 0 || ym.format == "MIX1", "{name}");
            if let Some(theirs) = reference.get(&name) {
                // YM2! and YM3 files carry no names: ST-Sound makes up its own.
                if ym.registers != 14 {
                    assert_eq!((ym.name.as_str(), ym.author.as_str(), ym.comment.as_str()), (theirs[2].trim(), theirs[3].trim(), theirs[4].trim()), "{name}");
                }
                if ym.rate > 0 && ym.registers > 0 {
                    assert_eq!(u64::from(ym.frames) * 1000 / u64::from(ym.rate), theirs[5].parse::<u64>().unwrap(), "{name}");
                }
                agreed += 1;
            }
            eprintln!("{name}: {} {:?} by {:?}, {} frames at {} Hz, loop {}, attributes ${:X}, {} digidrums, {} stream bytes, packed {:?}, notes {:?}", ym.format, ym.name, ym.author, ym.frames, ym.rate, ym.loop_frame, ym.attributes, ym.drums.len(), ym.stream_bytes, ym.packed, ym.notes);
        }
        eprintln!("{agreed} YM files read the same as ST-Sound.");
    }
}
