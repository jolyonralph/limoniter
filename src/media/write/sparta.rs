//! Atari 8-bit disks with SpartaDOS (2.x, 3.x and SpartaDOS X), in ATR images. The boot sector says where the
//! bitmap of free sectors is (a bit set for each free one), where the top directory's first sector map is, and how
//! many sectors are free. Every file, directories too, is a chain of sector maps: each map gives the next and the
//! previous, then the file's sectors in turn. A directory is 23-byte entries, its first describing the directory
//! itself (its parent's first map and its own length); each entry has a status (in use, deleted, a folder,
//! locked), the file's first map, its length in bytes, an 8.3 name and when it was written.
//!
//! SpartaDOS 1's older layout is only read.

use super::{Addition, Entry, ImageWriter, already};
use crate::media::tidy_name;

const ENTRY: usize = 23;
const IN_USE: u8 = 0x08;
const DELETED: u8 = 0x10;
const FOLDER: u8 = 0x20;
const LOCKED: u8 = 0x01;
/// SpartaDOS's own limit on a directory's length.
const MOST_DIRECTORY: usize = 32 * 1024;

pub struct Disk {
    bytes: Vec<u8>,
    sector_size: usize,
    /// The sectors the file system has, which the image may run past.
    sectors: usize,
}

fn le16(bytes: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([bytes[at], bytes[at + 1]]))
}

fn le24(bytes: &[u8], at: usize) -> usize {
    usize::from(bytes[at]) | usize::from(bytes[at + 1]) << 8 | usize::from(bytes[at + 2]) << 16
}

/// Whether an ATR's boot sector is SpartaDOS's, as the reader tells: the jump to its boot code, its version, and a
/// top directory and size.
pub fn is_sparta(bytes: &[u8]) -> bool {
    let Some(boot) = bytes.get(16..16 + 128) else {
        return false;
    };
    bytes[..2] == [0x96, 0x02] && matches!(boot[7], 0x80 | 0x40) && matches!(boot[32], 0x10 | 0x11 | 0x20 | 0x21) && le16(boot, 9) > 0 && le16(boot, 11) > 0
}

/// Today's date and the time now, as SpartaDOS keeps them: day, month, year in two digits; hours, minutes, seconds.
fn now() -> [u8; 6] {
    use chrono::{Datelike, Timelike};
    let now = chrono::Local::now();
    [now.day() as u8, now.month() as u8, (now.year() % 100) as u8, now.hour() as u8, now.minute() as u8, now.second() as u8]
}

/// A name as SpartaDOS keeps it: 8 and 3 capital letters, digits and underlines, padded with spaces.
fn sparta_name(name: &str) -> Result<[u8; 11], String> {
    let (stem, ending) = super::stem_and_ending(name.trim());
    let part = |text: &str, most: usize| -> String { text.chars().map(|c| c.to_ascii_uppercase()).filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(most).collect() };
    let (stem, ending) = (part(stem, 8), part(ending, 3));
    if !stem.starts_with(|c: char| c.is_ascii_uppercase() || c == '_') {
        return Err(format!("\"{name}\" can't be a SpartaDOS name: it needs to start with a letter."));
    }
    let mut out = [b' '; 11];
    out[..stem.len()].copy_from_slice(stem.as_bytes());
    out[8..8 + ending.len()].copy_from_slice(ending.as_bytes());
    Ok(out)
}

/// A new directory entry.
fn entry(status: u8, map: usize, length: usize, name: &[u8; 11]) -> [u8; ENTRY] {
    let mut out = [0u8; ENTRY];
    out[0] = status;
    out[1..3].copy_from_slice(&(map as u16).to_le_bytes());
    out[3..6].copy_from_slice(&(length as u32).to_le_bytes()[..3]);
    out[6..17].copy_from_slice(name);
    out[17..23].copy_from_slice(&now());
    out
}

