//! Apple II disks, DOS 3.3 and ProDOS, in either sector order and in 2IMG wrappers, as Beneath Apple DOS and the
//! ProDOS 8 Technical Reference Manual lay them out.
//!
//! DOS 3.3: the VTOC on track 17 sector 0 has a bitmap of free sectors and leads to the catalog, a chain of sectors
//! of seven entries. A file's entry leads to its track/sector lists, each listing up to 122 of its sectors; its
//! length is counted in sectors, the lists among them. A binary file starts with its load address and length, a
//! BASIC program with its length; text is high-bit ASCII, lines ending in &8D. Sectors are taken as DOS takes
//! them: from the tracks after the catalog outwards, then those before it, each track from sector 15 down.
//!
//! ProDOS: 512-byte blocks, a bitmap of the free ones, and directories of 39-byte entries, the volume directory at
//! block 2 and each folder a chain of blocks of its own. A file is a seedling (one block), a sapling (an index
//! block of up to 256) or a tree (a master index of index blocks), its type and auxiliary type (a binary's load
//! address) in its entry, with when it was made and changed.

use super::{Addition, Entry, ImageWriter, Kind, already};
use crate::basic_more;
use std::collections::HashSet;

const DISK_140K: usize = 35 * 16 * 256;
/// The two DOS sectors in each ProDOS block of a track, by the block's place on the track.
const BLOCK_SECTORS: [[usize; 2]; 8] = [[0, 14], [13, 12], [11, 10], [9, 8], [7, 6], [5, 4], [3, 2], [1, 15]];

const APPLESOFT: &str = "Applesoft BASIC";
const INTEGER: &str = "Integer BASIC";
const BINARY: &str = "Binary";
const TEXT: &str = "Text";
const SYSTEM: &str = "System program";
const UNTYPED: &str = "Untyped";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Order {
    Dos,
    Prodos,
}

/// An image's sectors and blocks, whichever order it keeps them in, after any 2IMG header.
struct Image {
    bytes: Vec<u8>,
    offset: usize,
    length: usize,
    order: Order,
}

impl Image {
    /// Where DOS's sector of a track is in the image.
    fn sector_at(&self, track: usize, sector: usize) -> usize {
        self.offset
            + match self.order {
                Order::Dos => (track * 16 + sector) * 256,
                Order::Prodos => {
                    let (block, half) = BLOCK_SECTORS.iter().enumerate().find_map(|(b, pair)| pair.iter().position(|&s| s == sector).map(|h| (b, h))).unwrap_or((0, 0));
                    track * 4096 + block * 512 + half * 256
                }
            }
    }

    /// Where a ProDOS block's two halves are in the image.
    fn block_at(&self, block: usize) -> [usize; 2] {
        match self.order {
            Order::Prodos => [self.offset + block * 512, self.offset + block * 512 + 256],
            Order::Dos => {
                let [first, second] = BLOCK_SECTORS[block % 8];
                [self.sector_at(block / 8, first), self.sector_at(block / 8, second)]
            }
        }
    }

    fn block(&self, block: usize) -> Vec<u8> {
        let [a, b] = self.block_at(block);
        [&self.bytes[a..a + 256], &self.bytes[b..b + 256]].concat()
    }

    fn put_block(&mut self, block: usize, data: &[u8]) {
        let [a, b] = self.block_at(block);
        let mut whole = [0u8; 512];
        whole[..data.len().min(512)].copy_from_slice(&data[..data.len().min(512)]);
        self.bytes[a..a + 256].copy_from_slice(&whole[..256]);
        self.bytes[b..b + 256].copy_from_slice(&whole[256..]);
    }
}

/// A writer for an Apple II disk: DOS 3.3 or ProDOS, whichever the reader finds on it.
pub fn open(bytes: Vec<u8>, ending: &str) -> Result<Box<dyn ImageWriter>, String> {
    let (offset, length, order) = if bytes.starts_with(b"2IMG") && bytes.len() >= 64 {
        let le32 = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize;
        if le32(16) & 0x8000_0000 != 0 {
            return Err("This 2IMG is marked locked, so it's only read.".into());
        }
        (le32(24), le32(28), Some(le32(12)))
    } else {
        (0, bytes.len(), None)
    };
    if order == Some(2) {
        return Err("This 2IMG holds a nibble image, which is only read.".into());
    }
    if offset.checked_add(length).is_none_or(|end| end > bytes.len()) || length < DISK_140K || !length.is_multiple_of(512) {
        return Err("This Apple II image is cut short or isn't a whole number of blocks.".into());
    }
    // The reader decides what's on the disk, and in which order.
    let volume = super::super::apple2::Apple2::open(std::sync::Arc::new(bytes.clone()), ending)?;
    let what = super::super::Volume::describe(&volume);
    let order = if what.contains(", DOS order)") { Order::Dos } else { Order::Prodos };
    let image = Image { bytes, offset, length, order };
    if what.contains("ProDOS,") {
        Ok(Box::new(Prodos::open(image)?))
    } else if what.contains("DOS 3.3") {
        Ok(Box::new(Dos33::open(image)?))
    } else {
        Err("Only DOS 3.3 and ProDOS disks are changed here; DOS 3.2's 13-sector disks are only read.".into())
    }
}

/// Whether bytes are a listing typed as text, rather than a tokenised program.
fn is_listing(bytes: &[u8]) -> bool {
    !bytes.is_empty() && std::str::from_utf8(bytes).is_ok_and(|text| !text.contains('\0') && text.lines().filter(|l| !l.trim().is_empty()).all(|l| l.trim_start().starts_with(|c: char| c.is_ascii_digit())))
}

/// An Applesoft program as it's saved: a listing tokenised, or a program already tokenised as it is.
fn applesoft(file: &Addition) -> Result<Vec<u8>, String> {
    if is_listing(&file.bytes) {
        let text = String::from_utf8_lossy(&file.bytes);
        let lines: Vec<&str> = text.lines().collect();
        return basic_more::tokenise_applesoft(&lines);
    }
    if basic_more::looks_like_applesoft(&file.bytes) {
        return Ok(file.bytes.clone());
    }
    Err(format!("{} is neither an Applesoft listing nor a tokenised Applesoft program.", file.name))
}

/// Lines ending as the Apple II's do, in a carriage return.
fn carriage_returns(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'\r' if bytes.get(at + 1) == Some(&b'\n') => {
                out.push(b'\r');
                at += 1;
            }
            b'\n' => out.push(b'\r'),
            b => out.push(b),
        }
        at += 1;
    }
    out
}

/// The kind and name a file most likely goes on as, its ending taken off when the kind says the same.
fn guessed(name: &str, bytes: &[u8]) -> (String, &'static str, Option<u16>) {
    let (stem, ending) = super::stem_and_ending(name);
    let text = !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_graphic() || matches!(b, b' ' | b'\r' | b'\n' | b'\t') || *b >= 0xA0 || *b == 0x8D);
    let (kind, address) = match ending.to_lowercase().as_str() {
        "bas" | "bbas" if is_listing(bytes) || basic_more::looks_like_applesoft(bytes) => (APPLESOFT, None),
        "int" => (INTEGER, None),
        "sys" | "system" => (SYSTEM, None),
        "txt" | "text" if text => (TEXT, None),
        "bin" | "hgr" | "pic" | "obj" => (BINARY, Some(0x2000)),
        _ if is_listing(bytes) => (APPLESOFT, None),
        _ if text => (TEXT, None),
        _ => (BINARY, Some(0x2000)),
    };
    let known = matches!(ending.to_lowercase().as_str(), "bas" | "int" | "sys" | "system" | "txt" | "text" | "bin" | "hgr");
    (if known { stem.to_string() } else { name.to_string() }, kind, address)
}

