//! Atari 8-bit disks with Atari DOS 2.0 or 2.5, in ATR images: single, enhanced or double density. A file is a
//! chain of sectors, each ending with the file's number (its directory slot), the next sector and how many bytes it
//! holds; the VTOC (sector 360, and on a DOS 2.5 enhanced disk sector 1024 too) marks which sectors are free.
//! MyDOS and SpartaDOS disks are only read.

use super::{Addition, Entry, ImageWriter, already};

const VTOC: usize = 360;
const DIRECTORY: usize = 361;
const VTOC2: usize = 1024;

pub struct Disk {
    bytes: Vec<u8>,
    sector_size: usize,
    sectors: usize,
}

/// A blank disk as DOS 2.0S (single density) or DOS 2.5 (enhanced density) formats one, with no DOS.SYS on it.
pub fn blank(enhanced: bool) -> Vec<u8> {
    let sectors = if enhanced { 1040 } else { 720 };
    let mut bytes = vec![0u8; 16 + sectors * 128];
    let paragraphs = sectors * 128 / 16;
    bytes[..2].copy_from_slice(&[0x96, 0x02]);
    bytes[2..4].copy_from_slice(&(paragraphs as u16).to_le_bytes());
    bytes[4..6].copy_from_slice(&128u16.to_le_bytes());
    bytes[6] = (paragraphs >> 16) as u8;
    let mut disk = Disk { bytes, sector_size: 128, sectors };
    // The boot sector of a disk formatted with no DOS on it: it boots to a "no DOS" message on a real Atari.
    let boot = disk.at(1);
    disk.bytes[boot..boot + 6].copy_from_slice(&[0x00, 0x03, 0x00, 0x07, 0x40, 0x15]);
    let vtoc = disk.at(VTOC);
    disk.bytes[vtoc] = 2;
    let usable: Vec<usize> = disk.usable().collect();
    for sector in usable {
        disk.set_free(sector, true);
    }
    let total = if enhanced { 1010u16 } else { 707 };
    disk.bytes[vtoc + 1..vtoc + 3].copy_from_slice(&total.to_le_bytes());
    disk.recount();
    disk.bytes
}

impl Disk {
    pub fn open(bytes: Vec<u8>) -> Result<Self, String> {
        if bytes.len() < 16 || bytes[..2] != [0x96, 0x02] {
            return Err("Only ATR images of Atari disks are changed here.".into());
        }
        let sector_size = usize::from(u16::from_le_bytes([bytes[4], bytes[5]]));
        let data = bytes.len() - 16;
        let sectors = match sector_size {
            128 => data / 128,
            256 if data >= 384 && (data - 384).is_multiple_of(256) => 3 + (data - 384) / 256,
            _ => return Err("Only Atari disks of 128 or 256-byte sectors, with short boot sectors, are changed here.".into()),
        };
        let disk = Self { bytes, sector_size, sectors };
        if !matches!(disk.sectors, 720 | 1040) {
            return Err("Only single, enhanced and double density Atari disks are changed here.".into());
        }
        if disk.sectors == 1040 && sector_size != 128 {
            return Err("This disk's size isn't one Atari DOS 2 makes.".into());
        }
        let vtoc = disk.at(VTOC);
        if disk.bytes[vtoc] != 2 {
            return Err("This disk isn't Atari DOS 2 (it may be MyDOS, SpartaDOS or a disk that boots itself), so it's only read.".into());
        }
        Ok(disk)
    }

    fn at(&self, sector: usize) -> usize {
        16 + if sector <= 3 { (sector - 1) * 128 } else { 384 + (sector - 4) * self.sector_size }
    }

    fn length(&self, sector: usize) -> usize {
        if sector <= 3 { 128 } else { self.sector_size }
    }

    fn enhanced(&self) -> bool {
        self.sectors == 1040
    }