/// A blank double density disk (720 sectors of 256 bytes) as SpartaDOS formats one, named `label`, with no DOS on
/// it: its boot sectors only say so, failing the boot as a disk with no DOS does.
pub fn blank(label: &str) -> Vec<u8> {
    let (sectors, sector_size) = (720usize, 256usize);
    let data = 384 + (sectors - 3) * sector_size;
    let mut bytes = vec![0u8; 16 + data];
    bytes[..2].copy_from_slice(&[0x96, 0x02]);
    bytes[2..4].copy_from_slice(&((data / 16) as u16).to_le_bytes());
    bytes[4..6].copy_from_slice(&(sector_size as u16).to_le_bytes());
    let mut disk = Disk { bytes, sector_size, sectors };
    let bitmap = 4;
    let bitmaps = (sectors + 8).div_ceil(8).div_ceil(sector_size);
    let root_map = bitmap + bitmaps;
    // The top directory: a map, and one sector holding its own entry.
    let root_data = root_map + 1;
    let map = disk.at(root_map);
    disk.bytes[map + 4..map + 6].copy_from_slice(&(root_data as u16).to_le_bytes());
    let name = *b"MAIN       ";
    let header = entry(0x28, 0, ENTRY, &name);
    let at = disk.at(root_data);
    disk.bytes[at..at + ENTRY].copy_from_slice(&header);
    let boot = disk.at(1);
    let first_free = root_data + 1;
    let label: Vec<u8> = label.bytes().filter(|b| b.is_ascii_graphic() || *b == b' ').take(8).collect();
    let b = &mut disk.bytes[boot..boot + 128];
    // Load three sectors at $3000 and jump to $3080, where SEC RTS fails the boot.
    b[..9].copy_from_slice(&[0x00, 0x03, 0x00, 0x30, 0x80, 0x30, 0x4C, 0x80, 0x30]);
    b[9..11].copy_from_slice(&(root_map as u16).to_le_bytes());
    b[11..13].copy_from_slice(&(sectors as u16).to_le_bytes());
    b[15] = bitmaps as u8;
    b[16..18].copy_from_slice(&(bitmap as u16).to_le_bytes());
    b[18..20].copy_from_slice(&(first_free as u16).to_le_bytes());
    b[20..22].copy_from_slice(&(first_free as u16).to_le_bytes());
    b[22..30].fill(b' ');
    b[22..22 + label.len()].copy_from_slice(&label);
    b[30] = 40; // tracks, on one side
    b[31] = 0; // 256-byte sectors
    b[32] = 0x20;
    b[38] = 1;
    b[39] = (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0x5A, |d| d.as_secs()) % 251) as u8;
    let code = disk.at(2);
    disk.bytes[code..code + 2].copy_from_slice(&[0x38, 0x60]);
    for sector in first_free..=sectors {
        disk.set_free(sector, true);
    }
    disk.recount();
    disk.bytes
}

/// Where an entry is: the directory's first map and the entry's index in it (0 being the directory's own).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Place {
    directory: usize,
    index: usize,
}

impl Disk {
    pub fn open(bytes: Vec<u8>) -> Result<Self, String> {
        let (sector_size, image_sectors) = super::atari::geometry(&bytes, 6)?;
        let mut disk = Self { bytes, sector_size, sectors: image_sectors };
        let boot = disk.boot().to_vec();
        if !matches!(boot[32], 0x20 | 0x21) {
            return Err("This is a SpartaDOS 1 disk, whose older layout is only read.".into());
        }
        let said_size = match boot[31] {
            0x80 => 128,
            0 => 256,
            _ => 0,
        };
        if said_size != sector_size {
            return Err("This SpartaDOS disk's sector size isn't the image's, so it's only read.".into());
        }
        let total = le16(&boot, 11);
        if total > image_sectors {
            return Err("This SpartaDOS disk says it's larger than its image, so it's only read.".into());
        }
        disk.sectors = total;
        let (bitmap, bitmaps, root) = (le16(&boot, 16), usize::from(boot[15]), le16(&boot, 9));
        if bitmap < 2 || bitmaps == 0 || bitmap + bitmaps - 1 > total || bitmaps * sector_size * 8 <= total || !(2..=total).contains(&root) {
            return Err("This SpartaDOS disk's boot sector doesn't make sense, so it's only read.".into());
        }
        // Every directory must be readable before anything is changed.
        disk.walk()?;
        Ok(disk)
    }

    fn boot(&self) -> &[u8] {
        &self.bytes[16..16 + 128]
    }

