//! C64 music files (.sid), as the PSID and RSID headers describe them (versions 1 to 4, as HVSC's
//! SID_file_format.txt sets out): the title, author and release, where the tune loads, its init and play routines,
//! its songs and their speeds, the machine and SID chips it was made for, and where a player may be put. Only read:
//! nothing here plays a tune. Its code and data can be taken out as a C64 program at its load address, to
//! disassemble, and the calls that play it written as 6502 source.

use crate::media::InfoSection;
use crate::preview::{Block, BlockKind, Span};

/// The two kinds of SID file: PSID, for a player's own environment, and RSID, which needs a real C64's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Magic {
    Psid,
    Rsid,
}

/// How often a song's play routine is called.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speed {
    /// Once a frame, from the vertical blank (the raster): 50 Hz on PAL, 60 Hz on NTSC.
    Vbi,
    /// From CIA 1's timer A: 60 Hz unless the tune sets the timer itself.
    Cia,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tune {
    pub magic: Option<Magic>,
    pub version: u16,
    pub data_offset: u16,
    /// Where the data loads, and whether that came from the data's own first two bytes (the header's was 0).
    pub load: u16,
    pub load_in_data: bool,
    /// As the header gives them: an init of 0 in a PSID means the load address.
    pub init: u16,
    pub play: u16,
    pub songs: u16,
    pub start_song: u16,
    pub speed: u32,
    pub name: String,
    pub author: String,
    pub released: String,
    pub flags: u16,
    pub reloc_start_page: u8,
    pub reloc_pages: u8,
    /// The second and third SIDs' addresses, from version 3 and 4.
    pub second_sid: Option<u16>,
    pub third_sid: Option<u16>,
    /// The C64 bytes, without any load address.
    pub data: Vec<u8>,
    /// Anything odd about it, said plainly.
    pub notes: Vec<String>,
}

/// Whether bytes start as a SID file does.
pub fn looks_like(bytes: &[u8]) -> bool {
    bytes.len() >= 0x76 && matches!(&bytes[..4], b"PSID" | b"RSID") && (1..=4).contains(&u16::from_be_bytes([bytes[4], bytes[5]]))
}

/// Whether a file's name says it's a SID tune.
pub fn named_as_sid(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".sid")
}

/// The text fields are Latin-1, padded with zeros.
fn text(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    bytes[..end].iter().map(|&b| if (0x20..0x7F).contains(&b) || b >= 0xA0 { char::from(b) } else { ' ' }).collect::<String>().trim_end().to_string()
}

/// A second or third SID's address from its byte: $Dxx0, for the even values the format allows.
fn sid_address(byte: u8) -> Option<u16> {
    (byte != 0).then(|| 0xD000 | (u16::from(byte) << 4))
}

fn valid_sid_byte(byte: u8) -> bool {
    byte.is_multiple_of(2) && ((0x42..=0x7F).contains(&byte) || (0xE0..=0xFE).contains(&byte))
}

pub fn read(bytes: &[u8]) -> Result<Tune, String> {
    if bytes.len() < 4 || !matches!(&bytes[..4], b"PSID" | b"RSID") {
        return Err("This isn't a SID tune: it doesn't start with PSID or RSID.".into());
    }
    if bytes.len() < 0x76 {
        return Err("The SID header is cut short.".into());
    }
    let word = |at: usize| u16::from_be_bytes([bytes[at], bytes[at + 1]]);
    let magic = if &bytes[..4] == b"RSID" { Magic::Rsid } else { Magic::Psid };
    let version = word(4);
    if !(1..=4).contains(&version) {
        return Err(format!("The SID header says it's version {version}; only 1 to 4 exist."));
    }
    let mut tune = Tune {
        magic: Some(magic),
        version,
        data_offset: word(6),
        load: word(8),
        init: word(0x0A),
        play: word(0x0C),
        songs: word(0x0E),
        start_song: word(0x10),
        speed: u32::from_be_bytes([bytes[0x12], bytes[0x13], bytes[0x14], bytes[0x15]]),
        name: text(&bytes[0x16..0x36]),
        author: text(&bytes[0x36..0x56]),
        released: text(&bytes[0x56..0x76]),
        ..Tune::default()
    };
    if version >= 2 && bytes.len() >= 0x7C {
        tune.flags = word(0x76);
        tune.reloc_start_page = bytes[0x78];
        tune.reloc_pages = bytes[0x79];
        // An address the format doesn't allow means no SID there, as players take it.
        for (which, at, from) in [("second", 0x7A, 3), ("third", 0x7B, 4)] {
            let byte = bytes[at];
            if version < from || byte == 0 {
                continue;
            }
            if valid_sid_byte(byte) {
                let address = sid_address(byte);
                if which == "second" {
                    tune.second_sid = address;
                } else if address != tune.second_sid {
                    tune.third_sid = address;
                }
            } else {
                tune.notes.push(format!("The {which} SID's address byte is ${byte:02X}, which isn't one the format allows (even, $42-$7E or $E0-$FE), so it's taken as no {which} SID."));
            }
        }
    }
    let expected = if version == 1 { 0x76 } else { 0x7C };
    if usize::from(tune.data_offset) != expected {
        tune.notes.push(format!("The data starts at ${:02X}, where a version {version} header ends at ${expected:02X}.", tune.data_offset));
    }
    let mut data = bytes.get(usize::from(tune.data_offset)..).ok_or("The SID file ends before its data starts.")?;
    if tune.load == 0 {
        if data.len() < 2 {
            return Err("The SID file's data is too short to hold the load address it should start with.".into());
        }
        tune.load = u16::from_le_bytes([data[0], data[1]]);
        tune.load_in_data = true;
        data = &data[2..];
    }
    tune.data = data.to_vec();
    tune.check(magic);
    Ok(tune)
}

