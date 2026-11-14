//! Atari 8-bit disks (400, 800, XL, XE): ATR images (a 16-byte header, then the sectors) and XFD (the sectors
//! alone), with Atari DOS 2.0 and 2.5, MyDOS, or SpartaDOS on them. Sectors count from 1; single and enhanced
//! density disks have 128-byte sectors, double density 256, except the first three, which are always 128.
//!
//! Atari DOS 2 and MyDOS: the VTOC is sector 360, the directory the 8 sectors after it, 8 entries of 16 bytes each:
//! a flag (0 for the end of the directory, bit 7 deleted, bit 6 in use, bit 5 locked, bit 4 a MyDOS folder), the
//! sectors used, the first sector, and an 8.3 name. The last 3 bytes of each of a file's sectors are its file number
//! and the next sector (10 bits, or all 16 on a MyDOS disk of more than 1023 sectors) and how many bytes it holds.
//!
//! SpartaDOS: sector 1 says where the root directory's sector map is, the disk's size and its name. Every file,
//! directories too, is a chain of sector maps: the next and previous map, then the file's sectors in turn. A
//! directory is 23-byte entries, the first describing itself: a status (bit 3 in use, 4 deleted, 5 a folder, 0
//! locked, 1 hidden), the file's first sector map, its length, its 8.3 name and when it was written.

use super::{Entry, Source, Volume, tidy_name};
use std::sync::Arc;

/// An image's sectors, numbered from 1.
struct Disk {
    source: Arc<dyn Source>,
    offset: u64,
    sector_size: u64,
    sectors: u64,
    /// Whether the first three sectors take a whole sector's space each, as some double-density images have them.
    padded_boot: bool,
}

impl Disk {
    fn length(&self, sector: u64) -> u64 {
        if sector <= 3 { 128 } else { self.sector_size }
    }

    fn sector(&self, sector: u64) -> Result<Vec<u8>, String> {
        if sector == 0 || sector > self.sectors {
            return Err(format!("sector {sector} isn't on the disk"));
        }
        let at = if sector <= 3 || self.sector_size == 128 || self.padded_boot {
            (sector - 1) * if sector <= 3 && !self.padded_boot { 128 } else { self.sector_size }
        } else {
            3 * 128 + (sector - 4) * self.sector_size
        };
        let length = self.length(sector);
        if self.offset + at + length > self.source.len() {
            return Err(format!("sector {sector} is past the end of the image"));
        }
        self.source.read_vec(self.offset + at, length as usize)
    }
}

fn le16(b: &[u8], at: usize) -> u64 {
    u64::from(u16::from_le_bytes([b[at], b[at + 1]]))
}

fn name_of(bytes: &[u8]) -> String {
    let text = |b: &[u8]| b.iter().map(|&c| char::from(c & 0x7F)).collect::<String>().trim_end().to_string();
    let (name, extension) = (text(&bytes[..8]), text(&bytes[8..11]));
    if extension.is_empty() { name } else { format!("{name}.{extension}") }
}

struct File {
    name: String,
    folder: Option<usize>,
    is_dir: bool,
    /// Atari DOS: the first sector and the file number; SpartaDOS: the first sector map and the length.
    start: u64,
    number: u64,
    length: u64,
    locked: bool,
    detail: String,
}

enum System {
    Dos2,
    Sparta,
}

pub struct Atari8 {
    disk: Disk,
    system: System,
    files: Vec<File>,
    what: String,
    notes: Vec<String>,
    /// MyDOS on a disk larger than an enhanced-density one links sectors with all 16 bits; DOS 2.5's 1040-sector
    /// disks keep the 10-bit links and the file number.
    wide_links: bool,
}

pub fn looks_like(head: &[u8], len: u64, ending: &str) -> bool {
    (head.len() >= 16 && head[0] == 0x96 && head[1] == 0x02) || (ending == "xfd" && len >= 720 * 128 && len.is_multiple_of(128))
}

