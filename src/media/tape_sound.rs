//! Tape sound: a program or tape image made into the sound its tape would make, as a WAV, to load on the real
//! machine from a phone, a computer's sound output or a cassette interface. Each machine's tape is made as its own
//! ROM writes it, at its own timings:
//!
//! - ZX Spectrum `.tap`, and `.tzx` (and the Amstrad CPC's `.cdt`, which is the same format): the ROM's pilot tone
//!   of 2168 T-states a pulse (8063 of them before a header, 3223 before data), sync pulses of 667 and 735, and
//!   855 for a 0 bit, 1710 for a 1, at 3.5 MHz. A TZX's standard, turbo, pure tone, pulse sequence, pure data,
//!   direct recording, generalised data, pause and loop blocks are played as the TZX format (1.20) says; its
//!   descriptions are skipped, and the few blocks left (CSW recordings, jumps and calls) are said.
//! - ZX81 `.p` and ZX80 `.o`: each bit four pulses (0) or nine (1) of 530 T-states high and 520 low, then 4689
//!   low, after five seconds of quiet, as the TZX format describes the ROM's. A ZX81 tape starts with the
//!   program's name; the ZX80's has none.
//! - Commodore `.prg`: the KERNAL's format, as the C64's ROM saves it. Each bit is a short (2840 Hz) and a medium
//!   (1953 Hz) wave, or the other way round; each byte starts with a long (1488 Hz) and a medium, and ends with a
//!   check bit. A 192-byte header and then the program, each sent twice after its leader and a countdown.
//! - Commodore `.tap`: its pulses as they are, at the clock of the machine it says (C64, VIC-20, C16), PAL or NTSC.
//! - BBC Micro and Electron `.uef`: its carrier tones, gaps and bytes, at 1200 baud (a 0 bit one wave of 1200 Hz,
//!   a 1 two of 2400 Hz) or 300. A BBC BASIC program (`.bbc`) is saved as the cassette filing system saves it,
//!   in blocks of 256 bytes with their CRCs.
//! - MSX `.cas`: each block after the tone the BIOS writes, long (15360 waves of 2400 Hz) before a file's
//!   header and short (3840) before the rest, as the MSX Red Book gives them; each byte a 0 start bit, eight bits
//!   and two 1 stop bits, at 1200 baud.
//!
//! The sound is a square wave, at 44100 samples a second in 8 bits, which every cassette input copes with.

/// The rate tape sound is made at.
pub const RATE: u32 = 44_100;

/// The Spectrum's clock, which TZX and CDT timings are counted in.
const SPECTRUM_CLOCK: f64 = 3_500_000.0;

/// How far from the middle each level goes, of 128.
const LOUDNESS: f64 = 100.0;

fn tstates(count: u32) -> f64 {
    f64::from(count) / SPECTRUM_CLOCK
}

/// The signal as stretches of one level, high or low, each for so many seconds.
#[derive(Default)]
pub struct Signal {
    pieces: Vec<(f64, bool)>,
    high: bool,
    /// Whether the last thing on it was a pause.
    quiet: bool,
}

impl Signal {
    /// One level for a time; a stretch the same as the one before makes it longer.
    fn hold(&mut self, seconds: f64, high: bool) {
        if seconds <= 0.0 {
            return;
        }
        self.high = high;
        self.quiet = false;
        match self.pieces.last_mut() {
            Some((length, level)) if *level == high => *length += seconds,
            _ => self.pieces.push((seconds, high)),
        }
    }

    /// An edge, and then the new level for a time: a pulse as TZX counts them.
    fn pulse(&mut self, seconds: f64) {
        self.hold(seconds, !self.high);
    }

    /// A whole wave: high for half its time, then low.
    fn wave(&mut self, seconds: f64) {
        self.hold(seconds / 2.0, true);
        self.hold(seconds / 2.0, false);
    }

    /// Quiet, as TZX makes it: the last pulse is ended by an edge and a millisecond of the other level, so that
    /// a loader timing it sees it end, and then it's low. Quiet straight after quiet just carries on.
    fn pause(&mut self, seconds: f64) {
        if seconds <= 0.0 {
            return;
        }
        if !self.pieces.is_empty() && !self.quiet {
            self.pulse(seconds.min(0.001));
            self.hold(seconds - 0.001, false);
        } else {
            self.hold(seconds, false);
        }
        self.quiet = true;
    }

    pub fn seconds(&self) -> f64 {
        self.pieces.iter().map(|(s, _)| s).sum()
    }