impl Tune {
    pub fn is_rsid(&self) -> bool {
        self.magic == Some(Magic::Rsid)
    }

    /// The init routine's address: a PSID's 0 means the load address.
    pub fn init_address(&self) -> u16 {
        if self.init == 0 && !self.is_rsid() { self.load } else { self.init }
    }

    /// The last address the data fills.
    pub fn end(&self) -> u32 {
        u32::from(self.load) + (self.data.len() as u32).max(1) - 1
    }

    /// The song the player starts with, from 1.
    pub fn start(&self) -> u16 {
        if self.start_song == 0 { 1 } else { self.start_song.min(self.songs.max(1)) }
    }

    /// Bit 0: a PSID's data is Compute!'s Sidplayer MUS data, for a player to be merged with.
    pub fn is_mus(&self) -> bool {
        !self.is_rsid() && self.flags & 1 != 0
    }

    /// Bit 1: an RSID is a BASIC program, to be RUN.
    pub fn is_basic(&self) -> bool {
        self.is_rsid() && self.flags & 2 != 0
    }

    /// Bit 1 of a PSID: it needs PlaySID's own extras, such as its samples.
    pub fn is_playsid_specific(&self) -> bool {
        !self.is_rsid() && self.flags & 2 != 0
    }

    pub fn clock(&self) -> &'static str {
        ["Unknown", "PAL", "NTSC", "PAL and NTSC"][usize::from((self.flags >> 2) & 3)]
    }

    /// Which SID chip each SID wants: the first's, then the second's and third's (which, unknown, mean the first's).
    pub fn model(&self, which: usize) -> &'static str {
        let bits = (self.flags >> (4 + 2 * which.min(2))) & 3;
        let bits = if which > 0 && bits == 0 { (self.flags >> 4) & 3 } else { bits };
        ["Unknown", "6581", "8580", "6581 and 8580"][usize::from(bits)]
    }

    /// How a song (from 1) is played. Songs past 32 share the 32nd's bit. An RSID's speed isn't used.
    pub fn speed_of(&self, song: u16) -> Speed {
        let bit = u32::from(song.saturating_sub(1).min(31));
        if self.speed & (1 << bit) != 0 { Speed::Cia } else { Speed::Vbi }
    }

    /// The SIDs it plays on, with their addresses.
    pub fn sid_count(&self) -> usize {
        1 + usize::from(self.second_sid.is_some()) + usize::from(self.third_sid.is_some())
    }

    /// The data as a C64 program: its load address, then the bytes, to disassemble or load.
    pub fn program(&self) -> Vec<u8> {
        let mut out = self.load.to_le_bytes().to_vec();
        out.extend_from_slice(&self.data);
        out
    }

    /// How many header bytes to skip to reach the C64 bytes in the file: the header, and the load address if the
    /// data starts with it.
    pub fn skip(&self) -> usize {
        usize::from(self.data_offset) + if self.load_in_data { 2 } else { 0 }
    }

    fn check(&mut self, magic: Magic) {
        let rsid = magic == Magic::Rsid;
        if rsid && self.version == 1 {
            self.notes.push("An RSID file should be version 2 or later.".into());
        }
        if self.songs == 0 || self.songs > 256 {
            self.notes.push(format!("It says it has {} songs, where the format allows 1 to 256.", self.songs));
        }
        if self.start_song == 0 || self.start_song > self.songs.max(1) {
            self.notes.push(format!("Its start song is {}, outside its {} songs, so players start with song {}.", self.start_song, self.songs, self.start()));
        }
        if self.end() > 0xFFFF {
            self.notes.push(format!("The data runs past the end of the C64's memory ($FFFF), to ${:X}.", self.end()));
        }
        if rsid {
            if !self.load_in_data {
                self.notes.push("An RSID's header load address should be 0, with the load address at the start of its data.".into());
            }
            if self.play != 0 {
                self.notes.push(format!("An RSID's play address should be 0, but it's ${:04X}.", self.play));
            }
            if self.speed != 0 {
                self.notes.push("An RSID's speed field should be 0: its own interrupts set the speed.".into());
            }
            if self.load < 0x07E8 {
                self.notes.push(format!("An RSID shouldn't load below $07E8, but this one loads at ${:04X}.", self.load));
            }
            let init = self.init;
            if !self.is_basic() && (init < 0x07E8 || (0xA000..=0xBFFF).contains(&init) || init >= 0xD000) {
                self.notes.push(format!("Its init address ${init:04X} is somewhere an RSID's mustn't be (below $07E8, in BASIC's ROM, or $D000 and up)."));
            }
        } else if self.flags & 1 != 0 {
            self.notes.push("Its data is Compute!'s Sidplayer MUS data, which needs that player merged with it to play.".into());
        }
        if let Some(range) = self.reloc_range()
            && (range.0 <= self.end() as u16 && u32::from(range.1) >= u32::from(self.load))
        {
            self.notes.push(format!("The memory it says is free for a player (${:04X}-${:04X}) overlaps the tune's own.", range.0, range.1));
        }
    }

    /// The pages it leaves free for a player, as addresses, when it says.
    pub fn reloc_range(&self) -> Option<(u16, u16)> {
        (self.version >= 2 && self.reloc_start_page != 0 && self.reloc_start_page != 0xFF && self.reloc_pages != 0)
            .then(|| (u16::from(self.reloc_start_page) << 8, ((u16::from(self.reloc_start_page) + u16::from(self.reloc_pages) - 1).min(0xFF) << 8) | 0xFF))
    }

    fn reloc_text(&self) -> String {
        match (self.version, self.reloc_start_page, self.reloc_range()) {
            (1, ..) => "Not said (version 1)".into(),
            (_, 0xFF, _) => "None: no memory is free for a player".into(),
            (_, _, Some((from, to))) => format!("${from:04X}-${to:04X} ({} pages)", self.reloc_pages),
            _ => "Not said: a player works out what's free".into(),
        }
    }

    fn speed_text(&self, song: u16) -> &'static str {
        if self.is_rsid() {
            return "its own interrupts";
        }
        match self.speed_of(song) {
            Speed::Vbi if self.play == 0 => "its own interrupt (no play address)",
            Speed::Vbi => "every frame (vertical blank)",
            Speed::Cia => "CIA 1 timer (60 Hz unless the tune sets it)",
        }
    }

    fn summary(&self) -> Vec<(String, String)> {
        let format = match self.magic {
            Some(Magic::Rsid) => format!("RSID version {} (needs a real C64's environment)", self.version),
            _ => format!("PSID version {}", self.version),
        };
        let or_none = |s: &str| if s.is_empty() { "(none)".to_string() } else { s.to_string() };
        let mut rows = vec![
            ("Title".to_string(), or_none(&self.name)),
            ("Author".into(), or_none(&self.author)),
            ("Released".into(), or_none(&self.released)),
            ("Format".into(), format),
            ("Loads at".into(), format!("${:04X}-${:04X} ({} bytes{})", self.load, self.end().min(0xFFFF), group(self.data.len() as u64), if self.load_in_data { ", address from the data" } else { "" })),
            ("Init".into(), if self.is_basic() { "BASIC program: RUN".to_string() } else { format!("${:04X}", self.init_address()) }),
            ("Play".into(), if self.play == 0 { "0: the tune sets up its own interrupt".into() } else { format!("${:04X}", self.play) }),
            ("Songs".into(), format!("{}, starting with song {}", self.songs, self.start())),
        ];
        if !self.is_rsid() {
            let cia = (1..=self.songs.clamp(1, 256)).filter(|&s| self.speed_of(s) == Speed::Cia).count();
            let speed = match cia {
                0 => "Every frame (vertical blank) for every song".to_string(),
                n if n == usize::from(self.songs.max(1)) => "CIA 1 timer for every song".to_string(),
                n => format!("CIA 1 timer for {n} songs, every frame for the rest (see Songs)"),
            };
            rows.push(("Speed".into(), speed));
        }
        if self.version >= 2 {
            rows.push(("Clock".into(), self.clock().to_string()));
            rows.push(("SID model".into(), self.model(0).to_string()));
            if self.sid_count() > 1 {
                rows.push(("SIDs".into(), self.sid_count().to_string()));
            }
            if let Some(second) = self.second_sid {
                rows.push(("Second SID".into(), format!("${second:04X}, {}", self.model(1))));
            }
            if let Some(third) = self.third_sid {
                rows.push(("Third SID".into(), format!("${third:04X}, {}", self.model(2))));
            }
            if self.is_rsid() {
                rows.push(("BASIC".into(), if self.is_basic() { "Yes: a BASIC program, RUN to start it".into() } else { "No".into() }));
            } else if self.is_playsid_specific() {
                rows.push(("PlaySID specific".into(), "Yes: it uses PlaySID's own extras, such as its samples".into()));
            }
            if self.is_mus() {
                rows.push(("Data".into(), "Compute!'s Sidplayer MUS data".into()));
            }
            rows.push(("Free for a player".into(), self.reloc_text()));
        }
        rows
    }

    /// The songs, each with its speed, while there are few enough to list.
    fn song_rows(&self) -> Vec<(String, String)> {
        (1..=self.songs.clamp(1, 256)).map(|s| (format!("Song {s}{}", if s == self.start() { " (start)" } else { "" }), self.speed_text(s).to_string())).collect()
    }

    /// What File properties shows: the tune, then its songs.
    pub fn sections(&self) -> Vec<InfoSection> {
        let mut first = self.summary();
        first.extend(self.notes.iter().map(|n| (String::new(), n.clone())));
        vec![InfoSection { title: "SID tune".into(), rows: first }, InfoSection { title: "Songs".into(), rows: self.song_rows() }]
    }

    /// The preview: the summary, then a table of the songs and their speeds.
    pub fn blocks(&self) -> Vec<Block> {
        let plain = |text: String| Span { text, ..Span::default() };
        let strong = |text: String| Span { text, strong: true, ..Span::default() };
        let block = |kind: BlockKind, spans: Vec<Span>| Block { kind, spans, indent: 0, quote: 0, marker: None };
        let mut blocks = vec![block(BlockKind::Heading(2), vec![plain(if self.name.is_empty() { "Untitled tune".into() } else { self.name.clone() })])];
        for (label, value) in self.summary().into_iter().skip(1) {
            blocks.push(block(BlockKind::Paragraph, vec![strong(format!("{label}: ")), plain(value)]));
        }
        for note in &self.notes {
            blocks.push(block(BlockKind::Paragraph, vec![Span { text: note.clone(), italic: true, ..Span::default() }]));
        }
        blocks.push(block(BlockKind::Heading(3), vec![plain("Songs".into())]));
        let mut rows = vec![["Song", "Played by"].map(|h| vec![strong(h.into())]).to_vec()];
        rows.extend(self.song_rows().into_iter().map(|(song, speed)| vec![vec![plain(song)], vec![plain(speed)]]));
        blocks.push(block(BlockKind::Table(rows), Vec::new()));
        blocks.push(block(BlockKind::Paragraph, vec![Span { text: "Tools, Commodore 64, SID music and packers opens its code as a C64 program to disassemble, and writes the calls that play it into 6502 source.".into(), italic: true, ..Span::default() }]));
        blocks
    }
}

