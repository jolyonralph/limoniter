//! BBC Micro DFS disks, single-sided (.ssd) and double-sided (.dsd), as Acorn's DFS keeps them: each side's
//! catalogue in its first two sectors, sector 0 the names (seven characters and a directory letter whose top bit
//! locks the file), sector 1 the disk's size, boot option and write cycle, then each file's load and execution
//! addresses, length and first sector, their top bits packed in one byte. A file is one run of sectors, and the
//! catalogue lists the files from the one furthest into the disk down, as DFS keeps it.
//!
//! When there's room for a file in all but not in one run, the side is compacted first, as *COMPACT would, every
//! file moved down to close the gaps. A .dsd's sides are folders, as the reader shows them.
//!
//! The kinds and their addresses are shared with ADFS's writer: a BBC BASIC program loads at &1900 and runs
//! BASIC at &8023, in the I/O processor, as SAVE gives it.

use super::{Addition, Entry, ImageWriter, Kind, already};
use crate::basic_more;
use crate::media::tidy_name;

pub(crate) const BASIC: &str = "BASIC program";
pub(crate) const CODE: &str = "Machine code";
pub(crate) const TEXT: &str = "Text";
pub(crate) const DATA: &str = "Data";

/// The kinds a file on an 8-bit Acorn disk can be, told apart by their load and execution addresses.
pub(crate) fn kinds() -> Vec<Kind> {
    vec![
        Kind::plain(BASIC, "A tokenised BBC BASIC program, as SAVE writes one, loading at &1900. A listing typed as text is tokenised first."),
        Kind { name: CODE, about: "A machine-code program, as *SAVE writes one, loading and running at the address.", address: true, autostart: false },
        Kind::plain(TEXT, "Text, such as *SPOOL writes or *EXEC reads, its lines ending in a carriage return."),
        Kind::plain(DATA, "A file BASIC's programs OPENIN and read, kept as it is."),
    ]
}