    /// The lengths of each stretch, in seconds: what a tape's reader times.
    #[cfg(test)]
    pub fn lengths(&self) -> Vec<f64> {
        self.pieces.iter().map(|(s, _)| *s).collect()
    }

    /// Each stretch's length and whether it's high.
    #[cfg(test)]
    pub fn pieces(&self) -> &[(f64, bool)] {
        &self.pieces
    }

    /// The samples, each the average of the levels over its time, so an edge between two samples falls between
    /// them in the sound too, rather than being moved to the nearer one.
    fn samples(&self) -> Vec<u8> {
        let rate = f64::from(RATE);
        let mut out = Vec::with_capacity((self.seconds() * rate) as usize + 2);
        let (mut sum, mut filled) = (0.0f64, 0.0f64);
        let sample = |sum: f64| (128.0 + sum * LOUDNESS).round().clamp(0.0, 255.0) as u8;
        for &(seconds, high) in &self.pieces {
            let level = if high { 1.0 } else { -1.0 };
            let mut left = seconds * rate;
            while left > 1e-9 {
                let take = left.min(1.0 - filled);
                sum += level * take;
                filled += take;
                left -= take;
                if filled >= 1.0 - 1e-9 {
                    out.push(sample(sum));
                    (sum, filled) = (0.0, 0.0);
                }
            }
        }
        if filled > 1e-9 {
            out.push(sample(sum / filled));
        }
        out
    }

    /// The signal as a mono, 8-bit WAV file.
    pub fn wav(&self) -> Vec<u8> {
        let samples = self.samples();
        let length = samples.len() as u32;
        let mut wav = Vec::with_capacity(44 + samples.len());
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + length).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&RATE.to_le_bytes());
        wav.extend_from_slice(&RATE.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&length.to_le_bytes());
        wav.extend_from_slice(&samples);
        wav
    }
}

/// A tape made into sound, the machine it's for, and anything on it that couldn't be played.
pub struct TapeSound {
    pub machine: &'static str,
    pub signal: Signal,
    pub notes: Vec<String>,
}

/// The endings of the files tape sound is made from.
pub const ENDINGS: [&str; 11] = ["tap", "tzx", "cdt", "p", "81", "o", "80", "prg", "uef", "cas", "bbc"];

/// Whether a file of this name may be made into tape sound (its bytes decide when it's tried).
pub fn may_make(name: &str) -> bool {
    let ending = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    ENDINGS.contains(&ending.as_str())
}

/// The sound of the tape a file is, or that a program would be saved on.
pub fn make(bytes: &[u8], name: &str) -> Result<TapeSound, String> {
    let ending = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let stem = stem.rsplit(['/', '\\']).next().unwrap_or(stem);
    let mut notes = Vec::new();
    let mut signal = Signal::default();
    let machine = if bytes.starts_with(b"ZXTape!\x1A") {
        tzx(&mut signal, bytes, &mut notes)?;
        if ending == "cdt" { "Amstrad CPC" } else { "ZX Spectrum" }
    } else if bytes.starts_with(b"C64-TAPE-RAW") || bytes.starts_with(b"C16-TAPE-RAW") {
        commodore_tap(&mut signal, bytes, &mut notes)?
    } else if bytes.starts_with(b"UEF File!\0") || (ending == "uef" && bytes.starts_with(&[0x1F, 0x8B])) {
        let unpacked;
        let bytes = if bytes.starts_with(&[0x1F, 0x8B]) {
            unpacked = super::amiga::gunzip(&bytes.to_vec())?;
            &unpacked[..]
        } else {
            bytes
        };
        uef(&mut signal, bytes, &mut notes)?;
        "BBC Micro or Electron"
    } else if bytes.starts_with(&MSX_MARK) {
        msx(&mut signal, bytes);
        "MSX"
    } else {
        match ending.as_str() {
            "tap" => {
                spectrum_tap(&mut signal, bytes)?;
                "ZX Spectrum"
            }
            "p" | "81" => {
                zx81(&mut signal, bytes, Some(stem))?;
                "ZX81"
            }
            "o" | "80" => {
                zx81(&mut signal, bytes, None)?;
                "ZX80"
            }
            "prg" => {
                commodore_prg(&mut signal, bytes, stem)?;
                "Commodore 64"
            }
            "bbc" => {
                bbc_program(&mut signal, bytes, stem)?;
                "BBC Micro or Electron"
            }
            "cas" => return Err(crate::i18n::tr("Only MSX cassettes can be made into tape sound, and this .cas isn't one: it doesn't start with an MSX block's marker.").into()),
            _ => return Err(crate::i18n::tr("Limoniter doesn't know how to make this file into tape sound.").into()),
        }
    };
    if signal.pieces.is_empty() {
        return Err(crate::i18n::tr("There's nothing on this tape to make a sound of.").into());
    }
    // A moment of quiet at the end, so the last edge isn't cut off.
    signal.pause(1.0);
    Ok(TapeSound { machine, signal, notes })
}