impl Atari8 {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let head = source.read_vec(0, source.len().min(16) as usize)?;
        let (offset, sector_size, data) = if head.len() == 16 && head[0] == 0x96 && head[1] == 0x02 {
            let paragraphs = le16(&head, 2) | u64::from(head[6]) << 16;
            (16, le16(&head, 4), (paragraphs * 16).min(source.len().saturating_sub(16)))
        } else {
            // An XFD: double density by its size, otherwise 128-byte sectors.
            let len = source.len();
            (0, if len == 183_936 || len == 184_320 { 256 } else { 128 }, len)
        };
        if !matches!(sector_size, 128 | 256 | 512) {
            return Err(format!("This Atari image says its sectors are {sector_size} bytes, which isn't a size Atari disks have."));
        }
        let padded_boot = sector_size > 128 && data.is_multiple_of(sector_size) && !(data - 384).is_multiple_of(sector_size);
        let sectors = if sector_size == 128 || padded_boot { data / sector_size } else { 3 + data.saturating_sub(384) / sector_size };
        let disk = Disk { source, offset, sector_size, sectors, padded_boot };
        let density = match (sector_size, sectors) {
            (128, 720) => "single density",
            (128, 1040) => "enhanced density",
            (256, 720) => "double density",
            _ => "",
        };
        let size = if density.is_empty() { format!("{sectors} sectors of {sector_size} bytes") } else { format!("{density}, {sectors} sectors") };
        let boot = disk.sector(1)?;
        // SpartaDOS's boot sector names its file system in byte 7, and its directory, size and name after it.
        if matches!(boot[7], 0x80 | 0x40) && matches!(boot[32], 0x10 | 0x11 | 0x20 | 0x21) && le16(&boot, 9) > 0 && (1..=sectors).contains(&le16(&boot, 11)) {
            let name: String = boot[22..30].iter().map(|&c| char::from(c & 0x7F)).collect::<String>().trim_end().to_string();
            let mut atari = Self { disk, system: System::Sparta, files: Vec::new(), what: format!("Atari 8-bit SpartaDOS disk, \"{name}\" ({size})"), notes: Vec::new(), wide_links: false };
            atari.sparta_directory(le16(&boot, 9), None, 0)?;
            return Ok(atari);
        }
        let vtoc = disk.sector(360).map_err(|_| "This Atari disk is too small to have Atari DOS's directory.".to_string())?;
        // MyDOS's VTOC on a large disk says in its first byte how many 256-byte pieces its bitmap takes, plus 2.
        if !matches!(vtoc[0], 0..=3) && !(sectors > 1040 && u64::from(vtoc[0]) == 2 + (sectors / 8 + 266) / 256) {
            return Err("This Atari disk has no Atari DOS VTOC: it may boot a program of its own. It can still be shown in hex.".into());
        }
        let mydos = sectors > 1040 || vtoc[0] == 3;
        let what = format!("Atari 8-bit {} disk ({size})", if mydos { "MyDOS" } else { "Atari DOS 2" });
        let wide_links = sectors > 1040 || (sectors > 1023 && vtoc[0] >= 3);
        let mut atari = Self { disk, system: System::Dos2, files: Vec::new(), what, notes: Vec::new(), wide_links };
        atari.dos2_directory(361, None, 0)?;
        // A file's first sector carries its file number. When most don't, what's in the directory's place isn't one:
        // often a game's own text, on a disk that boots itself.
        let checked: Vec<bool> = atari
            .files
            .iter()
            .filter(|f| !f.is_dir && f.folder.is_none())
            .map(|f| atari.disk.sector(f.start).ok().is_some_and(|data| atari.wide_links || u64::from(data[data.len() - 3] >> 2) == f.number))
            .collect();
        let wrong = checked.iter().filter(|ok| !**ok).count();
        // An empty directory is believed only on a disk DOS has freshly formatted: its VTOC counting the sectors
        // DOS 2 gives, every one of them free, and nothing written in them. (A game that loads itself may carry
        // a blank DOS's VTOC, its own code in the sectors that says are free.)
        let formatted = vtoc[0] == 2 && matches!(le16(&vtoc, 1), 707 | 1010) && le16(&vtoc, 3) == 707.min(le16(&vtoc, 1)) && (0..720).all(|n| (vtoc[10 + n / 8] & (0x80 >> (n % 8)) != 0) == (n >= 4 && !(360..=368).contains(&n)))
            && (4..360).chain(369..720).all(|n| atari.disk.sector(n).is_ok_and(|data| data.iter().all(|&b| b == 0)));
        // A larger disk MyDOS has freshly formatted: every sector its VTOC counts free, and nothing in them.
        let formatted = formatted
            || (mydos && le16(&vtoc, 1) > 0 && le16(&vtoc, 1) == le16(&vtoc, 3) && (4..340).chain(369..=sectors.min(4096)).all(|n| atari.disk.sector(n).is_ok_and(|data| data.iter().all(|&b| b == 0))));
        if (atari.files.is_empty() && !formatted && !atari.disk.sector(361)?.iter().any(|&b| b != 0)) || wrong * 2 > checked.len() {
            return Err("This Atari disk has no Atari DOS directory: it may boot a program of its own. It can still be shown in hex.".into());
        }
        Ok(atari)
    }

    /// An Atari DOS 2 or MyDOS directory: 8 sectors of 8 entries.
    fn dos2_directory(&mut self, first: u64, folder: Option<usize>, depth: usize) -> Result<(), String> {
        if depth > 8 {
            return Ok(());
        }
        let mut number = 0;
        'sectors: for sector in first..first + 8 {
            let data = self.disk.sector(sector)?;
            for entry in data[..128].as_chunks::<16>().0 {
                let flag = entry[0];
                if flag == 0 {
                    break 'sectors;
                }
                let this = number;
                number += 1;
                if flag & 0x80 != 0 || flag & 0x53 == 0 {
                    continue;
                }
                let name = name_of(&entry[5..16]);
                let (count, start) = (le16(entry, 1), le16(entry, 3));
                let is_dir = flag & 0x10 != 0;
                self.files.push(File { name, folder, is_dir, start, number: this, length: count, locked: flag & 0x20 != 0, detail: format!("{count} sectors from {start}") });
                // A folder on MyDOS; on another DOS the same flag bit can mean something else, so a folder that can't
                // be read is noted rather than stopping the disk opening.
                if is_dir {
                    let index = self.files.len() - 1;
                    if let Err(problem) = self.dos2_directory(start, Some(index), depth + 1) {
                        self.notes.push(format!("The folder {} can't be read: {problem}.", self.files[index].name));
                    }
                }
            }
        }
        Ok(())
    }

    /// A file's sectors under Atari DOS 2 or MyDOS, by their links. A sector whose file number isn't the file's is
    /// someone else's: the chain is broken there.
    fn dos2_read(&self, file: &File) -> Result<Vec<u8>, String> {
        let size = self.disk.sector_size;
        let wide = self.wide_links;
        let mut out = Vec::new();
        let mut sector = file.start;
        let mut seen = 0;
        while sector != 0 && seen < self.disk.sectors {
            seen += 1;
            let data = self.disk.sector(sector)?;
            let tail = data.len() - 3;
            let (a, b, count) = (data[tail], data[tail + 1], data[tail + 2]);
            if !wide && u64::from(a >> 2) != file.number {
                return Err(format!("sector {sector} belongs to another file, so this one's chain is broken"));
            }
            let used = if size == 128 { usize::from(count & 0x7F) } else { usize::from(count) };
            out.extend_from_slice(&data[..used.min(tail)]);
            sector = if wide { u64::from(a) << 8 | u64::from(b) } else { u64::from(a & 3) << 8 | u64::from(b) };
        }
        Ok(out)
    }

    /// A SpartaDOS file's bytes, through its sector maps.
    fn sparta_read(&self, first_map: u64, length: u64) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(length as usize);
        let mut map = first_map;
        let mut seen = 0;
        while map != 0 && (out.len() as u64) < length && seen < self.disk.sectors {
            seen += 1;
            let data = self.disk.sector(map)?;
            for pointer in data[4..].as_chunks::<2>().0 {
                if out.len() as u64 >= length {
                    break;
                }
                let sector = u64::from(u16::from_le_bytes(*pointer));
                if sector == 0 {
                    out.resize(out.len() + self.disk.sector_size as usize, 0);
                } else {
                    out.extend(self.disk.sector(sector)?);
                }
            }
            map = le16(&data, 0);
        }
        out.truncate(length as usize);
        Ok(out)
    }

    fn sparta_directory(&mut self, map: u64, folder: Option<usize>, depth: usize) -> Result<(), String> {
        if depth > 16 {
            return Ok(());
        }
        // The directory's own entry, first, says how long it is.
        let head = self.sparta_read(map, 23)?;
        let data = self.sparta_read(map, u64::from(head[3]) | u64::from(head[4]) << 8 | u64::from(head[5]) << 16)?;
        for entry in data.as_chunks::<23>().0.iter().skip(1) {
            let status = entry[0];
            if status == 0 {
                break;
            }
            if status & 0x10 != 0 || status & 0x08 == 0 {
                continue;
            }
            let is_dir = status & 0x20 != 0;
            let (first, length) = (le16(entry, 1), u64::from(entry[3]) | u64::from(entry[4]) << 8 | u64::from(entry[5]) << 16);
            let year = u64::from(entry[19]);
            let year = if year < 78 { 2000 + year } else { 1900 + year };
            let detail = if entry[17] == 0 { String::new() } else { format!("{year}-{:02}-{:02} {:02}:{:02}", entry[18], entry[17], entry[20], entry[21]) };
            self.files.push(File { name: name_of(&entry[6..17]), folder, is_dir, start: first, number: 0, length, locked: status & 0x01 != 0, detail });
            if is_dir {
                let index = self.files.len() - 1;
                self.sparta_directory(first, Some(index), depth + 1)?;
            }
        }
        Ok(())
    }
}

