//! HP's Logical Interchange Format (LIF), the disk and tape layout of HP's computers and instruments from about
//! 1980: the Series 80 (HP-85, 86, 87), the 9000 Series 200 and 300, the Integral PC, and the HP-41, 71B and 75
//! through HP-IL drives. Laid out as HP's LIF specification gives it: 256-byte records, a volume label in record 0,
//! and a directory of 32-byte entries, each a file's name, type, first record and length in records.
//!
//! LIF says little about what's inside a file beyond its type number; HP's machines each have their own. Type 1, LIF
//! ASCII, is the one interchange type every machine reads and writes: records of text each with a two-byte length.
//! Those are offered as text too.

use super::{Entry, MOST_FILE_BYTES, Source, Volume, tidy_name};
use std::sync::Arc;

const RECORD: u64 = 256;

/// Entries with this in `extra` are a LIF ASCII file's text, the records taken out of their lengths.
const AS_TEXT: u64 = 1;

fn be(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0, |n, b| (n << 8) | u64::from(*b))
}

/// LIF names are uppercase letters and digits padded with spaces; some machines allow more, all of it ASCII.
fn lif_name(bytes: &[u8]) -> String {
    bytes.iter().map(|b| if (0x20..0x7F).contains(b) { char::from(*b) } else { '?' }).collect::<String>().trim_end().to_string()
}

/// A LIF date: six BCD bytes, year, month, day, hour, minute, second. Nothing for zeros or anything not BCD.
fn lif_date(bytes: &[u8]) -> String {
    let digits: Option<Vec<u32>> = bytes.iter().map(|b| (b >> 4 <= 9 && b & 0xF <= 9).then(|| u32::from(b >> 4) * 10 + u32::from(b & 0xF))).collect();
    match digits {
        Some(d) if d.len() == 6 && d[1] >= 1 && d[1] <= 12 && d[2] >= 1 => {
            // Two-digit years: LIF began in 1980, so 70 to 99 are the 1900s.
            let year = if d[0] >= 70 { 1900 + d[0] } else { 2000 + d[0] };
            format!("{year}-{:02}-{:02} {:02}:{:02}", d[1], d[2], d[3], d[4])
        }
        _ => String::new(),
    }
}

/// What a type number is called. Only type 1 is LIF's own; the rest are each machine's, shown by number.
fn type_name(kind: u16) -> String {
    match kind {
        1 => "LIF ASCII".into(),
        _ => format!("Type &{kind:04X}"),
    }
}

/// Whether the start of an image reads as a LIF volume: its mark, a label of printable characters, and a directory
/// that starts after the label and is within the image.
pub fn looks_like_lif(head: &[u8], len: u64) -> bool {
    if head.len() < 32 || head[..2] != [0x80, 0x00] || !head[2..8].iter().all(|b| (0x20..0x7F).contains(b) || *b == 0) {
        return false;
    }
    let (start, length) = (be(&head[8..12]), be(&head[16..20]));
    start >= 1 && (1..0x10000).contains(&length) && (start + 1) * RECORD <= len
}

struct LifFile {
    name: String,
    kind: u16,
    start: u64,
    records: u64,
    date: String,
    volume: u16,
}

pub struct Lif {
    source: Arc<dyn Source>,
    label: String,
    created: String,
    files: Vec<LifFile>,
    notes: Vec<String>,
}

impl Lif {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let header = source.read_vec(0, RECORD as usize).map_err(|_| "The image is too short for a LIF volume.".to_string())?;
        if !looks_like_lif(&header, source.len()) {
            return Err("This isn't a LIF volume: record 0 has no LIF mark and directory.".into());
        }
        let (start, length) = (be(&header[8..12]), be(&header[16..20]));
        let label = lif_name(&header[2..8]);
        let directory_bytes = (length * RECORD).min(source.len().saturating_sub(start * RECORD));
        let directory = source.read_vec(start * RECORD, directory_bytes as usize)?;
        let mut files = Vec::new();
        let mut notes = Vec::new();
        let mut purged = 0;
        for entry in directory.as_chunks::<32>().0 {
            let kind = be(&entry[10..12]) as u16;
            match kind {
                0xFFFF => break,
                0 => {
                    purged += 1;
                    continue;
                }
                _ => {}
            }
            files.push(LifFile { name: lif_name(&entry[..10]), kind, start: be(&entry[12..16]), records: be(&entry[16..20]), date: lif_date(&entry[20..26]), volume: be(&entry[26..28]) as u16 });
        }
        if purged > 0 {
            notes.push(format!("{purged} purged file(s) are in the directory but not listed; their records may since have been reused."));
        }
        if files.iter().any(|f| f.volume & 0x7FFF > 1) {
            notes.push("Some files continue from other volumes of a set; only this volume's part of them is here.".into());
        }
        Ok(Self { source, label, created: lif_date(&header[36..42]), files, notes })
    }

    fn bytes(&self, file: &LifFile) -> Result<Vec<u8>, String> {
        let length = file.records * RECORD;
        if length > MOST_FILE_BYTES {
            return Err("This file is larger than the 1 GB that can be opened.".into());
        }
        let at = file.start * RECORD;
        if at + length > self.source.len() {
            return Err(format!("{} runs past the end of the image, which has been cut short.", file.name));
        }
        self.source.read_vec(at, length as usize)
    }
}