// ---------- The Spectrum: TAP and TZX ----------

/// A block as the Spectrum's ROM saves it: a pilot tone, two sync pulses, its bytes and a pause.
fn spectrum_block(signal: &mut Signal, data: &[u8], pause_ms: u32) {
    let pilot = if data.first().is_some_and(|flag| *flag < 0x80) { 8063 } else { 3223 };
    turbo_block(signal, data, [2168, 667, 735, 855, 1710], pilot, 8, pause_ms);
}

/// A block with timings of its own: the pilot pulse, the two sync pulses, and a 0 and a 1 bit's pulses, in
/// T-states; how many pilot pulses; and how many bits of the last byte are used.
fn turbo_block(signal: &mut Signal, data: &[u8], [pilot, sync1, sync2, zero, one]: [u32; 5], pilots: u32, used: u8, pause_ms: u32) {
    for _ in 0..pilots {
        signal.pulse(tstates(pilot));
    }
    signal.pulse(tstates(sync1));
    signal.pulse(tstates(sync2));
    pure_data(signal, data, zero, one, used);
    signal.pause(f64::from(pause_ms) / 1000.0);
}

/// Bytes as bits, highest first, each two pulses of its length.
fn pure_data(signal: &mut Signal, data: &[u8], zero: u32, one: u32, used: u8) {
    for (i, byte) in data.iter().enumerate() {
        let bits = if i + 1 == data.len() { used.clamp(1, 8) } else { 8 };
        for bit in 0..bits {
            let length = if byte & (0x80 >> bit) != 0 { one } else { zero };
            signal.pulse(tstates(length));
            signal.pulse(tstates(length));
        }
    }
}

fn spectrum_tap(signal: &mut Signal, bytes: &[u8]) -> Result<(), String> {
    let mut at = 0;
    let mut blocks = Vec::new();
    while at < bytes.len() {
        let Some(length) = bytes.get(at..at + 2).map(|b| usize::from(u16::from_le_bytes([b[0], b[1]]))) else {
            return Err(crate::i18n::tr("This isn't a Spectrum tape: it ends partway through a block's length.").into());
        };
        let Some(block) = bytes.get(at + 2..at + 2 + length) else {
            return Err(crate::i18n::tr("This isn't a Spectrum tape: a block goes on past the end of the file.").into());
        };
        blocks.push(block);
        at += 2 + length;
    }
    // A Jupiter Ace tape is laid out alike, but its header is 26 bytes long and its ROM's timings are its own.
    if blocks.first().is_some_and(|b| b.len() == 26) && blocks.first().is_some_and(|b| b.iter().fold(0, |a, x| a ^ x) == 0) && !blocks.iter().any(|b| b.len() == 19) {
        return Err(crate::i18n::tr("This looks like a Jupiter Ace tape, whose sound Limoniter doesn't make yet.").into());
    }
    for block in blocks {
        spectrum_block(signal, block, 1000);
    }
    Ok(())
}

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    bytes.get(at..at + 2).map(|b| u32::from(u16::from_le_bytes([b[0], b[1]])))
}

fn triple(bytes: &[u8], at: usize) -> Option<usize> {
    bytes.get(at..at + 3).map(|b| usize::from(b[0]) | usize::from(b[1]) << 8 | usize::from(b[2]) << 16)
}

fn long(bytes: &[u8], at: usize) -> Option<usize> {
    bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
}

/// One TZX block: its ID, where its body starts and where the block after it starts.
struct TzxBlock {
    id: u8,
    body: usize,
    next: usize,
}