    /// The sectors DOS may give to files: not the boot sectors, the VTOC and directory, or sector 720 (which DOS 2
    /// never uses), nor DOS 2.5's second VTOC and beyond.
    fn usable(&self) -> impl Iterator<Item = usize> {
        let last = if self.enhanced() { 1023 } else { 719 };
        (4..=last).filter(|s| !(VTOC..=DIRECTORY + 7).contains(s) && *s != 720)
    }

    /// A sector's bit: in the VTOC for sectors below 720, in DOS 2.5's second VTOC for those above.
    fn bit(&self, sector: usize) -> (usize, u8) {
        if sector < 720 {
            (self.at(VTOC) + 10 + sector / 8, 0x80 >> (sector % 8))
        } else {
            (self.at(VTOC2) + 84 + (sector - 720) / 8, 0x80 >> (sector % 8))
        }
    }

    fn is_free(&self, sector: usize) -> bool {
        let (at, mask) = self.bit(sector);
        self.bytes[at] & mask != 0
    }

    fn set_free(&mut self, sector: usize, free: bool) {
        let (at, mask) = self.bit(sector);
        if free {
            self.bytes[at] |= mask;
        } else {
            self.bytes[at] &= !mask;
        }
        // DOS 2.5's second VTOC repeats the first's bits for sectors 48 to 719.
        if self.enhanced() && (48..720).contains(&sector) {
            let at = self.at(VTOC2) + (sector - 48) / 8;
            if free {
                self.bytes[at] |= mask;
            } else {
                self.bytes[at] &= !mask;
            }
        }
    }

    /// Writes the free counts: the VTOC's, of sectors below 720, and DOS 2.5's of those above.
    fn recount(&mut self) {
        let low = self.usable().filter(|s| *s < 720 && self.is_free(*s)).count() as u16;
        let vtoc = self.at(VTOC);
        self.bytes[vtoc + 3..vtoc + 5].copy_from_slice(&low.to_le_bytes());
        if self.enhanced() {
            let high = self.usable().filter(|s| *s > 720 && self.is_free(*s)).count() as u16;
            let at = self.at(VTOC2);
            self.bytes[at + 122..at + 124].copy_from_slice(&high.to_le_bytes());
        }
    }

    fn free_sectors(&self) -> usize {
        self.usable().filter(|s| self.is_free(*s)).count()
    }

    /// The directory's 64 slots, where each is in the image.
    fn slots(&self) -> Vec<usize> {
        (0..64).map(|n| self.at(DIRECTORY + n / 8) + (n % 8) * 16).collect()
    }

    fn name(&self, slot: usize) -> String {
        let text = |b: &[u8]| b.iter().map(|&c| char::from(c & 0x7F)).collect::<String>().trim_end().to_string();
        let (name, ext) = (text(&self.bytes[slot + 5..slot + 13]), text(&self.bytes[slot + 13..slot + 16]));
        if ext.is_empty() { name } else { format!("{name}.{ext}") }
    }

    /// The slots in use, up to the end of the directory.
    fn files(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (number, slot) in self.slots().into_iter().enumerate() {
            let flag = self.bytes[slot];
            if flag == 0 {
                break;
            }
            if flag & 0x80 == 0 && flag & 0x43 != 0 {
                out.push((number, slot));
            }
        }
        out
    }

    fn find(&self, name: &str) -> Option<(usize, usize)> {
        self.files().into_iter().find(|(_, slot)| self.name(*slot).eq_ignore_ascii_case(name))
    }

    /// The sectors a file's chain runs through.
    fn chain(&self, number: usize, start: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut sector = start;
        while sector != 0 && sector <= self.sectors && out.len() <= self.sectors {
            let at = self.at(sector) + self.length(sector) - 3;
            if usize::from(self.bytes[at] >> 2) != number {
                break;
            }
            out.push(sector);
            sector = usize::from(self.bytes[at] & 3) << 8 | usize::from(self.bytes[at + 1]);
        }
        out
    }
}