/// The calls that play a tune, as 6502 source to put into a program: the addresses from its header, the song
/// chosen (from 1), and its data included at its load address. Written here, for any tune; nothing of the tune's
/// own player is copied.
pub fn replay_snippet(tune: &Tune, file_name: &str, song: u16) -> String {
    let song = song.clamp(1, tune.songs.max(1));
    let mut s = String::new();
    let mut line = |text: &str| {
        s.push_str(text);
        s.push('\n');
    };
    let by = if tune.author.is_empty() { String::new() } else { format!(" by {}", tune.author) };
    let released = if tune.released.is_empty() { String::new() } else { format!(" ({})", tune.released) };
    let title = if tune.name.is_empty() { file_name.to_string() } else { format!("\"{}\"", tune.name) };
    line(&format!("; --- SID tune: {title}{by}{released} ---"));
    line(&format!("; Song {song} of {}. Written by Limoniter from {file_name}'s header: the tune's code and data load at", tune.songs.max(1)));
    line(&format!("; ${:04X}-${:04X}, so keep your own code and data out of there.", tune.load, tune.end().min(0xFFFF)));
    let mut warn = |text: String| line(&format!("; {text}"));
    if tune.version >= 2 {
        let model = tune.model(0);
        warn(format!("It was made for a {} C64 and a {} SID.", match tune.clock() { "Unknown" => "PAL or NTSC", c => c }, match model { "Unknown" => "6581 or 8580", m => m }));
    }
    if let Some(second) = tune.second_sid {
        warn(format!("It also plays a second SID at ${second:04X}{}: a stock C64 has only the one at $D400.", tune.third_sid.map_or(String::new(), |t| format!(" and a third at ${t:04X}"))));
    }
    if tune.is_mus() {
        warn("Its data is Compute!'s Sidplayer MUS data, not a player: these calls can't play it without that player.".into());
    }
    if tune.is_playsid_specific() {
        warn("It uses PlaySID's own extras, so it may not sound right on a real C64.".into());
    }
    line("");
    if tune.is_basic() {
        line("; It's a BASIC program (an RSID with the BASIC flag): load it and RUN it; it has no routine to call.");
        line(&format!("; To keep it in your own program, load it at ${:04X} as BASIC would, put the song (from 0) in $030C, and RUN it.", tune.load));
        return s;
    }
    if tune.load <= 0xDFFF && tune.end() >= 0xD000 {
        line("; Part of it loads under the I/O at $D000-$DFFF, where LOAD (with the I/O in) can't put it: copy that part");
        line("; there yourself with $01 at $34, or load the program with a loader that does.");
    }
    let init_bank = bank_for(tune.init_address());
    let play_bank = bank_for(tune.play);
    let call_init = |line: &mut dyn FnMut(&str)| {
        if init_bank != 0x37 {
            line(&format!("\t\tlda #${init_bank:02x}\t\t\t; {}: init is under it", bank_words(init_bank)));
            line("\t\tsta $01");
        }
        line("\t\tlda #sid_song");
        line("\t\tjsr sid_init");
        if init_bank != 0x37 {
            line("\t\tlda #$37\t\t\t; the ROMs back");
            line("\t\tsta $01");
        }
    };
    let call_play = |line: &mut dyn FnMut(&str)| {
        if play_bank != 0x37 {
            line(&format!("\t\tlda #${play_bank:02x}\t\t\t; {}: play is under it", bank_words(play_bank)));
            line("\t\tsta $01");
        }
        line("\t\tjsr sid_play");
        if play_bank != 0x37 {
            line("\t\tlda #$37\t\t\t; the ROMs back, for the KERNAL's end of interrupt");
            line("\t\tsta $01");
        }
    };
    line(&format!("sid_load = ${:04X}", tune.load));
    line(&format!("sid_init = ${:04X}", tune.init_address()));
    if tune.play != 0 {
        line(&format!("sid_play = ${:04X}", tune.play));
    }
    line(&format!("sid_song = {}\t\t\t; song {song}: the init routine takes the song, from 0, in A", song - 1));
    line("");
    if tune.play == 0 || tune.is_rsid() {
        line("; The tune has no play routine to call: its init sets up its own interrupt, which plays it.");
        if tune.is_rsid() {
            line("; As an RSID it expects a C64 just after reset, with the KERNAL and BASIC ROMs in and its interrupts");
            line("; untouched; some RSID inits never return, and take the machine over.");
        }
        line("music_start:");
        call_init(&mut line);
        line("\t\tcli");
        line("\t\trts");
    } else if tune.speed_of(song) == Speed::Cia {
        line("; This song is played from CIA 1's timer A, as the KERNAL's own interrupt is: the init sets the timer");
        line("; for the tune's speed (if it doesn't, the KERNAL's 60 Hz stays), and the interrupt calls play.");
        line("music_start:");
        line("\t\tsei");
        call_init(&mut line);
        line("\t\tlda #<music_irq");
        line("\t\tsta $0314\t\t\t; the KERNAL's interrupt vector");
        line("\t\tlda #>music_irq");
        line("\t\tsta $0315");
        line("\t\tcli");
        line("\t\trts");
        line("");
        line("music_irq:");
        line("\t\tlda $dc0d\t\t\t; acknowledge CIA 1's interrupt");
        call_play(&mut line);
        line("\t\tjmp $ea81\t\t\t; the KERNAL's end of interrupt: registers back, then rti");
    } else {
        line("; Played once a frame from a raster interrupt, with CIA 1's timer interrupt turned off. Put your own");
        line("; per-frame work in music_irq too, or move the raster line to suit your effects.");
        line("music_start:");
        line("\t\tsei");
        line("\t\tlda #$7f");
        line("\t\tsta $dc0d\t\t\t; CIA 1's timer interrupts off");
        line("\t\tlda $dc0d\t\t\t; and any already waiting acknowledged");
        call_init(&mut line);
        line("\t\tlda #<music_irq");
        line("\t\tsta $0314\t\t\t; the KERNAL's interrupt vector");
        line("\t\tlda #>music_irq");
        line("\t\tsta $0315");
        line("\t\tlda #$80");
        line("\t\tsta $d012\t\t\t; the raster line to interrupt on");
        line("\t\tlda $d011");
        line("\t\tand #$7f\t\t\t; (its ninth bit clear)");
        line("\t\tsta $d011");
        line("\t\tlda #$01");
        line("\t\tsta $d01a\t\t\t; the VIC-II's raster interrupt on");
        line("\t\tsta $d019\t\t\t; and any already waiting acknowledged");
        line("\t\tcli");
        line("\t\trts");
        line("");
        line("music_irq:");
        line("\t\tasl $d019\t\t\t; acknowledge the raster interrupt");
        call_play(&mut line);
        line("\t\tjmp $ea81\t\t\t; the KERNAL's end of interrupt: registers back, then rti");
    }
    line("");
    line(&format!("; The tune, at its load address, without the file's {}-byte header{}:", tune.skip(), if tune.load_in_data { " and load address" } else { "" }));
    line("\t\t* = sid_load");
    line(&format!("\t\tincbin \"{file_name}\",{}", tune.skip()));
    line(&format!("; (ca65: .incbin \"{file_name}\",{0}; ACME: !binary \"{file_name}\",,{0}; 64tass: .binary \"{file_name}\",{0})", tune.skip()));
    s
}