/// The blocks of a TZX file, each found from its own length, as the format gives it for each kind.
fn tzx_blocks(bytes: &[u8]) -> Result<Vec<TzxBlock>, String> {
    let short = || crate::i18n::tr("The tape ends partway through a block.").to_string();
    let mut blocks = Vec::new();
    let mut at = 10;
    while at < bytes.len() {
        let id = bytes[at];
        let body = at + 1;
        let need = |n: Option<usize>| n.ok_or_else(short);
        let length = match id {
            0x10 => 4 + need(word(bytes, body + 2).map(|w| w as usize))?,
            0x11 => 18 + need(triple(bytes, body + 15))?,
            0x12 => 4,
            0x13 => 1 + 2 * usize::from(*bytes.get(body).ok_or_else(short)?),
            0x14 => 10 + need(triple(bytes, body + 7))?,
            0x15 => 8 + need(triple(bytes, body + 5))?,
            0x20 | 0x23 | 0x24 => 2,
            0x21 | 0x30 => 1 + usize::from(*bytes.get(body).ok_or_else(short)?),
            0x22 | 0x25 | 0x27 => 0,
            0x26 => 2 + 2 * need(word(bytes, body).map(|w| w as usize))?,
            0x28 | 0x32 => 2 + need(word(bytes, body).map(|w| w as usize))?,
            0x31 => 2 + usize::from(*bytes.get(body + 1).ok_or_else(short)?),
            0x33 => 1 + 3 * usize::from(*bytes.get(body).ok_or_else(short)?),
            0x34 => 8,
            0x35 => 20 + need(long(bytes, body + 16))?,
            0x40 => 4 + need(triple(bytes, body + 1))?,
            0x5A => 9,
            // Every other kind, from version 1.10 on, starts with its length.
            _ => 4 + need(long(bytes, body))?,
        };
        let next = body + length;
        if next > bytes.len() {
            return Err(short());
        }
        blocks.push(TzxBlock { id, body, next });
        at = next;
    }
    Ok(blocks)
}

fn tzx(signal: &mut Signal, bytes: &[u8], notes: &mut Vec<String>) -> Result<(), String> {
    let blocks = tzx_blocks(bytes)?;
    let mut loops: Vec<(usize, u32)> = Vec::new();
    let mut skipped: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < blocks.len() {
        let TzxBlock { id, body, next } = blocks[i];
        let b = &bytes[body..next];
        let w = |at: usize| word(b, at).unwrap_or(0);
        match id {
            0x10 => spectrum_block(signal, &b[4..], w(0)),
            0x11 => turbo_block(signal, &b[18..], [w(0), w(2), w(4), w(6), w(8)], w(10), b[12], w(13)),
            0x12 => {
                for _ in 0..w(2) {
                    signal.pulse(tstates(w(0)));
                }
            }
            0x13 => {
                for p in 0..usize::from(b[0]) {
                    signal.pulse(tstates(w(1 + 2 * p)));
                }
            }
            0x14 => {
                pure_data(signal, &b[10..], w(0), w(2), b[4]);
                signal.pause(f64::from(w(5)) / 1000.0);
            }
            0x15 => {
                // A sample a bit, high for 1, each so many T-states long.
                let (each, data) = (tstates(w(0)), &b[8..]);
                for (k, byte) in data.iter().enumerate() {
                    let bits = if k + 1 == data.len() { b[4].clamp(1, 8) } else { 8 };
                    for bit in 0..bits {
                        signal.hold(each, byte & (0x80 >> bit) != 0);
                    }
                }
                signal.pause(f64::from(w(2)) / 1000.0);
            }
            0x19 => generalised(signal, b),
            // A pause of 0 is "stop the tape": a few seconds' quiet gives time to stop it.
            0x20 => signal.pause(if w(0) == 0 { 3.0 } else { f64::from(w(0)) / 1000.0 }),
            0x24 => loops.push((i, w(0))),
            0x25 => {
                if let Some((start, left)) = loops.last_mut() {
                    if *left > 1 {
                        *left -= 1;
                        i = *start + 1;
                        continue;
                    }
                    loops.pop();
                }
            }
            0x2B => {
                let high = b.get(4).is_some_and(|l| *l != 0);
                signal.high = high;
            }
            // Names of groups, descriptions, messages, archive information, hardware, custom information and glue,
            // and "stop if a 48K Spectrum", which a sound can't do: nothing to play.
            0x21 | 0x22 | 0x2A | 0x30..=0x35 | 0x5A => {}
            _ => {
                if !skipped.contains(&id) {
                    skipped.push(id);
                }
            }
        }
        i += 1;
    }
    for id in skipped {
        let what = match id {
            0x18 => crate::i18n::tr("a CSW recording"),
            0x23 => crate::i18n::tr("a jump to another block"),
            0x26 | 0x27 => crate::i18n::tr("a call of other blocks"),
            0x28 => crate::i18n::tr("a choice of blocks"),
            _ => crate::i18n::tr("a kind it doesn't know"),
        };
        notes.push(crate::i18n::tf!("A block of type &{id:02X} ({what}) was left out."));
    }
    Ok(())
}

