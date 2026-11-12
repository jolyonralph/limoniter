//! Apple II disks: DOS 3.3 and ProDOS, on 140K 5.25-inch images in either sector order (.dsk, .do, .po), 800K
//! and hard disk ProDOS volumes (.po, .hdv), and 2IMG wrappers of either (.2mg), as Beneath Apple DOS and the
//! ProDOS 8 Technical Reference Manual lay them out.
//!
//! A 5.25-inch disk is 35 tracks of 16 256-byte sectors. An image keeps them either in DOS 3.3's order, or in
//! ProDOS's, a 512-byte block at a time; each block is two of DOS's sectors (block 0 of a track is sectors 0 and 14,
//! block 1 is 13 and 12, and so on), so either file system can be read from either order.
//!
//! DOS 3.3: the VTOC on track 17 sector 0 says where the catalog is; each catalog sector holds 7 entries of a file's
//! first track/sector list, its type (T text, I Integer BASIC, A Applesoft, B binary, S, R relocatable) and its
//! 30-character name in high-bit ASCII. Binary files start with their load address and length, BASIC programs with
//! their length; these are taken off, the address shown in Details.
//!
//! ProDOS: the volume directory starts at block 2. Each entry gives a storage type (seedling: one data block;
//! sapling: an index block of up to 256; tree: a master index of index blocks; a subdirectory; or an extended file
//! with a data and resource fork), its key block, length, type, auxiliary type (a binary's load address) and dates.

use super::{Entry, Source, Volume, tidy_name};
use std::sync::Arc;

const TRACK: u64 = 16 * 256;
const DISK_140K: u64 = 35 * TRACK;
/// The two DOS sectors in each ProDOS block of a track, by the block's place on the track.
const BLOCK_SECTORS: [[u64; 2]; 8] = [[0, 14], [13, 12], [11, 10], [9, 8], [7, 6], [5, 4], [3, 2], [1, 15]];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Order {
    Dos,
    Prodos,
}

/// An image's sectors and blocks, whichever order it keeps them in.
struct Disk {
    source: Arc<dyn Source>,
    /// Where the disk starts in the file: after a 2IMG header.
    offset: u64,
    length: u64,
    order: Order,
}

impl Disk {
    fn read(&self, at: u64, length: usize) -> Result<Vec<u8>, String> {
        if at + length as u64 > self.length {
            return Err("a sector past the end of the image".into());
        }
        self.source.read_vec(self.offset + at, length)
    }

    /// DOS 3.3's sector of a track.
    fn sector(&self, track: u64, sector: u64) -> Result<Vec<u8>, String> {
        match self.order {
            Order::Dos => self.read(track * TRACK + sector * 256, 256),
            Order::Prodos => {
                let (block, half) = BLOCK_SECTORS.iter().enumerate().find_map(|(b, pair)| pair.iter().position(|&s| s == sector).map(|h| (b as u64, h as u64))).unwrap_or((0, 0));
                self.read(track * TRACK + block * 512 + half * 256, 256)
            }
        }
    }

    /// ProDOS's 512-byte block.
    fn block(&self, block: u64) -> Result<Vec<u8>, String> {
        match self.order {
            Order::Prodos => self.read(block * 512, 512),
            Order::Dos => {
                let [first, second] = BLOCK_SECTORS[(block % 8) as usize];
                let mut out = self.sector(block / 8, first)?;
                out.extend(self.sector(block / 8, second)?);
                Ok(out)
            }
        }
    }
}

/// An Apple II name in high-bit ASCII, its padding taken off.
fn high_ascii(bytes: &[u8]) -> String {
    bytes.iter().map(|b| char::from(b & 0x7F)).map(|c| if c.is_control() { '?' } else { c }).collect::<String>().trim_end().to_string()
}

fn le16(b: &[u8], at: usize) -> u64 {
    u64::from(u16::from_le_bytes([b[at], b[at + 1]]))
}

// ---------- DOS 3.3 ----------

struct DosFile {
    name: String,
    kind: u8,
    locked: bool,
    first_list: (u64, u64),
    sectors: u64,
}

