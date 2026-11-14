//! TR-DOS disks (.trd) for the Spectrum's Beta Disk interface, and SCL archives of their files.
//!
//! A TRD keeps its logical tracks in order, sixteen 256-byte sectors each. Track 0's first eight sectors are the
//! catalogue, 128 entries of 16 bytes: an 8-character name, a type letter (B BASIC, C code, D data array, #
//! sequential), two parameters (a BASIC program's whole length and its length without variables; code's address and
//! length), the length in sectors, and the first sector and track. Sector 8 says where the first free sector is, the
//! disk's type, how many entries are used, how many sectors are free, TR-DOS's mark &10, how many entries are deleted
//! and the disk's label. A file is always one run of sectors after the last, as TR-DOS saves them; a deleted one
//! keeps its entry, its first byte 1, and its space until the disk is compacted (TR-DOS's MOVE), unless it was the
//! last, whose space is given straight back. A BASIC program is followed by &80 &AA and the line it runs from.
//!
//! An SCL is a TR-DOS disk's files packed for sending: "SINCLAIR", the number of files, each one's first 14 bytes of
//! catalogue entry, all their sectors in turn, and a 32-bit sum of every byte before it. It is rebuilt whole when
//! changed.

use super::tape::{is_listing, plus3_header};
use super::{Addition, Entry, ImageWriter, Kind, already};
use crate::basic;
use crate::charset;
use crate::media::tidy_name;

const PROGRAM: &str = "BASIC program";
const CODE: &str = "Code";
const SCREEN: &str = "Screen";
const DATA: &str = "Data array";
const STREAM: &str = "Sequential file";

/// The first sector after the catalogue track, where files start.
const FIRST: usize = 16;
/// The sectors an 80-track double-sided disk has for files, which an SCL's files must fit to be put on one.
const DISK_SECTORS: usize = 160 * 16 - FIRST;
/// The most sectors a file can have: its entry's count is a byte.
const MOST_SECTORS: usize = 255;
const INFO: usize = 8 * 256;

/// A new 80-track double-sided disk, as FORMAT leaves one, named `label`.
pub fn blank(label: &str) -> Vec<u8> {
    let mut bytes = vec![0u8; 160 * 16 * 256];
    let info = &mut bytes[INFO..INFO + 256];
    info[0xE1] = 0;
    info[0xE2] = 1;
    info[0xE3] = 0x16;
    info[0xE5..0xE7].copy_from_slice(&(DISK_SECTORS as u16).to_le_bytes());
    info[0xE7] = 0x10;
    info[0xE9..0xF2].fill(b' ');
    info[0xF5..0xFD].copy_from_slice(&zx_name(label));
    bytes
}

/// An empty SCL archive.
pub fn blank_scl() -> Vec<u8> {
    scl_bytes(&[])
}

/// A name as TR-DOS keeps it: eight characters of the Spectrum's set, padded with spaces.
fn zx_name(name: &str) -> [u8; 8] {
    let mut out = [b' '; 8];
    let chars = name.chars().filter_map(|c| match c {
        '\u{A3}' => Some(0x60),
        '\u{A9}' => Some(0x7F),
        ' '..='~' if c != '`' => Some(c as u8),
        _ => None,
    });
    for (slot, byte) in out.iter_mut().zip(chars) {
        *slot = byte;
    }
    out
}

/// A name as the reader shows it, its type letter after a dot.
fn shown(entry: &[u8]) -> String {
    let set = charset::known("zx-spectrum");
    let name: String = entry[..8].iter().map(|b| set.entry(0, *b & 0x7F).and_then(|e| e.unicode).filter(|c| !c.is_control()).unwrap_or('?')).collect();
    tidy_name(&format!("{}.{}", name.trim_end(), char::from(entry[8])))
}

/// The name a file is to have, without a type letter given after a dot, refused if nothing is left of it.
fn new_name(name: &str, kind: u8) -> Result<[u8; 8], String> {
    let name = name.trim();
    let stem = match name.rsplit_once('.') {
        Some((stem, letter)) if !stem.is_empty() && letter.len() == 1 && letter.eq_ignore_ascii_case(&char::from(kind).to_string()) => stem,
        _ => name,
    };
    let bytes = zx_name(stem);
    if bytes.iter().all(|b| *b == b' ') || bytes[0] <= 1 {
        return Err(format!("\"{name}\" has no characters a TR-DOS name can hold."));
    }
    Ok(bytes)
}

