//! Oric-1 and Atmos tapes in `.tap` files: the bytes the tape carried, each file after its leader.
//!
//! A file is a run of &16 sync bytes (at least three), &24, then a 9-byte header: two unused bytes, the type (&00 a
//! BASIC program, &80 machine code, &40 an array), whether it runs once loaded (non-zero if so), the end address
//! and the start address (each high byte first), and an unused byte; then the name, ending with a zero; then the
//! bytes from the start address to the end address, both included. This is how the Oric's ROM writes a tape, as
//! the Oric Atmos manual and the tape tools of the OSDK (osdk.org) describe it. A file of zeros or noise between
//! files is skipped, to the next leader.

use super::{Entry, Source, Volume, tidy_name};
use std::sync::Arc;

struct TapeFile {
    name: String,
    open_name: String,
    kind: String,
    detail: String,
    data: Vec<u8>,
}

pub struct OricTape {
    files: Vec<TapeFile>,
    notes: Vec<String>,
}

/// Where the next file's &24 is from `from`: after at least three &16s.
fn next_file(data: &[u8], from: usize) -> Option<usize> {
    let mut run = 0;
    for (at, &byte) in data.iter().enumerate().skip(from) {
        match byte {
            0x16 => run += 1,
            0x24 if run >= 3 => return Some(at),
            _ => run = 0,
        }
    }
    None
}

/// Whether a `.tap` file starts as an Oric tape does: its sync bytes and then a file's &24.
pub fn looks_like(head: &[u8]) -> bool {
    head.starts_with(&[0x16, 0x16, 0x16]) && next_file(head, 0).is_some_and(|at| head[..at].iter().all(|&b| b == 0x16))
}

impl OricTape {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let data = source.read_vec(0, source.len().min(16 * 1024 * 1024) as usize)?;
        let mut files = Vec::new();
        let mut notes = Vec::new();
        let mut at = 0;
        while let Some(marker) = next_file(&data, at) {
            let skipped = &data[at..marker];
            if skipped.iter().any(|&b| b != 0x16 && b != 0) {
                notes.push(format!("{} bytes before the file at byte {marker} aren't part of a file.", skipped.iter().filter(|&&b| b != 0x16).count()));
            }
            let Some(header) = data.get(marker + 1..marker + 10) else {
                notes.push(format!("The tape ends in the header of the file at byte {marker}."));
                break;
            };
            let (kind, autorun) = (header[2], header[3]);
            let end = usize::from(u16::from_be_bytes([header[4], header[5]]));
            let start = usize::from(u16::from_be_bytes([header[6], header[7]]));
            let name_at = marker + 10;
            let Some(name_length) = data.get(name_at..).and_then(|rest| rest.iter().take(17).position(|&b| b == 0)) else {
                notes.push(format!("The file at byte {marker} has no end to its name."));
                break;
            };
            let name: String = data[name_at..name_at + name_length].iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { char::from(b) } else { '_' }).collect();
            let name = if name.trim().is_empty() { format!("File {}", files.len() + 1) } else { tidy_name(&name) };
            let body_at = name_at + name_length + 1;
            if end < start {
                notes.push(format!("{name} ends at &{end:04X}, before it starts at &{start:04X}, so it's left out."));
                at = body_at;
                continue;
            }
            let length = end - start + 1;
            let body = &data[body_at.min(data.len())..(body_at + length).min(data.len())];
            if body.len() < length {
                notes.push(format!("{name} should be {length} bytes, but the tape ends after {}.", body.len()));
            }
            let runs = if autorun != 0 { ", runs when loaded" } else { "" };
            let (what, ending) = match kind {
                0x00 => ("BASIC program", "bas"),
                0x80 => ("Machine code", "bin"),
                0x40 => ("Array", "dat"),
                _ => ("File", "bin"),
            };
            files.push(TapeFile { open_name: format!("{name}.{ending}"), kind: what.into(), detail: format!("&{start:04X} to &{end:04X}{runs}"), data: body.to_vec(), name });
            at = body_at + body.len();
        }
        let trailing = &data[at.min(data.len())..];
        if trailing.iter().any(|&b| b != 0 && b != 0x16) {
            notes.push(format!("{} bytes after the last file aren't part of one.", trailing.len()));
        }
        if files.is_empty() {
            return Err("No Oric files were found on this tape.".into());
        }
        Ok(Self { files, notes })
    }
}

