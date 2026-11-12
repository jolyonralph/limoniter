//! Commodore disk images, as the 1541, 1571 and 1581 drives lay out their disks (D64, D71, D81), and T64 tape
//! archives. A file is a chain of 256-byte sectors, each starting with the track and sector of the next; names are
//! PETSCII, shown here through the Character table's PETSCII set and opened with an ending for their type.

use super::{Entry, Source, Volume, tidy_name};
use crate::charset;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    D64,
    D71,
    D81,
}

pub struct Disk {
    source: Arc<dyn Source>,
    kind: Kind,
    name: String,
    id: String,
    tracks: u8,
}

/// The sectors on a 1541 track: 21 to track 17, 19 to 24, 18 to 30, then 17.
fn sectors_1541(track: u8) -> u64 {
    match track {
        1..=17 => 21,
        18..=24 => 19,
        25..=30 => 18,
        _ => 17,
    }
}

const FILE_TYPES: [&str; 8] = ["DEL", "SEQ", "PRG", "USR", "REL", "CBM", "DIR", "???"];

/// PETSCII as shown in a directory: the capitals and graphics set, with the padding (shifted space, 0xA0) taken off.
fn petscii_name(bytes: &[u8]) -> String {
    let set = charset::known("petscii-c64");
    let end = bytes.iter().position(|b| *b == 0xA0).unwrap_or(bytes.len());
    bytes[..end].iter().map(|b| set.entry(0, *b).and_then(|e| e.unicode).filter(|c| !c.is_control()).unwrap_or('?')).collect()
}

impl Disk {
    pub fn open(source: Arc<dyn Source>, kind: Kind) -> Result<Self, String> {
        let len = source.len();
        let tracks = match kind {
            Kind::D64 if len >= 196_608 => 40,
            Kind::D64 => 35,
            Kind::D71 => 70,
            Kind::D81 => 80,
        };
        let mut disk = Self { source, kind, name: String::new(), id: String::new(), tracks };
        let (header, name_at) = match kind {
            Kind::D81 => (disk.sector(40, 0)?, 0x04),
            _ => (disk.sector(18, 0)?, 0x90),
        };
        disk.name = petscii_name(&header[name_at..name_at + 16]);
        // The disk's ID, then its DOS type ("2A", or "3D" on the 1581), with a padding byte between them.
        disk.id = format!("{} {}", petscii_name(&header[name_at + 18..name_at + 20]), petscii_name(&header[name_at + 21..name_at + 23]));
        Ok(disk)
    }

    fn offset(&self, track: u8, sector: u8) -> Option<u64> {
        if track == 0 || track > self.tracks {
            return None;
        }
        let sector = u64::from(sector);
        match self.kind {
            Kind::D81 => (sector < 40).then(|| (u64::from(track - 1) * 40 + sector) * 256),
            _ => {
                // The 1571's second side repeats the first's layout, after all 35 of its tracks.
                let (side, t) = if track > 35 && self.kind == Kind::D71 { (683, track - 35) } else { (0, track) };
                if sector >= sectors_1541(t) {
                    return None;
                }
                let before: u64 = (1..t).map(sectors_1541).sum();
                Some((side + before + sector) * 256)
            }
        }
    }

    fn sector(&self, track: u8, sector: u8) -> Result<Vec<u8>, String> {
        let at = self.offset(track, sector).ok_or_else(|| format!("track {track} sector {sector} isn't on the disk"))?;
        self.source.read_vec(at, 256)
    }

    /// A chain of sectors' data: each holds 254 bytes after the link, and the last says how many of its own it uses.
    fn chain(&self, track: u8, sector: u8, limit: usize) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        let (mut t, mut s) = (track, sector);
        let mut seen = std::collections::HashSet::new();
        while t != 0 {
            if !seen.insert((t, s)) || out.len() > limit {
                return Err("The file's chain of sectors loops back on itself.".into());
            }
            let data = self.sector(t, s)?;
            if data[0] == 0 {
                let used = usize::from(data[1]).max(1);
                out.extend_from_slice(&data[2..(used + 1).min(256)]);
                break;
            }
            out.extend_from_slice(&data[2..]);
            (t, s) = (data[0], data[1]);
        }
        Ok(out)
    }
}