// ---------- DOS 3.3 ----------

pub struct Dos33 {
    image: Image,
}

/// A boot sector for a disk with nothing on it to boot: the Disk II's ROM loads it at &0800 and runs it from &0801,
/// and it clears the screen and says so.
const NO_BOOT: [u8; 20] = [0x01, 0x20, 0x58, 0xFC, 0xA2, 0x00, 0xBD, 0x14, 0x08, 0xF0, 0x06, 0x20, 0xED, 0xFD, 0xE8, 0xD0, 0xF5, 0x4C, 0x11, 0x08];

fn put_no_boot(sector: &mut [u8]) {
    sector[..NO_BOOT.len()].copy_from_slice(&NO_BOOT);
    for (at, b) in b"NOTHING ON THIS DISK TO BOOT".iter().enumerate() {
        sector[NO_BOOT.len() + at] = b | 0x80;
    }
}

/// A blank 140K DOS 3.3 disk, in DOS order, as INIT leaves one but with no DOS on tracks 0 to 2 to boot: volume 254,
/// the catalog on track 17 from sector 15 down.
pub fn blank_dos() -> Vec<u8> {
    let mut bytes = vec![0u8; DISK_140K];
    put_no_boot(&mut bytes);
    let at = |track: usize, sector: usize| (track * 16 + sector) * 256;
    let vtoc = at(17, 0);
    bytes[vtoc + 1] = 17;
    bytes[vtoc + 2] = 15;
    bytes[vtoc + 3] = 3;
    bytes[vtoc + 6] = 254;
    bytes[vtoc + 0x27] = 122;
    bytes[vtoc + 0x30] = 17;
    bytes[vtoc + 0x31] = 1;
    bytes[vtoc + 0x34] = 35;
    bytes[vtoc + 0x35] = 16;
    bytes[vtoc + 0x37] = 1;
    // Every track free but 0 to 2, where DOS itself would be, and the catalog's 17.
    for track in 3..35 {
        if track != 17 {
            bytes[vtoc + 0x38 + track * 4] = 0xFF;
            bytes[vtoc + 0x39 + track * 4] = 0xFF;
        }
    }
    for sector in 1..16 {
        let catalog = at(17, sector);
        if sector > 1 {
            bytes[catalog + 1] = 17;
            bytes[catalog + 2] = (sector - 1) as u8;
        }
    }
    bytes
}

/// A DOS 3.3 name: capitals, 30 at most, starting with a letter, with no commas.
fn dos_name(name: &str) -> Result<[u8; 30], String> {
    let cleaned: String = name.trim().chars().map(|c| c.to_ascii_uppercase()).filter(|c| (' '..='~').contains(c) && *c != ',').take(30).collect();
    let cleaned = cleaned.trim_end();
    if !cleaned.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return Err(format!("\"{name}\" can't be a DOS 3.3 name: a name has to start with a letter."));
    }
    let mut out = [0xA0; 30];
    for (slot, b) in out.iter_mut().zip(cleaned.bytes()) {
        *slot = b | 0x80;
    }
    Ok(out)
}

fn dos_shown(name: &[u8]) -> String {
    crate::media::tidy_name(&name.iter().map(|b| char::from(b & 0x7F)).map(|c| if c.is_control() { '?' } else { c }).collect::<String>())
}

impl Dos33 {
    fn open(image: Image) -> Result<Self, String> {
        if image.length != DISK_140K {
            return Err("Only 140K DOS 3.3 disks are changed here.".into());
        }
        let disk = Self { image };
        let vtoc = disk.vtoc();
        let v = &disk.image.bytes[vtoc..vtoc + 256];
        if v[0x35] != 16 || v[0x34] != 35 || v[0x27] != 122 || u16::from_le_bytes([v[0x36], v[0x37]]) != 256 {
            return Err("This disk's VTOC isn't DOS 3.3's 35 tracks of 16 sectors, so it's only read.".into());
        }
        disk.catalog()?;
        Ok(disk)
    }

    fn vtoc(&self) -> usize {
        self.image.sector_at(17, 0)
    }

    fn is_free(&self, track: usize, sector: usize) -> bool {
        let at = self.vtoc() + 0x38 + track * 4 + if sector >= 8 { 0 } else { 1 };
        self.image.bytes[at] & (1 << (sector % 8)) != 0
    }

    fn set_free(&mut self, track: usize, sector: usize, free: bool) {
        let at = self.vtoc() + 0x38 + track * 4 + if sector >= 8 { 0 } else { 1 };
        if free {
            self.image.bytes[at] |= 1 << (sector % 8);
        } else {
            self.image.bytes[at] &= !(1 << (sector % 8));
        }
    }

    /// The catalog's sectors, in its chain's order.
    fn catalog(&self) -> Result<Vec<usize>, String> {
        let vtoc = self.vtoc();
        let (mut track, mut sector) = (usize::from(self.image.bytes[vtoc + 1]), usize::from(self.image.bytes[vtoc + 2]));
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        while track != 0 {
            if track >= 35 || sector >= 16 || !seen.insert((track, sector)) {
                return Err("The disk's catalog chain is broken, so it can't be changed safely.".into());
            }
            let at = self.image.sector_at(track, sector);
            out.push(at);
            (track, sector) = (usize::from(self.image.bytes[at + 1]), usize::from(self.image.bytes[at + 2]));
        }
        Ok(out)
    }

    /// Every catalog slot, used or not, in order.
    fn slots(&self) -> Vec<usize> {
        self.catalog().unwrap_or_default().into_iter().flat_map(|at| (0..7).map(move |n| at + 0x0B + n * 35)).collect()
    }

    /// The slots holding files, as the reader finds them: each catalog sector's up to its first never-used slot.
    fn files(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for at in self.catalog().unwrap_or_default() {
            for slot in (0..7).map(|n| at + 0x0B + n * 35) {
                match self.image.bytes[slot] {
                    0 => break,
                    0xFF => {}
                    _ => out.push(slot),
                }
            }
        }
        out
    }

    fn name(&self, slot: usize) -> String {
        dos_shown(&self.image.bytes[slot + 3..slot + 33])
    }

    fn find(&self, name: &str) -> Option<usize> {
        let files = self.files();
        files.iter().copied().find(|s| self.name(*s) == name).or_else(|| files.into_iter().find(|s| self.name(*s).eq_ignore_ascii_case(name)))
    }

    /// A file's track/sector lists and the sectors they list, checked to lie on the disk.
    fn sectors_of(&self, slot: usize) -> Result<Vec<(usize, usize)>, String> {
        let (mut track, mut sector) = (usize::from(self.image.bytes[slot]), usize::from(self.image.bytes[slot + 1]));
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        while track != 0 {
            if track >= 35 || sector >= 16 || !seen.insert((track, sector)) || seen.len() > 600 {
                return Err(format!("{}'s track/sector lists are broken, so it can't be changed safely.", self.name(slot)));
            }
            out.push((track, sector));
            let at = self.image.sector_at(track, sector);
            for pair in self.image.bytes[at + 0x0C..at + 256].chunks(2) {
                let (t, s) = (usize::from(pair[0]), usize::from(pair[1]));
                if (t, s) == (0, 0) {
                    continue;
                }
                if t >= 35 || s >= 16 {
                    return Err(format!("{}'s track/sector list points off the disk, so it can't be changed safely.", self.name(slot)));
                }
                out.push((t, s));
            }
            (track, sector) = (usize::from(self.image.bytes[at + 1]), usize::from(self.image.bytes[at + 2]));
        }
        Ok(out)
    }