    fn at(&self, sector: usize) -> usize {
        16 + if sector <= 3 { (sector - 1) * 128 } else { 384 + (sector - 4) * self.sector_size }
    }

    fn bit(&self, sector: usize) -> (usize, u8) {
        let first = le16(self.boot(), 16);
        (self.at(first + sector / 8 / self.sector_size) + sector / 8 % self.sector_size, 0x80 >> (sector % 8))
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
    }

    fn free_sectors(&self) -> usize {
        (4..=self.sectors).filter(|s| self.is_free(*s)).count()
    }

    /// Writes the free count, and steps the volume's sequence number so a SpartaDOS that had the disk in knows it's
    /// been changed.
    fn recount(&mut self) {
        let free = self.free_sectors() as u16;
        self.bytes[16 + 13..16 + 15].copy_from_slice(&free.to_le_bytes());
        self.bytes[16 + 38] = self.bytes[16 + 38].wrapping_add(1);
    }

    /// A free sector, taken; the lowest, as SpartaDOS takes them.
    fn allocate(&mut self) -> Result<usize, String> {
        let sector = (4..=self.sectors).find(|s| self.is_free(*s)).ok_or("The disk is full.")?;
        self.set_free(sector, false);
        let at = self.at(sector);
        self.bytes[at..at + self.sector_size].fill(0);
        Ok(sector)
    }

    fn valid(&self, sector: usize) -> Result<usize, String> {
        if (4..=self.sectors).contains(&sector) { Ok(sector) } else { Err(format!("A sector map points to sector {sector}, which isn't on the disk, so the disk is damaged.")) }
    }

    /// A file's chain of sector maps.
    fn maps(&self, first: usize) -> Result<Vec<usize>, String> {
        let mut out = Vec::new();
        let mut map = first;
        while map != 0 {
            if out.contains(&map) || out.len() > self.sectors {
                return Err("A file's sector maps loop, so the disk is damaged.".into());
            }
            out.push(self.valid(map)?);
            map = le16(&self.bytes, self.at(map));
        }
        Ok(out)
    }

    /// The sectors a file's maps give, in turn (0 for a sector never written).
    fn sectors_of(&self, first: usize) -> Result<Vec<usize>, String> {
        let mut out = Vec::new();
        for map in self.maps(first)? {
            let at = self.at(map);
            out.extend((4..self.sector_size).step_by(2).map(|i| le16(&self.bytes, at + i)));
        }
        Ok(out)
    }