/// How a file from elsewhere would most likely go on: a BBC BASIC program as one, text as text, the rest as code.
pub(crate) fn guess(name: &str, bytes: &[u8]) -> Addition {
    let (_, ending) = super::stem_and_ending(name);
    let ending = ending.to_lowercase();
    let text = !bytes.is_empty() && std::str::from_utf8(bytes).is_ok_and(|t| !t.chars().any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t')));
    let (kind, address) = if basic_more::looks_like_bbc(bytes) || (matches!(ending.as_str(), "bas" | "bbc") && text && is_listing(bytes)) {
        (BASIC, None)
    } else if text {
        (TEXT, None)
    } else if matches!(ending.as_str(), "dat" | "data") {
        (DATA, None)
    } else {
        (CODE, Some(0x1900))
    };
    Addition { name: name.to_string(), bytes: bytes.to_vec(), kind: kind.into(), address, ..Default::default() }
}

/// Whether text is a BASIC listing: every line that isn't blank starts with a line number.
fn is_listing(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines().map(str::trim_start).filter(|l| !l.is_empty()).peekable();
    lines.peek().is_some() && lines.all(|l| l.starts_with(|c: char| c.is_ascii_digit()))
}

/// A file's bytes as they're to be written, and its load and execution addresses, 32 bits as ADFS keeps them (DFS
/// keeps 18 of them). A listing given as a BASIC program is tokenised; text has its lines end in a carriage return.
pub(crate) fn prepared(file: &Addition) -> Result<(Vec<u8>, u32, u32), String> {
    match file.kind.as_str() {
        BASIC => {
            let bytes = if !basic_more::looks_like_bbc(&file.bytes) && is_listing(&file.bytes) {
                let text = String::from_utf8_lossy(&file.bytes);
                let lines: Vec<&str> = text.lines().collect();
                basic_more::tokenise_bbc(&lines)?
            } else {
                file.bytes.clone()
            };
            Ok((bytes, 0xFFFF_1900, 0xFFFF_8023))
        }
        CODE => {
            let address = 0xFFFF_0000 | u32::from(file.address.unwrap_or(0x1900));
            Ok((file.bytes.clone(), address, address))
        }
        TEXT => Ok((String::from_utf8_lossy(&file.bytes).replace("\r\n", "\r").replace('\n', "\r").into_bytes(), 0, 0)),
        _ => Ok((file.bytes.clone(), 0, 0)),
    }
}

/// The write cycle, a binary-coded decimal count, one on.
pub(crate) fn bcd_next(value: u8) -> u8 {
    let n = (value >> 4) * 10 + (value & 15);
    let n = if n >= 99 || value & 15 > 9 || value >> 4 > 9 { 0 } else { n + 1 };
    (n / 10) << 4 | (n % 10)
}

/// A blank DFS side as *FORMAT leaves it: an empty catalogue that says how many sectors the side has.
fn blank_side(disk: &mut [u8], offset: impl Fn(u32) -> usize, sectors: u32, label: &str) {
    let mut title = [0u8; 12];
    for (slot, c) in title.iter_mut().zip(label.chars().filter(|c| (' '..='~').contains(c))) {
        *slot = c as u8;
    }
    disk[offset(0)..offset(0) + 8].copy_from_slice(&title[..8]);
    disk[offset(1)..offset(1) + 4].copy_from_slice(&title[8..]);
    disk[offset(1) + 6] = (sectors >> 8) as u8 & 3;
    disk[offset(1) + 7] = sectors as u8;
}

/// A blank single-sided disk of 40 or 80 tracks, or a double-sided one of 80.
pub fn blank(tracks: u32, double: bool, label: &str) -> Vec<u8> {
    let sectors = tracks * 10;
    let mut disk = vec![0u8; (sectors * 256 * if double { 2 } else { 1 }) as usize];
    for side in 0..if double { 2 } else { 1 } {
        blank_side(&mut disk, |sector| offset(double, side, sector), sectors, label);
    }
    disk
}

/// A sector's place in the image: a .dsd keeps each track's ten sectors of side 0, then side 1's.
fn offset(double: bool, side: u8, sector: u32) -> usize {
    let sector = sector as usize;
    if double { ((sector / 10 * 2 + usize::from(side)) * 10 + sector % 10) * 256 } else { sector * 256 }
}

#[derive(Clone, Debug)]
struct DfsFile {
    name: [u8; 7],
    directory: u8,
    locked: bool,
    /// The 18-bit addresses and length DFS keeps.
    load: u32,
    exec: u32,
    length: u32,
    start: u32,
}

impl DfsFile {
    fn sectors(&self) -> u32 {
        self.length.div_ceil(256)
    }

    /// Its name as the reader shows it: the directory, a dot, then the name, its padding taken off.
    fn shown(&self) -> String {
        let name: String = self.name.iter().map(|b| b & 0x7F).take_while(|b| *b >= 0x20 && *b != 0x7F).map(char::from).collect();
        tidy_name(&format!("{}.{}", char::from(self.directory), name.trim_end()))
    }

    fn same_name(&self, other: &DfsFile) -> bool {
        self.directory.eq_ignore_ascii_case(&other.directory) && trimmed(&self.name).eq_ignore_ascii_case(trimmed(&other.name))
    }
}

fn trimmed(name: &[u8; 7]) -> &[u8] {
    let end = name.iter().rposition(|b| *b != b' ' && *b != 0).map_or(0, |p| p + 1);
    &name[..end]
}

/// One side's catalogue.
struct Side {
    sectors: u32,
    files: Vec<DfsFile>,
}

pub struct Dfs {
    bytes: Vec<u8>,
    double: bool,
}

/// A name as DFS keeps it: "D.NAME" for directory D, or $ when no directory is given; seven characters at most, of
/// those DFS allows. An ending such as .bas is left off.
fn dfs_name(name: &str) -> Result<(u8, [u8; 7]), String> {
    let name = name.trim();
    let chars: Vec<char> = name.chars().collect();
    let (directory, rest) = if chars.len() >= 3 && chars[1] == '.' && allowed(chars[0]) { (chars[0] as u8, &name[2..]) } else { (b'$', name) };
    let (stem, _) = super::stem_and_ending(rest);
    let kept: Vec<u8> = stem.chars().filter(|c| allowed(*c)).take(7).map(|c| c as u8).collect();
    if kept.is_empty() {
        return Err(format!("\"{name}\" has no characters a DFS name can hold."));
    }
    let mut out = [b' '; 7];
    out[..kept.len()].copy_from_slice(&kept);
    Ok((directory, out))
}

fn allowed(c: char) -> bool {
    ('!'..='~').contains(&c) && !".:\"#*".contains(c)
}

impl Dfs {
    pub fn open(bytes: Vec<u8>, double: bool) -> Result<Self, String> {
        if bytes.len() < 512 {
            return Err("The image is too short to hold a DFS catalogue.".into());
        }
        let mut dfs = Self { bytes, double };
        let mut most = 0;
        for side in 0..if double { 2 } else { 1 } {
            let at = offset(double, side, 1);
            // A .dsd's second side may never have been formatted.
            if side == 1 && (dfs.bytes.len() < at + 256 || u32::from(dfs.bytes[at + 6] & 3) << 8 | u32::from(dfs.bytes[at + 7]) < 2) {
                continue;
            }
            most = most.max(dfs.catalogue(side)?.sectors);
        }
        // Images are often cut short after their last file: the rest of the disk is put back, blank, so files can go
        // there.
        let whole = offset(double, if double { 1 } else { 0 }, most - 1) + 256;
        if dfs.bytes.len() < whole {
            dfs.bytes.resize(whole, 0);
        }
        Ok(dfs)
    }

    fn at(&self, side: u8, sector: u32) -> usize {
        offset(self.double, side, sector)
    }

    fn sector(&self, side: u8, sector: u32) -> &[u8] {
        let at = self.at(side, sector);
        &self.bytes[at..at + 256]
    }

    fn catalogue(&self, side: u8) -> Result<Side, String> {
        let (names, info) = (self.sector(side, 0), self.sector(side, 1));
        let sectors = u32::from(info[6] & 3) << 8 | u32::from(info[7]);
        if sectors < 2 {
            return Err(format!("Side {} has no DFS catalogue.", side + 1));
        }
        if self.at(side, 3) + 256 <= self.bytes.len() && self.sector(side, 2)[..8].iter().all(|b| *b == 0xAA) {
            return Err("This is a Watford DFS disk with a second catalogue, which is only read here.".into());
        }
        let count = usize::from(info[5]);
        if !count.is_multiple_of(8) || count > 31 * 8 {
            return Err("The catalogue's file count isn't a whole number of entries, so the disk is damaged.".into());
        }
        let mut files = Vec::new();
        for at in (8..8 + count).step_by(8) {
            let (name, about) = (&names[at..at + 8], &info[at..at + 8]);
            let high = about[6];
            let file = DfsFile {
                name: name[..7].try_into().unwrap_or([b' '; 7]),
                directory: name[7] & 0x7F,
                locked: name[7] & 0x80 != 0,
                load: u32::from(high >> 2 & 3) << 16 | u32::from(u16::from_le_bytes([about[0], about[1]])),
                exec: u32::from(high >> 6 & 3) << 16 | u32::from(u16::from_le_bytes([about[2], about[3]])),
                length: u32::from(high >> 4 & 3) << 16 | u32::from(u16::from_le_bytes([about[4], about[5]])),
                start: u32::from(high & 3) << 8 | u32::from(about[7]),
            };
            if file.start < 2 || file.start + file.sectors() > sectors {
                return Err(format!("{} lies outside the disk, so the catalogue is damaged.", file.shown()));
            }
            files.push(file);
        }
        let mut sorted = files.clone();
        sorted.sort_by_key(|f| f.start);
        if sorted.windows(2).any(|pair| pair[0].start + pair[0].sectors() > pair[1].start) {
            return Err("Two files on the disk overlap, so the catalogue is damaged.".into());
        }
        Ok(Side { sectors, files })
    }

    /// Writes a side's catalogue back, its files from the furthest in down, its write cycle one on.
    fn write_catalogue(&mut self, side: u8, mut files: Vec<DfsFile>) {
        files.sort_by_key(|f| std::cmp::Reverse(f.start));
        let (names, info) = (self.at(side, 0), self.at(side, 1));
        self.bytes[names + 8..names + 256].fill(0);
        self.bytes[info + 8..info + 256].fill(0);
        for (index, file) in files.iter().enumerate() {
            let at = 8 + index * 8;
            self.bytes[names + at..names + at + 7].copy_from_slice(&file.name);
            self.bytes[names + at + 7] = file.directory | if file.locked { 0x80 } else { 0 };
            let entry = &mut self.bytes[info + at..info + at + 8];
            entry[0..2].copy_from_slice(&(file.load as u16).to_le_bytes());
            entry[2..4].copy_from_slice(&(file.exec as u16).to_le_bytes());
            entry[4..6].copy_from_slice(&(file.length as u16).to_le_bytes());
            entry[6] = ((file.exec >> 16 & 3) << 6 | (file.length >> 16 & 3) << 4 | (file.load >> 16 & 3) << 2 | (file.start >> 8 & 3)) as u8;
            entry[7] = file.start as u8;
        }
        self.bytes[info + 5] = (files.len() * 8) as u8;
        self.bytes[info + 4] = bcd_next(self.bytes[info + 4]);
    }

    /// The side a folder (or the top) stands for: a .dsd's files are in its sides' folders.
    fn side_of(&self, folder: Option<&Entry>) -> Result<u8, String> {
        match folder {
            Some(side) if self.double && side.is_dir && side.location < 2 => Ok(side.location as u8),
            None if !self.double => Ok(0),
            None => Err("Open Side 1 or Side 2 first: each side of a double-sided disk has its own catalogue.".into()),
            Some(_) => Err("DFS has no folders.".into()),
        }
    }

    fn find(files: &[DfsFile], entry: &Entry) -> Result<usize, String> {
        if entry.is_dir {
            return Err("A side of the disk can't be changed, only the files on it.".into());
        }
        files.iter().position(|f| f.shown() == entry.name).ok_or_else(|| format!("{} isn't on the disk.", entry.name))
    }

    /// The first gap after the catalogue big enough for a file of this many sectors.
    fn gap(sectors: u32, files: &[DfsFile], wanted: u32) -> Option<u32> {
        let mut sorted: Vec<&DfsFile> = files.iter().collect();
        sorted.sort_by_key(|f| f.start);
        let mut next = 2;
        for file in sorted {
            if file.start - next >= wanted {
                return Some(next);
            }
            next = file.start + file.sectors();
        }
        (sectors - next >= wanted).then_some(next)
    }

    /// Moves every file down to close the gaps between them, as *COMPACT does.
    fn compact(&mut self, side: u8, files: &mut [DfsFile]) {
        files.sort_by_key(|f| f.start);
        let mut next = 2;
        for file in files.iter_mut() {
            if file.start != next {
                for n in 0..file.sectors() {
                    let (from, to) = (self.at(side, file.start + n), self.at(side, next + n));
                    self.bytes.copy_within(from..from + 256, to);
                }
                file.start = next;
            }
            next += file.sectors();
        }
    }

    fn free_on(&self, side: u8) -> Option<u32> {
        let catalogue = self.catalogue(side).ok()?;
        Some(catalogue.sectors - 2 - catalogue.files.iter().map(DfsFile::sectors).sum::<u32>())
    }
}

impl ImageWriter for Dfs {
    fn kinds(&self) -> Vec<Kind> {
        kinds()
    }

    fn guess(&self, name: &str, bytes: &[u8]) -> Addition {
        guess(name, bytes)
    }

    fn add(&mut self, folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        let side = self.side_of(folder)?;
        let Side { sectors, mut files } = self.catalogue(side)?;
        let (directory, name) = dfs_name(&file.name)?;
        let (bytes, load, exec) = prepared(file)?;
        let mut new = DfsFile { name, directory, locked: false, load: load & 0x3_FFFF, exec: exec & 0x3_FFFF, length: bytes.len() as u32, start: 0 };
        if let Some(index) = files.iter().position(|f| f.same_name(&new)) {
            if !file.replace {
                return Err(already(&files[index].shown()));
            }
            files.remove(index);
        }
        if bytes.len() >= 1 << 18 {
            return Err(format!("{} is larger than the 256K a DFS file can be.", file.name));
        }
        if files.len() >= 31 {
            return Err("The catalogue is full: a DFS side holds 31 files.".into());
        }
        let wanted = new.sectors();
        let free = sectors - 2 - files.iter().map(DfsFile::sectors).sum::<u32>();
        if wanted > free {
            return Err(format!("{} needs {wanted} sectors; the side has {free} free.", file.name));
        }
        new.start = match Self::gap(sectors, &files, wanted) {
            Some(start) => start,
            None => {
                self.compact(side, &mut files);
                Self::gap(sectors, &files, wanted).ok_or("The side's free space can't be put in one run.")?
            }
        };
        for (n, chunk) in bytes.chunks(256).enumerate() {
            let at = self.at(side, new.start + n as u32);
            self.bytes[at..at + 256].fill(0);
            self.bytes[at..at + chunk.len()].copy_from_slice(chunk);
        }
        files.push(new);
        self.write_catalogue(side, files);
        Ok(())
    }

    fn delete(&mut self, folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        let side = self.side_of(folder)?;
        let mut files = self.catalogue(side)?.files;
        let index = Self::find(&files, entry)?;
        files.remove(index);
        self.write_catalogue(side, files);
        Ok(())
    }

    fn rename(&mut self, folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        let side = self.side_of(folder)?;
        let mut files = self.catalogue(side)?.files;
        let index = Self::find(&files, entry)?;
        let (directory, new) = dfs_name(name)?;
        let renamed = DfsFile { name: new, directory, ..files[index].clone() };
        if let Some(other) = files.iter().position(|f| f.same_name(&renamed))
            && other != index
        {
            return Err(already(&renamed.shown()));
        }
        files[index] = renamed;
        self.write_catalogue(side, files);
        Ok(())
    }

    fn free(&self) -> Option<u64> {
        let sides: Vec<u32> = (0..if self.double { 2 } else { 1 }).filter_map(|side| self.free_on(side)).collect();
        Some(u64::from(sides.iter().sum::<u32>()) * 256)
    }

    fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{entry, names, reopened};
    use super::*;
    use crate::media::acorn::tests::bbc_program;
    use std::path::Path;
    use std::process::Command;

    fn add(disk: &mut Dfs, folder: Option<&Entry>, name: &str, bytes: &[u8]) {
        let addition = disk.guess(name, bytes);
        disk.add(folder, &addition).unwrap_or_else(|e| panic!("{name}: {e}"));
    }

    #[test]
    fn files_go_on_and_come_off_a_dfs_disk() {
        let mut disk = Dfs::open(blank(80, false, "GAMES"), false).unwrap();
        assert_eq!(disk.free(), Some(798 * 256));
        let big: Vec<u8> = (0..20_000u32).map(|i| (i * 7) as u8).collect();
        add(&mut disk, None, "prog.bas", &bbc_program());
        add(&mut disk, None, "elite", &big);
        add(&mut disk, None, "T.notes.txt", b"one\ntwo\n");
        let volume = reopened(&disk, "t.ssd");
        assert!(volume.describe().contains("GAMES"), "{}", volume.describe());
        assert_eq!(names(&*volume, None), ["$.elite", "$.prog", "T.notes"]);
        let prog = entry(&*volume, None, "$.prog");
        assert_eq!(volume.read(&prog).unwrap(), bbc_program());
        assert!(prog.detail.starts_with("Load FF1900 \u{B7} Exec FF8023"), "{}", prog.detail);
        assert_eq!(volume.read(&entry(&*volume, None, "$.elite")).unwrap(), big);
        assert_eq!(volume.read(&entry(&*volume, None, "T.notes")).unwrap(), b"one\rtwo\r");
        disk.rename(None, &entry(&*volume, None, "$.elite"), "B.Elite").unwrap();
        disk.delete(None, &entry(&*volume, None, "$.prog")).unwrap();
        let volume = reopened(&disk, "t.ssd");
        assert_eq!(names(&*volume, None), ["B.Elite", "T.notes"]);
        let again = disk.guess("Elite", &[1, 2, 3]);
        assert!(disk.add(None, &again).is_ok(), "$.Elite isn't B.Elite");
        let clash = disk.guess("B.ELITE", &[1]);
        assert!(disk.add(None, &clash).unwrap_err().starts_with(super::super::ALREADY));
    }

    #[test]
    fn a_fragmented_side_is_compacted_and_a_full_one_refused() {
        let mut disk = Dfs::open(blank(40, false, ""), false).unwrap();
        // Four files of 90 sectors fill 360 of the 398; taking out the first and third leaves two gaps of 90 and
        // 38 at the end, so 150 sectors only fit once the side is compacted.
        for n in 0..4u8 {
            add(&mut disk, None, &format!("F{n}"), &vec![n; 90 * 256]);
        }
        let volume = reopened(&disk, "t.ssd");
        disk.delete(None, &entry(&*volume, None, "$.F0")).unwrap();
        disk.delete(None, &entry(&*volume, None, "$.F2")).unwrap();
        let wide = vec![9u8; 150 * 256];
        add(&mut disk, None, "WIDE", &wide);
        let volume = reopened(&disk, "t.ssd");
        assert_eq!(volume.read(&entry(&*volume, None, "$.F1")).unwrap(), vec![1u8; 90 * 256]);
        assert_eq!(volume.read(&entry(&*volume, None, "$.F3")).unwrap(), vec![3u8; 90 * 256]);
        assert_eq!(volume.read(&entry(&*volume, None, "$.WIDE")).unwrap(), wide);
        let problem = disk.add(None, &disk.guess("MORE", &vec![0u8; 70 * 256])).unwrap_err();
        assert!(problem.contains("needs 70 sectors; the side has 68 free"), "{problem}");
        // 31 files fill the catalogue.
        let mut disk = Dfs::open(blank(40, false, ""), false).unwrap();
        for n in 0..31 {
            add(&mut disk, None, &format!("N{n}"), b"x");
        }
        assert!(disk.add(None, &disk.guess("N31", b"x")).unwrap_err().contains("31 files"));
    }

    #[test]
    fn each_side_of_a_dsd_is_its_own_catalogue() {
        let mut disk = Dfs::open(blank(80, true, "TWO"), true).unwrap();
        assert!(disk.add(None, &disk.guess("TOP", b"x")).is_err(), "a .dsd's files go on a side");
        let volume = reopened(&disk, "t.dsd");
        let sides = volume.list(None).unwrap();
        assert_eq!(sides.len(), 2);
        add(&mut disk, Some(&sides[1]), "BACK", b"on side two");
        add(&mut disk, Some(&sides[0]), "FRONT", &vec![5u8; 3000]);
        let volume = reopened(&disk, "t.dsd");
        let sides = volume.list(None).unwrap();
        assert_eq!(names(&*volume, Some(&sides[0])), ["$.FRONT"]);
        let back = entry(&*volume, Some(&sides[1]), "$.BACK");
        assert_eq!(volume.read(&back).unwrap(), b"on side two");
        disk.rename(Some(&sides[1]), &back, "W.BACK2").unwrap();
        assert_eq!(names(&*reopened(&disk, "t.dsd"), Some(&sides[1])), ["W.BACK2"]);
    }

    #[test]
    fn damaged_or_watford_catalogues_are_refused() {
        assert!(Dfs::open(vec![0; 100], false).is_err());
        let mut disk = blank(40, false, "");
        disk[256 + 5] = 3;
        assert!(Dfs::open(disk, false).is_err());
        let mut disk = blank(40, false, "");
        disk[512..520].fill(0xAA);
        assert!(Dfs::open(disk, false).err().unwrap().contains("Watford"));
        // A short image is grown to the whole disk.
        let short = blank(80, false, "")[..1024].to_vec();
        assert_eq!(Dfs::open(short, false).unwrap().bytes().len(), 800 * 256);
    }

    /// A disk with a BASIC program, code at &3000 and text, for the reference tools to read.
    fn sample() -> (Dfs, Vec<u8>) {
        let mut disk = Dfs::open(blank(80, false, "REF"), false).unwrap();
        let code: Vec<u8> = (0..1000u32).map(|i| (i * 13) as u8).collect();
        add(&mut disk, None, "prog.bas", &bbc_program());
        disk.add(None, &Addition { name: "C.CODE".into(), bytes: code.clone(), kind: CODE.into(), address: Some(0x3000), ..Default::default() }).unwrap();
        add(&mut disk, None, "T.README", b"hello\n");
        (disk, code)
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-dfs-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// MMB_Utils' Perl reads the catalogue, addresses and lengths, and copies the files out as they are.
    #[test]
    fn mmb_utils_reads_a_written_disk() {
        let tools = Path::new("/root/refmedia/acorn-tools/MMB_Utils");
        if !tools.join("info.pl").is_file() || super::super::tests::tool("perl").is_none() {
            return;
        }
        let dir = scratch("mmb");
        let (disk, code) = sample();
        let path = dir.join("t.ssd");
        std::fs::write(&path, disk.bytes()).unwrap();
        let info = Command::new("perl").arg(tools.join("info.pl")).arg(&path).output().unwrap();
        let info = String::from_utf8_lossy(&info.stdout);
        assert!(info.contains("Disk title: REF"), "{info}");
        let line = |name: &str| info.lines().find(|l| l.starts_with(name)).map(|l| l.split_whitespace().skip(1).collect::<Vec<_>>().join(" ")).unwrap_or_default();
        assert_eq!(line("C.CODE"), "FF3000 FF3000 0003E8 003", "{info}");
        assert_eq!(line("$.prog"), format!("FF1900 FF8023 {:06X} 002", bbc_program().len()), "{info}");
        assert_eq!(line("T.README"), "000000 000000 000006 007", "{info}");
        let out = dir.join("out");
        let got = Command::new("perl").arg(tools.join("getfile.pl")).arg(&path).arg(&out).output().unwrap();
        assert!(got.status.success(), "{}", String::from_utf8_lossy(&got.stderr));
        assert_eq!(std::fs::read(out.join("C.CODE")).unwrap(), code);
        // Files in the $ directory come out without it.
        assert_eq!(std::fs::read(out.join("prog")).unwrap(), bbc_program());
        std::fs::remove_dir_all(dir).ok();
    }

    /// beebtools lists both sides of a .dsd, with the addresses and lengths, and extracts the files.
    #[test]
    fn beebtools_reads_a_written_disk() {
        let beebtools = Path::new("/root/reftools/acorn/bin/beebtools");
        if !beebtools.is_file() {
            return;
        }
        let dir = scratch("beebtools");
        let (disk, code) = sample();
        let path = dir.join("t.ssd");
        std::fs::write(&path, disk.bytes()).unwrap();
        let listed = Command::new(beebtools).arg("cat").arg(&path).output().unwrap();
        let listed = String::from_utf8_lossy(&listed.stdout).to_string();
        assert!(listed.contains("C.CODE") && listed.contains("3000") && listed.contains("prog"), "{listed}");
        let out = dir.join("code.bin");
        let got = Command::new(beebtools).arg("extract").arg(&path).arg("C.CODE").arg("-o").arg(&out).output().unwrap();
        assert!(got.status.success(), "{}", String::from_utf8_lossy(&got.stderr));
        assert_eq!(std::fs::read(&out).unwrap(), code);

        let mut two = Dfs::open(blank(80, true, "TWO"), true).unwrap();
        let volume = reopened(&two, "t.dsd");
        let sides = volume.list(None).unwrap();
        add(&mut two, Some(&sides[1]), "BACK", &code);
        let path = dir.join("t.dsd");
        std::fs::write(&path, two.bytes()).unwrap();
        let listed = Command::new(beebtools).arg("cat").arg(&path).output().unwrap();
        let listed = String::from_utf8_lossy(&listed.stdout).to_string();
        assert!(listed.contains("BACK"), "{listed}");
        std::fs::remove_dir_all(dir).ok();
    }
}