/// A file made ready to go on: its first 14 bytes of catalogue entry and its sectors' bytes.
fn record(file: &Addition) -> Result<([u8; 14], Vec<u8>), String> {
    let header = plus3_header(&file.bytes);
    let body = if header.is_some() { &file.bytes[128..] } else { &file.bytes[..] };
    let (kind, first, second, data) = match file.kind.as_str() {
        PROGRAM => {
            let (program, length) = match header {
                // A +3DOS program keeps its variables, and the length of the program before them.
                Some((0, length, _, program_length)) => (body[..usize::from(length).min(body.len())].to_vec(), program_length),
                _ if is_listing(body) => {
                    let text = String::from_utf8_lossy(body);
                    let lines: Vec<&str> = text.lines().collect();
                    let program = basic::tokenise_spectrum(&lines, basic::ZxDialect::Spectrum)?;
                    let length = program.len() as u16;
                    (program, length)
                }
                _ => (body.to_vec(), body.len() as u16),
            };
            if program.len() > 0xFFFF {
                return Err(format!("{} is too big for a BASIC program.", file.name));
            }
            let total = program.len() as u16;
            let mut data = program;
            // TR-DOS's SAVE ... LINE leaves the line after the program and its variables.
            if let Some(line) = file.autostart.filter(|l| *l < 10000) {
                data.extend([0x80, 0xAA]);
                data.extend(line.to_le_bytes());
            }
            (b'B', total, length.min(total), data)
        }
        SCREEN => {
            if body.len() != 6912 {
                return Err(format!("A screen is 6912 bytes; {} is {}.", file.name, body.len()));
            }
            (b'C', 16384, 6912, body.to_vec())
        }
        DATA => {
            let first = match header {
                Some((1 | 2, _, first, _)) => first,
                _ => 0,
            };
            (b'D', first, body.len().min(0xFFFF) as u16, body.to_vec())
        }
        STREAM => (b'#', 0, body.len().min(0xFFFF) as u16, body.to_vec()),
        _ => (b'C', file.address.unwrap_or(32768), body.len().min(0xFFFF) as u16, body.to_vec()),
    };
    if data.len() > 0xFFFF || data.len().div_ceil(256) > MOST_SECTORS {
        return Err(format!("{} is {} bytes; a TR-DOS file can be at most 65,280.", file.name, data.len()));
    }
    let mut entry = [0u8; 14];
    entry[..8].copy_from_slice(&new_name(&file.name, kind)?);
    entry[8] = kind;
    entry[9..11].copy_from_slice(&first.to_le_bytes());
    entry[11..13].copy_from_slice(&second.to_le_bytes());
    entry[13] = data.len().div_ceil(256) as u8;
    Ok((entry, data))
}

fn kinds() -> Vec<Kind> {
    vec![
        Kind { name: PROGRAM, about: "A BASIC program, as SAVE \"name\" writes one. A listing typed as text is tokenised first.", address: false, autostart: true },
        Kind { name: CODE, about: "Bytes, as SAVE \"name\" CODE writes them, to load at an address.", address: true, autostart: false },
        Kind::plain(SCREEN, "A 6912-byte screen, as SAVE \"name\" SCREEN$ writes one."),
        Kind::plain(DATA, "An array, as SAVE \"name\" DATA writes one (type D)."),
        Kind::plain(STREAM, "A file of a program's own records, written through a stream (type #)."),
    ]
}

fn guess(name: &str, bytes: &[u8]) -> Addition {
    let (stem, ending) = super::stem_and_ending(name);
    let ending = ending.to_lowercase();
    let mut add = Addition { name: stem.to_string(), bytes: bytes.to_vec(), kind: CODE.into(), address: Some(32768), ..Default::default() };
    if let Some((kind, length, first, _)) = plus3_header(bytes) {
        match kind {
            0 => {
                add.kind = PROGRAM.into();
                add.autostart = (first < 10000).then_some(first);
            }
            1 | 2 => add.kind = DATA.into(),
            _ if first == 16384 && length == 6912 => add.kind = SCREEN.into(),
            _ => add.address = Some(first),
        }
    } else if ending == "scr" || (bytes.len() == 6912 && ending != "bin") {
        add.kind = SCREEN.into();
    } else if ending == "bas" || ending == "b" || is_listing(bytes) && matches!(ending.as_str(), "txt" | "") {
        add.kind = PROGRAM.into();
        add.autostart = is_listing(bytes).then_some(10);
    } else if ending == "d" {
        add.kind = DATA.into();
    } else if ending == "#" {
        add.kind = STREAM.into();
    }
    add
}

