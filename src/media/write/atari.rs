//! Atari 8-bit disks with Atari DOS 2.0, DOS 2.5 or MyDOS, in ATR images: single, enhanced or double density, and
//! MyDOS's larger disks up to 65535 sectors. A file is a chain of sectors, each ending with the file's number (its
//! directory slot), the next sector and how many bytes it holds; the VTOC (sector 360, and on a DOS 2.5 enhanced
//! disk sector 1024 too) marks which sectors are free. SpartaDOS disks are written by `sparta`.
//!
//! MyDOS keeps to DOS 2's layout and adds three things. Its VTOC grows downwards from sector 360 into 359, 358 and
//! so on when a disk has too many sectors for one, its first byte saying how many (lsatr's rule: 2, or 2 plus the
//! 256-byte pieces the bitmap needs on a disk of more than 943 single or 1023 double density sectors). A disk of
//! more than 1023 sectors links them with all 16 bits, as the file number no longer fits beside the sector. And a
//! folder is an entry flagged &10 whose eight sectors in a row are a directory like the top one.

use super::{Addition, Entry, ImageWriter, already};
use crate::media::tidy_name;

const VTOC: usize = 360;
const DIRECTORY: usize = 361;
const VTOC2: usize = 1024;
/// A MyDOS folder's flag, and the sectors its directory takes.
const FOLDER: u8 = 0x10;
const FOLDER_SECTORS: usize = 8;