impl Volume for Atari8 {
    fn describe(&self) -> String {
        self.what.clone()
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        let folder = dir.map(|d| d.location as usize);
        let mut out = Vec::new();
        for (index, file) in self.files.iter().enumerate().filter(|(_, f)| f.folder == folder) {
            if file.is_dir {
                let mut entry = Entry::dir(tidy_name(&file.name), index as u64, 0);
                entry.detail = file.detail.clone();
                out.push(entry);
                continue;
            }
            // Atari DOS keeps sectors, not lengths: each holds up to 125 (or 253) bytes.
            let size = match self.system {
                System::Sparta => file.length,
                System::Dos2 => file.length * (self.disk.sector_size - 3),
            };
            let mut entry = Entry::file(tidy_name(&file.name), size, index as u64);
            entry.kind = if file.locked { "File, locked".into() } else { "File".into() };
            entry.detail = file.detail.clone();
            out.push(entry);
        }
        Ok(out)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let file = self.files.get(entry.location as usize).ok_or("That file isn't on the disk.")?;
        match self.system {
            System::Dos2 => self.dos2_read(file),
            System::Sparta => self.sparta_read(file.start, file.length),
        }
    }

    fn notes(&self) -> Vec<String> {
        let mut notes = self.notes.clone();
        if matches!(self.system, System::Dos2) {
            notes.push("Atari DOS keeps sizes in sectors, so a file's size here is at most what it holds; opened, it's exact.".into());
        }
        notes
    }

