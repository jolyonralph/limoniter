//! Commodore tape images (.tap, "C64-TAPE-RAW"): the pulses a Datasette reads, as lengths in clock cycles. The
//! KERNAL's own tape format is decoded from them: each byte is a marker pulse pair, eight bits and a parity bit
//! made of short, medium and long pulses; each block is sent twice, after a countdown, with a checksum; a file is a
//! 192-byte header (its type, start and end addresses and name) and then its data. Turbo loaders' own formats
//! aren't decoded: where a tape goes on in one, it's said.

use super::{Entry, InfoSection, Source, Volume, tidy_name};
use std::sync::Arc;

/// A tape is read whole to be decoded; one longer than this isn't a real tape.
const MOST_TAPE_BYTES: u64 = 64 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pulse {
    Short,
    Medium,
    Long,
    Other,
}

/// A KERNAL block as read from the pulses: the countdown it started with, its bytes after that (the checksum
/// last), whether every byte's parity was right, and where on the tape it starts and ends (as pulse numbers).
#[derive(Clone, Debug)]
struct Block {
    repeat: bool,
    bytes: Vec<u8>,
    parity_ok: bool,
    first_pulse: usize,
    last_pulse: usize,
}

impl Block {
    fn payload(&self) -> &[u8] {
        &self.bytes[..self.bytes.len().saturating_sub(1)]
    }

    fn checksum_ok(&self) -> bool {
        self.parity_ok && self.bytes.len() >= 2 && self.payload().iter().fold(0u8, |a, b| a ^ b) == *self.bytes.last().unwrap_or(&0)
    }
}

/// One file found on the tape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TapeFile {
    pub name: String,
    /// The header's type: 1 a relocatable program (BASIC), 3 an absolute program, 4 a data file.
    pub kind: u8,
    pub start: u16,
    pub end: u16,
    pub data: Vec<u8>,
    /// Whether the data was read with a right checksum (from either copy).
    pub ok: bool,
    /// Whether any data was found for the header at all.
    pub has_data: bool,
    /// Where its header starts on the tape, in bytes from the start of the image.
    pub offset: u64,
}

impl TapeFile {
    fn kind_name(&self) -> &'static str {
        match self.kind {
            1 => "PRG (BASIC)",
            3 => "PRG",
            4 => "SEQ",
            _ => "?",
        }
    }
}

pub struct C64Tape {
    pub version: u8,
    pub machine: &'static str,
    pub files: Vec<TapeFile>,
    pub pulses: usize,
    /// The short pulse's length the tape was read with, in cycles.
    pub short: u32,
    notes: Vec<String>,
}

pub fn looks_like(head: &[u8]) -> bool {
    head.starts_with(b"C64-TAPE-RAW")
}

/// The pulses' lengths in cycles, and where each starts in the image.
fn pulses(bytes: &[u8], version: u8) -> (Vec<u32>, Vec<u32>) {
    let mut lengths = Vec::with_capacity(bytes.len());
    let mut offsets = Vec::with_capacity(bytes.len());
    let mut i = 0x14;
    while i < bytes.len() {
        let at = i as u32;
        let length = match bytes[i] {
            // Version 0 says only that the pulse was too long to count; later versions give its length.
            0 if version == 0 => 256 * 8,
            0 => {
                let cycles = bytes.get(i + 1..i + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], 0]));
                i += 3;
                cycles
            }
            b => u32::from(b) * 8,
        };
        lengths.push(length);
        offsets.push(at);
        i += 1;
    }
    // Version 2 (C16 and Plus/4) gives each half of a wave: two make a pulse as the others count them.
    if version == 2 {
        let whole: Vec<u32> = lengths.chunks(2).map(|c| c.iter().sum()).collect();
        let starts: Vec<u32> = offsets.chunks(2).map(|c| c[0]).collect();
        return (whole, starts);
    }
    (lengths, offsets)
}

/// The lengths the short pulse may be, from the leaders: the lengths of long runs of pulses that are all alike and
/// as long as the KERNAL's short pulses are on any Commodore's tape, commonest first. A turbo loader's leader can
/// be commoner than the KERNAL's, so each is tried.
fn short_pulses(lengths: &[u32]) -> Vec<u32> {
    let mut counts: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
    let mut i = 0;
    while i < lengths.len() {
        let first = lengths[i];
        let mut j = i;
        while j < lengths.len() && lengths[j].abs_diff(first) * 8 <= first {
            j += 1;
        }
        if j - i >= 64 && (0x20 * 8..=0x40 * 8).contains(&first) {
            let mut run: Vec<u32> = lengths[i..j].to_vec();
            run.sort_unstable();
            *counts.entry(run[run.len() / 2] / 8 * 8).or_default() += j - i;
        }
        i = j.max(i + 1);
    }
    let mut found: Vec<(u32, usize)> = counts.into_iter().collect();
    found.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    found.into_iter().take(8).map(|(length, _)| length).collect()
}