fn dos_vtoc_ok(vtoc: &[u8]) -> bool {
    vtoc.len() >= 0x38 && (1..35).contains(&vtoc[1]) && vtoc[2] < 16 && vtoc[0x27] == 122 && vtoc[0x34] == 35 && vtoc[0x35] == 16
}

fn dos_catalog(disk: &Disk) -> Result<(u8, Vec<DosFile>), String> {
    let vtoc = disk.sector(17, 0)?;
    if !dos_vtoc_ok(&vtoc) {
        return Err("no DOS 3.3 VTOC".into());
    }
    let mut files = Vec::new();
    let (mut track, mut sector) = (u64::from(vtoc[1]), u64::from(vtoc[2]));
    let mut seen = 0;
    while track != 0 && seen < 64 {
        seen += 1;
        let catalog = disk.sector(track, sector)?;
        for entry in catalog[0x0B..].as_chunks::<35>().0.iter().take(7) {
            match entry[0] {
                0 => break,
                // Deleted: its track is kept at the end of the name.
                0xFF => {}
                list_track => files.push(DosFile {
                    name: high_ascii(&entry[3..33]),
                    kind: entry[2] & 0x7F,
                    locked: entry[2] & 0x80 != 0,
                    first_list: (u64::from(list_track), u64::from(entry[1])),
                    sectors: le16(entry, 33),
                }),
            }
        }
        (track, sector) = (u64::from(catalog[1]), u64::from(catalog[2]));
    }
    Ok((vtoc[6], files))
}

fn dos_kind(kind: u8) -> (&'static str, char) {
    match kind {
        0x00 => ("Text", 'T'),
        0x01 => ("Integer BASIC", 'I'),
        0x02 => ("Applesoft BASIC", 'A'),
        0x04 => ("Binary", 'B'),
        0x08 => ("S-type", 'S'),
        0x10 => ("Relocatable", 'R'),
        0x20 => ("A-type", 'a'),
        _ => ("B-type", 'b'),
    }
}

/// A DOS 3.3 file's sectors, through its chain of track/sector lists. A random-access file can have sectors never
/// written, listed as track 0 sector 0: they read as zeros, except at the very end, where the file stops.
fn dos_read(disk: &Disk, file: &DosFile) -> Result<Vec<u8>, String> {
    let mut places = Vec::new();
    let (mut track, mut sector) = file.first_list;
    let mut lists = 0;
    while track != 0 && lists < 600 {
        lists += 1;
        let list = disk.sector(track, sector)?;
        places.extend(list[0x0C..].as_chunks::<2>().0.iter().map(|p| (u64::from(p[0]), u64::from(p[1]))));
        (track, sector) = (u64::from(list[1]), u64::from(list[2]));
    }
    while places.last() == Some(&(0, 0)) {
        places.pop();
    }
    let mut out = Vec::with_capacity(places.len() * 256);
    for (t, s) in places {
        if (t, s) == (0, 0) {
            out.extend([0u8; 256]);
        } else {
            out.extend(disk.sector(t, s)?);
        }
    }
    Ok(out)
}

// ---------- ProDOS ----------

struct ProdosFile {
    name: String,
    storage: u8,
    kind: u8,
    key: u64,
    length: u64,
    aux: u64,
    modified: String,
    /// The folder it's in, as an index into the folders, or None for the volume directory.
    folder: Option<usize>,
}

fn prodos_date(b: &[u8]) -> String {
    let date = le16(b, 0);
    if date == 0 {
        return String::new();
    }
    let (day, month, year) = (date & 31, date >> 5 & 15, date >> 9);
    let year = if year < 40 { 2000 + year } else { 1900 + year };
    format!("{year}-{month:02}-{day:02} {:02}:{:02}", b[3] & 31, b[2] & 63)
}