impl Volume for OricTape {
    fn describe(&self) -> String {
        format!("Oric tape (TAP) \u{B7} {} file(s)", self.files.len())
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        if dir.is_some() {
            return Ok(Vec::new());
        }
        Ok(self
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                let mut entry = Entry::file(file.name.clone(), file.data.len() as u64, index as u64);
                entry.open_name = file.open_name.clone();
                entry.kind = file.kind.clone();
                entry.detail = file.detail.clone();
                entry
            })
            .collect())
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        self.files.get(entry.location as usize).map(|file| file.data.clone()).ok_or_else(|| "That file isn't on the tape.".into())
    }

    fn notes(&self) -> Vec<String> {
        self.notes.clone()
    }
}

/// The first BASIC program on an Oric tape, for listing a `.tap` file as a whole.
pub fn first_program(data: &[u8]) -> Option<Vec<u8>> {
    let tape = OricTape::open(Arc::new(data.to_vec())).ok()?;
    tape.files.into_iter().find(|file| file.kind == "BASIC program").map(|file| file.data)
}

// ---------- Sedoric disks ----------
//
// An Oric disk image (`.dsk`) is in the MFM_DISK form Euphoric and Oricutron use: a 256-byte header ("MFM_DISK",
// then the number of sides, of tracks, and how they're laid out, each four bytes, low byte first), then each track
// as the 6400 bytes a drive's controller would read from it: gaps, three &A1 sync bytes and &FE before each
// sector's ID (track, side, sector, size), three more and &FB before its data, each with a CRC. The tracks are
// side 0's from the first and then side 1's (layout 1), or each track's two sides in turn (layout 2).
//
// Sedoric, the Oric's commonest DOS, keeps 256-byte sectors numbered from 1. On track 20 are the disk's name
// (sector 1), its bitmap (2) and the first sector of its directory (4). A track number with its top bit set is on
// side 1. Each directory sector links to the next (track, sector; 0 and 0 for none), says where its first free
// entry is, and holds fifteen 16-byte entries from byte 16: a name of nine letters and an extension of three, the
// track and sector of the file's first descriptor, its number of sectors (the low byte, then the top six bits of
// the next, whose top two bits say whether it's locked). A descriptor links to the next as well; the first is
// marked &FF and then gives the file's kind (&80 a BASIC program, &40 bytes from memory, &10 a sequential file,
// &08 a direct-access one; &01 if it runs when loaded), its start, end and run addresses and its number of
// sectors, then the track and sector of each sector of the file, as later descriptors do from their third byte.
// This is as André Chéramy's "Sedoric à nu" sets it out, and as the Sedoric-Extract tool
// (github.com/assinie/Sedoric-Extract) and the Oric DSK Manager read it.

const MFM_TRACK_BYTES: usize = 6400;
const MFM_HEADER_BYTES: usize = 256;
const SEDORIC_SECTOR: usize = 256;

/// Whether a `.dsk` file is an Oric disk image, by the header Euphoric gave it.
pub fn looks_like_disk(head: &[u8]) -> bool {
    head.starts_with(b"MFM_DISK")
}

/// CCITT's CRC-16, as a floppy controller checks a sector's ID and data with it.
fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for &byte in bytes {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

/// How a sector's data stood up to its checksum.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Check {
    Good,
    /// The checksum is &F7 &F7, the byte a floppy controller is told to write a CRC with: the tool that made the
    /// image wrote the format command's bytes rather than the CRC, so there's nothing to check.
    Unrecorded,
    Wrong,
}

/// A sector found on a track: its number, its bytes (or why there are none), and its checksum's verdict.
type FoundSector = (u8, Result<Vec<u8>, String>, Check);

/// A CRC as the image keeps it, checked against `bytes`.
fn check(bytes: &[u8], stored: &[u8]) -> Check {
    match stored {
        [0xF7, 0xF7] => Check::Unrecorded,
        _ if crc16(bytes) == u16::from_be_bytes([stored[0], stored[1]]) => Check::Good,
        _ => Check::Wrong,
    }
}