/// A generalised data block (0x19): its pilot and sync from a table of symbols each repeated, then its data, each
/// symbol a few bits long, from a table of its own. A symbol's flags say what its first pulse does to the level.
fn generalised(signal: &mut Signal, b: &[u8]) {
    let w = |at: usize| word(b, at).unwrap_or(0);
    let (pause, pilot_count) = (w(4), long(b, 6).unwrap_or(0));
    let (pilot_pulses, pilot_symbols) = (usize::from(b.get(10).copied().unwrap_or(0)), b.get(11).map_or(256, |&n| if n == 0 { 256 } else { usize::from(n) }));
    let data_count = long(b, 12).unwrap_or(0);
    let (data_pulses, data_symbols) = (usize::from(b.get(16).copied().unwrap_or(0)), b.get(17).map_or(256, |&n| if n == 0 { 256 } else { usize::from(n) }));
    let play = |signal: &mut Signal, table: usize, pulses: usize, symbol: usize| {
        let at = table + symbol * (2 * pulses + 1);
        let Some(&flags) = b.get(at) else { return };
        for p in 0..pulses {
            let length = w(at + 1 + 2 * p);
            if length == 0 {
                break;
            }
            let seconds = tstates(length);
            match (p, flags & 3) {
                (0, 1) => signal.hold(seconds, signal.high),
                (0, 2) => signal.hold(seconds, false),
                (0, 3) => signal.hold(seconds, true),
                _ => signal.pulse(seconds),
            }
        }
    };
    let mut at = 18;
    if pilot_count > 0 {
        let table = at;
        at += pilot_symbols * (2 * pilot_pulses + 1);
        for _ in 0..pilot_count {
            let (symbol, repeats) = (usize::from(b.get(at).copied().unwrap_or(0)), w(at + 1));
            for _ in 0..repeats {
                play(signal, table, pilot_pulses, symbol);
            }
            at += 3;
        }
    }
    if data_count > 0 {
        let table = at;
        at += data_symbols * (2 * data_pulses + 1);
        let bits = (usize::BITS - (data_symbols - 1).leading_zeros()).max(1) as usize;
        for n in 0..data_count {
            let mut symbol = 0;
            for k in 0..bits {
                let bit = n * bits + k;
                let set = b.get(at + bit / 8).is_some_and(|byte| byte & (0x80 >> (bit % 8)) != 0);
                symbol = symbol << 1 | usize::from(set);
            }
            play(signal, table, data_pulses, symbol);
        }
    }
    signal.pause(f64::from(pause) / 1000.0);
}

// ---------- The ZX81 and ZX80 ----------

/// A ZX81 program (from &4009) or a ZX80 one (from &4000), with the name a ZX81 tape starts with.
fn zx81(signal: &mut Signal, bytes: &[u8], name: Option<&str>) -> Result<(), String> {
    if bytes.is_empty() {
        return Err(crate::i18n::tr("The program is empty.").into());
    }
    let mut sent = Vec::new();
    if let Some(name) = name {
        sent = zx81_name(name);
    }
    sent.extend_from_slice(bytes);
    // The ROM's SAVE waits five seconds before it starts.
    signal.pause(5.0);
    for byte in sent {
        for bit in 0..8 {
            let pulses = if byte & (0x80 >> bit) != 0 { 9 } else { 4 };
            for _ in 0..pulses {
                signal.hold(tstates(530), true);
                signal.hold(tstates(520), false);
            }
            signal.hold(tstates(4689), false);
        }
    }
    Ok(())
}

/// A name in the ZX81's own characters, the last with its top bit set to mark the end, as SAVE "NAME" writes it.
fn zx81_name(name: &str) -> Vec<u8> {
    let set = crate::charset::known("zx81");
    let mut out: Vec<u8> = set.tables.first().map_or_else(Vec::new, |table| {
        name.chars().filter_map(|c| table.byte_of(c.to_ascii_uppercase())).filter(|b| (0x01..0x40).contains(b)).take(32).collect()
    });
    if out.is_empty() {
        // "TAPE", should the name have nothing the ZX81 can show.
        out = vec![0x39, 0x26, 0x35, 0x2A];
    }
    if let Some(last) = out.last_mut() {
        *last |= 0x80;
    }
    out
}

// ---------- Commodore: the KERNAL's format, and TAP files ----------

/// The C64's clock, PAL, that a TAP file's lengths are in unless it says otherwise.
const C64_PAL: f64 = 985_248.0;

