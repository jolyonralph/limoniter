//! Acorn ADFS discs with the old map, as the BBC Micro and Master's ADFS keep them (the S, M and L floppies, and
//! hard discs): the free space map in sectors 0 and 1, up to 82 runs of free sectors kept in order of where they
//! start, each sector with its checksum; the root directory in sectors 2 to 6. A directory is five sectors: a master
//! sequence number and "Hugo" at each end, up to 47 entries kept in order of their names, and its own name, parent
//! and title. Each entry has a ten-character name whose top bits are its access (R, W, L, D for a directory, E...),
//! its load and execution addresses, its length and where it starts; a file is one run of sectors.
//!
//! An .adl image keeps each track's side 0 sectors, then side 1's; ADFS numbers all of side 0 first, so the disc is
//! worked on in ADFS's order and put back in the image's. RISC OS's D format (old map, new directories) and its new
//! map (E and F) are only read.

use super::acorn::{bcd_next, guess, kinds, prepared};
use super::{Addition, Entry, ImageWriter, Kind, already};
use crate::media::tidy_name;

const DIRECTORY: usize = 0x500;
const ROOT: u32 = 2;
const MOST_ENTRIES: usize = 47;
const MOST_FREE_RUNS: usize = 82;

/// Access bits, as the names' top bits hold them.
const READ: u16 = 1;
const WRITE: u16 = 2;
const LOCKED: u16 = 4;
const FOLDER: u16 = 8;

/// The free space map's checksum: the bytes from 254 down to 0 added, the carry of each addition put back in.
fn checksum(sector: &[u8]) -> u8 {
    let mut total: u32 = 255;
    for &b in sector[..255].iter().rev() {
        if total > 255 {
            total = (total + 1) & 0xFF;
        }
        total += u32::from(b);
    }
    total as u8
}

fn le24(bytes: &[u8]) -> u32 {
    u32::from(bytes[0]) | u32::from(bytes[1]) << 8 | u32::from(bytes[2]) << 16
}

fn le32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// Where a sector of ADFS's order is in an .adl image: track by track, side 0's sixteen sectors then side 1's.
fn adl_place(sector: usize, sectors: usize) -> usize {
    let per_side = sectors / 2;
    let (side, track, within) = (sector / per_side, sector % per_side / 16, sector % 16);
    (track * 2 + side) * 16 + within
}

fn reorder(bytes: &[u8], to_adl: bool) -> Vec<u8> {
    let sectors = bytes.len() / 256;
    let mut out = vec![0u8; bytes.len()];
    for sector in 0..sectors {
        let place = adl_place(sector, sectors);
        let (from, to) = if to_adl { (sector, place) } else { (place, sector) };
        out[to * 256..to * 256 + 256].copy_from_slice(&bytes[from * 256..from * 256 + 256]);
    }
    out
}

/// Text in a fixed field of a directory, padded with carriage returns as ADFS pads it.
fn field(text: &[u8], length: usize) -> Vec<u8> {
    let mut out: Vec<u8> = text.iter().copied().take(length).collect();
    out.resize(length, 0x0D);
    out
}

/// A new, empty directory.
fn empty_directory(name: &[u8], parent: u32, title: &[u8]) -> Vec<u8> {
    let mut dir = vec![0u8; DIRECTORY];
    dir[0] = 1;
    dir[1..5].copy_from_slice(b"Hugo");
    dir[0x4CC..0x4D6].copy_from_slice(&field(name, 10));
    dir[0x4D6..0x4D9].copy_from_slice(&parent.to_le_bytes()[..3]);
    dir[0x4D9..0x4EC].copy_from_slice(&field(title, 19));
    dir[0x4FA] = 1;
    dir[0x4FB..0x4FF].copy_from_slice(b"Hugo");
    dir
}