/// A name as Atari DOS keeps it: 8 and 3 capital letters and digits, padded with spaces.
fn dos_name(name: &str) -> Result<[u8; 11], String> {
    let (stem, ending) = super::stem_and_ending(name.trim());
    let part = |text: &str, most: usize| -> String { text.chars().map(|c| c.to_ascii_uppercase()).filter(|c| c.is_ascii_alphanumeric()).take(most).collect() };
    let (stem, ending) = (part(stem, 8), part(ending, 3));
    if !stem.starts_with(|c: char| c.is_ascii_uppercase()) {
        return Err(format!("\"{name}\" can't be an Atari DOS name: it needs to start with a letter."));
    }
    let mut out = [b' '; 11];
    out[..stem.len()].copy_from_slice(stem.as_bytes());
    out[8..8 + ending.len()].copy_from_slice(ending.as_bytes());
    Ok(out)
}

fn shown(name: &[u8; 11]) -> String {
    let text = |b: &[u8]| String::from_utf8_lossy(b).trim_end().to_string();
    let (stem, ext) = (text(&name[..8]), text(&name[8..]));
    if ext.is_empty() { stem } else { format!("{stem}.{ext}") }
}

impl ImageWriter for Disk {
    fn add(&mut self, _folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        let name = dos_name(&file.name)?;
        if let Some((number, slot)) = self.find(&shown(&name)) {
            if !file.replace {
                return Err(already(&shown(&name)));
            }
            self.remove(number, slot);
        }
        let per_sector = self.sector_size - 3;
        let count = file.bytes.len().div_ceil(per_sector).max(1);
        if count > self.free_sectors() {
            return Err(format!("{} needs {count} sectors; the disk has {} free.", file.name, self.free_sectors()));
        }
        // The first free slot, deleted or never used; its index is the file's number.
        let (number, slot) = self.slots().into_iter().enumerate().find(|(_, slot)| self.bytes[*slot] == 0 || self.bytes[*slot] & 0x80 != 0).ok_or("The directory is full: Atari DOS holds 64 files.")?;
        let sectors: Vec<usize> = self.usable().filter(|s| self.is_free(*s)).take(count).collect();
        for (i, sector) in sectors.iter().enumerate() {
            let part = &file.bytes[(i * per_sector).min(file.bytes.len())..((i + 1) * per_sector).min(file.bytes.len())];
            let next = sectors.get(i + 1).copied().unwrap_or(0);
            let at = self.at(*sector);
            let length = self.length(*sector);
            self.bytes[at..at + length].fill(0);
            self.bytes[at..at + part.len()].copy_from_slice(part);
            self.bytes[at + length - 3] = (number as u8) << 2 | (next >> 8) as u8;
            self.bytes[at + length - 2] = next as u8;
            self.bytes[at + length - 1] = part.len() as u8;
            self.set_free(*sector, false);
        }
        // DOS 2.5 marks a file that reaches past sector 719 so that DOS 2.0, which can't follow it, leaves it be.
        let flag = if sectors.iter().any(|s| *s > 719) { 0x03 } else { 0x42 };
        self.bytes[slot] = flag;
        self.bytes[slot + 1..slot + 3].copy_from_slice(&(count as u16).to_le_bytes());
        self.bytes[slot + 3..slot + 5].copy_from_slice(&(sectors[0] as u16).to_le_bytes());
        self.bytes[slot + 5..slot + 16].copy_from_slice(&name);
        self.recount();
        Ok(())
    }

    fn delete(&mut self, _folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        let (number, slot) = self.find(&entry.name).ok_or_else(|| format!("{} isn't on the disk.", entry.name))?;
        if self.bytes[slot] & 0x20 != 0 {
            return Err(format!("{} is locked; unlock it on the Atari first.", entry.name));
        }
        self.remove(number, slot);
        Ok(())
    }