fn prodos_kind(kind: u8) -> &'static str {
    match kind {
        0x00 => "Untyped",
        0x01 => "Bad blocks",
        0x04 => "TXT",
        0x06 => "BIN",
        0x08 => "FOT (picture)",
        0x0F => "DIR",
        0x19 => "AppleWorks database",
        0x1A => "AppleWorks document",
        0x1B => "AppleWorks spreadsheet",
        0xB3 => "GS/OS application",
        0xC0 => "PNT (packed picture)",
        0xC1 => "PIC (picture)",
        0xE0 => "Archive",
        0xEF => "Pascal",
        0xF0 => "CMD",
        0xFA => "INT (Integer BASIC)",
        0xFB => "IVR",
        0xFC => "BAS (Applesoft)",
        0xFD => "VAR",
        0xFE => "REL",
        0xFF => "SYS",
        _ => "Other",
    }
}

/// A ProDOS name, in the case GS/OS gave it: when the top bit of an entry's minimum version byte is set, it and the
/// version byte below it are a mask, bit 14 down to bit 0 saying which of the name's letters are lower case.
fn prodos_name(name: &[u8], version: u8, min_version: u8) -> String {
    let mask = u16::from(min_version) << 8 | u16::from(version);
    name.iter()
        .enumerate()
        .map(|(i, &b)| {
            let c = char::from(b);
            if mask & 0x8000 != 0 && i < 15 && mask & (0x4000 >> i) != 0 { c.to_ascii_lowercase() } else { c }
        })
        .collect()
}

/// Whether block 2 (of either order) is a ProDOS volume directory's key block.
fn prodos_header_ok(block: &[u8]) -> bool {
    block.len() >= 0x2B && le16(block, 0) == 0 && block[4] >> 4 == 0xF && block[4] & 15 > 0 && block[0x23] == 0x27 && block[0x24] == 0x0D
}

/// Reads a directory and the folders inside it, adding their entries.
fn prodos_directory(disk: &Disk, key: u64, folder: Option<usize>, folders: &mut Vec<(String, Option<usize>)>, files: &mut Vec<ProdosFile>, depth: usize) -> Result<(), String> {
    if depth > 16 {
        return Ok(());
    }
    let mut block = key;
    let mut first = true;
    let mut seen = 0;
    while block != 0 && seen < 4096 {
        seen += 1;
        let data = disk.block(block)?;
        // The first entry of a directory's first block is its header.
        for (index, entry) in data[4..].as_chunks::<0x27>().0.iter().enumerate().take(13) {
            if first && index == 0 {
                continue;
            }
            let storage = entry[0] >> 4;
            let length = usize::from(entry[0] & 15);
            if storage == 0 || length == 0 {
                continue;
            }
            let name = prodos_name(&entry[1..1 + length], entry[0x1C], entry[0x1D]);
            let file = ProdosFile {
                name: name.clone(),
                storage,
                kind: entry[0x10],
                key: le16(entry, 0x11),
                length: u64::from(entry[0x15]) | u64::from(entry[0x16]) << 8 | u64::from(entry[0x17]) << 16,
                aux: le16(entry, 0x1F),
                modified: prodos_date(&entry[0x21..0x25]),
                folder,
            };
            if storage == 0xD {
                folders.push((name, folder));
                let index = folders.len() - 1;
                let key = file.key;
                files.push(file);
                prodos_directory(disk, key, Some(index), folders, files, depth + 1)?;
            } else {
                files.push(file);
            }
        }
        first = false;
        block = le16(&data, 2);
    }
    Ok(())
}