/// Whether a name and type are already used by another file; TR-DOS tells files apart by both.
fn same(a: &[u8], b: &[u8]) -> bool {
    a[..9] == b[..9]
}

// ---------- TRD ----------

pub struct Trd {
    bytes: Vec<u8>,
    /// The sectors the disk has, by its type.
    total: usize,
    /// Where the next file goes, as a logical sector.
    next: usize,
}

impl Trd {
    pub fn open(bytes: Vec<u8>) -> Result<Self, String> {
        if bytes.len() < INFO + 256 || bytes[INFO + 0xE7] != 0x10 {
            return Err("This doesn't have TR-DOS's disk information sector.".into());
        }
        let tracks = match bytes[INFO + 0xE3] {
            0x16 => 160,
            0x17 | 0x18 => 80,
            0x19 => 40,
            other => return Err(format!("The disk's type, &{other:02X}, isn't one TR-DOS has, so it isn't changed.")),
        };
        let mut disk = Self { bytes, total: tracks * 16, next: FIRST };
        // Each file lies after the one before it, inside the disk, as TR-DOS always puts them.
        let mut end = FIRST;
        for slot in 0..disk.used() {
            let entry = disk.entry(slot);
            let (start, sectors) = (Self::start(entry), usize::from(entry[13]));
            if entry[14] >= 16 || start < end || start + sectors > disk.total {
                return Err(format!("{} lies where TR-DOS wouldn't put it, so the disk isn't changed.", shown(entry)));
            }
            if entry[0] != 1 && (start + sectors) * 256 > disk.bytes.len() {
                return Err(format!("{} runs past the end of the image, which has been cut short.", shown(entry)));
            }
            end = start + sectors;
        }
        let said = usize::from(disk.bytes[INFO + 0xE2]) * 16 + usize::from(disk.bytes[INFO + 0xE1]);
        disk.next = if (end..=disk.total).contains(&said) { said } else { end };
        Ok(disk)
    }

    fn start(entry: &[u8]) -> usize {
        usize::from(entry[15]) * 16 + usize::from(entry[14])
    }

    fn entry(&self, slot: usize) -> &[u8] {
        &self.bytes[slot * 16..slot * 16 + 16]
    }

    /// The catalogue's entries in use, deleted ones too: up to the first that starts with 0.
    fn used(&self) -> usize {
        (0..128).find(|slot| self.bytes[slot * 16] == 0).unwrap_or(128)
    }

    fn deleted(&self) -> usize {
        (0..self.used()).filter(|slot| self.bytes[slot * 16] == 1).count()
    }

    /// The entries of files, as the reader numbers them.
    fn files(&self) -> Vec<usize> {
        (0..self.used()).filter(|slot| self.bytes[slot * 16] != 1).collect()
    }

    fn find(&self, entry: &Entry) -> Result<usize, String> {
        self.files().get(entry.location as usize).copied().filter(|slot| shown(self.entry(*slot)) == entry.name).ok_or_else(|| format!("{} isn't on the disk.", entry.name))
    }

    /// Sets the disk information sector from the catalogue.
    fn tidy_info(&mut self) {
        let (used, deleted, free) = (self.used(), self.deleted(), self.total - self.next);
        let info = &mut self.bytes[INFO..INFO + 256];
        info[0xE1] = (self.next % 16) as u8;
        info[0xE2] = (self.next / 16) as u8;
        info[0xE4] = used as u8;
        info[0xE5..0xE7].copy_from_slice(&(free as u16).to_le_bytes());
        info[0xF4] = deleted as u8;
    }

    /// Takes a file off: the last one's entry and space are given back, along with any deleted ones before it;
    /// another's entry is marked deleted.
    fn remove(&mut self, slot: usize) {
        self.bytes[slot * 16] = 1;
        let mut used = self.used();
        while used > 0 && self.bytes[(used - 1) * 16] == 1 {
            used -= 1;
            self.next = Self::start(self.entry(used));
            self.bytes[used * 16..used * 16 + 16].fill(0);
        }
        self.tidy_info();
    }