/// The sectors on one track's bytes, in the order they pass the head. A sector whose data runs past the end of
/// the track carries on at its start, as the track is a circle.
fn track_sectors(track: &[u8]) -> Vec<FoundSector> {
    let circle: Vec<u8> = track.iter().chain(track.iter().take(1100)).copied().collect();
    let mut found: Vec<FoundSector> = Vec::new();
    let mut at = 0;
    while let Some(mark) = circle.get(at..track.len() + 3).and_then(|rest| rest.windows(4).position(|w| w == [0xA1, 0xA1, 0xA1, 0xFE])).map(|p| p + at) {
        at = mark + 4;
        let Some(id) = circle.get(mark..mark + 10) else { break };
        if check(&id[..8], &id[8..]) == Check::Wrong {
            continue;
        }
        let (number, size) = (id[6], 128usize << (id[7] & 3));
        // The data's mark follows the ID within a gap's length; another ID first means this sector has no data.
        let window = circle.get(mark + 10..(mark + 80).min(circle.len())).unwrap_or_default();
        let Some(data_mark) = window.windows(4).position(|w| w[..3] == [0xA1, 0xA1, 0xA1] && matches!(w[3], 0xFB | 0xF8 | 0xFE)).map(|p| p + mark + 10) else {
            continue;
        };
        if circle[data_mark + 3] == 0xFE || found.iter().any(|(n, data, verdict)| *n == number && data.is_ok() && *verdict != Check::Wrong) {
            continue;
        }
        let (bytes, verdict) = match circle.get(data_mark..data_mark + 4 + size + 2) {
            Some(field) => (Ok(field[4..4 + size].to_vec()), check(&field[..4 + size], &field[4 + size..])),
            None => (Err(format!("sector {number} runs off its track")), Check::Wrong),
        };
        found.retain(|(n, _, _)| *n != number);
        found.push((number, bytes, verdict));
        at = data_mark + 4;
    }
    found
}

/// An MFM_DISK image's sectors, found by the track numbers Sedoric uses (side 1's with the top bit set).
struct MfmDisk {
    /// Each track's sectors, side 0's first.
    tracks: Vec<Vec<FoundSector>>,
    tracks_per_side: usize,
    sides: usize,
    /// Whether sectors whose checksums are wrong are read anyway: when most of a disk's are, the tool that made
    /// the image didn't keep them up to date, and they say nothing of damage.
    trust_wrong: bool,
    notes: Vec<String>,
}

impl MfmDisk {
    fn read(data: &[u8]) -> Result<Self, String> {
        if !looks_like_disk(data) || data.len() < MFM_HEADER_BYTES {
            return Err("This isn't an Oric disk image: it hasn't the MFM_DISK header.".into());
        }
        let field = |at: usize| u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]) as usize;
        let (sides, tracks_per_side, layout) = (field(8), field(12), field(16));
        if !(1..=2).contains(&sides) || !(1..=100).contains(&tracks_per_side) {
            return Err(format!("The disk image's header gives {sides} side(s) of {tracks_per_side} tracks, which no Oric drive has."));
        }
        let mut tracks = Vec::new();
        let mut missing = 0;
        for side in 0..sides {
            for track in 0..tracks_per_side {
                let index = if layout == 2 { track * sides + side } else { side * tracks_per_side + track };
                let from = MFM_HEADER_BYTES + index * MFM_TRACK_BYTES;
                let bytes = data.get(from..from + MFM_TRACK_BYTES);
                missing += usize::from(bytes.is_none());
                tracks.push(bytes.map(track_sectors).unwrap_or_default());
            }
        }
        let count = |verdict: Check| tracks.iter().flatten().filter(|(_, _, v)| *v == verdict).count();
        let (good, unrecorded, wrong) = (count(Check::Good), count(Check::Unrecorded), count(Check::Wrong));
        let mut notes = Vec::new();
        if missing > 0 {
            notes.push(format!("The image stops {missing} tracks short of the {} its header gives, so it's cut short.", sides * tracks_per_side));
        }
        if unrecorded > 0 {
            notes.push("The tool that made this image didn't record its sectors' checksums, so damage to them can't be found.".into());
        }
        let trust_wrong = wrong > good;
        if trust_wrong {
            notes.push(format!("{wrong} of the image's {} sectors have checksums that don't match, as some tools that made Oric disk images left them; their bytes are read as they are.", good + unrecorded + wrong));
        }
        Ok(Self { tracks, tracks_per_side, sides, trust_wrong, notes })
    }

    /// A sector by Sedoric's track number (side 1's with its top bit set) and its sector number.
    fn sector(&self, track: u8, sector: u8) -> Result<&[u8], String> {
        let (side, cylinder) = (usize::from(track >> 7), usize::from(track & 0x7F));
        let place = format!("track {cylinder}{}, sector {sector}", if side == 1 { " on side 1" } else { "" });
        if side >= self.sides || cylinder >= self.tracks_per_side {
            return Err(format!("{place} isn't on the disk"));
        }
        match self.tracks[side * self.tracks_per_side + cylinder].iter().find(|(n, _, _)| *n == sector) {
            Some((_, Ok(_), Check::Wrong)) if !self.trust_wrong => Err(format!("{place} fails its checksum, so it's damaged")),
            Some((_, Ok(bytes), _)) if bytes.len() == SEDORIC_SECTOR => Ok(bytes),
            Some((_, Ok(bytes), _)) => Err(format!("{place} is {} bytes, not Sedoric's 256", bytes.len())),
            Some((_, Err(problem), _)) => Err(format!("{place}: {problem}")),
            None => Err(format!("{place} isn't on the disk")),
        }
    }
}