/// The three waves the KERNAL writes, in seconds: short, medium and long.
const KERNAL_WAVES: [f64; 3] = [1.0 / 2840.0, 1.0 / 1953.0, 1.0 / 1488.0];

/// A program and its header as the KERNAL writes them, as the lengths of each wave: 0 short, 1 medium, 2 long.
pub(super) fn kernal_waves(program: &[u8], name: &str) -> Result<Vec<u8>, String> {
    if program.len() < 3 {
        return Err(crate::i18n::tr("A program file starts with where it loads, and this one is too short to have one.").into());
    }
    let start = u16::from_le_bytes([program[0], program[1]]);
    let data = &program[2..];
    let end = usize::from(start) + data.len();
    if end > 0x10000 {
        return Err(crate::i18n::tr("The program goes past the end of the memory it loads into.").into());
    }
    let mut header = vec![0x20u8; 192];
    // 1 is a program that loads where BASIC is; 3 one that loads where it says.
    header[0] = if start == 0x0801 { 1 } else { 3 };
    header[1..3].copy_from_slice(&start.to_le_bytes());
    header[3..5].copy_from_slice(&(end as u16).to_le_bytes());
    let petscii: Vec<u8> = name.chars().map(|c| c.to_ascii_uppercase()).filter(|c| (' '..='_').contains(c)).map(|c| c as u8).take(16).collect();
    header[5..5 + petscii.len()].copy_from_slice(&petscii);
    let mut waves = Vec::new();
    kernal_block(&mut waves, &header, 0x6A00);
    kernal_block(&mut waves, data, 0x1A00);
    Ok(waves)
}

/// One block: its leader, then the block, the gap, the block again and the trailer.
fn kernal_block(waves: &mut Vec<u8>, data: &[u8], leader: usize) {
    let checksum = data.iter().fold(0u8, |a, b| a ^ b);
    waves.extend(std::iter::repeat_n(0, leader));
    for (countdown, gap) in [(0x89u8, 0x4F), (0x09, 0x4E)] {
        for byte in (0..9).map(|k| countdown - k).chain(data.iter().copied()).chain([checksum]) {
            // A byte's marker, its bits lowest first, then the bit that makes the count of ones odd.
            waves.extend([2, 1]);
            let check = 1 ^ (byte.count_ones() as u8 & 1);
            for bit in (0..8).map(|k| byte >> k & 1).chain([check]) {
                waves.extend(if bit == 1 { [1, 0] } else { [0, 1] });
            }
        }
        // The end of the data, and then the gap before the copy (or the trailer after it).
        waves.extend([2, 0]);
        waves.extend(std::iter::repeat_n(0, gap));
    }
}

fn commodore_prg(signal: &mut Signal, bytes: &[u8], name: &str) -> Result<(), String> {
    signal.pause(1.0);
    for wave in kernal_waves(bytes, name)? {
        signal.wave(KERNAL_WAVES[usize::from(wave)]);
    }
    Ok(())
}

fn commodore_tap(signal: &mut Signal, bytes: &[u8], notes: &mut Vec<String>) -> Result<&'static str, String> {
    if bytes.len() < 0x14 {
        return Err(crate::i18n::tr("The tape image is too short to have its header.").into());
    }
    let (version, machine, video) = (bytes[12], bytes[13], bytes[14]);
    let (clock, said) = match (machine, video) {
        (1, 0) => (1_108_405.0, "VIC-20"),
        (1, _) => (1_022_727.0, "VIC-20"),
        (2, 0) => (886_724.0, "Commodore 16 or Plus/4"),
        (2, _) => (894_886.0, "Commodore 16 or Plus/4"),
        (_, 0) => (C64_PAL, "Commodore 64"),
        _ => (1_022_727.0, "Commodore 64"),
    };
    if version > 2 {
        notes.push(crate::i18n::tf!("It's TAP version {version}; only 0, 1 and 2 are known, so it was played as version 1."));
    }
    let mut i = 0x14;
    while i < bytes.len() {
        let cycles = match bytes[i] {
            0 if version == 0 => 256 * 8,
            0 => {
                let cycles = triple(bytes, i + 1).unwrap_or(0);
                i += 3;
                cycles
            }
            b => usize::from(b) * 8,
        };
        let seconds = cycles as f64 / clock;
        // Version 2 gives each half of a wave; the others give whole waves.
        if version == 2 { signal.pulse(seconds) } else { signal.wave(seconds) }
        i += 1;
    }
    Ok(said)
}

// ---------- Acorn: UEF, and BBC BASIC saved by the cassette filing system ----------