    /// Moves every file down over the space of deleted ones, as TR-DOS's MOVE does, and drops their entries.
    fn compact(&mut self) {
        let mut at = FIRST;
        let mut entries = Vec::new();
        for slot in self.files() {
            let mut entry = self.entry(slot).to_vec();
            let (start, sectors) = (Self::start(&entry), usize::from(entry[13]));
            if start != at {
                self.bytes.copy_within(start * 256..(start + sectors) * 256, at * 256);
            }
            entry[14] = (at % 16) as u8;
            entry[15] = (at / 16) as u8;
            entries.push(entry);
            at += sectors;
        }
        self.bytes[..INFO].fill(0);
        for (slot, entry) in entries.iter().enumerate() {
            self.bytes[slot * 16..slot * 16 + 16].copy_from_slice(entry);
        }
        self.next = at;
        self.tidy_info();
    }

    fn put(&mut self, file: &Addition) -> Result<(), String> {
        let (head, data) = record(file)?;
        if let Some(slot) = self.files().into_iter().find(|s| same(self.entry(*s), &head)) {
            if !file.replace {
                return Err(already(&shown(&head)));
            }
            self.remove(slot);
        }
        let sectors = usize::from(head[13]);
        if (self.used() >= 128 || self.next + sectors > self.total) && self.deleted() > 0 {
            self.compact();
        }
        if self.used() >= 128 {
            return Err("The catalogue is full: a TR-DOS disk holds 128 files.".into());
        }
        if self.next + sectors > self.total {
            return Err(format!("{} needs {sectors} sectors; the disk has {} free.", file.name, self.total - self.next));
        }
        let at = self.next * 256;
        if self.bytes.len() < at + sectors * 256 {
            self.bytes.resize(at + sectors * 256, 0);
        }
        self.bytes[at..at + sectors * 256].fill(0);
        self.bytes[at..at + data.len()].copy_from_slice(&data);
        let slot = self.used();
        let entry = &mut self.bytes[slot * 16..slot * 16 + 16];
        entry[..14].copy_from_slice(&head);
        entry[14] = (self.next % 16) as u8;
        entry[15] = (self.next / 16) as u8;
        self.next += sectors;
        self.tidy_info();
        Ok(())
    }
}

impl ImageWriter for Trd {
    fn kinds(&self) -> Vec<Kind> {
        kinds()
    }

    fn guess(&self, name: &str, bytes: &[u8]) -> Addition {
        guess(name, bytes)
    }

    fn add(&mut self, _folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        // Nothing is left half done: a replaced file comes back if the new one doesn't fit.
        let before = (self.bytes.clone(), self.next);
        self.put(file).inspect_err(|_| (self.bytes, self.next) = before)
    }

    fn delete(&mut self, _folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        let slot = self.find(entry)?;
        self.remove(slot);
        Ok(())
    }

    fn rename(&mut self, _folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        let slot = self.find(entry)?;
        let mut new = self.entry(slot).to_vec();
        let renamed = new_name(name, new[8])?;
        new[..8].copy_from_slice(&renamed);
        if self.files().into_iter().any(|s| s != slot && same(self.entry(s), &new)) {
            return Err(already(&shown(&new)));
        }
        self.bytes[slot * 16..slot * 16 + 8].copy_from_slice(&new[..8]);
        Ok(())
    }

    /// What's free at the end and what deleted files hold, as compacting gives that back when it's needed.
    fn free(&self) -> Option<u64> {
        let held: usize = self.files().iter().map(|s| usize::from(self.entry(*s)[13])).sum();
        Some((self.total - FIRST - held) as u64 * 256)
    }

    fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

// ---------- SCL ----------

pub struct Scl {
    files: Vec<([u8; 14], Vec<u8>)>,
    bytes: Vec<u8>,
}

/// An SCL archive of these files, its sum at the end.
fn scl_bytes(files: &[([u8; 14], Vec<u8>)]) -> Vec<u8> {
    let mut out = b"SINCLAIR".to_vec();
    out.push(files.len() as u8);
    for (entry, _) in files {
        out.extend(entry);
    }
    for (entry, data) in files {
        let start = out.len();
        out.extend(data);
        out.resize(start + usize::from(entry[13]) * 256, 0);
    }
    let sum = out.iter().fold(0u32, |sum, b| sum.wrapping_add(u32::from(*b)));
    out.extend(sum.to_le_bytes());
    out
}

impl Scl {
    pub fn open(bytes: Vec<u8>) -> Result<Self, String> {
        if bytes.len() < 9 || !bytes.starts_with(b"SINCLAIR") {
            return Err("This SCL file doesn't start with SINCLAIR.".into());
        }
        let count = usize::from(bytes[8]);
        let mut at = 9 + count * 14;
        if bytes.len() < at {
            return Err("This SCL archive has been cut short in its list of files.".into());
        }
        let mut files = Vec::new();
        for n in 0..count {
            let entry: [u8; 14] = bytes[9 + n * 14..9 + n * 14 + 14].try_into().unwrap_or([0; 14]);
            let length = usize::from(entry[13]) * 256;
            let data = bytes.get(at..at + length).ok_or_else(|| format!("{} runs past the end of the archive, which has been cut short.", shown(&entry)))?;
            files.push((entry, data.to_vec()));
            at += length;
        }
        Ok(Self { files, bytes })
    }