impl Volume for Disk {
    fn describe(&self) -> String {
        let drive = match self.kind {
            Kind::D64 => "1541 disk (D64)",
            Kind::D71 => "1571 disk (D71)",
            Kind::D81 => "1581 disk (D81)",
        };
        format!("Commodore {drive} \u{B7} \"{}\" {}", self.name, self.id)
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        if dir.is_some() {
            return Ok(Vec::new());
        }
        let (mut t, mut s) = match self.kind {
            Kind::D81 => (40, 3),
            _ => (18, 1),
        };
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        while t != 0 && seen.insert((t, s)) {
            let data = self.sector(t, s)?;
            for record in data.as_chunks::<32>().0.iter() {
                let kind = record[2];
                // A scratched file has no type and isn't shown, as the drive doesn't show it.
                if kind & 0x07 == 0 && kind & 0x80 == 0 {
                    continue;
                }
                let name = petscii_name(&record[5..21]);
                let blocks = u64::from(u16::from_le_bytes([record[30], record[31]]));
                let type_name = FILE_TYPES[usize::from(kind & 0x07)];
                let mut entry = Entry::file(tidy_name(&name), blocks * 254, u64::from(record[3]) << 8 | u64::from(record[4]));
                entry.kind = format!("{type_name}{}{}", if kind & 0x80 == 0 { " (not closed)" } else { "" }, if kind & 0x40 != 0 { " (locked)" } else { "" });
                entry.detail = format!("{blocks} blocks");
                entry.open_name = format!("{}.{}", tidy_name(&name), type_name.to_lowercase());
                out.push(entry);
            }
            (t, s) = (data[0], data[1]);
        }
        Ok(out)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        // A DEL entry is a scratched file or, on many disks, a line of directory art: it has no file to read.
        if entry.kind.starts_with("DEL") {
            return Err("A DEL entry holds no file: on this disk it's part of the directory's own layout.".into());
        }
        self.chain((entry.location >> 8) as u8, entry.location as u8, 16 * 1024 * 1024)
    }

    fn notes(&self) -> Vec<String> {
        vec!["Sizes are the directory's, in 254-byte blocks; a file opened has its exact length.".into()]
    }
}

/// A T64 tape archive: a 64-byte header, then 32-byte entries giving each file's load address and offset.
pub struct Tape {
    source: Arc<dyn Source>,
    name: String,
    entries: Vec<Entry>,
}

impl Tape {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let head = source.read_vec(0, 64.min(source.len() as usize))?;
        if head.len() < 64 {
            return Err("The T64 header is cut short.".into());
        }
        let count = usize::from(u16::from_le_bytes([head[34], head[35]])).clamp(1, 1000);
        let name = String::from_utf8_lossy(&head[40..64]).trim_end_matches(['\0', ' ']).to_string();
        let table = source.read_vec(64, (count * 32).min(source.len().saturating_sub(64) as usize))?;
        let mut records: Vec<(u64, u16, u16, String, u8)> = table
            .as_chunks::<32>().0.iter()
            .filter(|r| r[0] != 0)
            .map(|r| {
                let start = u16::from_le_bytes([r[2], r[3]]);
                let end = u16::from_le_bytes([r[4], r[5]]);
                let offset = u64::from(u32::from_le_bytes([r[8], r[9], r[10], r[11]]));
                (offset, start, end, petscii_name(&r[16..32]).trim_end().to_string(), r[1])
            })
            .collect();
        // Many T64 files give a wrong end address, so a file also ends where the next one starts, or at the end.
        let mut offsets: Vec<u64> = records.iter().map(|r| r.0).chain(std::iter::once(source.len())).collect();
        offsets.sort_unstable();
        records.sort_by_key(|r| r.0);
        let entries = records
            .into_iter()
            .map(|(offset, start, end, name, kind)| {
                let next = offsets.iter().copied().find(|o| *o > offset).unwrap_or(source.len());
                let length = u64::from(end.wrapping_sub(start)).min(next.saturating_sub(offset));
                let mut entry = Entry::file(tidy_name(&name), length + 2, offset);
                entry.extra = u64::from(start);
                entry.kind = if kind == 0x82 || kind == 1 { "PRG".into() } else { format!("type {kind:02X}") };
                entry.detail = format!("loads at ${start:04X}");
                entry.open_name = format!("{}.prg", tidy_name(&name));
                entry
            })
            .collect();
        Ok(Self { source, name, entries })
    }
}