    fn rename(&mut self, _folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        let (_, slot) = self.find(&entry.name).ok_or_else(|| format!("{} isn't on the disk.", entry.name))?;
        let new = dos_name(name)?;
        if self.find(&shown(&new)).is_some_and(|(_, other)| other != slot) {
            return Err(already(&shown(&new)));
        }
        self.bytes[slot + 5..slot + 16].copy_from_slice(&new);
        Ok(())
    }

    fn free(&self) -> Option<u64> {
        Some((self.free_sectors() * (self.sector_size - 3)) as u64)
    }

    fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl Disk {
    /// Deletes as DOS does: the entry marked deleted, its sectors freed.
    fn remove(&mut self, number: usize, slot: usize) {
        let start = usize::from(u16::from_le_bytes([self.bytes[slot + 3], self.bytes[slot + 4]]));
        for sector in self.chain(number, start) {
            if self.usable().any(|s| s == sector) {
                self.set_free(sector, true);
            }
        }
        self.bytes[slot] = 0x80;
        self.recount();
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{entry, names, reopened};
    use super::*;
    use std::process::Command;

    fn add(disk: &mut Disk, name: &str, bytes: &[u8]) {
        disk.add(None, &Addition { name: name.into(), bytes: bytes.to_vec(), ..Default::default() }).unwrap();
    }

    #[test]
    fn files_go_on_and_come_off_an_atari_dos_disk() {
        for enhanced in [false, true] {
            let mut disk = Disk::open(blank(enhanced)).unwrap();
            assert_eq!(disk.free_sectors(), if enhanced { 1010 } else { 707 });
            let big: Vec<u8> = (0..60_000u32).map(|i| (i * 5) as u8).collect();
            add(&mut disk, "autorun.sys", &[0xFF, 0xFF, 0, 0x30, 1, 0x30, 0x60, 0x60]);
            add(&mut disk, "Big File.dat", &big);
            let volume = reopened(&disk, "t.atr");
            assert_eq!(names(&*volume, None), ["AUTORUN.SYS", "BIGFILE.DAT"]);
            assert_eq!(volume.read(&entry(&*volume, None, "BIGFILE.DAT")).unwrap(), big, "enhanced {enhanced}");
            disk.rename(None, &entry(&*volume, None, "AUTORUN.SYS"), "game.xex").unwrap();
            disk.delete(None, &entry(&*volume, None, "BIGFILE.DAT")).unwrap();
            let volume = reopened(&disk, "t.atr");
            assert_eq!(names(&*volume, None), ["GAME.XEX"]);
            assert_eq!(disk.free_sectors(), if enhanced { 1009 } else { 706 });
        }
    }

    /// atrcopy reads back what's written, on both kinds of disk.
    #[test]
    fn atrcopy_reads_written_disks() {
        let atrcopy = std::path::Path::new("/root/reftools/bin/atrcopy");
        if !atrcopy.is_file() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("limoniter-atr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for enhanced in [false, true] {
            let mut disk = Disk::open(blank(enhanced)).unwrap();
            let big: Vec<u8> = (0..70_000u32).map(|i| (i * 11) as u8).collect();
            add(&mut disk, "small.txt", b"HELLO\x9B");
            add(&mut disk, "big.dat", &big);
            let path = dir.join(format!("t{enhanced}.atr"));
            std::fs::write(&path, disk.bytes()).unwrap();
            let listed = Command::new(atrcopy).arg(&path).arg("list").output().unwrap();
            let listed = format!("{}{}", String::from_utf8_lossy(&listed.stdout), String::from_utf8_lossy(&listed.stderr));
            assert!(listed.contains("SMALL") && listed.contains("BIG"), "{listed}");
            let out = Command::new(atrcopy).current_dir(&dir).arg(&path).arg("extract").arg("BIG.DAT").arg("SMALL.TXT").output().unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            assert_eq!(std::fs::read(dir.join("BIG.DAT")).unwrap(), big, "enhanced {enhanced}");
            assert_eq!(std::fs::read(dir.join("SMALL.TXT")).unwrap(), b"HELLO\x9B");
        }
        std::fs::remove_dir_all(dir).ok();
    }
}