/// What the 6510's port ($01) is set to while a routine at an address runs, as SID_file_format.txt says a player
/// does: BASIC's ROM out above $A000, both ROMs out above $E000, and the I/O out too between them.
fn bank_for(address: u16) -> u8 {
    match address {
        0..=0x9FFF => 0x37,
        0xA000..=0xCFFF => 0x36,
        0xE000.. => 0x35,
        _ => 0x34,
    }
}

fn bank_words(bank: u8) -> &'static str {
    match bank {
        0x36 => "BASIC's ROM out",
        0x35 => "both ROMs out, the I/O in",
        _ => "all RAM",
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

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A SID file made up here: a version, the load, init and play addresses (load 0 puts it in the data), the
    /// songs, start song and speed bits, the flags, and the C64 bytes.
    #[allow(clippy::too_many_arguments)]
    pub fn synthetic(magic: &[u8; 4], version: u16, load: u16, init: u16, play: u16, songs: u16, start: u16, speed: u32, flags: u16, extra: [u8; 4], data: &[u8]) -> Vec<u8> {
        let header = if version == 1 { 0x76 } else { 0x7C };
        let mut bytes = vec![0u8; header];
        bytes[..4].copy_from_slice(magic);
        for (at, value) in [(4, version), (6, header as u16), (8, load), (0x0A, init), (0x0C, play), (0x0E, songs), (0x10, start)] {
            bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
        }
        bytes[0x12..0x16].copy_from_slice(&speed.to_be_bytes());
        bytes[0x16..0x16 + 9].copy_from_slice(b"Test Tune");
        bytes[0x36..0x36 + 11].copy_from_slice(b"Some\xF8ne Els");
        bytes[0x56..0x56 + 13].copy_from_slice(b"2026 Nobody's");
        if version >= 2 {
            bytes[0x76..0x78].copy_from_slice(&flags.to_be_bytes());
            bytes[0x78..0x7C].copy_from_slice(&extra);
        }
        bytes.extend_from_slice(data);
        bytes
    }

    #[test]
    fn a_psid_header_says_everything_it_holds() {
        // Version 2, PAL, 8580, load address in the data, two songs: the second on the CIA timer.
        let mut data = vec![0x00, 0x10];
        data.extend([0x4C, 0x06, 0x10, 0x4C, 0x07, 0x10, 0x60, 0x60]);
        let bytes = synthetic(b"PSID", 2, 0, 0x1000, 0x1003, 2, 2, 0b10, 0b10_01_00, [0xC0, 0x10, 0, 0], &data);
        assert!(looks_like(&bytes));
        let tune = read(&bytes).unwrap();
        assert_eq!((tune.load, tune.load_in_data, tune.init_address(), tune.play), (0x1000, true, 0x1000, 0x1003));
        assert_eq!((tune.songs, tune.start()), (2, 2));
        assert_eq!((tune.speed_of(1), tune.speed_of(2)), (Speed::Vbi, Speed::Cia));
        assert_eq!((tune.clock(), tune.model(0), tune.model(1)), ("PAL", "8580", "8580"));
        assert_eq!(tune.author, "Some\u{F8}ne Els", "Latin-1 text");
        assert_eq!(tune.end(), 0x1007);
        assert_eq!(tune.reloc_range(), Some((0xC000, 0xCFFF)));
        assert_eq!(tune.program(), data, "the program is the load address and the bytes");
        assert_eq!(tune.skip(), 0x7E);
        assert!(tune.notes.is_empty(), "{:?}", tune.notes);
        let sections = tune.sections();
        let row = |label: &str| sections[0].rows.iter().find(|(l, _)| l == label).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(row("Loads at"), "$1000-$1007 (8 bytes, address from the data)");
        assert_eq!(row("Speed"), "CIA 1 timer for 1 songs, every frame for the rest (see Songs)");
        assert_eq!(row("Free for a player"), "$C000-$CFFF (16 pages)");
        assert_eq!(sections[1].rows[1], ("Song 2 (start)".to_string(), "CIA 1 timer (60 Hz unless the tune sets it)".to_string()));
    }

    #[test]
    fn version_1_and_3_and_4_and_rsid_are_read() {
        let v1 = read(&synthetic(b"PSID", 1, 0x1000, 0, 0x1003, 1, 1, 0, 0, [0; 4], &[0x60; 4])).unwrap();
        assert_eq!((v1.load, v1.load_in_data, v1.init_address(), v1.data.len()), (0x1000, false, 0x1000, 4));
        assert_eq!(v1.sections()[0].rows.iter().find(|r| r.0 == "Clock"), None, "version 1 says nothing of the clock");
        // Version 4: three SIDs at $D420 and $DE00, 6581 + 8580 + (unknown, so the first's).
        let v4 = read(&synthetic(b"PSID", 4, 0x1000, 0x1000, 0x1003, 1, 1, 0, 0b00_10_01_01_00, [0, 0, 0x42, 0xE0], &[0x60; 4])).unwrap();
        assert_eq!((v4.second_sid, v4.third_sid, v4.sid_count()), (Some(0xD420), Some(0xDE00), 3));
        assert_eq!((v4.model(0), v4.model(1), v4.model(2)), ("6581", "8580", "6581"));
        assert!(v4.notes.is_empty(), "{:?}", v4.notes);
        let bad = read(&synthetic(b"PSID", 3, 0x1000, 0x1000, 0x1003, 1, 1, 0, 0, [0, 0, 0x43, 0], &[0x60; 4])).unwrap();
        assert!(bad.notes.iter().any(|n| n.contains("second SID")), "{:?}", bad.notes);
        // An RSID that's a BASIC program.
        let mut basic = vec![0x01, 0x08];
        basic.extend([0x0B, 0x08, 0x0A, 0x00, 0x99, 0x22, 0x48, 0x22, 0x00, 0x00, 0x00]);
        let rsid = read(&synthetic(b"RSID", 2, 0, 0, 0, 1, 1, 0, 0b10, [0; 4], &basic)).unwrap();
        assert!(rsid.is_rsid() && rsid.is_basic());
        assert!(rsid.notes.is_empty(), "{:?}", rsid.notes);
        assert!(replay_snippet(&rsid, "b.sid", 1).contains("RUN"));
        // An RSID broken in the ways the format forbids.
        let broken = read(&synthetic(b"RSID", 2, 0x0400, 0xE000, 0x1003, 1, 1, 1, 0, [0; 4], &[0x60; 4])).unwrap();
        assert_eq!(broken.notes.len(), 5, "{:?}", broken.notes);
        assert!(read(b"PSID\x00\x09").is_err());
        assert!(read(b"not a tune at all").is_err());
        assert!(!looks_like(b"PSID"));
    }

    #[test]
    fn songs_past_32_share_the_last_speed_bit() {
        let tune = read(&synthetic(b"PSID", 2, 0x1000, 0x1000, 0x1003, 40, 0, 1 << 31, 0, [0; 4], &[0x60; 4])).unwrap();
        assert_eq!((tune.speed_of(31), tune.speed_of(32), tune.speed_of(40)), (Speed::Vbi, Speed::Cia, Speed::Cia));
        assert_eq!(tune.start(), 1, "a start song of 0 is taken as 1");
        assert_eq!(tune.notes.len(), 1, "{:?}", tune.notes);
    }

    #[test]
    fn the_replay_snippet_has_the_tunes_addresses() {
        let tune = read(&synthetic(b"PSID", 2, 0, 0x1000, 0x1003, 3, 1, 0b100, 0, [0; 4], &[0x00, 0x10, 0x60, 0x60, 0x60, 0x60])).unwrap();
        let vbi = replay_snippet(&tune, "tune.sid", 2);
        assert!(vbi.contains("sid_init = $1000") && vbi.contains("sid_play = $1003") && vbi.contains("sid_song = 1"), "{vbi}");
        assert!(vbi.contains("sta $d01a") && vbi.contains("incbin \"tune.sid\",126"), "{vbi}");
        let cia = replay_snippet(&tune, "tune.sid", 3);
        assert!(cia.contains("lda $dc0d") && !cia.contains("$d01a"), "{cia}");
        let own = read(&synthetic(b"PSID", 2, 0x1000, 0x1000, 0, 1, 1, 0, 0, [0; 4], &[0x60])).unwrap();
        let own = replay_snippet(&own, "own.sid", 1);
        assert!(!own.contains("sid_play") && own.contains("incbin \"own.sid\",124"), "{own}");
    }

    /// With LIMONITER_SID_DIR naming a folder of real SID files (HVSC's, say), each is read and, where sidplayfp
    /// is installed, what it says of the tune (`sidplayfp -v`) is compared with what's read here.
    #[test]
    fn real_tunes_agree_with_sidplayfp() {
        let Some(dir) = std::env::var_os("LIMONITER_SID_DIR").map(std::path::PathBuf::from) else {
            eprintln!("LIMONITER_SID_DIR isn't set, so no real tunes were read.");
            return;
        };
        let mut compared = 0;
        let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("sid"))).collect();
        files.sort();
        for path in files {
            let tune = read(&std::fs::read(&path).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let wav = std::env::temp_dir().join(format!("limoniter-sid-{}.wav", std::process::id()));
            let Ok(out) = std::process::Command::new("sidplayfp").arg("-v").arg("-t1").arg(format!("-w{}", wav.display())).arg(&path).output() else {
                eprintln!("{}: {} by {}, sidplayfp not installed", path.display(), tune.name, tune.author);
                continue;
            };
            let _ = std::fs::remove_file(&wav);
            let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            let field = |label: &str| said.lines().filter_map(|l| l.rsplit('|').next()?.split_once(':')).find(|(k, _)| k.trim() == label.trim()).map(|(_, v)| v.trim().to_string()).unwrap_or_default();
            let name = path.display().to_string();
            assert_eq!(field("Title       "), tune.name, "{name}");
            assert_eq!(field("Author      "), tune.author, "{name}");
            assert_eq!(field("Released    "), tune.released, "{name}");
            let addresses = said.lines().filter(|l| l.contains("INIT =") || l.contains("LOAD   =")).collect::<Vec<_>>().join(" ");
            assert!(addresses.contains(&format!("LOAD   = ${:04X}-${:04X}", tune.load, tune.end())), "{name}: {addresses}");
            if !tune.is_basic() {
                assert!(addresses.contains(&format!("INIT = ${:04X}", tune.init_address())), "{name}: {addresses}");
            }
            if tune.play != 0 {
                assert!(addresses.contains(&format!("PLAY = ${:04X}", tune.play)), "{name}: {addresses}");
            }
            assert!(field("Playlist    ").contains(&format!("(tune {}/{}", tune.start(), tune.songs)), "{name}: {}", field("Playlist    "));
            let second = said.lines().find_map(|l| l.split_once("2nd SID = $").map(|(_, v)| v[..4].to_ascii_uppercase()));
            assert_eq!(second, tune.second_sid.map(|a| format!("{a:04X}")), "{name}");
            let third = said.lines().find_map(|l| l.split_once("3rd SID = $").map(|(_, v)| v[..4].to_ascii_uppercase()));
            assert_eq!(third, tune.third_sid.map(|a| format!("{a:04X}")), "{name}");
            let speed = field("Play speed  ");
            let expected = if tune.is_rsid() { "" } else if tune.speed_of(tune.start()) == Speed::Cia { "CIA" } else { "VBI" };
            assert!(speed.contains(expected), "{name}: {speed}");
            let model = field("SID Details ");
            let wanted = match tune.model(0) {
                "6581" => "6581",
                "8580" => "8580",
                _ => "",
            };
            assert!(model.contains(wanted), "{name}: {model}");
            assert!(tune.notes.is_empty(), "{name}: HVSC's tunes follow the format, so nothing should be odd: {:?}", tune.notes);
            eprintln!("{name}: agrees with sidplayfp ({} {}, {} songs, load ${:04X}, {} SID{})", if tune.is_rsid() { "RSID" } else { "PSID" }, tune.version, tune.songs, tune.load, tune.sid_count(), if tune.sid_count() == 1 { "" } else { "s" });
            compared += 1;
        }
        eprintln!("{compared} tunes compared with sidplayfp.");
    }

    /// The snippet assembles with Limoniter's own 6502 assembler, into a program that has the tune at its load
    /// address and calls its init with the song.
    #[test]
    fn the_replay_snippet_assembles() {
        let data = [0x00, 0x10, 0x8D, 0x00, 0x02, 0x60, 0xEE, 0x20, 0xD0, 0x60];
        let bytes = synthetic(b"PSID", 2, 0, 0x1000, 0x1004, 2, 1, 0, 0, [0; 4], &data);
        let tune = read(&bytes).unwrap();
        let dir = std::env::temp_dir().join(format!("limoniter-sid-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("tune.sid"), &bytes).unwrap();
        for song in [1, 2] {
            let source = format!("\t\t* = $0801\n\t\t.byte $0b,$08,$0a,$00,$9e,$32,$30,$36,$31,$00,$00,$00\nstart:\t\tjsr music_start\nloop:\t\tjmp loop\n{}", replay_snippet(&tune, "tune.sid", song));
            let path = dir.join("main.asm");
            std::fs::write(&path, &source).unwrap();
            let assembled = crate::assemblers::assemble_own(crate::assemblers::Cpu::M6502, &path, &source);
            assert!(assembled.assembly.problems.is_empty(), "{:#?}\n{source}", assembled.assembly.problems);
            let chunks = &assembled.assembly.chunks;
            let at = |address: i64| chunks.iter().find(|c| (c.address..c.address + c.bytes.len() as i64).contains(&address)).map(|c| c.bytes[(address - c.address) as usize]);
            assert_eq!(at(0x1000), Some(0x8D), "the tune is at its load address");
            assert_eq!(at(0x1007), Some(0x60));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A program of a BASIC line's SYS, a call to music_start and a loop, then the snippet for a tune, as a .prg
    /// made by Limoniter's own assembler.
    fn program_playing(tune: &Tune, file: &std::path::Path, song: u16) -> Vec<u8> {
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        let source = format!("\t\t* = $0801\n\t\t.byte $0b,$08,$0a,$00,$9e,$32,$30,$36,$31,$00,$00,$00\nstart:\t\tjsr music_start\nloop:\t\tjmp loop\n{}", replay_snippet(tune, &name, song));
        let path = file.with_file_name("main.asm");
        std::fs::write(&path, &source).unwrap();
        let assembled = crate::assemblers::assemble_own(crate::assemblers::Cpu::M6502, &path, &source);
        assert!(assembled.assembly.problems.is_empty(), "{:#?}", assembled.assembly.problems);
        let chunks = &assembled.assembly.chunks;
        let low = chunks.iter().map(|c| c.address).min().unwrap();
        let high = chunks.iter().map(|c| c.address + c.bytes.len() as i64).max().unwrap();
        let mut prg = (low as u16).to_le_bytes().to_vec();
        prg.extend(assembled.assembly.image(low, high));
        prg
    }

    /// The snippet plays real tunes in VICE: with LIMONITER_SID_DIR naming a folder of SID files and
    /// LIMONITER_VICE_DATA the folder of VICE's ROMs (for -directory), each tune that loads after $0900 is built into
    /// a program that plays it, run headless in x64sc (under xvfb-run if it's there) for a few seconds while its sound
    /// is recorded, and the recording mustn't be silent.
    #[test]
    fn the_replay_snippet_plays_real_tunes_in_vice() {
        let (Some(dir), Some(data)) = (std::env::var_os("LIMONITER_SID_DIR"), std::env::var_os("LIMONITER_VICE_DATA")) else {
            eprintln!("LIMONITER_SID_DIR and LIMONITER_VICE_DATA aren't both set, so no tune was played in VICE.");
            return;
        };
        let work = std::env::temp_dir().join(format!("limoniter-sid-vice-{}", std::process::id()));
        std::fs::create_dir_all(&work).unwrap();
        let mut played = 0;
        let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("sid"))).collect();
        files.sort();
        for path in files {
            let bytes = std::fs::read(&path).unwrap();
            let tune = read(&bytes).unwrap();
            // The test program sits at $0801-$0900, and a stock C64 has only one SID.
            if tune.load < 0x0900 || tune.is_basic() || tune.sid_count() > 1 {
                continue;
            }
            let file = work.join(path.file_name().unwrap());
            std::fs::write(&file, &bytes).unwrap();
            let prg = work.join("play.prg");
            std::fs::write(&prg, program_playing(&tune, &file, tune.start())).unwrap();
            let wav = work.join("play.wav");
            let _ = std::fs::remove_file(&wav);
            let mut args: Vec<String> = ["-default", "-directory"].map(String::from).to_vec();
            args.push(std::path::PathBuf::from(&data).display().to_string());
            args.extend(["-sounddev", "wav", "-soundarg"].map(String::from));
            args.push(wav.display().to_string());
            // Not in warp, which records no sound: seven million cycles, about seven seconds.
            args.extend(["-limitcycles", "7000000", "-autostartprgmode", "1", "-autostart"].map(String::from));
            args.push(prg.display().to_string());
            let xvfb = std::process::Command::new("xvfb-run").arg("--help").output().is_ok();
            let mut command = if xvfb {
                let mut c = std::process::Command::new("xvfb-run");
                c.arg("-a").arg("x64sc");
                c
            } else {
                std::process::Command::new("x64sc")
            };
            let Ok(_) = command.args(&args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status() else {
                eprintln!("x64sc couldn't be started, so no tune was played.");
                return;
            };
            let sound = std::fs::read(&wav).unwrap_or_default();
            // 16-bit samples after the 44-byte header. A silent machine records nothing but zeros.
            let samples: Vec<f64> = sound.get(44..).unwrap_or(&[]).as_chunks::<2>().0.iter().map(|b| f64::from(i16::from_le_bytes(*b))).collect();
            let later = &samples[samples.len() / 2..];
            let mean = later.iter().sum::<f64>() / later.len().max(1) as f64;
            let spread = (later.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / later.len().max(1) as f64).sqrt();
            eprintln!("{}: song {}, {} samples, spread {spread:.0}", path.display(), tune.start(), samples.len());
            assert!(spread > 300.0, "{}: the tune didn't play", path.display());
            played += 1;
        }
        eprintln!("{played} tunes played in VICE.");
        let _ = std::fs::remove_dir_all(&work);
    }
}