/// A ProDOS file's bytes, through its index blocks; a pointer of 0 is a block of zeros.
fn prodos_read(disk: &Disk, storage: u8, key: u64, length: u64) -> Result<Vec<u8>, String> {
    let index = |block: u64| -> Result<Vec<u64>, String> {
        let data = disk.block(block)?;
        Ok((0..256).map(|i| u64::from(data[i]) | u64::from(data[256 + i]) << 8).collect())
    };
    let data_block = |block: u64| if block == 0 { Ok(vec![0u8; 512]) } else { disk.block(block) };
    let wanted = length.div_ceil(512) as usize;
    let mut out = Vec::with_capacity(wanted * 512);
    match storage {
        1 => out.extend(data_block(key)?),
        2 => {
            for block in index(key)?.into_iter().take(wanted) {
                out.extend(data_block(block)?);
            }
        }
        3 => {
            'master: for sub in index(key)? {
                let pointers = if sub == 0 { vec![0; 256] } else { index(sub)? };
                for block in pointers {
                    if out.len() >= wanted * 512 {
                        break 'master;
                    }
                    out.extend(data_block(block)?);
                }
            }
        }
        // An extended file: its key block gives the data fork's storage type, key, and length first.
        5 => {
            let extended = disk.block(key)?;
            let fork_length = u64::from(extended[5]) | u64::from(extended[6]) << 8 | u64::from(extended[7]) << 16;
            return prodos_read(disk, extended[0] & 15, le16(&extended, 1), fork_length);
        }
        other => return Err(format!("a file of storage type {other}, which ProDOS 8 doesn't have")),
    }
    out.truncate(length as usize);
    Ok(out)
}

// ---------- The volume ----------

enum System {
    Dos { files: Vec<DosFile> },
    Prodos { name: String, folders: Vec<(String, Option<usize>)>, files: Vec<ProdosFile> },
}

pub struct Apple2 {
    disk: Disk,
    system: System,
    what: String,
}

/// Whether an image's start shows a ProDOS volume, in either order, or it's a 2IMG; DOS 3.3's VTOC is further in
/// than this sees, so a 140K image is taken on its size and ending and tried when opened.
pub fn looks_like(head: &[u8], len: u64, ending: &str) -> bool {
    if head.starts_with(b"2IMG") {
        return true;
    }
    let prodos = |at: usize| head.get(at..at + 512).is_some_and(prodos_header_ok);
    // Block 2 in ProDOS order, and in DOS order (track 0 sectors 11 and 10).
    let dos_order_block_2 = || {
        let (a, b) = (head.get(11 * 256..12 * 256), head.get(10 * 256..11 * 256));
        a.zip(b).is_some_and(|(a, b)| prodos_header_ok(&[a, b].concat()))
    };
    prodos(1024) || dos_order_block_2() || (len == DISK_140K && matches!(ending, "dsk" | "do" | "po"))
}

impl Apple2 {
    pub fn open(source: Arc<dyn Source>, ending: &str) -> Result<Self, String> {
        let head = source.read_vec(0, source.len().min(64) as usize)?;
        let (offset, length, two_img_order) = if head.starts_with(b"2IMG") && head.len() >= 64 {
            let le32 = |at: usize| u64::from(u32::from_le_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]));
            let order = match le32(12) {
                0 => Some(Order::Dos),
                1 => Some(Order::Prodos),
                _ => return Err("This 2IMG holds a nibble image, which Limoniter doesn't decode yet.".into()),
            };
            (le32(24), le32(28), order)
        } else {
            (0, source.len(), None)
        };
        let orders: Vec<Order> = match (two_img_order, ending) {
            (Some(order), _) => vec![order],
            (None, "po" | "hdv") => vec![Order::Prodos, Order::Dos],
            _ => vec![Order::Dos, Order::Prodos],
        };
        for order in orders {
            // Only a 140K disk can be in DOS order.
            if order == Order::Dos && length != DISK_140K {
                continue;
            }
            let disk = Disk { source: source.clone(), offset, length, order };
            let order_name = if order == Order::Dos { "DOS order" } else { "ProDOS order" };
            let size = if length == DISK_140K { "140K 5.25-inch disk".to_string() } else { format!("{}K volume", length / 1024) };
            if let Ok(block) = disk.block(2)
                && prodos_header_ok(&block)
            {
                let name = String::from_utf8_lossy(&block[5..5 + usize::from(block[4] & 15)]).into_owned();
                let (mut folders, mut files) = (Vec::new(), Vec::new());
                prodos_directory(&disk, 2, None, &mut folders, &mut files, 0)?;
                let what = format!("Apple II ProDOS, /{name} ({size}, {order_name})");
                return Ok(Self { disk, system: System::Prodos { name, folders, files }, what });
            }
            if length == DISK_140K
                && let Ok((volume, files)) = dos_catalog(&disk)
            {
                let what = format!("Apple II DOS 3.3, volume {volume} ({size}, {order_name})");
                return Ok(Self { disk, system: System::Dos { files }, what });
            }
        }
        Err("This Apple II disk has no DOS 3.3 catalog or ProDOS volume: it's probably a game or program that loads itself its own way. It can still be shown in hex.".into())
    }
}