fn classify(length: u32, short: u32) -> Pulse {
    let s = short as f64;
    let l = f64::from(length);
    if l < s * 0.6 || l > s * 2.4 {
        Pulse::Other
    } else if l < s * 1.19 {
        Pulse::Short
    } else if l < s * 1.59 {
        Pulse::Medium
    } else {
        Pulse::Long
    }
}

/// The KERNAL's blocks, as found in the pulses.
fn blocks(kinds: &[Pulse]) -> Vec<Block> {
    use Pulse::*;
    let mut out = Vec::new();
    let mut i = 0;
    let at = |i: usize| kinds.get(i).copied().unwrap_or(Other);
    // A byte at i: a long-medium marker, then nine short-medium (0) or medium-short (1) pairs.
    let byte_at = |i: usize| -> Option<(u8, bool)> {
        if at(i) != Long || at(i + 1) != Medium {
            return None;
        }
        let mut value = 0u16;
        for bit in 0..9 {
            let pair = (at(i + 2 + bit * 2), at(i + 3 + bit * 2));
            let one = match pair {
                (Short, Medium) => false,
                (Medium, Short) => true,
                _ => return None,
            };
            if one {
                value |= 1 << bit;
            }
        }
        let data = value as u8;
        // The check bit makes the count of ones odd.
        let parity_ok = (data.count_ones() + u32::from(value >> 8)) % 2 == 1;
        Some((data, parity_ok))
    };
    while i < kinds.len() {
        let Some((first, parity)) = byte_at(i) else {
            i += 1;
            continue;
        };
        // A block starts with its countdown: $89 to $81 the first time, $09 to $01 the second.
        if first != 0x89 && first != 0x09 {
            i += 1;
            continue;
        }
        let start = i;
        let mut bytes = vec![first];
        let mut parity_ok = parity;
        let mut j = i + 20;
        loop {
            if at(j) == Long && at(j + 1) == Short {
                j += 2;
                break;
            }
            match byte_at(j) {
                Some((b, p)) => {
                    bytes.push(b);
                    parity_ok &= p;
                    j += 20;
                }
                None => break,
            }
        }
        let countdown_ok = bytes.len() >= 9 && (0..9).all(|k| bytes[k] == first - k as u8);
        if countdown_ok {
            out.push(Block { repeat: first == 0x09, bytes: bytes[9..].to_vec(), parity_ok, first_pulse: start, last_pulse: j });
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

impl C64Tape {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        if source.len() > MOST_TAPE_BYTES {
            return Err(crate::i18n::tr("The tape image is too long to be a real tape.").into());
        }
        let bytes = source.read_vec(0, source.len() as usize)?;
        if bytes.len() < 0x14 || !looks_like(&bytes) {
            return Err(crate::i18n::tr("This isn't a Commodore tape image: it doesn't start with C64-TAPE-RAW.").into());
        }
        let version = bytes[12];
        let machine = match bytes[13] {
            0 => "C64",
            1 => "VIC-20",
            2 => "C16 or Plus/4",
            _ => "unknown machine",
        };
        let mut notes = Vec::new();
        let said = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) as usize;
        if said != bytes.len() - 0x14 {
            notes.push(crate::i18n::tf!("The header says the pulses take {said} bytes; there are {}.", bytes.len() - 0x14));
        }
        if version > 2 {
            notes.push(crate::i18n::tf!("It's TAP version {version}; only 0, 1 and 2 are known, so it was read as version 1."));
        }
        let (lengths, offsets) = pulses(&bytes, version);
        let mut tape = C64Tape { version, machine, files: Vec::new(), pulses: lengths.len(), short: 0, notes };
        if machine == "C16 or Plus/4" {
            tape.notes.push(crate::i18n::tr("C16 and Plus/4 tapes use their own KERNAL's format, which isn't decoded here.").into());
            return Ok(tape);
        }
        // The short pulse that reads the most of the tape as KERNAL blocks.
        let tried = short_pulses(&lengths).into_iter().map(|short| {
            let kinds: Vec<Pulse> = lengths.iter().map(|l| classify(*l, short)).collect();
            let found = blocks(&kinds);
            (short, kinds, found)
        });
        let Some((short, kinds, found)) = tried.max_by_key(|(_, _, found)| found.iter().map(|b| b.bytes.len()).sum::<usize>()) else {
            tape.notes.push(crate::i18n::tr("No leader of the KERNAL's short pulses was found, so no files could be read: it may be all in a turbo loader's format.").into());
            return Ok(tape);
        };
        tape.short = short;
        // Each block twice: the first copy if it's sound, else the second.
        let mut logical: Vec<(Block, Option<Block>)> = Vec::new();
        for block in found {
            match logical.last_mut() {
                Some((first, second @ None)) if block.repeat && !first.repeat && first.payload().len() == block.payload().len() => *second = Some(block),
                _ => logical.push((block, None)),
            }
        }
        let best = |(first, second): &(Block, Option<Block>)| -> (Vec<u8>, bool, usize) {
            let end = second.as_ref().map_or(first.last_pulse, |s| s.last_pulse.max(first.last_pulse));
            if first.checksum_ok() {
                return (first.payload().to_vec(), true, end);
            }
            match second {
                Some(second) if second.checksum_ok() => (second.payload().to_vec(), true, end),
                _ => (first.payload().to_vec(), false, end),
            }
        };
        let mut last_used = 0usize;
        let mut i = 0;
        while i < logical.len() {
            let (payload, ok, end) = best(&logical[i]);
            let offset = u64::from(offsets.get(logical[i].0.first_pulse).copied().unwrap_or(0));
            last_used = last_used.max(end);
            let header = payload.len() == 192 && matches!(payload[0], 1 | 3 | 4 | 5);
            if !header {
                tape.notes.push(crate::i18n::tf!("A block of {} bytes at offset {offset} has no header before it{}.", payload.len(), if ok { "" } else { ", and its checksum is wrong" }));
                i += 1;
                continue;
            }
            if !ok {
                tape.notes.push(crate::i18n::tf!("The header at offset {offset} was read with a wrong checksum in both copies."));
            }
            let kind = payload[0];
            if kind == 5 {
                tape.notes.push(crate::i18n::tf!("An end-of-tape marker is at offset {offset}."));
                i += 1;
                continue;
            }
            let start = u16::from_le_bytes([payload[1], payload[2]]);
            let end_address = u16::from_le_bytes([payload[3], payload[4]]);
            // Names often start with colour or clear-screen codes, which show nothing: they are left out, as tapclean does.
            let shown: Vec<u8> = payload[5..21].iter().copied().filter(|b| !matches!(b, 0x00..=0x1F | 0x80..=0x9F)).collect();
            let name = super::cbm::petscii_name(&shown).trim_end().to_string();
            let mut file = TapeFile { name, kind, start, end: end_address, data: Vec::new(), ok: false, has_data: false, offset };
            i += 1;
            if kind == 4 {
                // A data file: blocks of 192 bytes, each starting with 2, until the next header.
                let mut all_ok = true;
                while i < logical.len() {
                    let (data, ok, end) = best(&logical[i]);
                    if data.len() != 192 || data[0] != 2 {
                        break;
                    }
                    last_used = last_used.max(end);
                    file.data.extend_from_slice(&data[1..]);
                    file.has_data = true;
                    all_ok &= ok;
                    i += 1;
                }
                // The last block is padded after the data's end with zeros.
                while file.data.last() == Some(&0) {
                    file.data.pop();
                }
                file.ok = all_ok && file.has_data;
            } else if i < logical.len() {
                let (data, ok, end) = best(&logical[i]);
                let header_next = data.len() == 192 && matches!(data[0], 1 | 3 | 4 | 5) && usize::from(end_address.wrapping_sub(start)) != 192;
                if !header_next {
                    last_used = last_used.max(end);
                    let expected = usize::from(end_address.wrapping_sub(start));
                    if data.len() != expected {
                        tape.notes.push(crate::i18n::tf!("{}'s data is {} bytes, where its header says {expected}.", file.name, data.len()));
                    }
                    file.data = data;
                    file.ok = ok;
                    file.has_data = true;
                    i += 1;
                }
            }
            if !file.has_data {
                tape.notes.push(crate::i18n::tf!("{}'s header was found, but not its data: it's probably loaded by a turbo loader.", if file.name.is_empty() { "A file" } else { &file.name }));
            } else if !file.ok {
                tape.notes.push(crate::i18n::tf!("{}'s data was read with a wrong checksum in both copies: some bytes are wrong.", file.name));
            }
            tape.files.push(file);
        }
        // What's after the last KERNAL block and isn't leader or silence is in some other format.
        let rest: Vec<&Pulse> = kinds[last_used.min(kinds.len())..].iter().collect();
        let busy = rest.iter().filter(|k| ***k != Pulse::Short).count();
        if busy > 2000 {
            let from = offsets.get(last_used).copied().unwrap_or(0);
            let mut widths: std::collections::BTreeMap<u32, usize> = std::collections::BTreeMap::new();
            for length in &lengths[last_used.min(lengths.len())..] {
                if *length < 0x100 * 8 {
                    *widths.entry(length / 8).or_default() += 1;
                }
            }
            let mut common: Vec<(u32, usize)> = widths.into_iter().collect();
            common.sort_by_key(|c| std::cmp::Reverse(c.1));
            let tops: Vec<String> = common.iter().take(3).map(|(w, _)| format!("${w:02X}")).collect();
            tape.notes.push(crate::i18n::tf!("From offset {from} on, {} pulses aren't in the KERNAL's format: most likely a turbo loader's, which isn't decoded (its commonest pulses are {} long, in units of 8 cycles).",
                rest.len(),
                tops.join(", ")
            ));
        }
        if tape.files.is_empty() && tape.notes.is_empty() {
            tape.notes.push(crate::i18n::tr("No KERNAL files were found on the tape.").into());
        }
        Ok(tape)
    }
}

impl Volume for C64Tape {
    fn describe(&self) -> String {
        let count = self.files.len();
        {
            let (machine, version) = (&self.machine, self.version);
            if count == 1 { crate::i18n::tf!("{machine} tape (TAP version {version}) \u{B7} 1 file") } else { crate::i18n::tf!("{machine} tape (TAP version {version}) \u{B7} {count} files") }
        }
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        if dir.is_some() {
            return Ok(Vec::new());
        }
        Ok(self
            .files
            .iter()
            .enumerate()
            .map(|(i, file)| {
                let name = if file.name.is_empty() { crate::i18n::tr("(no name)").to_string() } else { tidy_name(&file.name) };
                let size = if file.kind == 4 { file.data.len() as u64 } else { file.data.len() as u64 + 2 };
                let mut entry = Entry::file(name.clone(), size, i as u64);
                entry.kind = file.kind_name().into();
                entry.detail = match (file.kind, file.has_data, file.ok) {
                    (_, false, _) => crate::i18n::tr("header only: no data in the KERNAL's format").into(),
                    (4, _, ok) => format!("data file{}", if ok { "" } else { ", checksum wrong" }),
                    (_, _, ok) => format!("${:04X}-${:04X}{}", file.start, file.end.wrapping_sub(1), if ok { "" } else { ", checksum wrong" }),
                };
                entry.open_name = format!("{name}.{}", if file.kind == 4 { "seq" } else { "prg" });
                entry
            })
            .collect())
    }

    /// A program is given back as a PRG, its load address first, as the machine would save it to disk.
    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let file = self.files.get(entry.location as usize).ok_or(crate::i18n::tr("That file isn't on the tape."))?;
        if !file.has_data {
            return Err(crate::i18n::tf!("Only {}'s header is in the KERNAL's format: its data is in a turbo loader's, which isn't decoded.", file.name));
        }
        if file.kind == 4 {
            return Ok(file.data.clone());
        }
        let mut out = file.start.to_le_bytes().to_vec();
        out.extend_from_slice(&file.data);
        Ok(out)
    }

    fn notes(&self) -> Vec<String> {
        self.notes.clone()
    }

    fn info(&self) -> Vec<InfoSection> {
        let rows = vec![
            ("Machine".to_string(), self.machine.to_string()),
            (crate::i18n::tr("TAP version").into(), format!("{}{}", self.version, match self.version { 0 => " (long pauses not measured)", 1 => " (long pauses measured)", 2 => " (half waves)", _ => "" })),
            ("Pulses".into(), self.pulses.to_string()),
            (crate::i18n::tr("Short pulse").into(), if self.short == 0 { crate::i18n::tr("Not found").into() } else { crate::i18n::tf!("{} cycles (${:02X} in the file)", self.short, self.short / 8) }),
            (crate::i18n::tr("KERNAL files").into(), self.files.len().to_string()),
        ];
        let files = self
            .files
            .iter()
            .map(|f| (if f.name.is_empty() { crate::i18n::tr("(no name)").to_string() } else { f.name.clone() }, format!("{}, ${:04X}-${:04X}, at offset {}{}", f.kind_name(), f.start, f.end.wrapping_sub(1), f.offset, if !f.has_data { ", header only" } else if f.ok { "" } else { ", checksum wrong" })))
            .collect();
        vec![InfoSection { title: "Tape".into(), rows }, InfoSection { title: "Files".into(), rows: files }]
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Pulses as the KERNAL writes them, in TAP bytes: short $30, medium $42, long $56.
    pub struct Writer {
        pub pulses: Vec<u8>,
    }

    impl Writer {
        pub fn new() -> Self {
            Writer { pulses: Vec::new() }
        }

        fn byte(&mut self, value: u8) {
            self.pulses.extend([0x56, 0x42]);
            for bit in 0..8 {
                self.pulses.extend(if value >> bit & 1 == 1 { [0x42, 0x30] } else { [0x30, 0x42] });
            }
            let check = 1 ^ (value.count_ones() as u8 & 1);
            self.pulses.extend(if check == 1 { [0x42, 0x30] } else { [0x30, 0x42] });
        }

        /// A block, twice, as the KERNAL saves it: leader, countdown, bytes, checksum, end marker, both times.
        pub fn block(&mut self, data: &[u8], leader: usize) {
            for repeat in [false, true] {
                self.pulses.extend(std::iter::repeat_n(0x30, if repeat { 0x4F } else { leader }));
                for k in 0..9u8 {
                    self.byte(if repeat { 0x09 } else { 0x89 } - k);
                }
                for b in data {
                    self.byte(*b);
                }
                self.byte(data.iter().fold(0, |a, b| a ^ b));
                self.pulses.extend([0x56, 0x30]);
            }
            self.pulses.extend(std::iter::repeat_n(0x30, 0x4E));
        }

        pub fn program(&mut self, kind: u8, name: &str, start: u16, data: &[u8]) {
            let mut header = vec![kind];
            header.extend(start.to_le_bytes());
            header.extend((start + data.len() as u16).to_le_bytes());
            let mut title: Vec<u8> = name.bytes().collect();
            title.resize(16, 0x20);
            header.extend(title);
            header.resize(192, 0x20);
            self.block(&header, 0x6A00);
            self.pulses.push(0xFF);
            self.block(data, 0x1500);
        }

        pub fn image(&self, version: u8) -> Vec<u8> {
            let mut out = b"C64-TAPE-RAW".to_vec();
            out.extend([version, 0, 0, 0]);
            out.extend((self.pulses.len() as u32).to_le_bytes());
            out.extend(&self.pulses);
            out
        }
    }

    const HELLO: [u8; 12] = [0x0B, 0x08, 0x0A, 0x00, 0x99, 0x22, 0x48, 0x49, 0x22, 0x00, 0x00, 0x00];

    #[test]
    fn kernal_files_are_decoded_from_the_pulses() {
        let mut tape = Writer::new();
        tape.program(1, "HELLO", 0x0801, &HELLO);
        let code: Vec<u8> = (0..300u32).map(|i| (i * 13) as u8).collect();
        tape.program(3, "CODE", 0xC000, &code);
        let read = C64Tape::open(Arc::new(tape.image(0))).unwrap();
        assert_eq!(read.files.len(), 2, "{:?}", read.notes());
        assert!(read.notes().is_empty(), "{:?}", read.notes());
        assert_eq!(read.short, 0x30 * 8);
        let files = read.list(None).unwrap();
        assert_eq!((files[0].name.as_str(), files[0].kind.as_str(), files[0].detail.as_str(), files[0].open_name.as_str()), ("HELLO", "PRG (BASIC)", "$0801-$080C", "HELLO.prg"));
        let mut prg = vec![0x01, 0x08];
        prg.extend(HELLO);
        assert_eq!(read.read(&files[0]).unwrap(), prg);
        assert_eq!(&read.read(&files[1]).unwrap()[2..], &code[..]);
        assert_eq!(read.describe(), "C64 tape (TAP version 0) \u{B7} 2 files");
    }

    #[test]
    fn a_damaged_first_copy_is_read_from_the_second() {
        let mut tape = Writer::new();
        tape.program(3, "X", 0x1000, &[1, 2, 3, 4]);
        let mut image = tape.image(1);
        // In the data block's first copy, the second byte's lowest bit turned over: its parity, and the block's
        // checksum, are wrong.
        let header_block = (9 + 192 + 1) * 20 + 2;
        let data_start = 0x6A00 + header_block + 0x4F + header_block + 0x4E + 1 + 0x1500;
        let at = 0x14 + data_start + 10 * 20 + 2;
        assert_eq!(&image[at - 2..at + 2], &[0x56, 0x42, 0x30, 0x42]);
        image[at] = 0x42;
        image[at + 1] = 0x30;
        let read = C64Tape::open(Arc::new(image)).unwrap();
        assert_eq!(read.files.len(), 1, "{:?}", read.notes());
        assert!(read.files[0].ok, "{:?}", read.notes());
        assert_eq!(read.files[0].data, [1, 2, 3, 4]);
    }

    #[test]
    fn a_turbo_loader_after_the_kernal_file_is_noted() {
        let mut tape = Writer::new();
        tape.program(1, "LOADER", 0x0801, &HELLO);
        for i in 0..20_000u32 {
            tape.pulses.push(if i % 3 == 0 { 0x1A } else { 0x28 });
        }
        let read = C64Tape::open(Arc::new(tape.image(1))).unwrap();
        assert_eq!(read.files.len(), 1);
        assert!(read.notes().iter().any(|n| n.contains("turbo loader") && n.contains("$28")), "{:?}", read.notes());
    }

    #[test]
    fn version_1_long_pulses_are_skipped_and_a_header_without_data_is_said() {
        let mut tape = Writer::new();
        let mut header = vec![3u8, 0x00, 0x10, 0x00, 0x20];
        header.extend(b"GAME            ");
        header.resize(192, 0x20);
        tape.block(&header, 0x6A00);
        tape.pulses.extend([0, 0x10, 0x27, 0x00]);
        let read = C64Tape::open(Arc::new(tape.image(1))).unwrap();
        assert_eq!(read.files.len(), 1);
        assert!(!read.files[0].has_data);
        let files = read.list(None).unwrap();
        assert!(read.read(&files[0]).unwrap_err().contains("turbo"));
        assert!(read.notes().iter().any(|n| n.contains("GAME's header")), "{:?}", read.notes());
    }

    /// With LIMONITER_TAP_DIR naming a folder of real .tap files and a NAME.expected file beside each, listing
    /// "name start end" per file (as tapclean or VICE showed them), the files decoded agree.
    #[test]
    fn real_tapes_agree_with_the_reference() {
        let Some(dir) = std::env::var_os("LIMONITER_TAP_DIR").map(std::path::PathBuf::from) else {
            eprintln!("LIMONITER_TAP_DIR isn't set, so no real tapes were read.");
            return;
        };
        for path in std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("tap"))) {
            let tape = C64Tape::open(Arc::new(std::fs::read(&path).unwrap())).unwrap();
            eprintln!("{}: {}", path.display(), tape.describe());
            for f in &tape.files {
                eprintln!("  {:16} {} ${:04X}-${:04X} ok={} data={}", f.name, f.kind_name(), f.start, f.end, f.ok, f.has_data);
            }
            for n in tape.notes() {
                eprintln!("  note: {n}");
            }
            if let Ok(expected) = std::fs::read_to_string(path.with_extension("expected")) {
                let lines: Vec<&str> = expected.lines().filter(|l| !l.trim().is_empty()).collect();
                assert_eq!(lines.len(), tape.files.len(), "{}", path.display());
                let entries = tape.list(None).unwrap();
                for (n, (line, file)) in lines.iter().zip(&tape.files).enumerate() {
                    let got = format!("{} {:04X} {:04X}", file.name, file.start, file.end);
                    assert_eq!(line.trim().to_ascii_uppercase(), got.to_ascii_uppercase(), "{}", path.display());
                    assert!(file.ok, "{} {}", path.display(), file.name);
                    // The program as the reference took it off the tape (NAME.1.prg and so on), byte for byte.
                    if let Ok(theirs) = std::fs::read(path.with_extension(format!("{}.prg", n + 1))) {
                        assert_eq!(tape.read(&entries[n]).unwrap(), theirs, "{} {}", path.display(), file.name);
                        eprintln!("  {}: the same bytes as the reference's", file.name);
                    }
                }
            }
        }
    }
}