/// What a UEF's data is played at: the carrier's frequency is twice the base, and a 0 bit is one wave of the base.
struct Acorn {
    base: f64,
    /// Waves of the base frequency in a bit: 1 at 1200 baud, 4 at 300.
    per_bit: u32,
}

impl Acorn {
    fn bit(&self, signal: &mut Signal, one: bool) {
        for _ in 0..self.per_bit {
            if one {
                signal.wave(0.5 / self.base);
                signal.wave(0.5 / self.base);
            } else {
                signal.wave(1.0 / self.base);
            }
        }
    }

    fn carrier(&self, signal: &mut Signal, waves: u32) {
        for _ in 0..waves {
            signal.wave(0.5 / self.base);
        }
    }

    /// A byte as it goes on the tape: a 0 start bit, its bits lowest first, a parity bit if wanted, and stop bits.
    fn byte(&self, signal: &mut Signal, value: u8, bits: u8, parity: u8, stops: u8) {
        self.bit(signal, false);
        for k in 0..bits.min(8) {
            self.bit(signal, value >> k & 1 != 0);
        }
        let ones = (value & (((1u16 << bits.min(8)) - 1) as u8)).count_ones();
        match parity {
            b'E' => self.bit(signal, !ones.is_multiple_of(2)),
            b'O' => self.bit(signal, ones.is_multiple_of(2)),
            _ => {}
        }
        for _ in 0..stops {
            self.bit(signal, true);
        }
    }
}

fn uef(signal: &mut Signal, bytes: &[u8], notes: &mut Vec<String>) -> Result<(), String> {
    let mut acorn = Acorn { base: 1200.0, per_bit: 1 };
    let mut skipped: Vec<u16> = Vec::new();
    let mut at = 12;
    while at + 6 <= bytes.len() {
        let id = u16::from_le_bytes([bytes[at], bytes[at + 1]]);
        let length = long(bytes, at + 2).unwrap_or(0);
        let body = &bytes[at + 6..(at + 6 + length).min(bytes.len())];
        let w = |k: usize| word(body, k).unwrap_or(0);
        let float = |k: usize| body.get(k..k + 4).map_or(0.0, |f| f64::from(f32::from_le_bytes([f[0], f[1], f[2], f[3]])));
        match id {
            0x0100 => {
                for &b in body {
                    acorn.byte(signal, b, 8, b'N', 1);
                }
            }
            0x0102 => {
                // Bits, lowest first in each byte; the first byte says how many of the last byte's aren't used.
                let unused = usize::from(body.first().copied().unwrap_or(0));
                let total = (body.len().saturating_sub(1) * 8).saturating_sub(unused);
                for k in 0..total {
                    acorn.bit(signal, body[1 + k / 8] >> (k % 8) & 1 != 0);
                }
            }
            0x0104 if body.len() >= 3 => {
                // A negative number of stop bits means an extra short wave after them, which is as good as one more.
                let stops = (body[2] as i8).unsigned_abs();
                for &b in &body[3..] {
                    acorn.byte(signal, b, body[0], body[1], stops);
                }
            }
            0x0110 => acorn.carrier(signal, w(0)),
            0x0111 => {
                acorn.carrier(signal, w(0));
                acorn.byte(signal, 0xAA, 8, b'N', 1);
                acorn.carrier(signal, w(2));
            }
            0x0112 => signal.pause(f64::from(w(0)) / (2.0 * acorn.base)),
            0x0113 if float(0) > 0.0 => acorn.base = float(0),
            0x0116 => signal.pause(float(0)),
            0x0117 => acorn.per_bit = if w(0) == 300 { 4 } else { 1 },
            // What the tape is, its position markers and which side: nothing to play.
            0x0000..=0x00FF | 0x0120 | 0x0130 | 0x0131 => {}
            _ => {
                if !skipped.contains(&id) {
                    skipped.push(id);
                }
            }
        }
        at += 6 + length;
    }
    for id in skipped {
        notes.push(crate::i18n::tf!("A chunk of type &{id:04X}, which isn't played, was left out."));
    }
    Ok(())
}

/// A tokenised BBC BASIC program, each line &0D, its number and length, then its text, ending &0D &FF.
fn is_bbc_basic(bytes: &[u8]) -> bool {
    let mut at = 0;
    loop {
        match bytes.get(at..at + 2) {
            Some([0x0D, 0xFF]) => return at + 2 >= bytes.len().saturating_sub(16),
            Some([0x0D, _]) => {
                let Some(&length) = bytes.get(at + 3) else { return false };
                if length < 4 {
                    return false;
                }
                at += usize::from(length);
            }
            _ => return false,
        }
    }
}