    fn remove(&mut self, slot: usize) -> Result<(), String> {
        for (track, sector) in self.sectors_of(slot)? {
            self.set_free(track, sector, true);
        }
        // DOS keeps a deleted file's track in the last byte of its name, and marks the entry &FF.
        self.image.bytes[slot + 32] = self.image.bytes[slot];
        self.image.bytes[slot] = 0xFF;
        Ok(())
    }

    /// Free sectors in the order DOS takes them: the tracks after the catalog's outwards, then those before it
    /// inwards from 16, each from sector 15 down. Track 0 is never given to a file.
    fn free_order(&self) -> Vec<(usize, usize)> {
        (18..35).chain((1..17).rev()).flat_map(|t| (0..16).rev().map(move |s| (t, s))).filter(|(t, s)| self.is_free(*t, *s)).collect()
    }

    /// A file's bytes as DOS keeps them, with the length and address before them that its type has, and its type.
    fn stored(file: &Addition) -> Result<(u8, Vec<u8>), String> {
        let with_length = |data: Vec<u8>, before: &[u8]| -> Result<Vec<u8>, String> {
            if data.len() > 0xFFFF {
                return Err(format!("{} is longer than the 65,535 bytes DOS 3.3 can say a program or binary is.", file.name));
            }
            Ok([before, &(data.len() as u16).to_le_bytes(), &data].concat())
        };
        Ok(match file.kind.as_str() {
            APPLESOFT => (0x02, with_length(applesoft(file)?, &[])?),
            INTEGER => (0x01, with_length(file.bytes.clone(), &[])?),
            TEXT => (0x00, carriage_returns(&file.bytes).into_iter().filter(|b| *b != 0).map(|b| b | 0x80).collect()),
            _ => (0x04, with_length(file.bytes.clone(), &file.address.unwrap_or(0x2000).to_le_bytes())?),
        })
    }

    fn put(&mut self, name: [u8; 30], file: &Addition) -> Result<(), String> {
        let (kind, data) = Self::stored(file)?;
        let data_sectors = data.len().div_ceil(256);
        let lists = data_sectors.div_ceil(122).max(1);
        let free = self.free_order();
        if data_sectors + lists > free.len() {
            return Err(format!("{} needs {} sectors; the disk has {} free.", file.name, data_sectors + lists, free.len()));
        }
        let slot = self.slots().into_iter().find(|s| matches!(self.image.bytes[*s], 0 | 0xFF)).ok_or("The catalog is full: DOS 3.3 holds 105 files.")?;
        let mut taken = free.into_iter();
        let mut previous_list: Option<usize> = None;
        let (mut first, mut last_track) = ((0, 0), 17);
        for list in 0..lists {
            let (lt, ls) = taken.next().ok_or("The disk ran out of sectors.")?;
            last_track = lt;
            self.set_free(lt, ls, false);
            let list_at = self.image.sector_at(lt, ls);
            self.image.bytes[list_at..list_at + 256].fill(0);
            self.image.bytes[list_at + 5..list_at + 7].copy_from_slice(&((list * 122) as u16).to_le_bytes());
            match previous_list {
                Some(at) => {
                    self.image.bytes[at + 1] = lt as u8;
                    self.image.bytes[at + 2] = ls as u8;
                }
                None => first = (lt, ls),
            }
            previous_list = Some(list_at);
            for n in 0..122.min(data_sectors.saturating_sub(list * 122)) {
                let (t, s) = taken.next().ok_or("The disk ran out of sectors.")?;
                last_track = t;
                self.set_free(t, s, false);
                let at = self.image.sector_at(t, s);
                let index = list * 122 + n;
                let part = &data[index * 256..(index * 256 + 256).min(data.len())];
                self.image.bytes[at..at + 256].fill(0);
                self.image.bytes[at..at + part.len()].copy_from_slice(part);
                self.image.bytes[list_at + 0x0C + n * 2] = t as u8;
                self.image.bytes[list_at + 0x0D + n * 2] = s as u8;
            }
        }
        // DOS remembers the last track it took from, and which way it was going.
        let vtoc = self.vtoc();
        self.image.bytes[vtoc + 0x30] = last_track as u8;
        self.image.bytes[vtoc + 0x31] = if last_track > 17 { 1 } else { 0xFF };
        let entry = &mut self.image.bytes[slot..slot + 35];
        entry[0] = first.0 as u8;
        entry[1] = first.1 as u8;
        entry[2] = kind;
        entry[3..33].copy_from_slice(&name);
        entry[33..35].copy_from_slice(&((data_sectors + lists) as u16).to_le_bytes());
        Ok(())
    }

    fn free_sectors(&self) -> usize {
        (0..35).flat_map(|t| (0..16).map(move |s| (t, s))).filter(|(t, s)| self.is_free(*t, *s)).count()
    }
}

impl ImageWriter for Dos33 {
    fn kinds(&self) -> Vec<Kind> {
        vec![
            Kind::plain(APPLESOFT, "An Applesoft program (A). A listing typed as text is tokenised first."),
            Kind::plain(INTEGER, "A tokenised Integer BASIC program (I), as SAVE writes one."),
            Kind { name: BINARY, about: "Bytes for BLOAD and BRUN (B), kept with the address they load at.", address: true, autostart: false },
            Kind::plain(TEXT, "A text file (T), as a program's WRITE leaves one: high-bit ASCII, lines ending in a return."),
        ]
    }

    fn guess(&self, name: &str, bytes: &[u8]) -> Addition {
        let (name, kind, address) = guessed(name, bytes);
        Addition { name, bytes: bytes.to_vec(), kind: kind.into(), address, ..Default::default() }
    }

    fn add(&mut self, _folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        let name = dos_name(&file.name)?;
        let shown = dos_shown(&name);
        let saved = self.image.bytes.clone();
        if let Some(slot) = self.find(&shown) {
            if !file.replace {
                return Err(already(&shown));
            }
            self.remove(slot)?;
        }
        let result = self.put(name, file);
        if result.is_err() {
            self.image.bytes = saved;
        }
        result
    }

    fn delete(&mut self, _folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        let slot = self.find(&entry.name).ok_or_else(|| format!("{} isn't on the disk.", entry.name))?;
        self.remove(slot)
    }

    fn rename(&mut self, _folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        let slot = self.find(&entry.name).ok_or_else(|| format!("{} isn't on the disk.", entry.name))?;
        let new = dos_name(name)?;
        if self.find(&dos_shown(&new)).is_some_and(|other| other != slot) {
            return Err(already(&dos_shown(&new)));
        }
        self.image.bytes[slot + 3..slot + 33].copy_from_slice(&new);
        Ok(())
    }

    fn free(&self) -> Option<u64> {
        Some(self.free_sectors() as u64 * 256)
    }

    fn bytes(&self) -> &[u8] {
        &self.image.bytes
    }
}

// ---------- ProDOS ----------

pub struct Prodos {
    image: Image,
    total: usize,
    bitmap: usize,
}

/// Where an entry is: its directory block and its place in that block, 0 to 12.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Slot {
    block: usize,
    index: usize,
}

impl Slot {
    fn at(self) -> usize {
        4 + self.index * 0x27
    }
}

/// Now as ProDOS keeps a date and time: the date's two bytes, then the minute and the hour.
fn prodos_now() -> [u8; 4] {
    let (date, time) = super::fat::now();
    let (year, month, day) = (1980 + (date >> 9), date >> 5 & 15, date & 31);
    let date = (year % 100) << 9 | month << 5 | day;
    let [low, high] = date.to_le_bytes();
    [low, high, (time >> 5 & 63) as u8, (time >> 11) as u8]
}