    fn read(&self, first: usize, length: usize) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(length);
        for sector in self.sectors_of(first)? {
            if out.len() >= length {
                break;
            }
            if sector == 0 {
                out.resize(out.len() + self.sector_size, 0);
            } else {
                let at = self.at(self.valid(sector)?);
                out.extend_from_slice(&self.bytes[at..at + self.sector_size]);
            }
        }
        if out.len() < length {
            return Err("A file's sector maps end before it does, so the disk is damaged.".into());
        }
        out.truncate(length);
        Ok(out)
    }

    /// Writes bytes into a file from its start, taking more sectors and maps where it runs past those it has.
    fn store(&mut self, first: usize, bytes: &[u8]) -> Result<(), String> {
        let per_map = (self.sector_size - 4) / 2;
        let mut maps = self.maps(first)?;
        for (i, chunk) in bytes.chunks(self.sector_size).enumerate() {
            let (m, k) = (i / per_map, i % per_map);
            if m == maps.len() {
                let new = self.allocate()?;
                let last = *maps.last().expect("a file has a map");
                let (at, new_at) = (self.at(last), self.at(new));
                self.bytes[at..at + 2].copy_from_slice(&(new as u16).to_le_bytes());
                self.bytes[new_at + 2..new_at + 4].copy_from_slice(&(last as u16).to_le_bytes());
                maps.push(new);
            }
            let pointer = self.at(maps[m]) + 4 + 2 * k;
            let mut sector = le16(&self.bytes, pointer);
            if sector == 0 {
                sector = self.allocate()?;
                self.bytes[pointer..pointer + 2].copy_from_slice(&(sector as u16).to_le_bytes());
            }
            let at = self.at(self.valid(sector)?);
            self.bytes[at..at + self.sector_size].fill(0);
            self.bytes[at..at + chunk.len()].copy_from_slice(chunk);
        }
        Ok(())
    }

    /// A new file holding these bytes: its first map.
    fn create(&mut self, bytes: &[u8]) -> Result<usize, String> {
        let sectors = bytes.len().div_ceil(self.sector_size);
        let maps = sectors.div_ceil((self.sector_size - 4) / 2).max(1);
        if sectors + maps > self.free_sectors() {
            return Err(format!("This needs {} sectors; the disk has {} free.", sectors + maps, self.free_sectors()));
        }
        let first = self.allocate()?;
        self.store(first, bytes)?;
        Ok(first)
    }

    /// Frees every sector and map of a file.
    fn release(&mut self, first: usize) -> Result<(), String> {
        let maps = self.maps(first)?;
        let sectors = self.sectors_of(first)?;
        for sector in sectors.into_iter().filter(|s| *s != 0).chain(maps) {
            self.set_free(self.valid(sector)?, true);
        }
        Ok(())
    }

    /// A directory's bytes, its own entry first.
    fn directory_bytes(&self, map: usize) -> Result<Vec<u8>, String> {
        let head = self.read(map, ENTRY)?;
        let length = le24(&head, 3);
        if !(ENTRY..=0x10000).contains(&length) {
            return Err("A directory's length doesn't make sense, so the disk is damaged.".into());
        }
        self.read(map, length)
    }

    /// The entries in use in a directory, as the reader lists them.
    fn entries(&self, map: usize) -> Result<Vec<(Place, [u8; ENTRY])>, String> {
        let data = self.directory_bytes(map)?;
        let mut out = Vec::new();
        for (index, entry) in data.as_chunks::<ENTRY>().0.iter().enumerate().skip(1) {
            if entry[0] == 0 {
                break;
            }
            if entry[0] & DELETED == 0 && entry[0] & IN_USE != 0 {
                out.push((Place { directory: map, index }, *entry));
            }
        }
        Ok(out)
    }

    /// Every entry, in the reader's order, each folder's own straight after it: an entry's `location` is its place.
    fn walk(&self) -> Result<Vec<(Place, [u8; ENTRY])>, String> {
        let mut out = Vec::new();
        self.walk_in(le16(self.boot(), 9), 0, &mut out)?;
        Ok(out)
    }

    fn walk_in(&self, map: usize, depth: usize, out: &mut Vec<(Place, [u8; ENTRY])>) -> Result<(), String> {
        if depth > 16 {
            return Ok(());
        }
        for (place, entry) in self.entries(map)? {
            out.push((place, entry));
            if entry[0] & FOLDER != 0 {
                self.walk_in(le16(&entry, 1), depth + 1, out)?;
            }
        }
        Ok(())
    }

    fn directory(&self, folder: Option<&Entry>) -> Result<usize, String> {
        let Some(folder) = folder else {
            return Ok(le16(self.boot(), 9));
        };
        match self.walk()?.get(folder.location as usize) {
            Some((_, entry)) if entry[0] & FOLDER != 0 && tidy_name(&name_of(entry)) == folder.name => Ok(le16(entry, 1)),
            _ => Err(format!("The folder {} isn't on the disk.", folder.name)),
        }
    }

    fn find(&self, directory: usize, name: &str) -> Result<Option<(Place, [u8; ENTRY])>, String> {
        Ok(self.entries(directory)?.into_iter().find(|(_, e)| name_of(e).eq_ignore_ascii_case(name) || tidy_name(&name_of(e)) == name))
    }

    /// Writes an entry into a directory: over the one at `index`, or the first deleted one, or after the last, the
    /// directory growing (and its length said in its own entry and its parent's).
    fn put(&mut self, directory: usize, index: Option<usize>, new: &[u8; ENTRY]) -> Result<(), String> {
        let mut data = self.directory_bytes(directory)?;
        let count = data.len() / ENTRY;
        let index = index.or_else(|| (1..count).find(|i| data[i * ENTRY] & DELETED != 0 || data[i * ENTRY] & IN_USE == 0)).unwrap_or(count);
        if index == count {
            if data.len() + ENTRY > MOST_DIRECTORY {
                return Err("The directory is full.".into());
            }
            data.truncate(count * ENTRY);
            data.extend_from_slice(new);
            let length = data.len();
            data[3..6].copy_from_slice(&(length as u32).to_le_bytes()[..3]);
            self.store(directory, &data)?;
            let parent = le16(&data, 1);
            if parent != 0 {
                let mut above = self.directory_bytes(parent)?;
                let at = (1..above.len() / ENTRY).find(|i| above[i * ENTRY] & FOLDER != 0 && le16(&above, i * ENTRY + 1) == directory).ok_or("A folder isn't in its parent directory, so the disk is damaged.")?;
                above[at * ENTRY + 3..at * ENTRY + 6].copy_from_slice(&(length as u32).to_le_bytes()[..3]);
                self.store(parent, &above)?;
            }
        } else {
            data[index * ENTRY..(index + 1) * ENTRY].copy_from_slice(new);
            self.store(directory, &data)?;
        }
        Ok(())
    }

    fn set_status(&mut self, place: Place, status: u8) -> Result<(), String> {
        let mut data = self.directory_bytes(place.directory)?;
        data[place.index * ENTRY] = status;
        self.store(place.directory, &data)
    }
}