/// A LIF ASCII file's text: records each of a two-byte length and that many bytes (padded to an even length), up to
/// a length of &FFFF; &FFFE means the rest of the record is unused. Each record is a line.
pub fn lif_ascii(bytes: &[u8]) -> Result<String, String> {
    let mut lines = Vec::new();
    let mut at = 0;
    loop {
        let Some(length) = bytes.get(at..at + 2).map(be) else {
            return Err("The text runs to the end of the file without LIF's end mark.".into());
        };
        at += 2;
        match length {
            0xFFFF => break,
            0xFFFE => at = at.div_ceil(RECORD as usize) * RECORD as usize,
            n => {
                let n = n as usize;
                let line = bytes.get(at..at + n).ok_or("A line runs past the end of the file.")?;
                lines.push(line.iter().map(|b| char::from(*b)).collect::<String>());
                at += (n + 1) & !1;
            }
        }
    }
    Ok(lines.join("\n") + if lines.is_empty() { "" } else { "\n" })
}

impl Volume for Lif {
    fn describe(&self) -> String {
        let label = if self.label.is_empty() { "no label" } else { &self.label };
        let made = if self.created.is_empty() { String::new() } else { format!(" \u{B7} made {}", &self.created[..10]) };
        format!("HP LIF volume \u{B7} {label}{made}")
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        if dir.is_some() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for (index, file) in self.files.iter().enumerate() {
            let name = tidy_name(&file.name);
            let mut entry = Entry::file(name.clone(), file.records * RECORD, index as u64);
            entry.kind = type_name(file.kind);
            entry.detail = [format!("{} record(s) from record {}", file.records, file.start), file.date.clone()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" \u{B7} ");
            out.push(entry);
            if file.kind == 1 {
                let mut text = Entry::file(format!("{name}.txt"), file.records * RECORD, index as u64);
                text.extra = AS_TEXT;
                text.kind = "LIF ASCII, as text".into();
                text.detail = "the lines without their lengths".into();
                out.push(text);
            }
        }
        Ok(out)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let file = self.files.get(entry.location as usize).ok_or("That file isn't on the volume.")?;
        let bytes = self.bytes(file)?;
        if entry.extra == AS_TEXT {
            return lif_ascii(&bytes).map(String::into_bytes);
        }
        Ok(bytes)
    }

    fn notes(&self) -> Vec<String> {
        self.notes.clone()
    }
}

#[cfg(test)]
pub mod tests {
    use super::super::open;
    use super::*;