/// A ProDOS name: capitals, digits and dots, 15 at most, starting with a letter; other characters become dots.
fn prodos_name(name: &str) -> Result<Vec<u8>, String> {
    let cleaned: Vec<u8> = name.trim().chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() as u8 } else { b'.' }).take(15).collect();
    if !cleaned.first().is_some_and(u8::is_ascii_alphabetic) {
        return Err(format!("\"{name}\" can't be a ProDOS name: a name has to start with a letter."));
    }
    Ok(cleaned)
}

/// A blank ProDOS volume of this many blocks, in ProDOS order: nothing to boot, the volume directory's four blocks
/// from block 2 and the bitmap after them.
pub fn blank_prodos(blocks: usize, label: &str) -> Vec<u8> {
    let mut image = Image { bytes: vec![0u8; blocks * 512], offset: 0, length: blocks * 512, order: Order::Prodos };
    put_no_boot(&mut image.bytes);
    for block in 2..6 {
        let mut data = [0u8; 512];
        data[0..2].copy_from_slice(&(if block == 2 { 0 } else { block as u16 - 1 }).to_le_bytes());
        data[2..4].copy_from_slice(&(if block == 5 { 0 } else { block as u16 + 1 }).to_le_bytes());
        if block == 2 {
            let name = prodos_name(label).unwrap_or_else(|_| b"BLANK".to_vec());
            data[4] = 0xF0 | name.len() as u8;
            data[5..5 + name.len()].copy_from_slice(&name);
            data[0x1C..0x20].copy_from_slice(&prodos_now());
            data[0x22] = 0xC3;
            data[0x23] = 0x27;
            data[0x24] = 0x0D;
            data[0x27..0x29].copy_from_slice(&6u16.to_le_bytes());
            data[0x29..0x2B].copy_from_slice(&(blocks as u16).to_le_bytes());
        }
        image.put_block(block, &data);
    }
    let bitmap_blocks = blocks.div_ceil(4096);
    let used = 6 + bitmap_blocks;
    for block in used..blocks {
        let at = 6 * 512 + block / 8;
        image.bytes[at] |= 0x80 >> (block % 8);
    }
    image.bytes
}

impl Prodos {
    fn open(image: Image) -> Result<Self, String> {
        let key = image.block(2);
        let total = usize::from(u16::from_le_bytes([key[0x29], key[0x2A]]));
        let bitmap = usize::from(u16::from_le_bytes([key[0x27], key[0x28]]));
        if total < 16 || total * 512 > image.length || bitmap < 3 || bitmap + total.div_ceil(4096) > total {
            return Err("This ProDOS volume's header doesn't fit the image, so it can't be changed safely.".into());
        }
        let disk = Self { image, total, bitmap };
        disk.chain(2)?;
        Ok(disk)
    }

    fn is_free(&self, block: usize) -> bool {
        let at = self.image.block_at(self.bitmap + block / 4096)[(block % 4096 / 8) / 256] + (block % 4096 / 8) % 256;
        self.image.bytes[at] & (0x80 >> (block % 8)) != 0
    }

    fn set_free(&mut self, block: usize, free: bool) {
        let at = self.image.block_at(self.bitmap + block / 4096)[(block % 4096 / 8) / 256] + (block % 4096 / 8) % 256;
        if free {
            self.image.bytes[at] |= 0x80 >> (block % 8);
        } else {
            self.image.bytes[at] &= !(0x80 >> (block % 8));
        }
    }

    fn free_blocks(&self) -> Vec<usize> {
        (0..self.total).filter(|b| self.is_free(*b)).collect()
    }

    fn word(&self, block: usize, at: usize) -> usize {
        let data = self.image.block(block);
        usize::from(u16::from_le_bytes([data[at], data[at + 1]]))
    }

    /// A directory's blocks, from its key block.
    fn chain(&self, key: usize) -> Result<Vec<usize>, String> {
        let mut out = Vec::new();
        let mut block = key;
        while block != 0 {
            if block >= self.total || out.contains(&block) || out.len() > 4096 {
                return Err("A directory's chain of blocks is broken, so the disk can't be changed safely.".into());
            }
            out.push(block);
            block = self.word(block, 2);
        }
        Ok(out)
    }

    /// The key block of the directory a folder is, checked to be one.
    fn directory(&self, folder: Option<&Entry>) -> Result<usize, String> {
        let Some(folder) = folder else {
            return Ok(2);
        };
        let key = folder.extra as usize;
        if key < 3 || key >= self.total || self.image.block(key)[4] >> 4 != 0xE {
            return Err(format!("{} isn't a folder this disk has.", folder.name));
        }
        Ok(key)
    }

    /// Every entry slot of a directory, used or not, its header left out.
    fn slots(&self, key: usize) -> Result<Vec<Slot>, String> {
        Ok(self.chain(key)?.into_iter().flat_map(|block| (0..13).map(move |index| Slot { block, index })).filter(|s| !(s.block == key && s.index == 0)).collect())
    }

    fn entry(&self, slot: Slot) -> Vec<u8> {
        self.image.block(slot.block)[slot.at()..slot.at() + 0x27].to_vec()
    }

    fn put_entry(&mut self, slot: Slot, entry: &[u8]) {
        let mut data = self.image.block(slot.block);
        data[slot.at()..slot.at() + 0x27].copy_from_slice(entry);
        self.image.put_block(slot.block, &data);
    }

    /// The used slots of a directory, with each one's name.
    fn files(&self, key: usize) -> Result<Vec<(Slot, String)>, String> {
        Ok(self
            .slots(key)?
            .into_iter()
            .filter_map(|slot| {
                let entry = self.entry(slot);
                (entry[0] >> 4 != 0 && entry[0] & 15 != 0).then(|| (slot, String::from_utf8_lossy(&entry[1..1 + usize::from(entry[0] & 15)]).into_owned()))
            })
            .collect())
    }

    fn find(&self, key: usize, name: &str) -> Result<Option<Slot>, String> {
        Ok(self.files(key)?.into_iter().find(|(_, n)| crate::media::tidy_name(n).eq_ignore_ascii_case(name)).map(|(slot, _)| slot))
    }

    fn change_count(&mut self, key: usize, by: i32) {
        let mut data = self.image.block(key);
        let count = i32::from(u16::from_le_bytes([data[0x25], data[0x26]])) + by;
        data[0x25..0x27].copy_from_slice(&(count.max(0) as u16).to_le_bytes());
        self.image.put_block(key, &data);
    }

    /// The blocks a file has, its index blocks among them, checked to lie on the disk.
    fn blocks_of(&self, storage: u8, key: usize) -> Result<Vec<usize>, String> {
        let index = |block: usize| -> Vec<usize> {
            let data = self.image.block(block);
            (0..256).map(|i| usize::from(data[i]) | usize::from(data[256 + i]) << 8).filter(|b| *b != 0).collect()
        };
        let mut out = vec![key];
        match storage {
            1 => {}
            2 => out.extend(index(key)),
            3 => {
                for sub in index(key) {
                    if sub >= self.total {
                        break;
                    }
                    out.push(sub);
                    out.extend(index(sub));
                }
            }
            5 => {
                let extended = self.image.block(key);
                for fork in [0, 256] {
                    let (storage, fork_key) = (extended[fork] & 15, usize::from(u16::from_le_bytes([extended[fork + 1], extended[fork + 2]])));
                    if fork_key != 0 && matches!(storage, 1..=3) {
                        if fork_key >= self.total {
                            break;
                        }
                        out.extend(self.blocks_of(storage, fork_key)?);
                    }
                }
            }
            0xD => out = self.chain(key)?,
            _ => return Err("A file of a kind ProDOS 8 doesn't have is in the way, so it can't be changed safely.".into()),
        }
        if out.iter().any(|b| *b >= self.total) {
            return Err("A file's index points off the disk, so it can't be changed safely.".into());
        }
        Ok(out)
    }