    fn find(&self, entry: &Entry) -> Result<usize, String> {
        Some(entry.location as usize).filter(|i| self.files.get(*i).is_some_and(|(e, _)| shown(e) == entry.name)).ok_or_else(|| format!("{} isn't in the archive.", entry.name))
    }

    fn sectors(&self) -> usize {
        self.files.iter().map(|(e, _)| usize::from(e[13])).sum()
    }
}

impl ImageWriter for Scl {
    fn kinds(&self) -> Vec<Kind> {
        kinds()
    }

    fn guess(&self, name: &str, bytes: &[u8]) -> Addition {
        guess(name, bytes)
    }

    fn add(&mut self, _folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        let (head, data) = record(file)?;
        let mut files = self.files.clone();
        if let Some(index) = files.iter().position(|(e, _)| same(e, &head)) {
            if !file.replace {
                return Err(already(&shown(&head)));
            }
            files.remove(index);
        }
        // An archive is only any use if its files can go back on a disk.
        if files.len() >= 128 {
            return Err("The archive is full: a TR-DOS disk holds 128 files.".into());
        }
        let held: usize = files.iter().map(|(e, _)| usize::from(e[13])).sum();
        if held + usize::from(head[13]) > DISK_SECTORS {
            return Err(format!("{} needs {} sectors; a TR-DOS disk would have {} left for it.", file.name, head[13], DISK_SECTORS - held));
        }
        files.push((head, data));
        self.files = files;
        self.bytes = scl_bytes(&self.files);
        Ok(())
    }

    fn delete(&mut self, _folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        let index = self.find(entry)?;
        self.files.remove(index);
        self.bytes = scl_bytes(&self.files);
        Ok(())
    }

    fn rename(&mut self, _folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        let index = self.find(entry)?;
        let mut new = self.files[index].0;
        let renamed = new_name(name, new[8])?;
        new[..8].copy_from_slice(&renamed);
        if self.files.iter().enumerate().any(|(i, (e, _))| i != index && same(e, &new)) {
            return Err(already(&shown(&new)));
        }
        self.files[index].0 = new;
        self.bytes = scl_bytes(&self.files);
        Ok(())
    }

    fn free(&self) -> Option<u64> {
        Some(DISK_SECTORS.saturating_sub(self.sectors()) as u64 * 256)
    }

    fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{entry, names, reopened};
    use super::*;
    use std::process::Command;

    fn program() -> Vec<u8> {
        basic::tokenise_spectrum(&["10 PRINT \"HELLO\"", "20 GO TO 10"], basic::ZxDialect::Spectrum).unwrap()
    }

    fn add(writer: &mut dyn ImageWriter, name: &str, bytes: &[u8]) {
        let addition = writer.guess(name, bytes);
        writer.add(None, &addition).unwrap();
    }

    fn info(disk: &Trd) -> (usize, usize, usize, usize) {
        let i = &disk.bytes[INFO..INFO + 256];
        (usize::from(i[0xE2]) * 16 + usize::from(i[0xE1]), usize::from(i[0xE4]), usize::from(u16::from_le_bytes([i[0xE5], i[0xE6]])), usize::from(i[0xF4]))
    }