struct DiskFile {
    name: String,
    open_name: String,
    kind: String,
    detail: String,
    data: Result<Vec<u8>, String>,
}

pub struct OricDisk {
    label: String,
    files: Vec<DiskFile>,
    notes: Vec<String>,
}

impl OricDisk {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        if source.len() > 4 * 1024 * 1024 {
            return Err("This file is too big to be an Oric disk image.".into());
        }
        let disk = MfmDisk::read(&source.read_vec(0, source.len() as usize)?)?;
        let not_sedoric = |why: String| {
            format!("This Oric disk isn't a Sedoric disk ({why}), and other Oric DOSes, such as OricDOS, Jasmin's FT-DOS, RANDOS, XL DOS and the Infocom games' own format, aren't read yet.")
        };
        let bitmap = disk.sector(20, 2).map_err(not_sedoric)?;
        if bitmap[..2] != [0xFF, 0x00] {
            return Err(not_sedoric("its bitmap, on track 20, isn't marked as Sedoric marks it".into()));
        }
        let mut notes = disk.notes.clone();
        let label = match disk.sector(20, 1) {
            Ok(system) => system[9..30].iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { char::from(b) } else { '_' }).collect::<String>().trim().to_string(),
            Err(problem) => {
                notes.push(format!("The disk's name can't be read: {problem}."));
                String::new()
            }
        };
        let mut files = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut next = (20u8, 4u8);
        while next != (0, 0) {
            if !seen.insert(next) {
                notes.push("The directory leads back to a sector it has already used, so it's read only that far.".into());
                break;
            }
            let directory = match disk.sector(next.0, next.1) {
                Ok(bytes) => bytes,
                Err(problem) => {
                    notes.push(format!("Part of the directory can't be read, so files may be missing: {problem}."));
                    break;
                }
            };
            next = (directory[0], directory[1]);
            // Where the sector's first free entry is; 0 when the sector is full.
            let used = match usize::from(directory[2]) {
                0 => SEDORIC_SECTOR,
                end => end,
            };
            for entry in directory[16..used.clamp(16, SEDORIC_SECTOR)].chunks_exact(16) {
                if entry[12] != 0 || entry[13] != 0 {
                    files.push(read_file(&disk, entry));
                }
            }
        }
        for file in &files {
            if let Err(problem) = &file.data {
                notes.push(format!("{} can't be read: {problem}", file.name));
            }
        }
        Ok(Self { label, files, notes })
    }
}