/// Names a file so the previews know it: an Applesoft program as .bas, a hi-res screen as .hgr.
fn open_name(name: &str, applesoft: bool, hires: bool, text: bool) -> String {
    let tidy = tidy_name(name);
    if applesoft {
        format!("{tidy}.bas")
    } else if hires {
        format!("{tidy}.hgr")
    } else if text && !tidy.contains('.') {
        format!("{tidy}.txt")
    } else {
        tidy
    }
}

/// Whether a binary's address and length make it a hi-res screen.
fn hires(address: u64, length: u64) -> bool {
    matches!(address, 0x2000 | 0x4000) && (8184..=8192).contains(&length)
}

impl Volume for Apple2 {
    fn describe(&self) -> String {
        self.what.clone()
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        match &self.system {
            System::Dos { files, .. } => {
                let mut out = Vec::new();
                for (index, file) in files.iter().enumerate() {
                    let (kind, letter) = dos_kind(file.kind);
                    let mut entry = Entry::file(tidy_name(&file.name), file.sectors * 256, index as u64);
                    entry.kind = format!("{letter} {kind}{}", if file.locked { ", locked" } else { "" });
                    entry.detail = format!("{} sectors", file.sectors);
                    // A binary's address and length are in its first sector.
                    if file.kind == 0x04
                        && let Ok(list) = self.disk.sector(file.first_list.0, file.first_list.1)
                        && list[0x0C] != 0
                        && let Ok(first) = self.disk.sector(u64::from(list[0x0C]), u64::from(list[0x0D]))
                    {
                        let (address, length) = (le16(&first, 0), le16(&first, 2));
                        entry.size = length;
                        entry.detail = format!("${address:04X}, {length} bytes");
                        entry.open_name = open_name(&file.name, false, hires(address, length), false);
                    } else {
                        entry.open_name = open_name(&file.name, file.kind == 0x02, false, file.kind == 0x00);
                    }
                    out.push(entry);
                }
                Ok(out)
            }
            System::Prodos { files, folders, .. } => {
                let folder = dir.map(|d| d.location as usize);
                let mut out = Vec::new();
                for (index, file) in files.iter().enumerate().filter(|(_, f)| f.folder == folder) {
                    if file.storage == 0xD {
                        let at = folders.iter().position(|(name, parent)| *name == file.name && *parent == folder).unwrap_or(0);
                        let mut entry = Entry::dir(tidy_name(&file.name), at as u64, 0);
                        entry.detail = file.modified.clone();
                        out.push(entry);
                        continue;
                    }
                    let mut entry = Entry::file(tidy_name(&file.name), file.length, index as u64);
                    entry.kind = prodos_kind(file.kind).to_string();
                    if file.storage == 5 {
                        entry.kind.push_str(", with a resource fork");
                    }
                    let address = if matches!(file.kind, 0x06 | 0xFC | 0xFF) { format!("${:04X}, ", file.aux) } else { String::new() };
                    entry.detail = format!("{address}{}", file.modified);
                    entry.open_name = open_name(&file.name, file.kind == 0xFC, (file.kind == 0x06 && hires(file.aux, file.length)) || (file.kind == 0x08 && file.length <= 8192), file.kind == 0x04);
                    out.push(entry);
                }
                Ok(out)
            }
        }
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        match &self.system {
            System::Dos { files, .. } => {
                let file = files.get(entry.location as usize).ok_or("That file isn't on the disk.")?;
                let mut data = dos_read(&self.disk, file)?;
                match file.kind {
                    // A text file ends at its first zero.
                    0x00 => {
                        if let Some(end) = data.iter().position(|&b| b == 0) {
                            data.truncate(end);
                        }
                    }
                    // BASIC: a length, then the program.
                    0x01 | 0x02 if data.len() >= 2 => {
                        let length = le16(&data, 0) as usize;
                        data.drain(..2);
                        data.truncate(length);
                    }
                    // Binary: an address and a length, then the bytes.
                    0x04 if data.len() >= 4 => {
                        let length = le16(&data, 2) as usize;
                        data.drain(..4);
                        data.truncate(length);
                    }
                    _ => {}
                }
                Ok(data)
            }
            System::Prodos { files, .. } => {
                let file = files.get(entry.location as usize).ok_or("That file isn't on the disk.")?;
                prodos_read(&self.disk, file.storage, file.key, file.length)
            }
        }
    }