    /// Takes an entry off its directory, its blocks freed; a folder only when it's empty.
    fn remove(&mut self, key: usize, slot: Slot) -> Result<(), String> {
        let entry = self.entry(slot);
        let storage = entry[0] >> 4;
        let first = usize::from(u16::from_le_bytes([entry[0x11], entry[0x12]]));
        if storage == 0xD && !self.files(first)?.is_empty() {
            return Err("The folder has files in it; take them off first.".into());
        }
        for block in self.blocks_of(storage, first)? {
            self.set_free(block, true);
        }
        let mut cleared = entry;
        cleared[0] = 0;
        self.put_entry(slot, &cleared);
        self.change_count(key, -1);
        Ok(())
    }

    /// A free slot in a directory, a block added to a folder that's full, which `blocks` is to have room for.
    fn room(&mut self, key: usize, free: &mut Vec<usize>) -> Result<Slot, String> {
        if let Some(slot) = self.slots(key)?.into_iter().find(|s| self.entry(*s)[0] >> 4 == 0) {
            return Ok(slot);
        }
        if key == 2 {
            return Err("The volume directory is full: it holds 51 files. Put the file in a folder.".into());
        }
        if free.is_empty() {
            return Err("The disk is full.".into());
        }
        let block = free.remove(0);
        self.set_free(block, false);
        let last = *self.chain(key)?.last().ok_or("The folder has no blocks.")?;
        let mut data = vec![0u8; 512];
        data[0..2].copy_from_slice(&(last as u16).to_le_bytes());
        self.image.put_block(block, &data);
        let mut tail = self.image.block(last);
        tail[2..4].copy_from_slice(&(block as u16).to_le_bytes());
        self.image.put_block(last, &tail);
        // The folder's entry in its parent grows by a block.
        let header = self.image.block(key);
        let (parent, number) = (usize::from(u16::from_le_bytes([header[0x27], header[0x28]])), usize::from(header[0x29]));
        if parent < self.total && (1..=13).contains(&number) {
            let slot = Slot { block: parent, index: number - 1 };
            let mut entry = self.entry(slot);
            if usize::from(u16::from_le_bytes([entry[0x11], entry[0x12]])) == key {
                let blocks = u16::from_le_bytes([entry[0x13], entry[0x14]]) + 1;
                entry[0x13..0x15].copy_from_slice(&blocks.to_le_bytes());
                entry[0x15..0x18].copy_from_slice(&(u32::from(blocks) * 512).to_le_bytes()[..3]);
                self.put_entry(slot, &entry);
            }
        }
        Ok(Slot { block, index: 0 })
    }

    /// A new entry, with the times stamped now.
    fn new_entry(name: &[u8], storage: u8, kind: u8, key: usize, blocks: usize, length: usize, aux: u16, directory: usize) -> Vec<u8> {
        let mut entry = vec![0u8; 0x27];
        entry[0] = storage << 4 | name.len() as u8;
        entry[1..1 + name.len()].copy_from_slice(name);
        entry[0x10] = kind;
        entry[0x11..0x13].copy_from_slice(&(key as u16).to_le_bytes());
        entry[0x13..0x15].copy_from_slice(&(blocks as u16).to_le_bytes());
        entry[0x15..0x18].copy_from_slice(&(length as u32).to_le_bytes()[..3]);
        entry[0x18..0x1C].copy_from_slice(&prodos_now());
        entry[0x1E] = 0xE3;
        entry[0x1F..0x21].copy_from_slice(&aux.to_le_bytes());
        entry[0x21..0x25].copy_from_slice(&prodos_now());
        entry[0x25..0x27].copy_from_slice(&(directory as u16).to_le_bytes());
        entry
    }

    /// A file's type, auxiliary type and bytes as ProDOS keeps them.
    fn stored(file: &Addition) -> Result<(u8, u16, Vec<u8>), String> {
        Ok(match file.kind.as_str() {
            APPLESOFT => (0xFC, 0x0801, applesoft(file)?),
            INTEGER => (0xFA, 0, file.bytes.clone()),
            TEXT => (0x04, 0, carriage_returns(&file.bytes)),
            SYSTEM => (0xFF, 0x2000, file.bytes.clone()),
            UNTYPED => (0x00, 0, file.bytes.clone()),
            _ => (0x06, file.address.unwrap_or(0x2000), file.bytes.clone()),
        })
    }

    fn put(&mut self, key: usize, name: &[u8], file: &Addition) -> Result<(), String> {
        let (kind, aux, data) = Self::stored(file)?;
        if data.len() > 0xFF_FFFF {
            return Err(format!("{} is larger than the 16 MB a ProDOS file can be.", file.name));
        }
        let count = data.len().div_ceil(512).max(1);
        let (storage, needed) = match count {
            1 => (1, 1),
            2..=256 => (2, count + 1),
            _ => (3, count + count.div_ceil(256) + 1),
        };
        let mut free = self.free_blocks();
        let slot = self.room(key, &mut free)?;
        if needed > free.len() {
            return Err(format!("{} needs {needed} blocks; the disk has {} free.", file.name, free.len()));
        }
        let blocks: Vec<usize> = free[..needed].to_vec();
        for block in &blocks {
            self.set_free(*block, false);
        }
        let (index_blocks, data_blocks) = blocks.split_at(needed - count);
        for (n, block) in data_blocks.iter().enumerate() {
            self.image.put_block(*block, &data[(n * 512).min(data.len())..(n * 512 + 512).min(data.len())]);
        }
        let index_of = |pointers: &[usize]| -> Vec<u8> {
            let mut out = vec![0u8; 512];
            for (i, p) in pointers.iter().enumerate() {
                out[i] = *p as u8;
                out[256 + i] = (*p >> 8) as u8;
            }
            out
        };
        let key_block = match storage {
            1 => data_blocks[0],
            2 => {
                self.image.put_block(index_blocks[0], &index_of(data_blocks));
                index_blocks[0]
            }
            _ => {
                let subs = &index_blocks[1..];
                for (sub, part) in subs.iter().zip(data_blocks.chunks(256)) {
                    self.image.put_block(*sub, &index_of(part));
                }
                self.image.put_block(index_blocks[0], &index_of(subs));
                index_blocks[0]
            }
        };
        let entry = Self::new_entry(name, storage, kind, key_block, needed, data.len(), aux, key);
        self.put_entry(slot, &entry);
        self.change_count(key, 1);
        Ok(())
    }
}

impl ImageWriter for Prodos {
    fn kinds(&self) -> Vec<Kind> {
        vec![
            Kind::plain(APPLESOFT, "An Applesoft program (BAS). A listing typed as text is tokenised first."),
            Kind { name: BINARY, about: "Bytes for BLOAD and BRUN (BIN), the load address as its auxiliary type.", address: true, autostart: false },
            Kind::plain(TEXT, "A text file (TXT), lines ending in a return."),
            Kind::plain(SYSTEM, "A program ProDOS runs itself (SYS), loading at $2000."),
            Kind::plain(INTEGER, "A tokenised Integer BASIC program (INT)."),
            Kind::plain(UNTYPED, "Bytes of no particular type ($00)."),
        ]
    }