/// The cassette filing system's blocks for a file: each `*`, then the name and a zero, where it loads and runs,
/// the block's number, its length, its flag (&80 on the last) and four spare bytes, their CRC, then up to 256 bytes
/// of data and its CRC.
pub(super) fn cfs_blocks(name: &str, data: &[u8], load: u32, exec: u32) -> Vec<Vec<u8>> {
    let mut title: Vec<u8> = name.bytes().filter(u8::is_ascii_alphanumeric).take(10).collect();
    if title.is_empty() {
        title = b"PROGRAM".to_vec();
    }
    let chunks: Vec<&[u8]> = if data.is_empty() { vec![&[][..]] } else { data.chunks(256).collect() };
    let last = chunks.len() - 1;
    chunks
        .iter()
        .enumerate()
        .map(|(number, chunk)| {
            let mut header = title.clone();
            header.push(0);
            header.extend_from_slice(&load.to_le_bytes());
            header.extend_from_slice(&exec.to_le_bytes());
            header.extend_from_slice(&(number as u16).to_le_bytes());
            header.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
            header.push(if number == last { 0x80 } else { 0 });
            header.extend_from_slice(&[0; 4]);
            let mut block = vec![b'*'];
            block.extend_from_slice(&header);
            block.extend_from_slice(&super::acorn::tape_crc(&header).to_be_bytes());
            if !chunk.is_empty() {
                block.extend_from_slice(chunk);
                block.extend_from_slice(&super::acorn::tape_crc(chunk).to_be_bytes());
            }
            block
        })
        .collect()
}

fn bbc_program(signal: &mut Signal, bytes: &[u8], name: &str) -> Result<(), String> {
    if !is_bbc_basic(bytes) {
        return Err(crate::i18n::tr("This isn't a tokenised BBC BASIC program, so it can't be saved as a BBC tape.").into());
    }
    let acorn = Acorn { base: 1200.0, per_bit: 1 };
    signal.pause(1.0);
    // BASIC's own addresses: it loads where PAGE is when it's loaded, and runs in BASIC.
    for (k, block) in cfs_blocks(name, bytes, 0xFFFF_0E00, 0xFFFF_8023).iter().enumerate() {
        // The first block's carrier is 5.1 seconds long, the rest's 0.9, as the MOS saves them.
        acorn.carrier(signal, if k == 0 { 12240 } else { 2160 });
        for &b in block {
            acorn.byte(signal, b, 8, b'N', 1);
        }
    }
    acorn.carrier(signal, 2160);
    Ok(())
}

// ---------- MSX ----------

/// What stands in a .cas file for the tone before each block.
const MSX_MARK: [u8; 8] = [0x1F, 0xA6, 0xDE, 0xBA, 0xCC, 0x13, 0x7D, 0x74];

/// Waves of 2400 Hz in the BIOS's long and short tones at 1200 baud.
const MSX_LONG_TONE: u32 = 15360;
const MSX_SHORT_TONE: u32 = 3840;

fn msx(signal: &mut Signal, bytes: &[u8]) {
    // The blocks, each starting at a marker on an eight-byte boundary.
    let mut starts: Vec<usize> = (0..bytes.len()).step_by(8).filter(|&at| bytes[at..].starts_with(&MSX_MARK)).collect();
    starts.push(bytes.len());
    for pair in starts.windows(2) {
        let block = &bytes[pair[0] + 8..pair[1]];
        // A file's header is ten bytes of its kind (BASIC, ASCII or binary) and its name.
        let header = block.len() >= 10 && matches!(block[0], 0xD3 | 0xEA | 0xD0) && block[..10].iter().all(|b| *b == block[0]);
        signal.pause(if header { 2.0 } else { 1.0 });
        for _ in 0..if header { MSX_LONG_TONE } else { MSX_SHORT_TONE } {
            signal.wave(1.0 / 2400.0);
        }
        for &byte in block {
            signal.wave(1.0 / 1200.0);
            for k in 0..8 {
                msx_bit(signal, byte >> k & 1 != 0);
            }
            msx_bit(signal, true);
            msx_bit(signal, true);
        }
    }
}

fn msx_bit(signal: &mut Signal, one: bool) {
    if one {
        signal.wave(1.0 / 2400.0);
        signal.wave(1.0 / 2400.0);
    } else {
        signal.wave(1.0 / 1200.0);
    }
}

#[cfg(test)]
mod tests;
