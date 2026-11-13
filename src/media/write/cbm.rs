//! Commodore disks, D64, D71 and D81, written as the drives write them: a file is a chain of sectors, laid out
//! from the directory track outwards with the drive's interleave, each sector marked used in the BAM; a directory
//! entry gives its type, name, first sector and length in blocks. A deleted file is scratched, as the drive does:
//! its type cleared and its sectors freed.

use super::{Addition, Entry, ImageWriter, Kind as FileKind, already};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    D64,
    D71,
    D81,
}

pub struct Disk {
    bytes: Vec<u8>,
    kind: Kind,
}

/// The sectors on a 1541 track.
fn sectors_1541(track: u8) -> u8 {
    match track {
        1..=17 => 21,
        18..=24 => 19,
        25..=30 => 18,
        _ => 17,
    }
}

/// A name in PETSCII, sixteen bytes padded with shifted spaces. Lower case is taken as the capitals it shows as,
/// and the characters a drive command would trip on are left out.
fn petscii(name: &str) -> [u8; 16] {
    let mut out = [0xA0; 16];
    let bytes = name.chars().filter_map(|c| match c {
        'a'..='z' => Some(c.to_ascii_uppercase() as u8),
        '\u{A3}' => Some(0x5C),
        ',' | ':' | '*' | '?' | '=' | '"' => None,
        ' '..='@' | 'A'..='Z' | '[' | ']' => Some(c as u8),
        _ => None,
    });
    for (slot, byte) in out.iter_mut().zip(bytes) {
        *slot = byte;
    }
    out
}

/// A blank disk, formatted as the drive's NEW command leaves it, named and with the ID "LM".
pub fn blank(kind: Kind, label: &str) -> Vec<u8> {
    let size = match kind {
        Kind::D64 => 174_848,
        Kind::D71 => 349_696,
        Kind::D81 => 819_200,
    };
    let mut disk = Disk { bytes: vec![0; size], kind };
    let name = petscii(label);
    match kind {
        Kind::D81 => {
            let header = disk.offset(40, 0).unwrap();
            let h = &mut disk.bytes[header..header + 256];
            h[0] = 40;
            h[1] = 3;
            h[2] = b'D';
            h[4..20].copy_from_slice(&name);
            h[20..22].copy_from_slice(&[0xA0, 0xA0]);
            h[22..24].copy_from_slice(b"LM");
            h[24] = 0xA0;
            h[25..27].copy_from_slice(b"3D");
            h[27..29].copy_from_slice(&[0xA0, 0xA0]);
            for (sector, link) in [(1u8, (40u8, 2u8)), (2, (0, 0xFF))] {
                let at = disk.offset(40, sector).unwrap();
                let b = &mut disk.bytes[at..at + 256];
                b[0] = link.0;
                b[1] = link.1;
                b[2] = b'D';
                b[3] = 0xBB;
                b[4..6].copy_from_slice(b"LM");
                b[6] = 0xC0;
            }
            let dir = disk.offset(40, 3).unwrap();
            disk.bytes[dir + 1] = 0xFF;
        }
        _ => {
            let header = disk.offset(18, 0).unwrap();
            let h = &mut disk.bytes[header..header + 256];
            h[0] = 18;
            h[1] = 1;
            h[2] = 0x41;
            h[3] = if kind == Kind::D71 { 0x80 } else { 0 };
            h[0x90..0xA0].copy_from_slice(&name);
            h[0xA0..0xA2].copy_from_slice(&[0xA0, 0xA0]);
            h[0xA2..0xA4].copy_from_slice(b"LM");
            h[0xA4] = 0xA0;
            h[0xA5..0xA7].copy_from_slice(b"2A");
            h[0xA7..0xAB].copy_from_slice(&[0xA0; 4]);
            let dir = disk.offset(18, 1).unwrap();
            disk.bytes[dir + 1] = 0xFF;
        }
    }
    for track in 1..=disk.tracks() {
        for sector in 0..disk.sectors(track) {
            disk.set_free(track, sector, true);
        }
    }
    // The header and BAM sectors are marked used, and on a 1571 the whole of the second side's BAM track; the rest
    // of the directory track is kept for the directory by the drive, though the BAM shows it free.
    let own: &[u8] = match kind {
        Kind::D64 => &[18],
        Kind::D71 => &[18, 53],
        Kind::D81 => &[40],
    };
    for track in own {
        for sector in 0..disk.sectors(*track) {
            if *track == 53 || sector <= if *track == 40 { 3 } else { 1 } {
                disk.set_free(*track, sector, false);
            }
        }
    }
    disk.bytes
}