/// A blank disc as *FORMAT leaves one: everything after the root directory free. 640 sectors is an S disc, 1,280
/// an M and 2,560 an L, which goes in an .adl image's order.
pub fn blank(sectors: u32, label: &str) -> Vec<u8> {
    let mut disc = vec![0u8; sectors as usize * 256];
    let title: Vec<u8> = label.chars().filter(|c| ('!'..='~').contains(c) || *c == ' ').map(|c| c as u8).collect();
    let root = empty_directory(b"$", ROOT, if title.is_empty() { b"$" } else { &title });
    disc[0x200..0x700].copy_from_slice(&root);
    let mut adfs = Adfs { disc, adl: false, image: Vec::new(), sectors, free: vec![(7, sectors - 7)] };
    adfs.write_map();
    if sectors == 2560 { reorder(&adfs.disc, true) } else { adfs.disc }
}

#[derive(Clone, Debug)]
struct DirEntry {
    name: [u8; 10],
    access: u16,
    load: u32,
    exec: u32,
    length: u32,
    address: u32,
    sequence: u8,
}

impl DirEntry {
    fn read(raw: &[u8]) -> Self {
        let mut name = [0u8; 10];
        let mut access = 0;
        for (i, b) in raw[..10].iter().enumerate() {
            name[i] = b & 0x7F;
            access |= u16::from(b >> 7) << i;
        }
        Self { name, access, load: le32(&raw[10..14]), exec: le32(&raw[14..18]), length: le32(&raw[18..22]), address: le24(&raw[22..25]), sequence: raw[25] }
    }

    fn write(&self, raw: &mut [u8]) {
        for i in 0..10 {
            raw[i] = self.name[i] & 0x7F | ((self.access >> i & 1) as u8) << 7;
        }
        raw[10..14].copy_from_slice(&self.load.to_le_bytes());
        raw[14..18].copy_from_slice(&self.exec.to_le_bytes());
        raw[18..22].copy_from_slice(&self.length.to_le_bytes());
        raw[22..25].copy_from_slice(&self.address.to_le_bytes()[..3]);
        raw[25] = self.sequence;
    }

    /// The name itself, up to its carriage return.
    fn bare(&self) -> &[u8] {
        let end = self.name.iter().position(|b| *b < 0x20 || *b == 0x7F).unwrap_or(10);
        &self.name[..end]
    }

    /// Its name as the reader shows it.
    fn shown(&self) -> String {
        tidy_name(&String::from_utf8_lossy(self.bare()).trim_end().replace('/', "."))
    }

    fn is_folder(&self) -> bool {
        self.access & FOLDER != 0
    }

    fn sectors(&self) -> u32 {
        self.length.div_ceil(256)
    }
}