impl Volume for Tape {
    fn describe(&self) -> String {
        format!("Commodore tape archive (T64) \u{B7} {}", if self.name.is_empty() { "no name" } else { &self.name })
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        Ok(if dir.is_some() { Vec::new() } else { self.entries.clone() })
    }

    /// A file is given back as a PRG, its load address first, as the machine would save it.
    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let mut out = (entry.extra as u16).to_le_bytes().to_vec();
        out.extend(self.source.read_vec(entry.location, (entry.size - 2) as usize)?);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{find, fixture, names, volume};

    const HELLO: [u8; 14] = [0x01, 0x08, 0x0B, 0x08, 0x0A, 0x00, 0x99, 0x22, 0x48, 0x49, 0x22, 0x00, 0x00, 0x00];

    #[test]
    fn commodore_disks_list_as_the_drive_does_and_give_back_exact_files() {
        for (name, describe) in [("test.d64", "Commodore 1541 disk (D64) \u{B7} \"TEST DISK\" AB 2A"), ("test.d71", "Commodore 1571 disk (D71) \u{B7} \"DOUBLE\" CD 2A"), ("test.d81", "Commodore 1581 disk (D81) \u{B7} \"BIG DISK\" EF 3D")] {
            let disk = volume(name);
            assert_eq!(disk.describe(), describe, "{name}");
            let top = disk.list(None).unwrap();
            let hello = find(&top, "HELLO");
            assert_eq!((hello.kind.as_str(), hello.open_name.as_str()), ("PRG", "HELLO.prg"));
            assert_eq!(disk.read(hello).unwrap(), HELLO, "{name}: c1541 wrote these bytes");
        }
        let disk = volume("test.d64");
        let top = disk.list(None).unwrap();
        assert_eq!(names(&top), ["HELLO", "NOTES"]);
        assert_eq!(disk.read(find(&top, "NOTES")).unwrap(), b"SEQUENTIAL DATA\r");
        assert_eq!(find(&top, "NOTES").kind, "SEQ");
    }

    #[test]
    fn del_entries_are_listed_as_the_drive_lists_them_but_hold_no_file() {
        use super::super::{Entry, Volume};
        // Directory art: a closed DEL entry of 0 blocks, pointing at the directory track as such entries do.
        let mut bytes = crate::media::tests::fixture("test.d64");
        let entry_at = (357 + 1) * 256 + 64;
        bytes[entry_at + 2] = 0x80;
        bytes[entry_at + 3] = 18;
        bytes[entry_at + 4] = 1;
        bytes[entry_at + 5..entry_at + 21].copy_from_slice(&[0x60; 16]);
        let disk: Box<dyn Volume> = super::super::open(std::sync::Arc::new(bytes), "art.d64").unwrap();
        let top = disk.list(None).unwrap();
        let art: &Entry = &top[2];
        assert_eq!((art.kind.as_str(), art.size), ("DEL", 0));
        assert!(disk.read(art).unwrap_err().contains("DEL"));
    }

    #[test]
    fn a_tape_archive_gives_back_its_files_as_prgs() {
        let tape = volume("test.t64");
        assert!(tape.describe().contains("TEST TAPE"));
        let top = tape.list(None).unwrap();
        assert_eq!(tape.read(find(&top, "HELLO")).unwrap(), HELLO);
        assert!(fixture("test.t64").len() > 64);
    }
}