/// A file from its directory entry: its descriptors, then its sectors, as long as they can be read.
fn read_file(disk: &MfmDisk, entry: &[u8]) -> DiskFile {
    let text = |bytes: &[u8]| -> String { bytes.iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { char::from(b) } else { '_' }).collect::<String>().trim().to_string() };
    let (stem, extension) = (text(&entry[..9]), text(&entry[9..12]));
    let name = tidy_name(&if extension.is_empty() { stem } else { format!("{stem}.{extension}") });
    let locked = if entry[15] & 0xC0 == 0xC0 { ", locked" } else { "" };
    let mut file = DiskFile { open_name: name.clone(), name, kind: "File".into(), detail: String::new(), data: Ok(Vec::new()) };
    let mut sectors = Vec::new();
    let mut header = None;
    let mut seen = std::collections::HashSet::new();
    let mut next = (entry[12], entry[13]);
    while next != (0, 0) {
        let descriptor = match disk.sector(next.0, next.1) {
            Ok(bytes) if seen.insert(next) => bytes,
            Ok(_) => {
                file.data = Err("its list of sectors leads back on itself.".into());
                return file;
            }
            Err(problem) => {
                file.data = Err(format!("its list of sectors can't be read: {problem}."));
                return file;
            }
        };
        let first = header.is_none();
        if first {
            if descriptor[2] != 0xFF {
                file.data = Err("its first descriptor isn't marked as one.".into());
                return file;
            }
            let word = |at: usize| u16::from_le_bytes([descriptor[at], descriptor[at + 1]]);
            header = Some((descriptor[3], word(4), word(6), word(8), usize::from(word(10))));
        }
        let count = header.map_or(0, |h| h.4);
        for pair in descriptor[if first { 12 } else { 2 }..].chunks_exact(2) {
            if sectors.len() < count && pair != [0, 0] {
                sectors.push((pair[0], pair[1]));
            }
        }
        next = (descriptor[0], descriptor[1]);
        if sectors.len() >= count {
            break;
        }
    }
    let Some((kind, start, end, run, count)) = header else {
        file.data = Err("it has no descriptor.".into());
        return file;
    };
    let autorun = if kind & 0x01 != 0 { ", runs when loaded" } else { "" };
    let length = if kind & 0x08 != 0 {
        file.kind = "Direct-access file".into();
        file.detail = format!("{start} records of {end} bytes{locked}");
        usize::from(start) * usize::from(end)
    } else if kind & 0x10 != 0 {
        file.kind = "Sequential file".into();
        file.detail = format!("{count} sectors{locked}");
        count * SEDORIC_SECTOR
    } else {
        let basic = kind & 0x80 != 0;
        file.kind = if basic { "BASIC program" } else { "Machine code or data" }.into();
        if basic {
            file.open_name = format!("{}.bas", file.name);
        }
        let run = if run != 0 && !basic { format!(", run at &{run:04X}") } else { String::new() };
        file.detail = format!("&{start:04X} to &{end:04X}{run}{autorun}{locked}");
        if end < start {
            file.data = Err(format!("it ends at &{end:04X}, before it starts at &{start:04X}."));
            return file;
        }
        usize::from(end - start) + 1
    };
    if sectors.len() < count {
        file.data = Err(format!("its descriptors list {} of its {count} sectors.", sectors.len()));
        return file;
    }
    if length > count * SEDORIC_SECTOR {
        file.data = Err(format!("it should be {length} bytes, but has only {count} sectors."));
        return file;
    }
    let mut data = Vec::with_capacity(count * SEDORIC_SECTOR);
    for (track, sector) in sectors {
        match disk.sector(track, sector) {
            Ok(bytes) => data.extend_from_slice(bytes),
            Err(problem) => {
                file.data = Err(format!("part of it can't be read: {problem}."));
                return file;
            }
        }
    }
    data.truncate(length);
    file.data = Ok(data);
    file
}