/// A writer for an Atari disk: SpartaDOS, or Atari DOS 2 and MyDOS.
pub fn open(bytes: Vec<u8>) -> Result<Box<dyn ImageWriter>, String> {
    if super::sparta::is_sparta(&bytes) {
        return Ok(Box::new(super::sparta::Disk::open(bytes)?));
    }
    Ok(Box::new(Disk::open(bytes)?))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum System {
    /// Atari DOS 2.0S or 2.0D: 720 sectors, sector 720 never used.
    Dos2,
    /// DOS 2.5 on an enhanced density disk: a second VTOC at sector 1024 for sectors 720 to 1023.
    Dos25,
    MyDos,
}

pub struct Disk {
    bytes: Vec<u8>,
    sector_size: usize,
    sectors: usize,
    system: System,
    /// How many sectors the VTOC takes, from 360 down.
    vtoc_sectors: usize,
}

/// The ATR header for a disk of so many sectors of a size, the first three always 128 bytes.
fn atr(sectors: usize, sector_size: usize) -> Vec<u8> {
    let data = if sector_size == 128 { sectors * 128 } else { 384 + (sectors - 3) * sector_size };
    let mut bytes = vec![0u8; 16 + data];
    let paragraphs = data / 16;
    bytes[..2].copy_from_slice(&[0x96, 0x02]);
    bytes[2..4].copy_from_slice(&(paragraphs as u16).to_le_bytes());
    bytes[4..6].copy_from_slice(&(sector_size as u16).to_le_bytes());
    bytes[6] = (paragraphs >> 16) as u8;
    bytes
}

/// An ATR's sector size and count, or why it isn't one changed here: one of at least `least` sectors.
pub(super) fn geometry(bytes: &[u8], least: usize) -> Result<(usize, usize), String> {
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
    if !(least..=65535).contains(&sectors) {
        return Err("This Atari disk's size isn't one a DOS makes.".into());
    }
    Ok((sector_size, sectors))
}

/// What MyDOS writes in the VTOC's first byte for a disk: 2 while the bitmap fits in sector 360, otherwise 2 plus
/// the 256-byte pieces it takes.
fn mydos_code(sector_size: usize, sectors: usize) -> usize {
    if (sector_size == 128 && sectors > 943) || (sector_size == 256 && sectors > 1023) { 2 + (sectors / 8 + 266) / 256 } else { 2 }
}

/// A blank disk as DOS 2.0S (single density) or DOS 2.5 (enhanced density) formats one, with no DOS.SYS on it.
pub fn blank(enhanced: bool) -> Vec<u8> {
    let sectors = if enhanced { 1040 } else { 720 };
    let system = if enhanced { System::Dos25 } else { System::Dos2 };
    format(Disk { bytes: atr(sectors, 128), sector_size: 128, sectors, system, vtoc_sectors: 1 })
}

/// A blank double-sided double density disk (1440 sectors of 256 bytes) as MyDOS formats one, with no DOS on it.
pub fn blank_mydos() -> Vec<u8> {
    mydos_blank(1440, 256)
}

fn mydos_blank(sectors: usize, sector_size: usize) -> Vec<u8> {
    let code = mydos_code(sector_size, sectors);
    format(Disk { bytes: atr(sectors, sector_size), sector_size, sectors, system: System::MyDos, vtoc_sectors: if code == 2 { 1 } else { (code - 2) * 256 / sector_size } })
}

fn format(mut disk: Disk) -> Vec<u8> {
    // The boot sector of a disk formatted with no DOS on it: it boots to a "no DOS" message on a real Atari.
    let boot = disk.at(1);
    disk.bytes[boot..boot + 6].copy_from_slice(&[0x00, 0x03, 0x00, 0x07, 0x40, 0x15]);
    let vtoc = disk.at(VTOC);
    disk.bytes[vtoc] = if disk.system == System::MyDos { mydos_code(disk.sector_size, disk.sectors) as u8 } else { 2 };
    let usable: Vec<usize> = disk.usable().collect();
    for sector in usable {
        disk.set_free(sector, true);
    }
    let total = match disk.system {
        System::Dos2 => 707u16,
        System::Dos25 => 1010,
        System::MyDos => disk.usable().count() as u16,
    };
    disk.bytes[vtoc + 1..vtoc + 3].copy_from_slice(&total.to_le_bytes());
    disk.recount();
    disk.bytes
}

/// Where an entry is: its slot in its directory (the file's number) and the slot's place in the image.
#[derive(Clone, Copy, Debug)]
struct Place {
    number: usize,
    slot: usize,
}

impl Disk {
    pub fn open(bytes: Vec<u8>) -> Result<Self, String> {
        let (sector_size, sectors) = geometry(&bytes, 369)?;
        let mut disk = Self { bytes, sector_size, sectors, system: System::Dos2, vtoc_sectors: 1 };
        let code = usize::from(disk.bytes[disk.at(VTOC)]);
        let mydos = mydos_code(sector_size, sectors);
        disk.system = match code {
            2 if sectors == 720 => System::Dos2,
            2 if sectors == 1040 && sector_size == 128 => System::Dos25,
            2 if mydos == 2 => System::MyDos,
            // MyDOS writes 3 on some disks small enough for 2.
            3.. if code == mydos || (code == 3 && mydos == 2) => {
                disk.vtoc_sectors = if mydos == 2 { 1 } else { (code - 2) * 256 / sector_size };
                System::MyDos
            }
            0 | 1 | 2 | 3.. => return Err("This disk isn't Atari DOS 2 or MyDOS (it may be another DOS, or a disk that boots itself), so it's only read.".into()),
        };
        // DOS keeps sector 0 and the boot sectors marked in use; a VTOC that doesn't is something else's.
        if (0..4).any(|s| disk.is_free(s)) || disk.vtoc().any(|s| disk.is_free(s)) {
            return Err("This disk's VTOC marks its boot sectors or itself free, so it isn't one DOS wrote; it's only read.".into());
        }
        Ok(disk)
    }

    fn at(&self, sector: usize) -> usize {
        16 + if sector <= 3 { (sector - 1) * 128 } else { 384 + (sector - 4) * self.sector_size }
    }

    fn length(&self, sector: usize) -> usize {
        if sector <= 3 { 128 } else { self.sector_size }
    }

    /// The sectors the VTOC takes, from 360 down.
    fn vtoc(&self) -> impl Iterator<Item = usize> + use<> {
        (VTOC + 1 - self.vtoc_sectors..=VTOC).rev()
    }

    /// Whether files link their sectors with all 16 bits rather than a file number and 10 bits.
    fn wide(&self) -> bool {
        self.system == System::MyDos && self.sectors > 1023
    }

    /// The sectors DOS may give to files: not the boot sectors, the VTOC and directory; under DOS 2, not sector 720
    /// (which it never uses), nor DOS 2.5's second VTOC and beyond.
    fn usable(&self) -> impl Iterator<Item = usize> + use<> {
        let last = match self.system {
            System::Dos2 => 719,
            System::Dos25 => 1023,
            // MyDOS leaves sector 720 alone on a single density disk of 720, as DOS 2 does.
            System::MyDos if self.sectors == 720 && self.sector_size == 128 => 719,
            System::MyDos => self.sectors,
        };
        let skip_720 = self.system != System::MyDos;
        let first_vtoc = VTOC + 1 - self.vtoc_sectors;
        (4..=last).filter(move |s| !(first_vtoc..=DIRECTORY + 7).contains(s) && !(skip_720 && *s == 720))
    }

    /// A sector's bit: in the VTOC's bitmap from its byte 10, running on into the sectors below 360 when it's
    /// MyDOS's long one; in DOS 2.5's second VTOC for sectors from 720.
    fn bit(&self, sector: usize) -> (usize, u8) {
        let mask = 0x80 >> (sector % 8);
        if self.system == System::Dos25 && sector >= 720 {
            return (self.at(VTOC2) + 84 + (sector - 720) / 8, mask);
        }
        let offset = 10 + sector / 8;
        (self.at(VTOC - offset / self.sector_size) + offset % self.sector_size, mask)
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
        if self.system == System::Dos25 && (48..720).contains(&sector) {
            let at = self.at(VTOC2) + (sector - 48) / 8;
            if free {
                self.bytes[at] |= mask;
            } else {
                self.bytes[at] &= !mask;
            }
        }
    }

    /// Writes the free counts: the VTOC's, of sectors below 720 under DOS 2.5 (which counts those above in its
    /// second VTOC), of them all otherwise.
    fn recount(&mut self) {
        let dos25 = self.system == System::Dos25;
        let low = self.usable().filter(|s| (!dos25 || *s < 720) && self.is_free(*s)).count() as u16;
        let vtoc = self.at(VTOC);
        self.bytes[vtoc + 3..vtoc + 5].copy_from_slice(&low.to_le_bytes());
        if dos25 {
            let high = self.usable().filter(|s| *s > 720 && self.is_free(*s)).count() as u16;
            let at = self.at(VTOC2);
            self.bytes[at + 122..at + 124].copy_from_slice(&high.to_le_bytes());
        }
    }

    fn free_sectors(&self) -> usize {
        self.usable().filter(|s| self.is_free(*s)).count()
    }

    /// A directory's 64 slots, where each is in the image.
    fn slots(&self, directory: usize) -> Vec<usize> {
        (0..64).map(|n| self.at(directory + n / 8) + (n % 8) * 16).collect()
    }

    fn name(&self, slot: usize) -> String {
        let text = |b: &[u8]| b.iter().map(|&c| char::from(c & 0x7F)).collect::<String>().trim_end().to_string();
        let (name, ext) = (text(&self.bytes[slot + 5..slot + 13]), text(&self.bytes[slot + 13..slot + 16]));
        if ext.is_empty() { name } else { format!("{name}.{ext}") }
    }

    fn flag(&self, place: Place) -> u8 {
        self.bytes[place.slot]
    }

    fn start(&self, place: Place) -> usize {
        usize::from(u16::from_le_bytes([self.bytes[place.slot + 3], self.bytes[place.slot + 4]]))
    }

    /// The entries in use in a directory, files and folders, up to its end.
    fn files(&self, directory: usize) -> Vec<Place> {
        let mut out = Vec::new();
        if directory + 7 > self.sectors {
            return out;
        }
        for (number, slot) in self.slots(directory).into_iter().enumerate() {
            let flag = self.bytes[slot];
            if flag == 0 {
                break;
            }
            if flag & 0x80 == 0 && flag & 0x53 != 0 {
                out.push(Place { number, slot });
            }
        }
        out
    }

    fn find(&self, directory: usize, name: &str) -> Option<Place> {
        self.files(directory).into_iter().find(|p| self.name(p.slot).eq_ignore_ascii_case(name) || tidy_name(&self.name(p.slot)) == name)
    }

    fn is_folder(&self, place: Place) -> bool {
        self.system == System::MyDos && self.flag(place) & FOLDER != 0
    }

    /// Every entry in the order the reader lists them, each folder's own straight after it: an entry's `location`
    /// is its place in this list.
    fn walk(&self, directory: usize, depth: usize, out: &mut Vec<Place>) {
        if depth > 8 {
            return;
        }
        for place in self.files(directory) {
            out.push(place);
            if self.bytes[place.slot] & FOLDER != 0 {
                self.walk(self.start(place), depth + 1, out);
            }
        }
    }

    /// The first sector of a folder's directory: the top one's, or that of the folder the browser shows.
    fn directory(&self, folder: Option<&Entry>) -> Result<usize, String> {
        let Some(folder) = folder else {
            return Ok(DIRECTORY);
        };
        let mut all = Vec::new();
        self.walk(DIRECTORY, 0, &mut all);
        match all.get(folder.location as usize) {
            Some(place) if self.is_folder(*place) && tidy_name(&self.name(place.slot)) == folder.name => Ok(self.start(*place)),
            _ => Err(format!("The folder {} isn't on the disk.", folder.name)),
        }
    }

    /// The sectors a file's chain runs through: each must carry the file's number unless the links are wide, and
    /// a chain is never followed past the sectors the directory gives it, so a damaged one can't run on.
    fn chain(&self, number: usize, start: usize, count: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut sector = start;
        while sector >= 4 && sector <= self.sectors && out.len() < count.min(self.sectors) && !out.contains(&sector) {
            let at = self.at(sector) + self.length(sector) - 3;
            let (a, b) = (usize::from(self.bytes[at]), usize::from(self.bytes[at + 1]));
            if !self.wide() && a >> 2 != number {
                break;
            }
            out.push(sector);
            sector = if self.wide() { a << 8 | b } else { (a & 3) << 8 | b };
        }
        out
    }

    fn free_slot(&self, directory: usize) -> Result<Place, String> {
        self.slots(directory)
            .into_iter()
            .enumerate()
            .find(|(_, slot)| self.bytes[*slot] == 0 || self.bytes[*slot] & 0x80 != 0)
            .map(|(number, slot)| Place { number, slot })
            .ok_or_else(|| "The directory is full: Atari DOS holds 64 files in each.".into())
    }

    /// Deletes as DOS does: the entry marked deleted, its sectors freed.
    fn remove(&mut self, place: Place) {
        let count = usize::from(u16::from_le_bytes([self.bytes[place.slot + 1], self.bytes[place.slot + 2]]));
        let sectors = if self.is_folder(place) {
            (self.start(place)..self.start(place) + FOLDER_SECTORS).collect()
        } else {
            self.chain(place.number, self.start(place), count.max(1))
        };
        for sector in sectors {
            if self.usable().any(|s| s == sector) {
                self.set_free(sector, true);
            }
        }
        self.bytes[place.slot] = 0x80;
        self.recount();
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

pub(super) fn shown(name: &[u8; 11]) -> String {
    let text = |b: &[u8]| String::from_utf8_lossy(b).trim_end().to_string();
    let (stem, ext) = (text(&name[..8]), text(&name[8..]));
    if ext.is_empty() { stem } else { format!("{stem}.{ext}") }
}

impl ImageWriter for Disk {
    /// Adds a file; where it can't go on whole (a file it replaces having been taken off), the disk is left as it was.
    fn add(&mut self, folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        let saved = self.bytes.clone();
        self.add_file(folder, file).inspect_err(|_| self.bytes = saved)
    }

    fn delete(&mut self, folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        self.delete_entry(folder, entry)
    }

    fn rename(&mut self, folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        self.rename_entry(folder, entry, name)
    }

    fn make_folder(&mut self, folder: Option<&Entry>, name: &str) -> Result<(), String> {
        self.new_folder(folder, name)
    }

    fn has_folders(&self) -> bool {
        self.system == System::MyDos
    }

    fn free(&self) -> Option<u64> {
        Some((self.free_sectors() * (self.sector_size - 3)) as u64)
    }

    fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl Disk {
    fn add_file(&mut self, folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        let directory = self.directory(folder)?;
        let name = dos_name(&file.name)?;
        if let Some(place) = self.find(directory, &shown(&name)) {
            if self.is_folder(place) {
                return Err(format!("{} is a folder here, so a file can't replace it.", shown(&name)));
            }
            if !file.replace {
                return Err(already(&shown(&name)));
            }
            if self.flag(place) & 0x20 != 0 {
                return Err(format!("{} is locked; unlock it on the Atari first.", shown(&name)));
            }
            self.remove(place);
        }
        let per_sector = self.sector_size - 3;
        let count = file.bytes.len().div_ceil(per_sector).max(1);
        if count > self.free_sectors() {
            return Err(format!("{} needs {count} sectors; the disk has {} free.", file.name, self.free_sectors()));
        }
        if count > 0xFFFF {
            return Err(format!("{} is too large for an Atari DOS file.", file.name));
        }
        // The first free slot, deleted or never used; its index is the file's number.
        let place = self.free_slot(directory)?;
        let sectors: Vec<usize> = self.usable().filter(|s| self.is_free(*s)).take(count).collect();
        let wide = self.wide();
        for (i, sector) in sectors.iter().enumerate() {
            let part = &file.bytes[(i * per_sector).min(file.bytes.len())..((i + 1) * per_sector).min(file.bytes.len())];
            let next = sectors.get(i + 1).copied().unwrap_or(0);
            let at = self.at(*sector);
            let length = self.length(*sector);
            self.bytes[at..at + length].fill(0);
            self.bytes[at..at + part.len()].copy_from_slice(part);
            self.bytes[at + length - 3] = if wide { (next >> 8) as u8 } else { (place.number as u8) << 2 | (next >> 8) as u8 };
            self.bytes[at + length - 2] = next as u8;
            self.bytes[at + length - 1] = part.len() as u8;
            self.set_free(*sector, false);
        }
        // DOS 2.5 marks a file that reaches past sector 719 so that DOS 2.0, which can't follow it, leaves it be;
        // MyDOS marks one whose links are all 16 bits.
        let flag = if wide {
            0x46
        } else if self.system == System::Dos25 && sectors.iter().any(|s| *s > 719) {
            0x03
        } else {
            0x42
        };
        let slot = place.slot;
        self.bytes[slot] = flag;
        self.bytes[slot + 1..slot + 3].copy_from_slice(&(count as u16).to_le_bytes());
        self.bytes[slot + 3..slot + 5].copy_from_slice(&(sectors[0] as u16).to_le_bytes());
        self.bytes[slot + 5..slot + 16].copy_from_slice(&name);
        self.recount();
        Ok(())
    }

    fn delete_entry(&mut self, folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        let directory = self.directory(folder)?;
        let place = self.find(directory, &entry.name).ok_or_else(|| format!("{} isn't on the disk.", entry.name))?;
        if self.flag(place) & 0x20 != 0 {
            return Err(format!("{} is locked; unlock it on the Atari first.", entry.name));
        }
        if self.is_folder(place) && !self.files(self.start(place)).is_empty() {
            return Err("The folder has files in it; take them off first.".into());
        }
        self.remove(place);
        Ok(())
    }

    fn rename_entry(&mut self, folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        let directory = self.directory(folder)?;
        let place = self.find(directory, &entry.name).ok_or_else(|| format!("{} isn't on the disk.", entry.name))?;
        let new = dos_name(name)?;
        if self.find(directory, &shown(&new)).is_some_and(|other| other.slot != place.slot) {
            return Err(already(&shown(&new)));
        }
        self.bytes[place.slot + 5..place.slot + 16].copy_from_slice(&new);
        Ok(())
    }

    fn new_folder(&mut self, folder: Option<&Entry>, name: &str) -> Result<(), String> {
        if self.system != System::MyDos {
            return Err("Atari DOS 2 has no folders; MyDOS and SpartaDOS disks do.".into());
        }
        let directory = self.directory(folder)?;
        let new = dos_name(name)?;
        if self.find(directory, &shown(&new)).is_some() {
            return Err(already(&shown(&new)));
        }
        let place = self.free_slot(directory)?;
        // A folder's directory is eight free sectors in a row.
        let usable: Vec<usize> = self.usable().collect();
        let start = usable
            .windows(FOLDER_SECTORS)
            .find(|run| run[FOLDER_SECTORS - 1] == run[0] + FOLDER_SECTORS - 1 && run.iter().all(|s| self.is_free(*s)))
            .map(|run| run[0])
            .ok_or("The disk has no eight free sectors in a row for the folder's directory.")?;
        for sector in start..start + FOLDER_SECTORS {
            let at = self.at(sector);
            self.bytes[at..at + self.sector_size].fill(0);
            self.set_free(sector, false);
        }
        let slot = place.slot;
        self.bytes[slot] = FOLDER;
        self.bytes[slot + 1..slot + 3].copy_from_slice(&(FOLDER_SECTORS as u16).to_le_bytes());
        self.bytes[slot + 3..slot + 5].copy_from_slice(&(start as u16).to_le_bytes());
        self.bytes[slot + 5..slot + 16].copy_from_slice(&new);
        self.recount();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{entry, names, reopened, tool};
    use super::*;
    use std::process::Command;

    fn add(disk: &mut Disk, folder: Option<&Entry>, name: &str, bytes: &[u8]) {
        disk.add(folder, &Addition { name: name.into(), bytes: bytes.to_vec(), ..Default::default() }).unwrap();
    }

    #[test]
    fn files_go_on_and_come_off_an_atari_dos_disk() {
        for enhanced in [false, true] {
            let mut disk = Disk::open(blank(enhanced)).unwrap();
            assert_eq!(disk.free_sectors(), if enhanced { 1010 } else { 707 });
            let big: Vec<u8> = (0..60_000u32).map(|i| (i * 5) as u8).collect();
            add(&mut disk, None, "autorun.sys", &[0xFF, 0xFF, 0, 0x30, 1, 0x30, 0x60, 0x60]);
            add(&mut disk, None, "Big File.dat", &big);
            let volume = reopened(&disk, "t.atr");
            assert_eq!(names(&*volume, None), ["AUTORUN.SYS", "BIGFILE.DAT"]);
            assert_eq!(volume.read(&entry(&*volume, None, "BIGFILE.DAT")).unwrap(), big, "enhanced {enhanced}");
            disk.rename(None, &entry(&*volume, None, "AUTORUN.SYS"), "game.xex").unwrap();
            disk.delete(None, &entry(&*volume, None, "BIGFILE.DAT")).unwrap();
            let volume = reopened(&disk, "t.atr");
            assert_eq!(names(&*volume, None), ["GAME.XEX"]);
            assert_eq!(disk.free_sectors(), if enhanced { 1009 } else { 706 });
            assert!(disk.make_folder(None, "Sub").is_err(), "DOS 2 has no folders");
        }
    }

    /// A MyDOS disk of more than 1023 sectors, single and double density: files linked with all 16 bits reaching
    /// its last sectors, and folders within folders.
    #[test]
    fn mydos_disks_take_files_and_folders() {
        // Single density 1040 (two VTOC sectors), a double density 1440 and a large 8MB disk.
        for (sectors, size) in [(1040, 128), (1440, 256), (32768, 256)] {
            let mut disk = Disk::open(mydos_blank(sectors, size)).unwrap_or_else(|e| panic!("{sectors}: {e}"));
            assert_eq!(disk.system, System::MyDos);
            let free = disk.free_sectors();
            assert_eq!(free, sectors - 3 - disk.vtoc_sectors - 8, "{sectors}");
            // Enough to fill most of a small disk, so its sectors past 1023 are used.
            let big: Vec<u8> = (0..(free - 20) * (size - 3)).map(|i| (i * 7 / 3) as u8).collect();
            let big = &big[..big.len().min(400_000)];
            disk.make_folder(None, "Games").unwrap();
            let volume = reopened(&disk, "m.atr");
            let games = entry(&*volume, None, "GAMES");
            assert!(games.is_dir);
            add(&mut disk, Some(&games), "big.dat", big);
            disk.make_folder(Some(&games), "Inner").unwrap();
            let volume = reopened(&disk, "m.atr");
            let games = entry(&*volume, None, "GAMES");
            let inner = entry(&*volume, Some(&games), "INNER");
            add(&mut disk, Some(&inner), "deep.txt", b"DEEP\x9B");
            add(&mut disk, None, "top.txt", b"TOP\x9B");
            let volume = reopened(&disk, "m.atr");
            assert!(volume.describe().contains("MyDOS"), "{}", volume.describe());
            assert_eq!(names(&*volume, None), ["GAMES", "TOP.TXT"]);
            let games = entry(&*volume, None, "GAMES");
            assert_eq!(names(&*volume, Some(&games)), ["BIG.DAT", "INNER"]);
            assert_eq!(volume.read(&entry(&*volume, Some(&games), "BIG.DAT")).unwrap(), big, "{sectors}");
            let inner = entry(&*volume, Some(&games), "INNER");
            assert_eq!(volume.read(&entry(&*volume, Some(&inner), "DEEP.TXT")).unwrap(), b"DEEP\x9B");
            assert!(disk.delete(Some(&games), &inner).is_err(), "a folder with files in it stays");
            disk.delete(Some(&inner), &entry(&*volume, Some(&inner), "DEEP.TXT")).unwrap();
            disk.delete(Some(&games), &inner).unwrap();
            disk.delete(Some(&games), &entry(&*volume, Some(&games), "BIG.DAT")).unwrap();
            disk.rename(None, &games, "Toys").unwrap();
            let volume = reopened(&disk, "m.atr");
            assert_eq!(names(&*volume, None), ["TOYS", "TOP.TXT"]);
            assert_eq!(disk.free_sectors(), free - 9, "the folder and the one file left");
        }
    }

    #[test]
    fn real_mydos_disks_open_for_writing() {
        let path = std::path::Path::new("../testmedia/atari8/MyDOS_4.50T__Utils_1988_Wordmark.atr");
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let disk = Disk::open(bytes).unwrap();
        assert_eq!((disk.system, disk.vtoc_sectors), (System::MyDos, 1));
        assert_eq!(disk.free_sectors(), 22, "as its VTOC counts them");
    }

    /// atrcopy reads back what's written, on both kinds of DOS 2 disk.
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
            add(&mut disk, None, "small.txt", b"HELLO\x9B");
            add(&mut disk, None, "big.dat", &big);
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

    /// lsatr (from dmsc's mkatr) recognises written MyDOS disks as MyDOS, and extracts every file, folders and all.
    #[test]
    fn lsatr_reads_written_mydos_disks() {
        let Some(lsatr) = tool("lsatr") else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("limoniter-mydos-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut disk = Disk::open(blank_mydos()).unwrap();
        let big: Vec<u8> = (0..300_000u32).map(|i| (i * 13 / 7) as u8).collect();
        disk.make_folder(None, "docs").unwrap();
        let volume = reopened(&disk, "m.atr");
        add(&mut disk, Some(&entry(&*volume, None, "DOCS")), "readme.txt", b"READ ME\x9B");
        add(&mut disk, None, "big.dat", &big);
        let path = dir.join("m.atr");
        std::fs::write(&path, disk.bytes()).unwrap();
        let listed = Command::new(&lsatr).arg(&path).output().unwrap();
        let listed = format!("{}{}", String::from_utf8_lossy(&listed.stdout), String::from_utf8_lossy(&listed.stderr));
        assert!(listed.contains("MyDOS") && listed.contains("1440 sectors") && !listed.contains("invalid"), "{listed}");
        let out = dir.join("out");
        let done = Command::new(&lsatr).arg("-X").arg(&out).arg(&path).output().unwrap();
        assert!(done.status.success(), "{}", String::from_utf8_lossy(&done.stderr));
        assert_eq!(std::fs::read(out.join("BIG.DAT")).unwrap(), big);
        assert_eq!(std::fs::read(out.join("DOCS/README.TXT")).unwrap(), b"READ ME\x9B");
        std::fs::remove_dir_all(dir).ok();
    }
}