    /// A LIF volume as HP's machines write one: label, directory at record 2 of 2 records, a LIF ASCII file split
    /// across a record by &FFFE, a binary file of another type, a purged entry, then the end mark.
    pub fn lif_volume(records: usize) -> Vec<u8> {
        let mut disk = vec![0u8; records * 256];
        disk[..2].copy_from_slice(&[0x80, 0x00]);
        disk[2..8].copy_from_slice(b"LETTRS");
        disk[8..12].copy_from_slice(&2u32.to_be_bytes());
        disk[12..14].copy_from_slice(&0x1000u16.to_be_bytes());
        disk[16..20].copy_from_slice(&2u32.to_be_bytes());
        disk[24..36].copy_from_slice(&[0, 0, 0, 66, 0, 0, 0, 1, 0, 0, 0, 16]);
        disk[36..42].copy_from_slice(&[0x86, 0x03, 0x14, 0x09, 0x30, 0x00]);
        let mut entry = |slot: usize, name: &[u8; 10], kind: u16, start: u32, length: u32| {
            let at = 512 + slot * 32;
            disk[at..at + 10].copy_from_slice(name);
            disk[at + 10..at + 12].copy_from_slice(&kind.to_be_bytes());
            disk[at + 12..at + 16].copy_from_slice(&start.to_be_bytes());
            disk[at + 16..at + 20].copy_from_slice(&length.to_be_bytes());
            disk[at + 20..at + 26].copy_from_slice(&[0x86, 0x03, 0x15, 0x10, 0x45, 0x00]);
            disk[at + 26..at + 28].copy_from_slice(&0x8001u16.to_be_bytes());
        };
        entry(0, b"LETTER1   ", 1, 4, 2);
        entry(1, b"OLDDRAFT  ", 0, 6, 1);
        entry(2, b"PROGRAM   ", 0xE214, 7, 1);
        entry(3, b"          ", 0xFFFF, 0, 0);
        // LETTER1: "Dear Sir," and "Thank you." in record 4, the rest of the record skipped, then "Yours." in record 5.
        let mut text = Vec::new();
        for line in [&b"Dear Sir,"[..], b"Thank you."] {
            text.extend((line.len() as u16).to_be_bytes());
            text.extend(line);
            if line.len() % 2 == 1 {
                text.push(0);
            }
        }
        text.extend([0xFF, 0xFE]);
        disk[4 * 256..4 * 256 + text.len()].copy_from_slice(&text);
        disk[5 * 256..5 * 256 + 10].copy_from_slice(b"\x00\x06Yours.\xFF\xFF");
        disk[7 * 256..7 * 256 + 4].copy_from_slice(b"CODE");
        disk
    }

    #[test]
    fn lif_volumes_list_their_files_and_read_lif_ascii_as_text() {
        let disk = lif_volume(66 * 16);
        assert!(looks_like_lif(&disk, disk.len() as u64));
        let volume = open(Arc::new(disk), "letters.lif").unwrap();
        assert_eq!(volume.describe(), "HP LIF volume \u{B7} LETTRS \u{B7} made 1986-03-14");
        let files = volume.list(None).unwrap();
        let names: Vec<&str> = files.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["LETTER1", "LETTER1.txt", "PROGRAM"], "the purged file isn't listed");
        assert_eq!((files[0].kind.as_str(), files[0].detail.as_str()), ("LIF ASCII", "2 record(s) from record 4 \u{B7} 1986-03-15 10:45"));
        assert_eq!(volume.read(&files[0]).unwrap().len(), 512, "the file itself is its whole records");
        assert_eq!(volume.read(&files[1]).unwrap(), b"Dear Sir,\nThank you.\nYours.\n");
        assert_eq!((files[2].kind.as_str(), &volume.read(&files[2]).unwrap()[..4]), ("Type &E214", &b"CODE"[..]));
        assert_eq!(volume.notes(), ["1 purged file(s) are in the directory but not listed; their records may since have been reused."]);
    }

    #[test]
    fn lif_is_found_in_any_disk_image_and_damage_gives_errors() {
        let disk = lif_volume(66 * 16);
        for name in ["disk.hpi", "disk.img", "disk.dsk"] {
            assert!(super::super::is_image(name, &disk[..0x8800], disk.len() as u64), "{name}");
        }
        // Inside a sector image, as an HP 9121 or HP 150 drive's disk would be read.
        let imd = super::super::sectors::tests::imd_of(&disk, (99, 99), (99, 99));
        assert!(open(Arc::new(imd), "lettrs.imd").unwrap().describe().contains("HP LIF volume \u{B7} LETTRS"));
        assert!(!looks_like_lif(&[0x80, 0, b'A', b'B', b'C', b'D', b'E', b'F', 0, 0, 0, 0x50, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 4096), "a directory past the end");
        for seed in 0..80u32 {
            let mut bytes = disk.clone();
            let mut state = seed.wrapping_mul(2_654_435_761);
            for _ in 0..16 {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                bytes[state as usize % 2048] = (state >> 16) as u8;
            }
            if let Ok(volume) = open(Arc::new(bytes), "d.lif") {
                for entry in volume.list(None).unwrap_or_default() {
                    let _ = volume.read(&entry);
                }
            }
        }
    }
}