impl Disk {
    pub fn open(bytes: Vec<u8>, kind: Kind) -> Result<Self, String> {
        let disk = Self { bytes, kind };
        let fits = match kind {
            Kind::D64 => matches!(disk.bytes.len(), 174_848 | 175_531 | 196_608 | 197_376),
            Kind::D71 => matches!(disk.bytes.len(), 349_696 | 351_062),
            Kind::D81 => matches!(disk.bytes.len(), 819_200 | 822_400),
        };
        if !fits {
            return Err("The disk image isn't a size its drive makes, so it isn't changed here.".into());
        }
        let (track, mark) = if kind == Kind::D81 { (40, b'D') } else { (18, 0x41) };
        let header = disk.offset(track, 0).ok_or("The disk has no header.")?;
        if disk.bytes[header + 2] != mark {
            return Err("The disk's header isn't a DOS this writes for, so it isn't changed here.".into());
        }
        Ok(disk)
    }

    /// The tracks files may use: a 40-track 1541 disk's last five are left to the speeders that made them.
    fn tracks(&self) -> u8 {
        match self.kind {
            Kind::D64 => 35,
            Kind::D71 => 70,
            Kind::D81 => 80,
        }
    }

    fn sectors(&self, track: u8) -> u8 {
        match self.kind {
            Kind::D81 => 40,
            Kind::D71 if track > 35 => sectors_1541(track - 35),
            _ => sectors_1541(track),
        }
    }

    fn offset(&self, track: u8, sector: u8) -> Option<usize> {
        if track == 0 || track > self.tracks() || sector >= self.sectors(track) {
            return None;
        }
        let before: usize = (1..track).map(|t| usize::from(self.sectors(t))).sum();
        Some((before + usize::from(sector)) * 256)
    }

    fn sector(&self, track: u8, sector: u8) -> Result<usize, String> {
        self.offset(track, sector).ok_or_else(|| format!("track {track} sector {sector} isn't on the disk"))
    }

    fn directory_track(&self) -> u8 {
        if self.kind == Kind::D81 { 40 } else { 18 }
    }

    /// Where a track's count of free sectors is in the BAM, and where its bitmap starts.
    fn bam(&self, track: u8) -> (usize, usize) {
        match self.kind {
            Kind::D81 => {
                let at = self.offset(40, if track <= 40 { 1 } else { 2 }).unwrap() + 0x10 + usize::from((track - 1) % 40) * 6;
                (at, at + 1)
            }
            Kind::D71 if track > 35 => (self.offset(18, 0).unwrap() + 0xDD + usize::from(track - 36), self.offset(53, 0).unwrap() + usize::from(track - 36) * 3),
            _ => {
                let at = self.offset(18, 0).unwrap() + 4 * usize::from(track);
                (at, at + 1)
            }
        }
    }

    fn is_free(&self, track: u8, sector: u8) -> bool {
        let (_, map) = self.bam(track);
        self.bytes[map + usize::from(sector / 8)] & (1 << (sector % 8)) != 0
    }

    fn set_free(&mut self, track: u8, sector: u8, free: bool) {
        if self.is_free(track, sector) == free {
            return;
        }
        let (count, map) = self.bam(track);
        self.bytes[map + usize::from(sector / 8)] ^= 1 << (sector % 8);
        self.bytes[count] = if free { self.bytes[count].wrapping_add(1) } else { self.bytes[count].wrapping_sub(1) };
    }

    /// Free sectors for files: every track but the directory's (and a 1571's second BAM track).
    fn free_sectors(&self) -> usize {
        (1..=self.tracks()).filter(|t| !self.reserved(*t)).map(|t| (0..self.sectors(t)).filter(|s| self.is_free(t, *s)).count()).sum()
    }

    fn reserved(&self, track: u8) -> bool {
        track == self.directory_track() || (self.kind == Kind::D71 && track == 53)
    }

    /// The next sector for a file, as the drive picks it: on from the last one by the interleave on the same
    /// track, or on the nearest track to the directory with room, working outwards on both sides.
    fn next_free(&self, after: Option<(u8, u8)>) -> Option<(u8, u8)> {
        let interleave = if self.kind == Kind::D81 { 1 } else { 10 };
        let order = self.track_order();
        let start = after.map_or(0, |(t, _)| order.iter().position(|o| *o == t).unwrap_or(0));
        for (step, track) in order.iter().copied().enumerate().skip(start) {
            let count = self.sectors(track);
            let first = match after {
                Some((t, s)) if step == start && t == track => (s + interleave) % count,
                _ => 0,
            };
            for n in 0..count {
                let sector = (first + n) % count;
                if self.is_free(track, sector) {
                    return Some((track, sector));
                }
            }
        }
        None
    }