    #[test]
    fn files_go_on_and_come_off_a_trdos_disk() {
        let mut disk = Trd::open(blank("WORK")).unwrap();
        assert_eq!(info(&disk), (16, 0, 2544, 0));
        let big: Vec<u8> = (0..20_000u32).map(|i| (i * 7) as u8).collect();
        add(&mut disk, "hello.bas", b"10 PRINT \"HELLO\"\n20 GO TO 10\n");
        add(&mut disk, "picture.scr", &[0x55; 6912]);
        add(&mut disk, "game.bin", &[1, 2, 3]);
        add(&mut disk, "big.d", &big);
        let volume = reopened(&disk, "t.trd");
        assert_eq!(volume.describe(), "TR-DOS disk (TRD) \u{B7} WORK");
        assert_eq!(names(&*volume, None), ["hello.B", "picture.C", "game.C", "big.D"]);
        let hello = volume.read(&entry(&*volume, None, "hello.B")).unwrap();
        assert_eq!(hello, basic::plus3dos_basic(&program(), program().len(), Some(10)), "tokenised, and runs from line 10");
        assert_eq!(volume.read(&entry(&*volume, None, "picture.C")).unwrap(), [0x55; 6912]);
        assert_eq!(entry(&*volume, None, "game.C").detail, "3 bytes at 32768");
        assert_eq!(volume.read(&entry(&*volume, None, "big.D")).unwrap(), big);
        assert_eq!(info(&disk), (16 + 1 + 27 + 1 + 79, 4, 2544 - 108, 0));

        // A program taken off goes back on as the same program.
        let mut copy = Trd::open(blank("")).unwrap();
        add(&mut copy, "hello.bas", &hello);
        let again = reopened(&copy, "c.trd");
        assert_eq!(again.read(&entry(&*again, None, "hello.B")).unwrap(), hello);

        // A file in the middle is only marked deleted; the last gives its space back.
        disk.delete(None, &entry(&*volume, None, "picture.C")).unwrap();
        assert_eq!(info(&disk), (124, 4, 2436, 1));
        disk.delete(None, &entry(&*reopened(&disk, "t.trd"), None, "big.D")).unwrap();
        assert_eq!(info(&disk), (45, 3, 2515, 1));
        let volume = reopened(&disk, "t.trd");
        assert_eq!(names(&*volume, None), ["hello.B", "game.C"]);
        disk.rename(None, &entry(&*volume, None, "game.C"), "start.C").unwrap();
        assert!(disk.rename(None, &entry(&*reopened(&disk, "t.trd"), None, "start.C"), "hello").is_ok(), "the same name with another type is another file");
        let volume = reopened(&disk, "t.trd");
        assert_eq!(names(&*volume, None), ["hello.B", "hello.C"]);
        assert_eq!(volume.read(&entry(&*volume, None, "hello.C")).unwrap(), [1, 2, 3]);
        let taken = disk.guess("hello.bas", b"10 REM\n");
        assert!(disk.add(None, &taken).unwrap_err().starts_with(super::super::ALREADY));
    }

    #[test]
    fn a_full_disk_is_compacted_over_deleted_files_or_refused() {
        let mut disk = Trd::open(blank("FULL")).unwrap();
        let chunk = vec![0xAB; 250 * 256];
        for n in 0..10 {
            add(&mut disk, &format!("f{n}.bin"), &chunk);
        }
        // 2500 of 2544 sectors used; something of 100 sectors can't go on.
        let hundred = vec![0xCD; 100 * 256];
        let mut wanted = disk.guess("new.bin", &hundred);
        assert!(disk.add(None, &wanted).unwrap_err().contains("needs 100 sectors; the disk has 44 free"));
        disk.delete(None, &entry(&*reopened(&disk, "t.trd"), None, "f3.C")).unwrap();
        assert_eq!(info(&disk).3, 1);
        wanted.name = "new".into();
        disk.add(None, &wanted).unwrap();
        assert_eq!(info(&disk), (16 + 2350, 10, 2544 - 2350, 0), "compacted, the deleted entry gone");
        let volume = reopened(&disk, "t.trd");
        assert_eq!(names(&*volume, None), ["f0.C", "f1.C", "f2.C", "f4.C", "f5.C", "f6.C", "f7.C", "f8.C", "f9.C", "new.C"]);
        assert_eq!(volume.read(&entry(&*volume, None, "f9.C")).unwrap(), chunk);
        assert_eq!(volume.read(&entry(&*volume, None, "new.C")).unwrap(), hundred);
        // Replacing with something that doesn't fit leaves the old one there, even after compacting for it.
        add(&mut disk, "pad.bin", &vec![0; 190 * 256]);
        let mut too_big = disk.guess("f0.bin", &vec![0; 255 * 256]);
        too_big.replace = true;
        let before = disk.bytes().to_vec();
        assert!(disk.add(None, &too_big).unwrap_err().contains("needs 255 sectors; the disk has 254 free"));
        assert_eq!(disk.bytes(), before);
        let volume = reopened(&disk, "t.trd");
        assert_eq!(volume.read(&entry(&*volume, None, "f0.C")).unwrap(), chunk);
    }