fn name_of(entry: &[u8; ENTRY]) -> String {
    let text = |b: &[u8]| b.iter().map(|&c| char::from(c & 0x7F)).collect::<String>().trim_end().to_string();
    let (name, ext) = (text(&entry[6..14]), text(&entry[14..17]));
    if ext.is_empty() { name } else { format!("{name}.{ext}") }
}

/// The changes themselves; each is made whole or, where it can't be, not at all.
impl Disk {
    fn atomically(&mut self, change: impl FnOnce(&mut Self) -> Result<(), String>) -> Result<(), String> {
        let saved = self.bytes.clone();
        let result = change(self);
        if result.is_err() {
            self.bytes = saved;
        }
        result
    }

    fn add_file(&mut self, folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        let directory = self.directory(folder)?;
        let name = sparta_name(&file.name)?;
        let shown = super::atari::shown(&name);
        if file.bytes.len() > 0xFF_FFFF {
            return Err(format!("{} is larger than the 16 MB a SpartaDOS file can be.", file.name));
        }
        let mut index = None;
        if let Some((place, old)) = self.find(directory, &shown)? {
            if old[0] & FOLDER != 0 {
                return Err(format!("{shown} is a folder here, so a file can't replace it."));
            }
            if !file.replace {
                return Err(already(&shown));
            }
            if old[0] & LOCKED != 0 {
                return Err(format!("{shown} is protected; unprotect it on the Atari first."));
            }
            self.release(le16(&old, 1))?;
            index = Some(place.index);
        }
        let first = self.create(&file.bytes).map_err(|e| format!("{}: {e}", file.name))?;
        self.put(directory, index, &entry(IN_USE, first, file.bytes.len(), &name))?;
        self.recount();
        Ok(())
    }

    fn delete_entry(&mut self, folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        let directory = self.directory(folder)?;
        let (place, old) = self.find(directory, &entry.name)?.ok_or_else(|| format!("{} isn't on the disk.", entry.name))?;
        if old[0] & LOCKED != 0 {
            return Err(format!("{} is protected; unprotect it on the Atari first.", entry.name));
        }
        if old[0] & FOLDER != 0 && !self.entries(le16(&old, 1))?.is_empty() {
            return Err("The folder has files in it; take them off first.".into());
        }
        self.release(le16(&old, 1))?;
        self.set_status(place, DELETED)?;
        self.recount();
        Ok(())
    }

    fn rename_entry(&mut self, folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        let directory = self.directory(folder)?;
        let (place, old) = self.find(directory, &entry.name)?.ok_or_else(|| format!("{} isn't on the disk.", entry.name))?;
        let new = sparta_name(name)?;
        if self.find(directory, &super::atari::shown(&new))?.is_some_and(|(other, _)| other != place) {
            return Err(already(&super::atari::shown(&new)));
        }
        let mut renamed = old;
        renamed[6..17].copy_from_slice(&new);
        self.put(directory, Some(place.index), &renamed)?;
        // A folder's own entry carries its name too.
        if old[0] & FOLDER != 0 {
            let inner = le16(&old, 1);
            let mut data = self.directory_bytes(inner)?;
            data[6..17].copy_from_slice(&new);
            self.store(inner, &data)?;
        }
        self.recount();
        Ok(())
    }