    /// The tracks in the order files fill them: from beside the directory outwards, below it first.
    fn track_order(&self) -> Vec<u8> {
        let middle = self.directory_track();
        let mut order = Vec::new();
        for distance in 1..=self.tracks() {
            for track in [middle.checked_sub(distance), middle.checked_add(distance)].into_iter().flatten() {
                if (1..=self.tracks()).contains(&track) && !self.reserved(track) {
                    order.push(track);
                }
            }
        }
        // A 1571's second side is used once the first is full, as the drive does in 1541 mode.
        if self.kind == Kind::D71 {
            order.sort_by_key(|t| *t > 35);
        }
        order
    }

    /// The directory's sectors, in their chain.
    fn directory(&self) -> Result<Vec<(u8, u8)>, String> {
        let track = self.directory_track();
        let (mut t, mut s) = (track, if self.kind == Kind::D81 { 3 } else { 1 });
        let mut out = Vec::new();
        while t != 0 {
            if out.contains(&(t, s)) || out.len() > 40 {
                return Err("The directory's chain loops back on itself.".into());
            }
            out.push((t, s));
            let at = self.sector(t, s)?;
            (t, s) = (self.bytes[at], self.bytes[at + 1]);
        }
        Ok(out)
    }

    /// Where each directory entry is in the image, with a file on it or not.
    fn slots(&self) -> Result<Vec<usize>, String> {
        let mut out = Vec::new();
        for (t, s) in self.directory()? {
            let at = self.sector(t, s)?;
            out.extend((0..8).map(|i| at + i * 32));
        }
        Ok(out)
    }

    fn slot_of(&self, entry: &Entry) -> Result<usize, String> {
        let (track, sector) = ((entry.location >> 8) as u8, entry.location as u8);
        self.slots()?
            .into_iter()
            .find(|at| self.bytes[at + 2] & 0x07 != 0 && self.bytes[at + 3] == track && self.bytes[at + 4] == sector)
            .ok_or_else(|| "That file isn't in the disk's directory.".into())
    }

    /// A free directory entry, the directory grown by a sector on its track if it's full.
    fn free_slot(&mut self) -> Result<usize, String> {
        if let Some(at) = self.slots()?.into_iter().find(|at| self.bytes[at + 2] == 0) {
            return Ok(at);
        }
        let track = self.directory_track();
        let (last_t, last_s) = *self.directory()?.last().unwrap();
        let interleave = if self.kind == Kind::D81 { 1 } else { 3 };
        let count = self.sectors(track);
        let sector = (0..count).map(|n| (last_s + interleave + n) % count).find(|s| self.is_free(track, *s)).ok_or("The directory is full.")?;
        self.set_free(track, sector, false);
        let last = self.sector(last_t, last_s)?;
        self.bytes[last] = track;
        self.bytes[last + 1] = sector;
        let at = self.sector(track, sector)?;
        self.bytes[at..at + 256].fill(0);
        self.bytes[at + 1] = 0xFF;
        Ok(at)
    }

    /// The sectors a file's chain runs through.
    fn chain(&self, track: u8, sector: u8) -> Result<Vec<(u8, u8)>, String> {
        let (mut t, mut s) = (track, sector);
        let mut out = Vec::new();
        while t != 0 {
            if out.contains(&(t, s)) || out.len() > 4000 {
                return Err("The file's chain of sectors loops back on itself.".into());
            }
            out.push((t, s));
            let at = self.sector(t, s)?;
            (t, s) = (self.bytes[at], self.bytes[at + 1]);
        }
        Ok(out)
    }

    fn name_taken(&self, name: &[u8; 16]) -> Result<Option<usize>, String> {
        Ok(self.slots()?.into_iter().find(|at| self.bytes[at + 2] & 0x07 != 0 && self.bytes[at + 5..at + 21] == *name))
    }
}

impl ImageWriter for Disk {
    fn kinds(&self) -> Vec<FileKind> {
        vec![
            FileKind::plain("PRG", "A program, or any file that starts with the address it loads at."),
            FileKind::plain("SEQ", "A sequential file: text or data, read a byte at a time."),
            FileKind::plain("USR", "A user file, kept as it is."),
        ]
    }