    #[test]
    fn damaged_trdos_disks_are_refused() {
        let mut bad = blank("X");
        bad[INFO + 0xE3] = 0x42;
        assert!(Trd::open(bad).is_err());
        let mut disk = Trd::open(blank("X")).unwrap();
        add(&mut disk, "a.bin", &[1; 600]);
        let mut cut = disk.bytes().to_vec();
        cut.truncate(17 * 256);
        assert!(Trd::open(cut).err().unwrap().contains("cut short"));
        let mut wild = disk.bytes().to_vec();
        wild[15] = 200;
        assert!(Trd::open(wild).is_err());
        assert!(Trd::open(vec![0; 100]).is_err());
        assert!(Scl::open(b"SINCLAIR\x02abc".to_vec()).is_err());
        assert!(record(&Addition { name: "huge".into(), bytes: vec![0; 70_000], kind: CODE.into(), ..Default::default() }).is_err());
    }

    #[test]
    fn files_go_in_and_come_out_of_an_scl_archive() {
        let mut scl = Scl::open(blank_scl()).unwrap();
        add(&mut scl, "hello.bas", b"10 PRINT \"HELLO\"\n20 GO TO 10\n");
        add(&mut scl, "game.bin", &[9; 300]);
        let volume = reopened(&scl, "t.scl");
        assert_eq!(names(&*volume, None), ["hello.B", "game.C"]);
        assert_eq!(volume.read(&entry(&*volume, None, "game.C")).unwrap(), [9; 300]);
        assert_eq!(volume.read(&entry(&*volume, None, "hello.B")).unwrap(), basic::plus3dos_basic(&program(), program().len(), Some(10)));
        let sum = scl.bytes()[..scl.bytes().len() - 4].iter().map(|b| u32::from(*b)).sum::<u32>();
        assert_eq!(scl.bytes()[scl.bytes().len() - 4..], sum.to_le_bytes());
        scl.rename(None, &entry(&*volume, None, "game.C"), "run.C").unwrap();
        scl.delete(None, &entry(&*reopened(&scl, "t.scl"), None, "hello.B")).unwrap();
        let volume = reopened(&scl, "t.scl");
        assert_eq!(names(&*volume, None), ["run.C"]);
        assert_eq!(volume.read(&entry(&*volume, None, "run.C")).unwrap(), [9; 300]);
        assert_eq!(scl.free(), Some((2544 - 2) * 256));
    }