    fn new_folder(&mut self, folder: Option<&Entry>, name: &str) -> Result<(), String> {
        let directory = self.directory(folder)?;
        let new = sparta_name(name)?;
        if self.find(directory, &super::atari::shown(&new))?.is_some() {
            return Err(already(&super::atari::shown(&new)));
        }
        let first = self.create(&entry(IN_USE | FOLDER, directory, ENTRY, &new))?;
        self.put(directory, None, &entry(IN_USE | FOLDER, first, ENTRY, &new))?;
        self.recount();
        Ok(())
    }

}

impl ImageWriter for Disk {
    fn add(&mut self, folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        self.atomically(|disk| disk.add_file(folder, file))
    }

    fn delete(&mut self, folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        self.atomically(|disk| disk.delete_entry(folder, entry))
    }

    fn rename(&mut self, folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        self.atomically(|disk| disk.rename_entry(folder, entry, name))
    }

    fn make_folder(&mut self, folder: Option<&Entry>, name: &str) -> Result<(), String> {
        self.atomically(|disk| disk.new_folder(folder, name))
    }

    fn has_folders(&self) -> bool {
        true
    }

    fn free(&self) -> Option<u64> {
        Some((self.free_sectors() * self.sector_size) as u64)
    }

    fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{entry as find, names, reopened, tool};
    use super::*;
    use std::process::Command;

    fn add(disk: &mut Disk, folder: Option<&Entry>, name: &str, bytes: &[u8]) {
        disk.add(folder, &Addition { name: name.into(), bytes: bytes.to_vec(), ..Default::default() }).unwrap();
    }

    #[test]
    fn files_and_folders_go_on_a_spartados_disk() {
        let mut disk = Disk::open(blank("Test")).unwrap();
        let free = disk.free_sectors();
        assert_eq!(free, 720 - 6);
        // Big enough to need two sector maps.
        let big: Vec<u8> = (0..60_000u32).map(|i| (i * 3 / 5) as u8).collect();
        add(&mut disk, None, "big.dat", &big);
        add(&mut disk, None, "x.com", b"\xFF\xFF\x00\x30\x00\x30\x60");
        disk.make_folder(None, "Tools").unwrap();
        let volume = reopened(&disk, "s.atr");
        assert!(volume.describe().contains("SpartaDOS"), "{}", volume.describe());
        assert_eq!(names(&*volume, None), ["BIG.DAT", "X.COM", "TOOLS"]);
        assert_eq!(volume.read(&find(&*volume, None, "BIG.DAT")).unwrap(), big);
        let tools = find(&*volume, None, "TOOLS");
        // Enough files to grow the folder's directory past its first sector.
        for n in 0..30 {
            add(&mut disk, Some(&tools), &format!("f{n}.txt"), &[format!("FILE {n}").as_bytes(), b"\x9B"].concat());
        }
        disk.make_folder(Some(&tools), "Inner").unwrap();
        let volume = reopened(&disk, "s.atr");
        let tools = find(&*volume, None, "TOOLS");
        assert_eq!(names(&*volume, Some(&tools)).len(), 31);
        assert_eq!(volume.read(&find(&*volume, Some(&tools), "F29.TXT")).unwrap(), b"FILE 29\x9B");
        let inner = find(&*volume, Some(&tools), "INNER");
        add(&mut disk, Some(&inner), "deep", b"DEEP");
        let volume = reopened(&disk, "s.atr");
        let tools = find(&*volume, None, "TOOLS");
        let inner = find(&*volume, Some(&tools), "INNER");
        assert_eq!(volume.read(&find(&*volume, Some(&inner), "DEEP")).unwrap(), b"DEEP");
        assert!(find(&*volume, None, "BIG.DAT").detail.starts_with("20"), "dated");
        // Replacing, renaming and taking off.
        assert!(disk.add(None, &Addition { name: "big.dat".into(), bytes: vec![1], ..Default::default() }).unwrap_err().starts_with(super::super::ALREADY));
        disk.add(None, &Addition { name: "big.dat".into(), bytes: vec![1, 2, 3], replace: true, ..Default::default() }).unwrap();
        assert!(disk.delete(None, &tools).is_err(), "a folder with files in it stays");
        disk.rename(Some(&tools), &inner, "Deeper").unwrap();
        disk.rename(None, &tools, "Kit").unwrap();
        let volume = reopened(&disk, "s.atr");
        assert_eq!(names(&*volume, None), ["BIG.DAT", "X.COM", "KIT"]);
        assert_eq!(volume.read(&find(&*volume, None, "BIG.DAT")).unwrap(), [1, 2, 3]);
        let kit = find(&*volume, None, "KIT");
        let deeper = find(&*volume, Some(&kit), "DEEPER");
        disk.delete(Some(&deeper), &find(&*volume, Some(&deeper), "DEEP")).unwrap();
        disk.delete(Some(&kit), &deeper).unwrap();
        for n in 0..30 {
            disk.delete(Some(&kit), &find(&*volume, Some(&kit), &format!("F{n}.TXT"))).unwrap();
        }
        disk.delete(None, &kit).unwrap();
        disk.delete(None, &find(&*volume, None, "BIG.DAT")).unwrap();
        disk.delete(None, &find(&*volume, None, "X.COM")).unwrap();
        assert!(reopened(&disk, "s.atr").list(None).unwrap().is_empty());
        // The top directory grew a sector for the entries it's had; everything else is free again.
        assert_eq!(disk.free_sectors(), free);
    }