    fn guess(&self, name: &str, bytes: &[u8]) -> Addition {
        let (name, kind, address) = guessed(name, bytes);
        Addition { name, bytes: bytes.to_vec(), kind: kind.into(), address, ..Default::default() }
    }

    fn add(&mut self, folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        let key = self.directory(folder)?;
        let name = prodos_name(&file.name)?;
        let shown = String::from_utf8_lossy(&name).into_owned();
        let saved = self.image.bytes.clone();
        if let Some(slot) = self.find(key, &shown)? {
            if !file.replace {
                return Err(already(&shown));
            }
            if self.entry(slot)[0] >> 4 == 0xD {
                return Err(format!("{shown} is a folder here, so a file can't replace it."));
            }
            self.remove(key, slot)?;
        }
        let result = self.put(key, &name, file);
        if result.is_err() {
            self.image.bytes = saved;
        }
        result
    }

    fn delete(&mut self, folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        let key = self.directory(folder)?;
        let slot = self.find(key, &entry.name)?.ok_or_else(|| format!("{} isn't in this folder.", entry.name))?;
        self.remove(key, slot)
    }

    fn rename(&mut self, folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        let key = self.directory(folder)?;
        let slot = self.find(key, &entry.name)?.ok_or_else(|| format!("{} isn't in this folder.", entry.name))?;
        let new = prodos_name(name)?;
        let shown = String::from_utf8_lossy(&new).into_owned();
        if self.find(key, &shown)?.is_some_and(|other| other != slot) {
            return Err(already(&shown));
        }
        let mut record = self.entry(slot);
        record[0] = record[0] & 0xF0 | new.len() as u8;
        record[1..16].fill(0);
        record[1..1 + new.len()].copy_from_slice(&new);
        // A GS/OS lower-case mask would no longer fit the name.
        if record[0x1D] & 0x80 != 0 {
            record[0x1C] = 0;
            record[0x1D] = 0;
        }
        self.put_entry(slot, &record);
        // A folder's own header keeps its name too.
        if record[0] >> 4 == 0xD {
            let first = usize::from(u16::from_le_bytes([record[0x11], record[0x12]]));
            let mut header = self.image.block(first);
            header[4] = 0xE0 | new.len() as u8;
            header[5..20].fill(0);
            header[5..5 + new.len()].copy_from_slice(&new);
            self.image.put_block(first, &header);
        }
        Ok(())
    }

    fn make_folder(&mut self, folder: Option<&Entry>, name: &str) -> Result<(), String> {
        let key = self.directory(folder)?;
        let name = prodos_name(name)?;
        let shown = String::from_utf8_lossy(&name).into_owned();
        if self.find(key, &shown)?.is_some() {
            return Err(already(&shown));
        }
        let saved = self.image.bytes.clone();
        let mut free = self.free_blocks();
        let slot = match self.room(key, &mut free) {
            Ok(slot) => slot,
            Err(problem) => {
                self.image.bytes = saved;
                return Err(problem);
            }
        };
        let Some(&block) = free.first() else {
            self.image.bytes = saved;
            return Err("The disk is full.".into());
        };
        self.set_free(block, false);
        let mut data = vec![0u8; 512];
        data[4] = 0xE0 | name.len() as u8;
        data[5..5 + name.len()].copy_from_slice(&name);
        data[0x14] = 0x75;
        data[0x1C..0x20].copy_from_slice(&prodos_now());
        data[0x22] = 0xE3;
        data[0x23] = 0x27;
        data[0x24] = 0x0D;
        data[0x27..0x29].copy_from_slice(&(slot.block as u16).to_le_bytes());
        data[0x29] = slot.index as u8 + 1;
        data[0x2A] = 0x27;
        self.image.put_block(block, &data);
        let entry = Self::new_entry(&name, 0xD, 0x0F, block, 1, 512, 0, key);
        self.put_entry(slot, &entry);
        self.change_count(key, 1);
        Ok(())
    }

    fn has_folders(&self) -> bool {
        true
    }

    fn free(&self) -> Option<u64> {
        Some(self.free_blocks().len() as u64 * 512)
    }