    fn guess(&self, name: &str, bytes: &[u8]) -> Addition {
        let (stem, ending) = super::stem_and_ending(name);
        let ending = ending.to_lowercase();
        // A PC64 file (.P00, .S00...) has a header naming the file, before its own bytes.
        if bytes.starts_with(b"C64File\0") && bytes.len() >= 26 {
            let inner: String = bytes[8..24].iter().take_while(|b| **b != 0).map(|b| char::from(*b)).collect();
            let kind = match ending.chars().next() {
                Some('s') => "SEQ",
                Some('u') => "USR",
                _ => "PRG",
            };
            return Addition { name: inner, bytes: bytes[26..].to_vec(), kind: kind.into(), ..Default::default() };
        }
        let kind = match ending.as_str() {
            "seq" | "txt" => "SEQ",
            "usr" => "USR",
            _ => "PRG",
        };
        Addition { name: stem.to_string(), bytes: bytes.to_vec(), kind: kind.into(), ..Default::default() }
    }

    fn add(&mut self, _folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        let name = petscii(&file.name);
        if name[0] == 0xA0 {
            return Err("The name has no characters a Commodore disk can hold.".into());
        }
        if let Some(at) = self.name_taken(&name)? {
            if !file.replace {
                return Err(already(&file.name));
            }
            let (t, s) = (self.bytes[at + 3], self.bytes[at + 4]);
            self.delete(None, &Entry::file(String::new(), 0, u64::from(t) << 8 | u64::from(s)))?;
        }
        let blocks = file.bytes.len().div_ceil(254).max(1);
        if blocks > self.free_sectors() {
            return Err(format!("{} needs {blocks} blocks; the disk has {} free.", file.name, self.free_sectors()));
        }
        let slot = self.free_slot()?;
        let mut sectors = Vec::with_capacity(blocks);
        let mut last = None;
        for _ in 0..blocks {
            let next = self.next_free(last).ok_or("The disk is full.")?;
            self.set_free(next.0, next.1, false);
            sectors.push(next);
            last = Some(next);
        }
        for (index, (t, s)) in sectors.iter().enumerate() {
            let at = self.sector(*t, *s)?;
            let part = file.bytes.get(index * 254..((index + 1) * 254).min(file.bytes.len())).unwrap_or(&[]);
            self.bytes[at..at + 256].fill(0);
            match sectors.get(index + 1) {
                Some((nt, ns)) => {
                    self.bytes[at] = *nt;
                    self.bytes[at + 1] = *ns;
                }
                // The last sector says where its bytes end: the index of the last one used.
                None => self.bytes[at + 1] = (part.len() + 1) as u8,
            }
            self.bytes[at + 2..at + 2 + part.len()].copy_from_slice(part);
        }
        let kind = match file.kind.as_str() {
            "SEQ" => 1,
            "USR" => 3,
            _ => 2,
        };
        let entry = &mut self.bytes[slot + 2..slot + 32];
        entry.fill(0);
        entry[0] = 0x80 | kind;
        entry[1] = sectors[0].0;
        entry[2] = sectors[0].1;
        entry[3..19].copy_from_slice(&name);
        entry[28..30].copy_from_slice(&(blocks as u16).to_le_bytes());
        Ok(())
    }

    fn delete(&mut self, _folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        let slot = self.slot_of(entry)?;
        let kind = self.bytes[slot + 2] & 0x07;
        if kind == 4 {
            return Err("A relative (REL) file's side sectors aren't followed here, so it isn't scratched.".into());
        }
        for (t, s) in self.chain(self.bytes[slot + 3], self.bytes[slot + 4])? {
            if !self.reserved(t) {
                self.set_free(t, s, true);
            }
        }
        self.bytes[slot + 2] = 0;
        Ok(())
    }

    fn rename(&mut self, _folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        let slot = self.slot_of(entry)?;
        let new = petscii(name);
        if new[0] == 0xA0 {
            return Err("The name has no characters a Commodore disk can hold.".into());
        }
        if self.name_taken(&new)?.is_some_and(|at| at != slot) {
            return Err(already(name));
        }
        self.bytes[slot + 5..slot + 21].copy_from_slice(&new);
        Ok(())
    }

    fn free(&self) -> Option<u64> {
        Some(self.free_sectors() as u64 * 254)
    }

    fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{entry, names, reopened, tool};
    use super::*;
    use std::process::Command;