/// A name as ADFS keeps it: ten characters at most, of those it allows, a dot (which ADFS uses between directories)
/// written as the slash an Acorn uses before an ending.
fn adfs_name(name: &str) -> Result<[u8; 10], String> {
    let kept: Vec<u8> = name.trim().chars().map(|c| if c == '.' { '/' } else { c }).filter(|c| ('!'..='~').contains(c) && !"$&%@\\^:#*\"".contains(*c)).take(10).map(|c| c as u8).collect();
    if kept.is_empty() {
        return Err(format!("\"{name}\" has no characters an ADFS name can hold."));
    }
    let mut out = [0x0Du8; 10];
    out[..kept.len()].copy_from_slice(&kept);
    Ok(out)
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

pub struct Adfs {
    /// The disc in ADFS's order.
    disc: Vec<u8>,
    adl: bool,
    /// The image as saved, in its own order.
    image: Vec<u8>,
    sectors: u32,
    /// The free space, as (first sector, sectors), in order.
    free: Vec<(u32, u32)>,
}

impl Adfs {
    pub fn open(bytes: Vec<u8>, ending: &str) -> Result<Self, String> {
        let adl = ending == "adl";
        if bytes.len() < 0x700 || !bytes.len().is_multiple_of(256) {
            return Err("The image is too short, or not whole sectors, for an ADFS disc.".into());
        }
        let disc = if adl { reorder(&bytes, false) } else { bytes.clone() };
        if &disc[0x201..0x205] != b"Hugo" {
            return Err(if disc.get(0x401..0x405) == Some(b"Nick") {
                "This is a RISC OS ADFS D disc, whose directories are only read here.".into()
            } else {
                "This is a RISC OS ADFS disc with the new map (E or F format), which is only read here.".into()
            });
        }
        if checksum(&disc[..256]) != disc[255] || checksum(&disc[256..512]) != disc[511] {
            return Err("The free space map's checksums are wrong, so the disc is damaged.".into());
        }
        let sectors = le24(&disc[0xFC..0xFF]);
        if sectors < 7 || sectors as usize * 256 > disc.len() {
            return Err(format!("The map says the disc has {sectors} sectors, and the image holds {}.", disc.len() / 256));
        }
        let end = usize::from(disc[256 + 0xFE]);
        if !end.is_multiple_of(3) || end / 3 > MOST_FREE_RUNS {
            return Err("The free space map's length isn't whole entries, so the disc is damaged.".into());
        }
        let free: Vec<(u32, u32)> = (0..end / 3).map(|i| (le24(&disc[i * 3..]), le24(&disc[256 + i * 3..]))).collect();
        let mut next = 7;
        for (start, length) in &free {
            if *start < next || *length == 0 || start + length > sectors {
                return Err("The free space map is out of order or past the disc's end, so the disc is damaged.".into());
            }
            next = start + length;
        }
        let adfs = Self { disc, adl, image: bytes, sectors, free };
        adfs.directory(ROOT)?;
        Ok(adfs)
    }

    /// The image again from the disc, after a change.
    fn store(&mut self) {
        self.image = if self.adl { reorder(&self.disc, true) } else { self.disc.clone() };
    }

    fn write_map(&mut self) {
        let (zero, one) = self.disc.split_at_mut(256);
        zero[..0xF6].fill(0);
        one[..0xF6].fill(0);
        for (i, (start, length)) in self.free.iter().enumerate() {
            zero[i * 3..i * 3 + 3].copy_from_slice(&start.to_le_bytes()[..3]);
            one[i * 3..i * 3 + 3].copy_from_slice(&length.to_le_bytes()[..3]);
        }
        zero[0xFC..0xFF].copy_from_slice(&self.sectors.to_le_bytes()[..3]);
        one[0xFE] = (self.free.len() * 3) as u8;
        zero[255] = checksum(zero);
        one[255] = checksum(&one[..256]);
    }

    /// Takes a run of sectors from the first free space it fits in.
    fn allocate(&mut self, wanted: u32) -> Result<u32, String> {
        let wanted = wanted.max(1);
        let Some(index) = self.free.iter().position(|(_, length)| *length >= wanted) else {
            let total: u32 = self.free.iter().map(|(_, length)| length).sum();
            return Err(if total >= wanted {
                format!("There are {total} sectors free, but not {wanted} in one piece: the disc needs compacting, as *COMPACT would.")
            } else {
                format!("That needs {wanted} sectors; the disc has {total} free.")
            });
        };
        let (start, length) = self.free[index];
        if length == wanted {
            self.free.remove(index);
        } else {
            self.free[index] = (start + wanted, length - wanted);
        }
        Ok(start)
    }

    /// Gives a run of sectors back, joined to the free space either side.
    fn release(&mut self, start: u32, length: u32) -> Result<(), String> {
        if length == 0 {
            return Ok(());
        }
        let index = self.free.iter().position(|(s, _)| *s > start).unwrap_or(self.free.len());
        self.free.insert(index, (start, length));
        if index + 1 < self.free.len() && start + length == self.free[index + 1].0 {
            self.free[index].1 += self.free[index + 1].1;
            self.free.remove(index + 1);
        }
        if index > 0 && self.free[index - 1].0 + self.free[index - 1].1 == start {
            self.free[index - 1].1 += self.free[index].1;
            self.free.remove(index);
        }
        if self.free.len() > MOST_FREE_RUNS {
            return Err("The free space map is full: there are too many gaps, and the disc needs compacting.".into());
        }
        Ok(())
    }

    fn sector_range(&self, address: u32, length: usize) -> Result<std::ops::Range<usize>, String> {
        let at = address as usize * 256;
        if address < 2 || at + length > self.sectors as usize * 256 {
            return Err("A directory lies past the disc's end, so the disc is damaged.".into());
        }
        Ok(at..at + length)
    }

    /// A directory's entries, checked as ADFS checks one: both marks and sequence numbers must agree.
    fn directory(&self, address: u32) -> Result<Vec<DirEntry>, String> {
        let dir = &self.disc[self.sector_range(address, DIRECTORY)?];
        if &dir[1..5] != b"Hugo" || &dir[0x4FB..0x4FF] != b"Hugo" || dir[0] != dir[0x4FA] {
            return Err("A directory is broken: its start and end don't match, as ADFS would say.".into());
        }
        let mut entries = Vec::new();
        for raw in dir[5..5 + MOST_ENTRIES * 26].chunks(26) {
            if raw[0] == 0 {
                break;
            }
            let entry = DirEntry::read(raw);
            if entry.address + entry.sectors() > self.sectors {
                return Err(format!("{} lies past the disc's end, so the disc is damaged.", entry.shown()));
            }
            entries.push(entry);
        }
        Ok(entries)
    }

    /// Writes a directory's entries back in ADFS's order of names, its sequence number one on.
    fn write_directory(&mut self, address: u32, mut entries: Vec<DirEntry>) -> Result<u8, String> {
        if entries.len() > MOST_ENTRIES {
            return Err("The directory is full: an ADFS directory holds 47 entries.".into());
        }
        entries.sort_by_key(|e| e.bare().to_ascii_uppercase());
        let range = self.sector_range(address, DIRECTORY)?;
        let dir = &mut self.disc[range];
        let sequence = bcd_next(dir[0]);
        dir[5..0x4CC].fill(0);
        for (i, entry) in entries.iter().enumerate() {
            entry.write(&mut dir[5 + i * 26..5 + i * 26 + 26]);
        }
        dir[0] = sequence;
        dir[0x4FA] = sequence;
        Ok(sequence)
    }

    fn folder_address(folder: Option<&Entry>) -> Result<u32, String> {
        match folder {
            None => Ok(ROOT),
            Some(f) if f.is_dir => Ok(f.location as u32),
            Some(f) => Err(format!("{} isn't a directory.", f.name)),
        }
    }

    fn find(entries: &[DirEntry], entry: &Entry) -> Result<usize, String> {
        entries.iter().position(|e| e.shown() == entry.name && e.is_folder() == entry.is_dir).ok_or_else(|| format!("{} isn't in this directory.", entry.name))
    }

    /// Puts an entry in a directory, with the directory's new sequence number, and the map and image after it.
    fn put(&mut self, address: u32, mut entries: Vec<DirEntry>, mut entry: DirEntry) -> Result<(), String> {
        let sequence = bcd_next(self.disc[address as usize * 256]);
        entry.sequence = sequence;
        entries.push(entry);
        self.write_directory(address, entries)?;
        self.write_map();
        self.store();
        Ok(())
    }
}

impl ImageWriter for Adfs {
    fn kinds(&self) -> Vec<Kind> {
        kinds()
    }

    fn guess(&self, name: &str, bytes: &[u8]) -> Addition {
        guess(name, bytes)
    }

    fn add(&mut self, folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        let address = Self::folder_address(folder)?;
        let mut entries = self.directory(address)?;
        let name = adfs_name(&file.name)?;
        let (bytes, load, exec) = prepared(file)?;
        if let Some(index) = entries.iter().position(|e| same(e.bare(), &name[..name.iter().position(|b| *b == 0x0D).unwrap_or(10)])) {
            let old = &entries[index];
            if old.is_folder() {
                return Err(format!("{} is a directory here, so a file can't replace it.", old.shown()));
            }
            if !file.replace {
                return Err(already(&old.shown()));
            }
            let old = entries.remove(index);
            self.release(old.address, old.sectors())?;
        }
        if entries.len() >= MOST_ENTRIES {
            return Err("The directory is full: an ADFS directory holds 47 entries.".into());
        }
        if bytes.len() > 0xFF_FFFF {
            return Err(format!("{} is larger than the disc can hold.", file.name));
        }
        let length = bytes.len() as u32;
        let start = self.allocate(length.div_ceil(256))?;
        let at = start as usize * 256;
        self.disc[at..at + (length.div_ceil(256).max(1) as usize) * 256].fill(0);
        self.disc[at..at + bytes.len()].copy_from_slice(&bytes);
        self.put(address, entries, DirEntry { name, access: READ | WRITE, load, exec, length, address: start, sequence: 0 })
    }

    fn delete(&mut self, folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        let address = Self::folder_address(folder)?;
        let mut entries = self.directory(address)?;
        let old = entries.remove(Self::find(&entries, entry)?);
        if old.is_folder() && !self.directory(old.address)?.is_empty() {
            return Err(format!("{} has things in it; take them out first.", old.shown()));
        }
        self.release(old.address, old.sectors())?;
        self.write_directory(address, entries)?;
        self.write_map();
        self.store();
        Ok(())
    }

    fn rename(&mut self, folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        let address = Self::folder_address(folder)?;
        let mut entries = self.directory(address)?;
        let mut renamed = entries.remove(Self::find(&entries, entry)?);
        renamed.name = adfs_name(name)?;
        if let Some(other) = entries.iter().find(|e| same(e.bare(), renamed.bare())) {
            return Err(already(&other.shown()));
        }
        // A directory keeps its own name too.
        if renamed.is_folder() {
            let range = self.sector_range(renamed.address, DIRECTORY)?;
            self.disc[range][0x4CC..0x4D6].copy_from_slice(&field(renamed.bare(), 10));
        }
        self.put(address, entries, renamed)
    }

    fn make_folder(&mut self, folder: Option<&Entry>, name: &str) -> Result<(), String> {
        let address = Self::folder_address(folder)?;
        let entries = self.directory(address)?;
        let name = adfs_name(name)?;
        let new = DirEntry { name, access: FOLDER | LOCKED | READ, load: 0, exec: 0, length: DIRECTORY as u32, address: 0, sequence: 0 };
        if let Some(other) = entries.iter().find(|e| same(e.bare(), new.bare())) {
            return Err(already(&other.shown()));
        }
        if entries.len() >= MOST_ENTRIES {
            return Err("The directory is full: an ADFS directory holds 47 entries.".into());
        }
        let start = self.allocate(5)?;
        let at = start as usize * 256;
        self.disc[at..at + DIRECTORY].copy_from_slice(&empty_directory(new.bare(), address, new.bare()));
        self.put(address, entries, DirEntry { address: start, ..new })
    }

    fn has_folders(&self) -> bool {
        true
    }

    fn free(&self) -> Option<u64> {
        Some(self.free.iter().map(|(_, length)| u64::from(*length) * 256).sum())
    }

    fn bytes(&self) -> &[u8] {
        &self.image
    }
}

#[cfg(test)]
mod tests {
    use super::super::acorn::CODE;
    use super::super::tests::{entry, names, reopened};
    use super::*;
    use crate::media::acorn::tests::bbc_program;
    use std::path::Path;
    use std::process::Command;

    fn add(disc: &mut Adfs, folder: Option<&Entry>, name: &str, bytes: &[u8]) {
        let addition = disc.guess(name, bytes);
        disc.add(folder, &addition).unwrap_or_else(|e| panic!("{name}: {e}"));
    }

    #[test]
    fn files_and_directories_go_on_an_adfs_disc() {
        for (sectors, name) in [(640, "t.ads"), (1280, "t.adf"), (2560, "t.adl")] {
            let ending = name.rsplit('.').next().unwrap();
            let mut disc = Adfs::open(blank(sectors, "Mine"), ending).unwrap();
            assert_eq!(disc.free(), Some(u64::from(sectors - 7) * 256));
            let big: Vec<u8> = (0..400_000u32).map(|i| (i * 7 / 3) as u8).collect();
            let big = &big[..(sectors as usize - 40) * 256 / 2];
            add(&mut disc, None, "Prog", &bbc_program());
            add(&mut disc, None, "zebra.txt", b"stripes\n");
            add(&mut disc, None, "Big", big);
            disc.make_folder(None, "Games").unwrap();
            let volume = reopened(&disc, name);
            assert!(volume.describe().contains("Mine"), "{}", volume.describe());
            assert_eq!(names(&*volume, None), ["Games", "Big", "Prog", "zebra.txt"]);
            assert_eq!(volume.read(&entry(&*volume, None, "Big")).unwrap(), big);
            assert_eq!(volume.read(&entry(&*volume, None, "zebra.txt")).unwrap(), b"stripes\r");
            let prog = entry(&*volume, None, "Prog");
            assert_eq!(volume.read(&prog).unwrap(), bbc_program());
            assert!(prog.detail.starts_with("Load FFFF1900 \u{B7} Exec FFFF8023"), "{}", prog.detail);
            let games = entry(&*volume, None, "Games");
            add(&mut disc, Some(&games), "Elite", &[0xA9, 0, 0x60]);
            disc.make_folder(Some(&games), "Old").unwrap();
            let volume = reopened(&disc, name);
            assert_eq!(names(&*volume, Some(&games)), ["Old", "Elite"]);
            assert!(disc.delete(None, &games).unwrap_err().contains("things in it"));
            disc.rename(None, &games, "Fun").unwrap();
            disc.delete(None, &entry(&*volume, None, "Big")).unwrap();
            let volume = reopened(&disc, name);
            assert_eq!(names(&*volume, None), ["Fun", "Prog", "zebra.txt"]);
            let fun = entry(&*volume, None, "Fun");
            assert_eq!(volume.read(&entry(&*volume, Some(&fun), "Elite")).unwrap(), [0xA9, 0, 0x60]);
            disc.delete(Some(&fun), &entry(&*volume, Some(&fun), "Elite")).unwrap();
            disc.delete(Some(&fun), &entry(&*volume, Some(&fun), "Old")).unwrap();
            disc.delete(None, &fun).unwrap();
            // Everything given back joins up again after the program's and the text's sector.
            assert_eq!(disc.free, vec![(9, sectors - 9)]);
        }
    }

    #[test]
    fn full_fragmented_or_damaged_discs_are_refused() {
        let mut disc = Adfs::open(blank(640, ""), "ads").unwrap();
        for n in 0..47 {
            add(&mut disc, None, &format!("F{n}"), &[n as u8; 300]);
        }
        assert!(disc.add(None, &disc.guess("More", b"x")).unwrap_err().contains("47 entries"));
        let mut disc = Adfs::open(blank(640, ""), "ads").unwrap();
        add(&mut disc, None, "A", &vec![1u8; 300 * 256]);
        add(&mut disc, None, "B", &vec![2u8; 300 * 256]);
        let volume = reopened(&disc, "t.ads");
        disc.delete(None, &entry(&*volume, None, "A")).unwrap();
        // 300 sectors free where A was and 33 at the end: 320 is there in all, but not in one piece.
        let problem = disc.add(None, &disc.guess("C", &vec![3u8; 320 * 256])).unwrap_err();
        assert!(problem.contains("compacting"), "{problem}");
        let mut broken = blank(640, "");
        broken[3] ^= 1;
        assert!(Adfs::open(broken, "ads").err().unwrap().contains("checksums"));
        let mut broken = blank(640, "");
        broken[0x6FA] = 9;
        assert!(Adfs::open(broken, "ads").err().unwrap().contains("broken"));
        assert!(Adfs::open(vec![0; 256], "adf").is_err());
        let mut d = vec![0u8; 819_200];
        d[0x401..0x405].copy_from_slice(b"Nick");
        assert!(Adfs::open(d, "adf").err().unwrap().contains("D disc"));
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-adfs-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// An L disc with a program, code in a directory and the free space split, for the reference tools.
    fn sample() -> (Adfs, Vec<u8>) {
        let mut disc = Adfs::open(blank(2560, "Ref"), "adl").unwrap();
        let code: Vec<u8> = (0..70_000u32).map(|i| (i * 11) as u8).collect();
        add(&mut disc, None, "Gap", &[0; 1000]);
        disc.make_folder(None, "Games").unwrap();
        let volume = reopened(&disc, "t.adl");
        let games = entry(&*volume, None, "Games");
        disc.add(Some(&games), &Addition { name: "Code".into(), bytes: code.clone(), kind: CODE.into(), address: Some(0x3000), ..Default::default() }).unwrap();
        add(&mut disc, None, "Prog", &bbc_program());
        disc.delete(None, &entry(&*volume, None, "Gap")).unwrap();
        (disc, code)
    }

    /// beebtools checks the map's checksums and the directories' marks, lists them and extracts the files.
    #[test]
    fn beebtools_reads_a_written_disc() {
        let beebtools = Path::new("/root/reftools/acorn/bin/beebtools");
        if !beebtools.is_file() {
            return;
        }
        let dir = scratch("beebtools");
        let (disc, code) = sample();
        let path = dir.join("t.adl");
        std::fs::write(&path, disc.bytes()).unwrap();
        let listed = Command::new(beebtools).arg("cat").arg(&path).output().unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&listed.stdout), String::from_utf8_lossy(&listed.stderr));
        assert!(listed.status.success() && text.contains("Games") && text.contains("Prog") && !text.contains("Gap"), "{text}");
        let out = dir.join("code.bin");
        let got = Command::new(beebtools).arg("extract").arg(&path).arg("$.Games.Code").arg("-o").arg(&out).output().unwrap();
        assert!(got.status.success(), "{}", String::from_utf8_lossy(&got.stderr));
        assert_eq!(std::fs::read(&out).unwrap(), code);
        let summary = Command::new(beebtools).arg("disc").arg(&path).output().unwrap();
        // It counts the free sectors (though it calls them bytes), with thousands separated.
        let summary = String::from_utf8_lossy(&summary.stdout).replace(',', "");
        let free = disc.free().unwrap() / 256;
        assert!(summary.contains(&format!("Free:   {free} ")), "{free} free sectors: {summary}");
        std::fs::remove_dir_all(dir).ok();
    }

    /// David Boddie's ADFSlib reads the catalogue and the files back.
    #[test]
    fn adfslib_reads_a_written_disc() {
        let lib = Path::new("/root/refmedia/acorn-tools/adfslib");
        if !lib.join("ADFSlib.py").is_file() {
            return;
        }
        let dir = scratch("adfslib");
        let (disc, code) = sample();
        let path = dir.join("t.adl");
        std::fs::write(&path, disc.bytes()).unwrap();
        let script = "import sys\nsys.path.insert(0, sys.argv[1])\nimport ADFSlib\ndisc = ADFSlib.ADFSdisc(open(sys.argv[2], 'rb'))\nfor f in disc.files:\n    if isinstance(f, ADFSlib.ADFSdirectory):\n        for g in f.files:\n            print(f.name + '.' + g.name, len(g.data), hex(g.load_address))\n            if g.name == 'Code':\n                open(sys.argv[3], 'wb').write(g.data)\n    else:\n        print(f.name, len(f.data), hex(f.load_address))\n";
        let out = dir.join("code.bin");
        let run = Command::new("python3").arg("-I").arg("-c").arg(script).arg(lib).arg(&path).arg(&out).output().unwrap();
        let text = String::from_utf8_lossy(&run.stdout).to_string();
        assert!(run.status.success(), "{}{text}", String::from_utf8_lossy(&run.stderr));
        assert!(text.contains("Games.Code 70000 0xffff3000") && text.contains("Prog"), "{text}");
        assert_eq!(std::fs::read(&out).unwrap(), code);
        std::fs::remove_dir_all(dir).ok();
    }
}