    #[test]
    fn real_spartados_disks_take_a_file() {
        for name in ["SpartaDOS_v3.2f_Toolkit_1994_02_25_Fine_Tooned_Engineering.atr", "BW_Tape_System_Disk_19xx_BeWeSoft_req_SpartaDOS.atr"] {
            let Ok(bytes) = std::fs::read(std::path::Path::new("../testmedia/atari8").join(name)) else {
                return;
            };
            let before = reopened(&Disk::open(bytes.clone()).unwrap(), "r.atr").list(None).unwrap().len();
            let mut disk = Disk::open(bytes).unwrap();
            add(&mut disk, None, "new.txt", b"NEW\x9B");
            let volume = reopened(&disk, "r.atr");
            assert_eq!(volume.list(None).unwrap().len(), before + 1, "{name}");
            assert_eq!(volume.read(&find(&*volume, None, "NEW.TXT")).unwrap(), b"NEW\x9B");
        }
    }

    /// lsatr (from dmsc's mkatr) lists and extracts written SpartaDOS disks, folders and dates included.
    #[test]
    fn lsatr_reads_written_spartados_disks() {
        let Some(lsatr) = tool("lsatr") else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("limoniter-sparta-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut disk = Disk::open(blank("Mine")).unwrap();
        let big: Vec<u8> = (0..100_000u32).map(|i| (i * 13 / 7) as u8).collect();
        disk.make_folder(None, "docs").unwrap();
        let volume = reopened(&disk, "s.atr");
        let docs = find(&*volume, None, "DOCS");
        for n in 0..20 {
            add(&mut disk, Some(&docs), &format!("note{n}.txt"), &[format!("NOTE {n}").as_bytes(), b"\x9B"].concat());
        }
        add(&mut disk, None, "big.dat", &big);
        let path = dir.join("s.atr");
        std::fs::write(&path, disk.bytes()).unwrap();
        let listed = Command::new(&lsatr).arg(&path).output().unwrap();
        let listed = format!("{}{}", String::from_utf8_lossy(&listed.stdout), String::from_utf8_lossy(&listed.stderr));
        assert!(listed.contains("volume name 'Mine'") && listed.contains("/DOCS/NOTE19.TXT") && !listed.contains("invalid") && !listed.contains("short"), "{listed}");
        let out = dir.join("out");
        let done = Command::new(&lsatr).arg("-X").arg(&out).arg(&path).output().unwrap();
        assert!(done.status.success(), "{}", String::from_utf8_lossy(&done.stderr));
        assert_eq!(std::fs::read(out.join("BIG.DAT")).unwrap(), big);
        assert_eq!(std::fs::read(out.join("DOCS/NOTE7.TXT")).unwrap(), b"NOTE 7\x9B");
        std::fs::remove_dir_all(dir).ok();
    }
}