    #[test]
    fn files_go_on_and_come_off_a_commodore_disk_with_its_bam_kept_right() {
        for (kind, name) in [(Kind::D64, "t.d64"), (Kind::D71, "t.d71"), (Kind::D81, "t.d81")] {
            let mut disk = Disk::open(blank(kind, "WORK"), kind).unwrap();
            let empty = disk.free_sectors();
            assert_eq!(empty, match kind {
                Kind::D64 => 664,
                Kind::D71 => 1328,
                Kind::D81 => 3160,
            });
            let big: Vec<u8> = (0..20_000u32).map(|i| (i * 7) as u8).collect();
            for (file, bytes) in [("hello.prg", &[1u8, 8, 0, 0][..]), ("notes.seq", b"SOME TEXT\r"), ("big.prg", &big)] {
                let addition = disk.guess(file, bytes);
                disk.add(None, &addition).unwrap();
            }
            assert_eq!(disk.free_sectors(), empty - 1 - 1 - 79, "{name}");
            let volume = reopened(&disk, name);
            assert_eq!(names(&*volume, None), ["HELLO", "NOTES", "BIG"]);
            assert_eq!(entry(&*volume, None, "NOTES").kind, "SEQ");
            assert_eq!(volume.read(&entry(&*volume, None, "BIG")).unwrap(), big, "{name}");
            let addition = disk.guess("hello.prg", &[9]);
            assert!(disk.add(None, &addition).unwrap_err().starts_with(super::super::ALREADY));
            disk.add(None, &Addition { replace: true, ..addition }).unwrap();
            disk.rename(None, &entry(&*reopened(&disk, name), None, "NOTES"), "readme").unwrap();
            disk.delete(None, &entry(&*reopened(&disk, name), None, "BIG")).unwrap();
            let volume = reopened(&disk, name);
            // The new HELLO takes the first free entry, where the old one was, as the drive would put it.
            assert_eq!(names(&*volume, None), ["HELLO", "README"]);
            assert_eq!(volume.read(&entry(&*volume, None, "HELLO")).unwrap(), [9]);
            assert_eq!(disk.free_sectors(), empty - 2);
        }
    }

    #[test]
    fn a_full_directory_grows_onto_its_track() {
        let mut disk = Disk::open(blank(Kind::D64, "MANY"), Kind::D64).unwrap();
        for n in 0..20 {
            disk.add(None, &Addition { name: format!("FILE{n}"), bytes: vec![n as u8], kind: "PRG".into(), ..Default::default() }).unwrap();
        }
        assert_eq!(disk.directory().unwrap().len(), 3);
        let volume = reopened(&disk, "m.d64");
        assert_eq!(volume.list(None).unwrap().len(), 20);
        assert_eq!(volume.read(&entry(&*volume, None, "FILE19")).unwrap(), [19]);
    }

    /// VICE's c1541 lists and reads what's written, and its VALIDATE (which rebuilds the BAM from the files)
    /// changes nothing: so the BAM was already what the drive makes it.
    #[test]
    fn c1541_reads_written_disks_and_validate_finds_nothing_to_fix() {
        let Some(c1541) = tool("c1541") else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("limoniter-cbm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (kind, name) in [(Kind::D64, "t.d64"), (Kind::D71, "t.d71"), (Kind::D81, "t.d81")] {
            let mut disk = Disk::open(blank(kind, "CHECKED"), kind).unwrap();
            let big: Vec<u8> = (0..9000u32).map(|i| (i * 13) as u8).collect();
            disk.add(None, &Addition { name: "BIG".into(), bytes: big.clone(), kind: "PRG".into(), ..Default::default() }).unwrap();
            disk.add(None, &Addition { name: "small".into(), bytes: b"HI\r".to_vec(), kind: "SEQ".into(), ..Default::default() }).unwrap();
            disk.add(None, &Addition { name: "GONE".into(), bytes: vec![1; 3000], kind: "PRG".into(), ..Default::default() }).unwrap();
            disk.delete(None, &entry(&*reopened(&disk, name), None, "GONE")).unwrap();
            let path = dir.join(name);
            std::fs::write(&path, disk.bytes()).unwrap();
            let listed = Command::new(&c1541).arg(&path).arg("-list").output().unwrap();
            let listed = String::from_utf8_lossy(&listed.stdout).to_lowercase();
            assert!(listed.contains("\"checked ") && listed.contains("\"big\"") && listed.contains("\"small\"") && !listed.contains("gone"), "{name}: {listed}");
            let out = dir.join(format!("{name}.big"));
            Command::new(&c1541).arg(&path).arg("-read").arg("big").arg(&out).output().unwrap();
            assert_eq!(std::fs::read(&out).unwrap(), big, "{name}");
            let before = std::fs::read(&path).unwrap();
            Command::new(&c1541).arg(&path).arg("-validate").output().unwrap();
            let after = std::fs::read(&path).unwrap();
            let changed: Vec<usize> = (0..before.len()).filter(|i| before[*i] != after[*i]).collect();
            assert!(changed.is_empty(), "{name}: VALIDATE changed bytes at {:?}", &changed[..changed.len().min(16)]);
        }
        std::fs::remove_dir_all(dir).ok();
    }
}