    fn notes(&self) -> Vec<String> {
        match &self.system {
            System::Dos { .. } => vec!["Binary and BASIC files are given without the address and length DOS 3.3 keeps at their start; a binary's address is in Details. Text ends at its first zero byte.".into()],
            System::Prodos { name, .. } => vec![format!("ProDOS volume /{name}; dates are when each file was last changed.")],
        }
    }

    /// DOS 3.3 text is high-bit ASCII; ProDOS text usually isn't.
    fn text_preset(&self, entry: &Entry) -> Option<&'static str> {
        match &self.system {
            System::Dos { files, .. } => files.get(entry.location as usize).filter(|f| f.kind == 0x00).map(|_| "apple-ii"),
            System::Prodos { .. } => None,
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A DOS 3.3 disk in DOS order with a text file, an Applesoft program and a binary.
    pub fn dos33_disk() -> Vec<u8> {
        let mut disk = vec![0u8; DISK_140K as usize];
        let sector = |t: u64, s: u64| ((t * 16 + s) * 256) as usize;
        let vtoc = sector(17, 0);
        disk[vtoc + 1] = 17;
        disk[vtoc + 2] = 15;
        disk[vtoc + 3] = 3;
        disk[vtoc + 6] = 254;
        disk[vtoc + 0x27] = 122;
        disk[vtoc + 0x34] = 35;
        disk[vtoc + 0x35] = 16;
        let catalog = sector(17, 15);
        let mut put_entry = |slot: usize, list: (u8, u8), kind: u8, name: &str| {
            let at = catalog + 0x0B + slot * 35;
            disk[at] = list.0;
            disk[at + 1] = list.1;
            disk[at + 2] = kind;
            let mut padded: Vec<u8> = name.bytes().map(|b| b | 0x80).collect();
            padded.resize(30, 0xA0);
            disk[at + 3..at + 33].copy_from_slice(&padded);
            disk[at + 33] = 2;
        };
        put_entry(0, (18, 0), 0x00, "HELLO TEXT");
        put_entry(1, (19, 0), 0x02, "PROGRAM");
        put_entry(2, (20, 0), 0x84, "PICTURE");
        // Each file: a T/S list on sector 0 of its track, its data on sector 1.
        for (track, data) in [(18u64, b"\xC8\xC9\x8D\x00junk".to_vec()), (19, vec![3, 0, 1, 2, 3, 9, 9]), (20, vec![0x00, 0x20, 0x00, 0x20, 0xAA])] {
            let list = sector(track, 0);
            disk[list + 0x0C] = track as u8;
            disk[list + 0x0D] = 1;
            let at = sector(track, 1);
            disk[at..at + data.len()].copy_from_slice(&data);
        }
        disk
    }

    #[test]
    fn dos33_files_are_read_and_their_headers_taken_off() {
        let apple = Apple2::open(Arc::new(dos33_disk()), "dsk").unwrap();
        assert!(apple.describe().starts_with("Apple II DOS 3.3, volume 254"), "{}", apple.describe());
        let files = apple.list(None).unwrap();
        let names: Vec<&str> = files.iter().map(|e| e.open_name.as_str()).collect();
        assert_eq!(names, ["HELLO TEXT.txt", "PROGRAM.bas", "PICTURE.hgr"]);
        assert_eq!(apple.read(&files[0]).unwrap(), b"\xC8\xC9\x8D", "text ends at its zero");
        assert_eq!(apple.read(&files[1]).unwrap(), [1, 2, 3], "the length is taken off");
        assert_eq!(apple.read(&files[2]).unwrap().len(), 252, "a binary's address and length are taken off, and it's as long as its one sector");
        assert_eq!(files[2].kind, "B Binary, locked");
        assert_eq!(apple.text_preset(&files[0]), Some("apple-ii"));
    }

    #[test]
    fn gs_os_lower_case_names() {
        assert_eq!(prodos_name(b"FTYPE.AUX", 0xC0, 0x9C), "FType.Aux", "as the Finder's own file is named");
        assert_eq!(prodos_name(b"README", 0x00, 0x00), "README", "without the top bit it's no mask");
    }

    #[test]
    fn a_prodos_disk_in_either_order() {
        // A ProDOS-order disk: the volume directory at block 2 with a seedling text file and a folder holding one.
        let mut disk = vec![0u8; DISK_140K as usize];
        let block = |b: usize| b * 512;
        let header = block(2);
        disk[header + 4] = 0xF4;
        disk[header + 5..header + 9].copy_from_slice(b"DEMO");
        disk[header + 0x23] = 0x27;
        disk[header + 0x24] = 0x0D;
        let entry = |disk: &mut Vec<u8>, at: usize, storage: u8, name: &str, kind: u8, key: u16, length: u32| {
            disk[at] = storage << 4 | name.len() as u8;
            disk[at + 1..at + 1 + name.len()].copy_from_slice(name.as_bytes());
            disk[at + 0x10] = kind;
            disk[at + 0x11..at + 0x13].copy_from_slice(&key.to_le_bytes());
            disk[at + 0x15..at + 0x18].copy_from_slice(&length.to_le_bytes()[..3]);
        };
        entry(&mut disk, header + 4 + 0x27, 1, "README", 0x04, 7, 5);
        entry(&mut disk, header + 4 + 0x27 * 2, 0xD, "STUFF", 0x0F, 8, 512);
        disk[block(7)..block(7) + 5].copy_from_slice(b"Hello");
        disk[block(8) + 4] = 0xE5;
        entry(&mut disk, block(8) + 4 + 0x27, 1, "INNER", 0x06, 9, 3);
        disk[block(9)..block(9) + 3].copy_from_slice(&[1, 2, 3]);
        assert!(looks_like(&disk[..0x8800], disk.len() as u64, "po"));
        let apple = Apple2::open(Arc::new(disk.clone()), "po").unwrap();
        assert!(apple.describe().contains("/DEMO"));
        let top = apple.list(None).unwrap();
        assert_eq!(top.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["README", "STUFF"]);
        assert_eq!(apple.read(&top[0]).unwrap(), b"Hello");
        let inner = apple.list(Some(&top[1])).unwrap();
        assert_eq!(apple.read(&inner[0]).unwrap(), [1, 2, 3]);

        // The same disk in DOS order: each block's two halves moved to their DOS sectors.
        let mut dos = vec![0u8; DISK_140K as usize];
        for b in 0..280 {
            let [first, second] = BLOCK_SECTORS[b % 8];
            let track = (b / 8) as u64;
            for (half, sector) in [first, second].into_iter().enumerate() {
                let to = ((track * 16 + sector) * 256) as usize;
                dos[to..to + 256].copy_from_slice(&disk[b * 512 + half * 256..b * 512 + half * 256 + 256]);
            }
        }
        assert!(looks_like(&dos[..0x8800], dos.len() as u64, "dsk"));
        let apple = Apple2::open(Arc::new(dos), "dsk").unwrap();
        assert!(apple.describe().contains("DOS order"));
        assert_eq!(apple.read(&apple.list(None).unwrap()[0]).unwrap(), b"Hello");
    }
}