    /// Text on an Atari is ATASCII, its lines ending with byte $9B.
    fn text_preset(&self, _entry: &Entry) -> Option<&'static str> {
        Some("atari-8bit")
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A single-density ATR with Atari DOS 2: one file of two sectors, "HELLO.TXT".
    pub fn dos2_atr() -> Vec<u8> {
        let mut disk = vec![0u8; 720 * 128];
        let sector = |n: usize| (n - 1) * 128;
        disk[sector(360)] = 2;
        let directory = sector(361);
        disk[directory] = 0x42;
        disk[directory + 1] = 2;
        disk[directory + 3] = 4;
        disk[directory + 5..directory + 16].copy_from_slice(b"HELLO   TXT");
        // Sector 4: 125 bytes of 'A', on to sector 5; sector 5: "BC" and the end.
        let s4 = sector(4);
        disk[s4..s4 + 125].fill(b'A');
        disk[s4 + 125] = 0;
        disk[s4 + 126] = 5;
        disk[s4 + 127] = 125;
        let s5 = sector(5);
        disk[s5..s5 + 2].copy_from_slice(b"BC");
        disk[s5 + 127] = 2;
        let mut atr = vec![0x96, 0x02];
        let paragraphs = (disk.len() / 16) as u16;
        atr.extend(paragraphs.to_le_bytes());
        atr.extend(128u16.to_le_bytes());
        atr.resize(16, 0);
        atr.extend(disk);
        atr
    }

    #[test]
    fn an_atari_dos_2_file_follows_its_sector_links() {
        let atr = dos2_atr();
        assert!(looks_like(&atr[..64], atr.len() as u64, "atr"));
        let atari = Atari8::open(Arc::new(atr)).unwrap();
        assert!(atari.describe().contains("Atari DOS 2") && atari.describe().contains("single density"), "{}", atari.describe());
        let files = atari.list(None).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].name, "HELLO.TXT");
        let data = atari.read(&files[0]).unwrap();
        assert_eq!(data.len(), 127);
        assert_eq!(&data[123..], b"AABC");
        assert_eq!(atari.text_preset(&files[0]), Some("atari-8bit"));
    }

    #[test]
    fn double_density_keeps_its_first_three_sectors_short() {
        let mut disk = vec![0u8; 3 * 128 + 717 * 256];
        disk[384 + 356 * 256] = 2; // the VTOC, sector 360
        let directory = 384 + 357 * 256;
        disk[directory] = 0x42;
        disk[directory + 1] = 1;
        disk[directory + 3] = 4;
        disk[directory + 5..directory + 16].copy_from_slice(b"DD      DAT");
        disk[384..387].copy_from_slice(b"XYZ");
        disk[384 + 255] = 3;
        let mut atr = vec![0x96, 0x02];
        atr.extend(((disk.len() / 16) as u16).to_le_bytes());
        atr.extend(256u16.to_le_bytes());
        atr.resize(16, 0);
        atr.extend(disk);
        let atari = Atari8::open(Arc::new(atr)).unwrap();
        assert!(atari.describe().contains("double density"), "{}", atari.describe());
        assert_eq!(atari.read(&atari.list(None).unwrap()[0]).unwrap(), b"XYZ");
    }
}