impl Volume for OricDisk {
    fn describe(&self) -> String {
        let label = if self.label.is_empty() { String::new() } else { format!(" \u{B7} {}", self.label) };
        format!("Oric disk (Sedoric){label} \u{B7} {} file(s)", self.files.len())
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        if dir.is_some() {
            return Ok(Vec::new());
        }
        Ok(self
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                let mut entry = Entry::file(file.name.clone(), file.data.as_ref().map_or(0, |d| d.len() as u64), index as u64);
                entry.open_name = file.open_name.clone();
                entry.kind = file.kind.clone();
                entry.detail = file.detail.clone();
                entry
            })
            .collect())
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let file = self.files.get(entry.location as usize).ok_or_else(|| "That file isn't on the disk.".to_string())?;
        file.data.clone().map_err(|problem| format!("{} can't be read: {problem}", file.name))
    }

    fn notes(&self) -> Vec<String> {
        self.notes.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tape file: leader, &24, the header, the name, the bytes.
    fn file(kind: u8, autorun: u8, start: u16, name: &[u8], body: &[u8]) -> Vec<u8> {
        let end = start + body.len() as u16 - 1;
        let mut out = vec![0x16; 4];
        out.push(0x24);
        out.extend([0, 0, kind, autorun]);
        out.extend(end.to_be_bytes());
        out.extend(start.to_be_bytes());
        out.push(0);
        out.extend_from_slice(name);
        out.push(0);
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn an_oric_tape_holds_basic_and_machine_code() {
        // 10 PRINT "HI", at &0501.
        let program = [0x0C, 0x05, 0x0A, 0x00, 0xBA, b' ', b'"', b'H', b'I', b'"', 0x00, 0x00, 0x00];
        let mut tape = file(0x00, 0xC7, 0x0501, b"HELLO", &program);
        tape.extend([0u8; 5]);
        tape.extend(file(0x80, 0x00, 0x9800, b"", &[0xA9, 0x00, 0x60]));
        assert!(looks_like(&tape));
        let image = super::super::open(Arc::new(tape.clone()), "games.tap").unwrap();
        assert_eq!(image.describe(), "Oric tape (TAP) \u{B7} 2 file(s)");
        let files = image.list(None).unwrap();
        assert_eq!((files[0].open_name.as_str(), files[0].detail.as_str()), ("HELLO.bas", "&0501 to &050D, runs when loaded"));
        assert_eq!((files[1].open_name.as_str(), files[1].kind.as_str()), ("File 2.bin", "Machine code"));
        assert_eq!(image.read(&files[1]).unwrap(), [0xA9, 0x00, 0x60]);
        assert!(image.notes().is_empty(), "{:?}", image.notes());
        let bytes = image.read(&files[0]).unwrap();
        assert_eq!(crate::basic::list(&bytes, "bas", crate::charset::utf8()).lines, ["10 PRINT \"HI\""]);
        assert_eq!(crate::basic::list(&tape, "tap", crate::charset::utf8()).lines, ["10 PRINT \"HI\""], "a whole tape lists its program");
        for cut in 0..tape.len() {
            if let Ok(image) = super::super::open(Arc::new(tape[..cut].to_vec()), "cut.tap") {
                for entry in image.list(None).unwrap() {
                    let _ = image.read(&entry);
                }
            }
        }
    }


    /// One MFM_DISK track of 256-byte sectors, each as a controller formats it, with right CRCs unless `stale`.
    fn mfm_track(track: u8, sectors: &[(u8, Vec<u8>)], stale: bool) -> Vec<u8> {
        let mut out = vec![0x4E; 40];
        for (number, data) in sectors {
            out.extend([0; 12]);
            let id = [0xA1, 0xA1, 0xA1, 0xFE, track, 0, *number, 1];
            out.extend(id);
            out.extend(crc16(&id).to_be_bytes());
            out.extend([0x4E; 22]);
            out.extend([0; 12]);
            let mut field = vec![0xA1, 0xA1, 0xA1, 0xFB];
            field.extend(data);
            let crc = if stale { 0x1234 } else { crc16(&field) };
            out.extend(field);
            out.extend(crc.to_be_bytes());
            out.extend([0x4E; 24]);
        }
        out.resize(MFM_TRACK_BYTES, 0x4E);
        out
    }

    /// A one-sided Sedoric disk of 22 tracks with one BASIC program, HELLO.COM, and its sectors' CRCs.
    fn sedoric_disk(stale: bool) -> Vec<u8> {
        let mut disk = b"MFM_DISK".to_vec();
        disk.extend(1u32.to_le_bytes());
        disk.extend(22u32.to_le_bytes());
        disk.extend(1u32.to_le_bytes());
        disk.resize(MFM_HEADER_BYTES, 0);
        let sector = |fill: &[(usize, &[u8])]| {
            let mut bytes = vec![0u8; 256];
            for (at, part) in fill {
                bytes[*at..*at + part.len()].copy_from_slice(part);
            }
            bytes
        };
        // 10 PRINT "HI", at &0501.
        let program = [0x0C, 0x05, 0x0A, 0x00, 0xBA, b' ', b'"', b'H', b'I', b'"', 0x00, 0x00, 0x00];
        let name = *b"DEMO DISK            ";
        let mut entry = *b"HELLO    COM\x14\x05\x01\x40";
        entry[12] = 20;
        let descriptor = sector(&[(0, &[0, 0, 0xFF, 0x81, 0x01, 0x05, 0x0D, 0x05, 0, 0, 1, 0, 21, 1])]);
        for track in 0..22u8 {
            let sectors: Vec<(u8, Vec<u8>)> = (1..=17u8)
                .map(|n| {
                    let bytes = match (track, n) {
                        (20, 1) => sector(&[(9, &name)]),
                        (20, 2) => sector(&[(0, &[0xFF, 0x00])]),
                        (20, 4) => sector(&[(2, &[0x20]), (16, &entry)]),
                        (20, 5) => descriptor.clone(),
                        (21, 1) => sector(&[(0, &program)]),
                        _ => vec![0; 256],
                    };
                    (n, bytes)
                })
                .collect();
            disk.extend(mfm_track(track, &sectors, stale));
        }
        disk
    }

    #[test]
    fn a_sedoric_disk_gives_back_its_files() {
        let disk = sedoric_disk(false);
        assert!(looks_like_disk(&disk));
        let image = super::super::open(Arc::new(disk.clone()), "demo.dsk").unwrap();
        assert_eq!(image.describe(), "Oric disk (Sedoric) \u{B7} DEMO DISK \u{B7} 1 file(s)");
        assert!(image.notes().is_empty(), "{:?}", image.notes());
        let files = image.list(None).unwrap();
        assert_eq!((files[0].name.as_str(), files[0].open_name.as_str(), files[0].kind.as_str()), ("HELLO.COM", "HELLO.COM.bas", "BASIC program"));
        assert_eq!(files[0].detail, "&0501 to &050D, runs when loaded");
        let bytes = image.read(&files[0]).unwrap();
        assert_eq!(bytes.len(), 13);
        assert_eq!(crate::basic::list(&bytes, "bas", crate::charset::utf8()).lines, ["10 PRINT \"HI\""]);

        // Checksums a tool never kept up to date are read through, with a note; a few wrong ones mean damage.
        let stale = super::super::open(Arc::new(sedoric_disk(true)), "demo.dsk").unwrap();
        assert!(stale.notes().iter().any(|n| n.contains("checksums that don't match")), "{:?}", stale.notes());
        assert_eq!(stale.read(&stale.list(None).unwrap()[0]).unwrap(), bytes);
        let mut damaged = disk.clone();
        // The program's sector, on track 21: a byte of its data changed.
        let program_at = MFM_HEADER_BYTES + 21 * MFM_TRACK_BYTES + 40 + 12 + 10 + 22 + 12 + 4;
        damaged[program_at] ^= 0xFF;
        let damaged = super::super::open(Arc::new(damaged), "demo.dsk").unwrap();
        let problem = damaged.read(&damaged.list(None).unwrap()[0]).unwrap_err();
        assert!(problem.contains("track 21, sector 1 fails its checksum"), "{problem}");

        // Cut short anywhere, or with bytes changed, it never panics.
        for cut in (0..disk.len()).step_by(997) {
            if let Ok(image) = super::super::open(Arc::new(disk[..cut].to_vec()), "cut.dsk") {
                for entry in image.list(None).unwrap() {
                    let _ = image.read(&entry);
                }
            }
        }
        let mut state = 12345u32;
        for _ in 0..200 {
            let mut spoilt = disk.clone();
            for _ in 0..40 {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let at = MFM_HEADER_BYTES + 20 * MFM_TRACK_BYTES + state as usize % (2 * MFM_TRACK_BYTES);
                spoilt[at] = (state >> 16) as u8;
            }
            if let Ok(image) = super::super::open(Arc::new(spoilt), "spoilt.dsk") {
                for entry in image.list(None).unwrap() {
                    let _ = image.read(&entry);
                }
            }
        }
    }

    #[test]
    fn other_oric_disks_say_why_they_are_not_read() {
        let mut disk = sedoric_disk(false);
        // Track 20's bitmap sector unmarked, as OricDOS's and others' disks are.
        let bitmap_at = MFM_HEADER_BYTES + 20 * MFM_TRACK_BYTES + 40 + (12 + 10 + 22 + 12 + 4 + 256 + 2 + 24) + 12 + 10 + 22 + 12 + 4;
        disk[bitmap_at] = 0;
        let problem = super::super::open(Arc::new(disk), "other.dsk").err().unwrap();
        assert!(problem.contains("isn't a Sedoric disk"), "{problem}");
    }
}