    /// trdos-list (github.com/morozov/trdos-list) lists a written disk and archive as TR-DOS's LIST would: the disk
    /// information, and each file's sectors, parameters and the line a program runs from.
    #[test]
    fn trdos_list_reads_written_images() {
        let tool = std::path::Path::new("/root/reftools/trdos/trdos-list");
        if !tool.is_file() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("limoniter-trdos-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut disk = Trd::open(blank("REFDISK")).unwrap();
        add(&mut disk, "hello.bas", b"10 PRINT \"HELLO\"\n20 GO TO 10\n");
        add(&mut disk, "game.bin", &[1, 2, 3]);
        add(&mut disk, "middle.bin", &[1; 1000]);
        add(&mut disk, "tail.bin", &[4; 10]);
        // trdos-list stops listing at a deleted entry, so the one deleted here comes after the files it's to show.
        disk.delete(None, &entry(&*reopened(&disk, "t.trd"), None, "middle.C")).unwrap();
        let mut scl = Scl::open(blank_scl()).unwrap();
        add(&mut scl, "hello.bas", b"10 PRINT \"HELLO\"\n20 GO TO 10\n");
        add(&mut scl, "game.bin", &[1, 2, 3]);
        let length = program().len();
        for (name, bytes) in [("t.trd", disk.bytes()), ("t.scl", scl.bytes())] {
            let path = dir.join(name);
            std::fs::write(&path, bytes).unwrap();
            let out = Command::new(tool).arg(&path).output().unwrap();
            let listed = String::from_utf8_lossy(&out.stdout);
            // It gives up, failing, at the deleted entry; the archive has none.
            assert!(out.status.success() || name == "t.trd", "{listed}");
            let lines: Vec<&str> = listed.lines().collect();
            let hello = format!("hello   <B>  1  {length:05} {length:05} 10");
            assert!(lines.contains(&hello.as_str()), "{name}: {listed}");
            assert!(lines.contains(&"game    <C>  1  32768 00003"), "{name}: {listed}");
            if name == "t.trd" {
                assert!(listed.contains("Title:  REFDISK  Disk Drive: A"), "{listed}");
                assert!(listed.contains("4 File(s)       80 Track D. Side"), "{listed}");
                assert!(listed.contains("1 Del. File(s)  Free Sector 2537"), "{listed}");
            } else {
                assert!(listed.contains("Free Sector 2542"), "{listed}");
            }
        }
        std::fs::remove_dir_all(dir).ok();
    }

    /// With LIMONITER_ZESARUX naming the program: a Pentagon with a Beta Disk, in a real ZEsarUX, starts TR-DOS on
    /// a written disk and RUNs its "boot" program; that loads a code file past a deleted one, through TR-DOS,
    /// and pokes a mark. Then the same once the disk is compacted.
    #[test]
    #[ignore]
    fn a_written_disk_boots_in_zesarux() {
        use std::io::{Read, Write};
        let Ok(program) = std::env::var("LIMONITER_ZESARUX") else { return };
        let folder = std::env::temp_dir().join(format!("limoniter-trdos-boot-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let mut disk = Trd::open(blank("BOOTTEST")).unwrap();
        add(&mut disk, "junk.bin", &[0xEE; 1500]);
        let boot = disk.guess("boot.bas", b"10 RANDOMIZE USR 15619: REM : LOAD \"data\" CODE\n20 POKE 30000,123\n");
        disk.add(None, &boot).unwrap();
        let data: Vec<u8> = (0..700u32).map(|i| (i * 13 + 1) as u8).collect();
        let mut code = disk.guess("data.bin", &data);
        code.address = Some(40000);
        disk.add(None, &code).unwrap();
        disk.delete(None, &entry(&*reopened(&disk, "t.trd"), None, "junk.C")).unwrap();
        let mut compacted = Trd::open(disk.bytes().to_vec()).unwrap();
        compacted.compact();
        assert_eq!(info(&compacted).3, 0);

        for (round, image) in [disk.bytes(), compacted.bytes()].into_iter().enumerate() {
            let file = folder.join(format!("t{round}.trd"));
            std::fs::write(&file, image).unwrap();
            let port = 10300 + (std::process::id() % 400) as u16 + round as u16;
            let mut child = Command::new(&program)
                .args(["--vo", "null", "--ao", "null", "--noconfigfile", "--enable-remoteprotocol", "--remoteprotocol-port", &port.to_string(), "--machine", "Pentagon", "--enable-betadisk", "--enable-trd", "--trd-no-persistent-writes", "--trd-file"])
                .arg(&file)
                .current_dir(std::path::Path::new(&program).parent().unwrap())
                .spawn()
                .unwrap();
            // A ZRCP command and its answer, up to the next prompt.
            let ask = |command: &str| -> String {
                for _ in 0..100 {
                    if let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                        stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
                        let read_prompt = |stream: &mut std::net::TcpStream| {
                            let mut text = Vec::new();
                            let mut byte = [0u8];
                            while !text.ends_with(b"command> ") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
                                text.push(byte[0]);
                            }
                            String::from_utf8_lossy(&text).replace("command> ", "")
                        };
                        read_prompt(&mut stream);
                        stream.write_all(format!("{command}\n").as_bytes()).unwrap();
                        return read_prompt(&mut stream);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                panic!("ZEsarUX didn't answer");
            };
            let key = |code: u8| {
                ask(&format!("send-keys-event {code} 1"));
                std::thread::sleep(std::time::Duration::from_millis(150));
                ask(&format!("send-keys-event {code} 0"));
                std::thread::sleep(std::time::Duration::from_millis(150));
            };
            std::thread::sleep(std::time::Duration::from_secs(3));
            // The 128 menu's last item, TR-DOS, then its RUN, which runs "boot".
            for _ in 0..4 {
                key(144);
            }
            key(129);
            std::thread::sleep(std::time::Duration::from_secs(2));
            for code in [b'r', b'u', b'n', 129] {
                key(code);
            }
            let mut mark = String::new();
            for _ in 0..60 {
                std::thread::sleep(std::time::Duration::from_millis(500));
                mark = ask("read-memory 30000 1");
                if mark.trim() == "7B" {
                    break;
                }
            }
            let loaded = ask("read-memory 40000 700");
            let _ = child.kill();
            let _ = child.wait();
            assert_eq!(mark.trim(), "7B", "round {round}: boot didn't run to its end");
            let wanted: String = data.iter().map(|b| format!("{b:02X}")).collect();
            assert_eq!(loaded.trim(), wanted, "round {round}: the code file loaded through TR-DOS");
        }
        let _ = std::fs::remove_dir_all(&folder);
    }
}