    fn bytes(&self) -> &[u8] {
        &self.image.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{entry, names, reopened};
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn add(disk: &mut dyn ImageWriter, folder: Option<&Entry>, name: &str, bytes: &[u8]) {
        let addition = disk.guess(name, bytes);
        disk.add(folder, &addition).unwrap_or_else(|e| panic!("{name}: {e}"));
    }

    fn program() -> Vec<u8> {
        b"10 HOME\n20 PRINT \"HELLO\": REM GREET\n30 FOR I = 1 TO 10: ? I: NEXT\n40 GOTO 10\n".to_vec()
    }

    /// A 140K disk's blocks put in DOS's sector order, or back.
    fn reordered(bytes: &[u8], to_dos: bool) -> Vec<u8> {
        let mut out = vec![0u8; bytes.len()];
        for block in 0..280 {
            let [first, second] = BLOCK_SECTORS[block % 8];
            for (half, sector) in [first, second].into_iter().enumerate() {
                let dos = ((block / 8) * 16 + sector) * 256;
                let prodos = block * 512 + half * 256;
                let (from, to) = if to_dos { (prodos, dos) } else { (dos, prodos) };
                out[to..to + 256].copy_from_slice(&bytes[from..from + 256]);
            }
        }
        out
    }

    #[test]
    fn applesoft_listings_are_tokenised_as_applesoft_does() {
        let program = basic_more::tokenise_applesoft(&["10 PRINT \"A B\": GOTO 10", "20 X = ATN(1) : Y = A TO B", "30 DATA 1, 2 : REM  x"]).unwrap();
        assert!(basic_more::looks_like_applesoft(&program));
        let listed = basic_more::list_applesoft(&program).lines;
        assert_eq!(listed[0], "10 PRINT \"A B\": GOTO 10");
        assert_eq!(&program[4..12], &[0xBA, b'"', b'A', b' ', b'B', b'"', b':', 0xAB]);
        assert!(program.windows(2).any(|w| w == [0xE1, b'(']), "ATN, not AT N");
        assert!(program.windows(3).any(|w| w == [b'A', 0xC1, b'B']), "A TO B, not AT OB");
        assert!(program.windows(7).any(|w| w == [0x83, b' ', b'1', b',', b' ', b'2', b' ']), "DATA keeps its spaces");
    }

    #[test]
    fn files_go_on_and_come_off_a_dos33_disk() {
        let mut disk = open(blank_dos(), "dsk").unwrap();
        assert_eq!(disk.free(), Some(32 * 16 * 256 - 16 * 256));
        let big: Vec<u8> = (0..40_000u32).map(|i| (i * 7) as u8).collect();
        let text: Vec<u8> = (0..200).flat_map(|i| format!("LINE {i}\n").into_bytes()).collect();
        add(&mut *disk, None, "hello.bas", &program());
        add(&mut *disk, None, "picture.bin", &big);
        add(&mut *disk, None, "notes.txt", &text);
        let volume = reopened(&*disk, "t.dsk");
        assert_eq!(names(&*volume, None), ["HELLO", "PICTURE", "NOTES"]);
        let hello = entry(&*volume, None, "HELLO");
        assert_eq!(hello.kind, "A Applesoft BASIC");
        assert_eq!(volume.read(&hello).unwrap(), basic_more::tokenise_applesoft(&String::from_utf8(program()).unwrap().lines().collect::<Vec<_>>()).unwrap());
        let picture = entry(&*volume, None, "PICTURE");
        assert_eq!(picture.detail, "$2000, 40000 bytes");
        assert_eq!(volume.read(&picture).unwrap(), big);
        let expected: Vec<u8> = text.iter().map(|b| if *b == b'\n' { 0x8D } else { b | 0x80 }).collect();
        assert_eq!(volume.read(&entry(&*volume, None, "NOTES")).unwrap(), expected);
        let free = disk.free().unwrap();
        disk.rename(None, &picture, "big picture").unwrap();
        disk.delete(None, &entry(&*volume, None, "NOTES")).unwrap();
        let replacement = Addition { replace: true, ..disk.guess("hello.bas", b"10 END\n") };
        disk.add(None, &replacement).unwrap();
        assert!(disk.add(None, &disk.guess("hello.bas", b"10 END\n")).unwrap_err().starts_with(super::super::ALREADY));
        let volume = reopened(&*disk, "t.dsk");
        assert_eq!(names(&*volume, None), ["HELLO", "BIG PICTURE"]);
        assert!(disk.free().unwrap() > free);
        assert!(disk.add(None, &disk.guess("1st.txt", b"x")).is_err(), "a DOS name starts with a letter");
        let huge = Addition { name: "HUGE".into(), bytes: vec![1; 140_000], kind: TEXT.into(), ..Default::default() };
        let before = disk.bytes().to_vec();
        assert!(disk.add(None, &huge).unwrap_err().contains("free"));
        assert_eq!(disk.bytes(), before, "a refusal leaves the disk as it was");
    }

    #[test]
    fn a_dos33_disk_in_prodos_order_is_written_in_its_own_order() {
        let mut disk = open(reordered(&blank_dos(), false), "po").unwrap();
        add(&mut *disk, None, "code.bin", &[0xA9, 0x00, 0x60]);
        let volume = reopened(&*disk, "t.po");
        assert!(volume.describe().contains("DOS 3.3") && volume.describe().contains("ProDOS order"), "{}", volume.describe());
        assert_eq!(volume.read(&entry(&*volume, None, "CODE")).unwrap(), [0xA9, 0x00, 0x60]);
        let back = reordered(disk.bytes(), true);
        let volume = super::super::super::open(std::sync::Arc::new(back), "t.dsk").unwrap();
        assert_eq!(volume.read(&entry(&*volume, None, "CODE")).unwrap(), [0xA9, 0x00, 0x60]);
    }

    #[test]
    fn files_and_folders_go_on_a_prodos_disk() {
        let mut disk = open(blank_prodos(1600, "work disk"), "po").unwrap();
        assert_eq!(disk.free(), Some((1600 - 7) * 512));
        let small = b"tiny".to_vec();
        let middle: Vec<u8> = (0..100_000u32).map(|i| (i * 13) as u8).collect();
        let large: Vec<u8> = (0..300_000u32).map(|i| ((i * 31) >> 3) as u8).collect();
        add(&mut *disk, None, "hello.bas", &program());
        add(&mut *disk, None, "tiny.bin", &small);
        disk.make_folder(None, "games").unwrap();
        let volume = reopened(&*disk, "t.po");
        assert!(volume.describe().contains("/WORK.DISK"), "{}", volume.describe());
        let games = entry(&*volume, None, "GAMES");
        disk.make_folder(Some(&games), "Arcade").unwrap();
        add(&mut *disk, Some(&games), "middle.bin", &middle);
        add(&mut *disk, Some(&games), "large", &large);
        // More than a block's 12 entries make the folder grow.
        for n in 0..20 {
            add(&mut *disk, Some(&games), &format!("f{n}.txt"), format!("file {n}\n").as_bytes());
        }
        let volume = reopened(&*disk, "t.po");
        let games = entry(&*volume, None, "GAMES");
        assert_eq!(names(&*volume, Some(&games)).len(), 23);
        assert_eq!(volume.read(&entry(&*volume, Some(&games), "MIDDLE")).unwrap(), middle);
        assert_eq!(volume.read(&entry(&*volume, Some(&games), "LARGE")).unwrap(), large);
        assert_eq!(volume.read(&entry(&*volume, Some(&games), "F19")).unwrap(), b"file 19\r");
        assert_eq!(volume.read(&entry(&*volume, None, "TINY")).unwrap(), small);
        assert_eq!(entry(&*volume, Some(&games), "MIDDLE").detail[..6], *"$2000,");
        let arcade = entry(&*volume, Some(&games), "ARCADE");
        add(&mut *disk, Some(&arcade), "deep.txt", b"down here");
        let volume = reopened(&*disk, "t.po");
        let games = entry(&*volume, None, "GAMES");
        let arcade = entry(&*volume, Some(&games), "ARCADE");
        assert_eq!(volume.read(&entry(&*volume, Some(&arcade), "DEEP")).unwrap(), b"down here");
        assert!(disk.delete(Some(&games), &arcade).unwrap_err().contains("files in it"));
        disk.delete(Some(&arcade), &entry(&*volume, Some(&arcade), "DEEP")).unwrap();
        disk.rename(Some(&games), &arcade, "shooters").unwrap();
        let volume = reopened(&*disk, "t.po");
        let games = entry(&*volume, None, "GAMES");
        let shooters = entry(&*volume, Some(&games), "SHOOTERS");
        assert!(names(&*volume, Some(&shooters)).is_empty());
        let before = disk.free().unwrap();
        disk.delete(Some(&games), &shooters).unwrap();
        disk.delete(Some(&games), &entry(&*volume, Some(&games), "LARGE")).unwrap();
        // 300,000 bytes are 586 blocks, their 3 index blocks and the master index; the folder's one block is back too.
        assert_eq!(disk.free().unwrap() - before, (586 + 3 + 1 + 1) * 512);
        let volume = reopened(&*disk, "t.po");
        assert_eq!(names(&*volume, None), ["HELLO", "TINY", "GAMES"]);
    }

    #[test]
    fn prodos_in_dos_order_and_in_a_2img() {
        let mut disk = open(reordered(&blank_prodos(280, "ORDER"), true), "dsk").unwrap();
        add(&mut *disk, None, "a.txt", b"one");
        disk.make_folder(None, "sub").unwrap();
        let volume = reopened(&*disk, "t.dsk");
        assert!(volume.describe().contains("DOS order"));
        assert_eq!(volume.read(&entry(&*volume, None, "A")).unwrap(), b"one");
        let mut two = b"2IMG".to_vec();
        two.resize(64, 0);
        two[8..10].copy_from_slice(&64u16.to_le_bytes());
        two[12] = 1;
        two[24] = 64;
        two[28..32].copy_from_slice(&(DISK_140K as u32).to_le_bytes());
        two.extend(blank_prodos(280, "WRAPPED"));
        two.extend(b"comment");
        let mut disk = open(two, "2mg").unwrap();
        add(&mut *disk, None, "b.txt", b"two");
        assert!(disk.bytes().ends_with(b"comment"));
        let volume = reopened(&*disk, "t.2mg");
        assert_eq!(volume.read(&entry(&*volume, None, "B")).unwrap(), b"two");
    }

    #[test]
    fn damaged_apple_disks_are_refused_not_panicked_over() {
        // A catalog that points at itself.
        let mut bytes = blank_dos();
        let at = 17 * 16 * 256 + 15 * 256;
        bytes[at + 1] = 17;
        bytes[at + 2] = 15;
        assert!(open(bytes, "dsk").is_err());
        // A file whose list leads off the disk.
        let mut disk = open(blank_dos(), "dsk").unwrap();
        add(&mut *disk, None, "x.bin", &[1, 2, 3]);
        let mut bytes = disk.bytes().to_vec();
        let list = (18 * 16 + 15) * 256;
        bytes[list + 0x0C] = 40;
        let mut disk = open(bytes, "dsk").unwrap();
        let file = Entry::file("X".into(), 3, 0);
        assert!(disk.delete(None, &file).is_err());
        // A ProDOS volume whose header says it's larger than the image.
        let mut bytes = blank_prodos(280, "X");
        bytes[2 * 512 + 0x29..2 * 512 + 0x2B].copy_from_slice(&1600u16.to_le_bytes());
        assert!(open(bytes, "po").is_err());
        assert!(open(vec![0; 1000], "po").is_err());
        assert!(open(vec![0x55; DISK_140K], "dsk").is_err());
    }

    fn applecommander() -> Option<PathBuf> {
        Some(PathBuf::from("/root/reftools/ac/ac")).filter(|p| p.is_file())
    }

    fn a2kit() -> Option<PathBuf> {
        super::super::tests::tool("a2kit").or_else(|| Some(PathBuf::from("/root/.cargo/bin/a2kit")).filter(|p| p.is_file()))
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-apple2-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn run(tool: &Path, args: &[&str]) -> (bool, Vec<u8>, String) {
        let out = Command::new(tool).args(args).output().unwrap();
        (out.status.success(), out.stdout, String::from_utf8_lossy(&out.stderr).into_owned())
    }

    /// Thousands grouped with commas, as AppleCommander shows bytes free.
    fn group(n: u64) -> String {
        let digits = n.to_string();
        let mut out = String::new();
        for (i, c) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
        }
        out
    }

    /// AppleCommander lists and copies out what's written, on both file systems.
    #[test]
    fn applecommander_reads_written_disks() {
        let Some(ac) = applecommander() else {
            return;
        };
        let dir = scratch("ac");
        let big: Vec<u8> = (0..40_000u32).map(|i| (i * 7) as u8).collect();
        let mut dos = open(blank_dos(), "dsk").unwrap();
        add(&mut *dos, None, "hello.bas", &program());
        dos.add(None, &Addition { name: "PICTURE".into(), bytes: big.clone(), kind: BINARY.into(), address: Some(0x4000), ..Default::default() }).unwrap();
        add(&mut *dos, None, "notes.txt", b"ONE\nTWO\n");
        add(&mut *dos, None, "gone.txt", b"x");
        let volume = reopened(&*dos, "t.dsk");
        dos.delete(None, &entry(&*volume, None, "GONE")).unwrap();
        let path = dir.join("dos.dsk");
        std::fs::write(&path, dos.bytes()).unwrap();
        let p = path.to_str().unwrap();
        let (ok, listed, problem) = run(&ac, &["-ll", p]);
        let listed = String::from_utf8_lossy(&listed);
        assert!(ok, "{problem}");
        assert!(listed.contains("PICTURE") && listed.contains("A=$4000") && !listed.contains("GONE"), "{listed}");
        assert!(listed.contains(&format!("{} bytes free", group(dos.free().unwrap()))), "{listed}");
        // Its raw bytes, the address and length taken off, up to the end of the last sector.
        let got = run(&ac, &["-g", p, "PICTURE"]).1;
        assert!(got.len() < big.len() + 256 && got[..big.len()] == big[..], "{} bytes", got.len());
        assert_eq!(run(&ac, &["-e", p, "NOTES"]).1, b"ONE\nTWO\n");
        let (_, basic, _) = run(&ac, &["-e", p, "HELLO"]);
        assert!(String::from_utf8_lossy(&basic).contains("30  FOR I = 1 TO 10: PRINT I: NEXT"), "{}", String::from_utf8_lossy(&basic));

        let mut prodos = open(blank_prodos(1600, "REF"), "po").unwrap();
        let large: Vec<u8> = (0..300_000u32).map(|i| ((i * 31) >> 3) as u8).collect();
        prodos.make_folder(None, "DIR").unwrap();
        let volume = reopened(&*prodos, "t.po");
        let folder = entry(&*volume, None, "DIR");
        prodos.add(Some(&folder), &Addition { name: "LARGE".into(), bytes: large.clone(), kind: BINARY.into(), address: Some(0x0C00), ..Default::default() }).unwrap();
        for n in 0..15 {
            add(&mut *prodos, Some(&folder), &format!("F{n}.TXT"), b"text\n");
        }
        add(&mut *prodos, None, "hello.bas", &program());
        let path = dir.join("pro.po");
        std::fs::write(&path, prodos.bytes()).unwrap();
        let p = path.to_str().unwrap();
        let (ok, listed, problem) = run(&ac, &["-ll", p]);
        let listed = String::from_utf8_lossy(&listed);
        assert!(ok, "{problem}");
        assert!(listed.contains("LARGE") && listed.contains("F14") && listed.contains("HELLO"), "{listed}");
        assert!(listed.contains(&format!("{} bytes free", group(prodos.free().unwrap()))), "{listed}");
        assert_eq!(run(&ac, &["-g", p, "DIR/LARGE"]).1, large);
        std::fs::remove_dir_all(dir).ok();
    }

    /// a2kit, a second reader, lists both kinds of disk, finds the free space each says, and tokenises Applesoft as
    /// the writer does.
    #[test]
    fn a2kit_reads_written_disks() {
        let Some(a2kit) = a2kit() else {
            return;
        };
        let dir = scratch("a2kit");
        let tokenised = Command::new(&a2kit).args(["tokenize", "-t", "atxt", "-a", "2049"]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(&program())?;
            child.wait_with_output()
        });
        let ours = basic_more::tokenise_applesoft(&String::from_utf8(program()).unwrap().lines().collect::<Vec<_>>()).unwrap();
        assert_eq!(tokenised.unwrap().stdout, ours);

        let mut dos = open(blank_dos(), "dsk").unwrap();
        add(&mut *dos, None, "hello.bas", &program());
        add(&mut *dos, None, "data.bin", &[9u8; 30_000]);
        let path = dir.join("dos.dsk");
        std::fs::write(&path, dos.bytes()).unwrap();
        let (ok, listed, problem) = run(&a2kit, &["catalog", "-d", path.to_str().unwrap()]);
        let listed = String::from_utf8_lossy(&listed);
        assert!(ok, "{problem}");
        assert!(listed.contains(" A 002 HELLO") && listed.contains(" B 119 DATA"), "{listed}");
        let (_, got, _) = run(&a2kit, &["get", "-f", "DATA", "-t", "bin", "-d", path.to_str().unwrap()]);
        assert!(got.len() >= 30_000 && got[..30_000] == [9u8; 30_000][..], "{} bytes", got.len());

        let mut prodos = open(blank_prodos(280, "KIT"), "po").unwrap();
        prodos.make_folder(None, "SUB").unwrap();
        let volume = reopened(&*prodos, "t.po");
        add(&mut *prodos, Some(&entry(&*volume, None, "SUB")), "inner.bin", &[5u8; 2000]);
        let path = dir.join("pro.po");
        std::fs::write(&path, prodos.bytes()).unwrap();
        let p = path.to_str().unwrap();
        let (ok, stat, problem) = run(&a2kit, &["stat", "-d", p]);
        assert!(ok, "{problem}");
        let stat = String::from_utf8_lossy(&stat);
        assert!(stat.contains(&format!("\"free_blocks\":{}", prodos.free().unwrap() / 512)), "{stat}");
        let (ok, got, problem) = run(&a2kit, &["get", "-f", "/KIT/SUB/INNER", "-t", "bin", "-d", p]);
        assert!(ok, "{problem}");
        assert_eq!(got, vec![5u8; 2000]);
        std::fs::remove_dir_all(dir).ok();
    }
}
