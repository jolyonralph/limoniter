//! Spectrum +3 and Amstrad CPC disks in DSK images (standard or extended): CP/M's file system, as +3DOS and
//! AMSDOS keep it. The disk's geometry comes from the +3's boot sector, or from the CPC's sector numbers. A file is
//! directory entries of 16K each, listing the 1K blocks it holds; a deleted file's entries have &E5 for their user
//! number. A file for the +3 normally starts with a 128-byte +3DOS header saying what it is, which BASIC's LOAD
//! needs; one is put on a program, code or screen added here.

use super::{Addition, Entry, ImageWriter, Kind, already};
use crate::basic;
use std::collections::HashMap;

const PROGRAM: &str = "Program";
const CODE: &str = "Code";
const SCREEN: &str = "Screen";
const PLAIN: &str = "Plain file";
const AS_IT_IS: &str = "+3DOS file, as it is";

pub struct Disk {
    bytes: Vec<u8>,
    sectors: HashMap<(u8, u8, u8), usize>,
    sector_size: usize,
    per_track: usize,
    first_id: u8,
    sides: usize,
    reserved: usize,
    block: usize,
    directory_blocks: usize,
    blocks: usize,
    plus3: bool,
}

/// A blank +3 disk as the +3's FORMAT makes one: 40 tracks of nine 512-byte sectors, one side, its boot sector
/// giving the disk's specification.
pub fn blank() -> Vec<u8> {
    let mut out = b"MV - CPCEMU Disk-File\r\nDisk-Info\r\n".to_vec();
    out.extend(b"Limoniter     ");
    out.resize(0x30, 0);
    out.extend([40, 1, 0x00, 0x13]);
    out.resize(256, 0);
    for track in 0..40u8 {
        let mut header = b"Track-Info\r\n".to_vec();
        header.resize(0x10, 0);
        header.extend([track, 0, 0, 0, 2, 9, 0x52, 0xE5]);
        for id in 1..=9u8 {
            header.extend([track, 0, id, 2, 0, 0, 0, 0]);
        }
        header.resize(256, 0);
        out.extend(header);
        let mut data = vec![0xE5u8; 9 * 512];
        if track == 0 {
            // The disk specification: +3 format, one side, 40 tracks, 9 sectors of 512 bytes, one reserved track,
            // 1K blocks, two directory blocks, and the gaps.
            data[..10].copy_from_slice(&[0, 0, 40, 9, 2, 1, 3, 2, 0x2A, 0x52]);
            data[10..512].fill(0);
        }
        out.extend(data);
    }
    out
}

impl Disk {
    pub fn open(bytes: Vec<u8>) -> Result<Self, String> {
        let extended = bytes.starts_with(b"EXTENDED CPC DSK");
        if !extended && !bytes.starts_with(b"MV - CPC") {
            return Err("This isn't a CPC or +3 DSK image.".into());
        }
        let (cylinders, heads) = (usize::from(bytes[0x30]), usize::from(bytes[0x31]).max(1));
        let standard = usize::from(u16::from_le_bytes([bytes[0x32], bytes[0x33]]));
        let mut sectors = HashMap::new();
        let mut first_id = None;
        let mut sizes = Vec::new();
        let mut at = 256;
        for index in 0..cylinders * heads {
            let size = if extended { usize::from(bytes[0x34 + index]) * 256 } else { standard };
            if size == 0 {
                continue;
            }
            let track = bytes.get(at..at + 256).ok_or("The DSK image ends partway through a track.")?;
            if !track.starts_with(b"Track-Info") {
                return Err(format!("Track {index} of the DSK image has no track header."));
            }
            let mut data = at + 256;
            for s in 0..usize::from(track[0x15]).min(29) {
                let info = &track[0x18 + s * 8..0x20 + s * 8];
                let length = if extended { usize::from(u16::from_le_bytes([info[6], info[7]])) } else { 128usize << info[3].min(6) };
                if data + length > bytes.len() {
                    return Err("The DSK image ends partway through a sector.".into());
                }
                sectors.insert((track[0x10], track[0x11], info[2]), data);
                sizes.push(length);
                if index == 0 && s == 0 {
                    first_id = Some(info[2]);
                }
                data += length;
            }
            at += size;
        }
        if sizes.iter().any(|s| *s != 512) {
            return Err("Only disks of 512-byte sectors throughout are changed here; this one has copy protection or an odd format.".into());
        }
        let first = first_id.ok_or("The disk has no sectors.")?;
        // (sectors a track, reserved tracks, block size, directory blocks, tracks a side, sides, +3)
        let spec = match first {
            0xC1 => (9, 0, 1024, 2, cylinders, heads, false),
            0x41 => (9, 2, 1024, 2, cylinders, heads, false),
            _ => {
                let boot_at = *sectors.get(&(0, 0, first)).ok_or("The disk has no boot sector.")?;
                let b = &bytes[boot_at..boot_at + 10];
                let b = if b[0] <= 3 && b[3] != 0 { [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]] } else { [0, 0, 40, 9, 2, 1, 3, 2] };
                if b[4] != 2 {
                    return Err("The disk's specification gives sectors other than 512 bytes.".into());
                }
                (usize::from(b[3]), usize::from(b[5]), 128usize << b[6].min(4), usize::from(b[7]).max(1), usize::from(b[2]), if b[1] & 3 == 0 { 1 } else { 2 }, true)
            }
        };
        let (per_track, reserved, block, directory_blocks, tracks, sides, plus3) = spec;
        let data_bytes = (tracks * sides).saturating_sub(reserved) * per_track * 512;
        let blocks = data_bytes / block;
        if blocks > 255 {
            return Err("Disks of more than 255 blocks (16-bit block numbers) aren't changed here yet.".into());
        }
        let disk = Self { bytes, sectors, sector_size: 512, per_track, first_id: first, sides, reserved, block, directory_blocks, blocks, plus3 };
        // Every block the directory and files need has to be in the image.
        for block in 0..disk.blocks {
            for part in 0..disk.block / 512 {
                disk.sector_at(block, part)?;
            }
        }
        Ok(disk)
    }

    /// Where a block's part (a 512-byte sector) is in the image.
    fn sector_at(&self, block: usize, part: usize) -> Result<usize, String> {
        let index = block * (self.block / self.sector_size) + part;
        let track = self.reserved + index / self.per_track;
        let place = index % self.per_track;
        let (cylinder, head) = if self.sides == 2 { (track / 2, track % 2) } else { (track, 0) };
        self.sectors
            .get(&(cylinder as u8, head as u8, self.first_id.wrapping_add(place as u8)))
            .copied()
            .ok_or_else(|| format!("track {cylinder} side {head} sector {} isn't in the image", self.first_id.wrapping_add(place as u8)))
    }

    fn write_block(&mut self, block: usize, data: &[u8]) {
        for part in 0..self.block / self.sector_size {
            let at = self.sector_at(block, part).expect("checked when opened");
            let slice = &mut self.bytes[at..at + self.sector_size];
            slice.fill(0xE5);
            let from = (part * self.sector_size).min(data.len());
            let to = ((part + 1) * self.sector_size).min(data.len());
            slice[..to - from].copy_from_slice(&data[from..to]);
        }
    }

    /// The directory's slots, where each is.
    fn slots(&self) -> Vec<usize> {
        let per_block = self.block / 32;
        (0..self.directory_blocks * per_block)
            .map(|n| {
                let (block, within) = (n / per_block, (n % per_block) * 32);
                self.sector_at(block, within / self.sector_size).expect("checked when opened") + within % self.sector_size
            })
            .collect()
    }

    fn name_of(&self, slot: usize) -> String {
        let text = |b: &[u8]| b.iter().map(|&c| char::from(c & 0x7F)).collect::<String>().trim_end().to_string();
        let (stem, ext) = (text(&self.bytes[slot + 1..slot + 9]), text(&self.bytes[slot + 9..slot + 12]));
        if ext.is_empty() { stem } else { format!("{stem}.{ext}") }
    }

    /// The blocks in use: the directory's, and every file entry's.
    fn used(&self) -> Vec<bool> {
        let mut used = vec![false; self.blocks];
        for block in used.iter_mut().take(self.directory_blocks) {
            *block = true;
        }
        for slot in self.slots() {
            if self.bytes[slot] <= 15 {
                for b in &self.bytes[slot + 16..slot + 32] {
                    if let Some(u) = used.get_mut(usize::from(*b)).filter(|_| *b != 0) {
                        *u = true;
                    }
                }
            }
        }
        used
    }

    fn free_blocks(&self) -> usize {
        self.used().iter().filter(|u| !**u).count()
    }

    /// A file's directory entries, by user 0 and its name.
    fn entries_of(&self, name: &str) -> Vec<usize> {
        self.slots().into_iter().filter(|s| self.bytes[*s] <= 15 && self.name_of(*s).eq_ignore_ascii_case(name)).collect()
    }
}

/// A name as CP/M keeps it: 8 and 3 capitals, padded with spaces.
fn cpm_name(name: &str) -> Result<[u8; 11], String> {
    let (stem, ending) = super::stem_and_ending(name.trim());
    let part = |text: &str, most: usize| -> String { text.chars().map(|c| c.to_ascii_uppercase()).filter(|c| c.is_ascii_graphic() && !"<>.,;:=?*[]|/\\\"".contains(*c)).take(most).collect() };
    let (stem, ending) = (part(stem, 8), part(ending, 3));
    if stem.is_empty() {
        return Err(format!("\"{name}\" has no characters a CP/M name can hold."));
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

/// A +3DOS header: type, length, and the two parameters (autostart line or load address; program length).
fn plus3_header(kind: u8, data: &[u8], first: u16, second: u16) -> Vec<u8> {
    let mut header = b"PLUS3DOS\x1A\x01\x00".to_vec();
    header.extend(((data.len() + 128) as u32).to_le_bytes());
    header.push(kind);
    header.extend((data.len() as u16).to_le_bytes());
    header.extend(first.to_le_bytes());
    header.extend(second.to_le_bytes());
    header.resize(127, 0);
    let sum = header.iter().fold(0u8, |sum, b| sum.wrapping_add(*b));
    header.push(sum);
    header.extend_from_slice(data);
    header
}

fn is_listing(bytes: &[u8]) -> bool {
    !bytes.is_empty() && std::str::from_utf8(bytes).is_ok_and(|text| text.lines().any(|l| l.trim_start().starts_with(|c: char| c.is_ascii_digit())) && !text.contains('\0'))
}

impl ImageWriter for Disk {
    fn kinds(&self) -> Vec<Kind> {
        if !self.plus3 {
            return vec![Kind::plain(PLAIN, "Kept as it is, with no header.")];
        }
        vec![
            Kind { name: PROGRAM, about: "A BASIC program with a +3DOS header, as SAVE writes one. A listing typed as text is tokenised first.", address: false, autostart: true },
            Kind { name: CODE, about: "Bytes with a +3DOS header, to LOAD \"name\" CODE at an address.", address: true, autostart: false },
            Kind::plain(SCREEN, "A 6912-byte screen with a +3DOS header, for LOAD \"name\" SCREEN$."),
            Kind::plain(AS_IT_IS, "A file that already has its +3DOS header, kept as it is."),
            Kind::plain(PLAIN, "Kept as it is, with no header: text, or a file for a program that reads its own."),
        ]
    }

    fn guess(&self, name: &str, bytes: &[u8]) -> Addition {
        let (stem, ending) = super::stem_and_ending(name);
        let ending = ending.to_lowercase();
        let mut add = Addition { name: name.to_string(), bytes: bytes.to_vec(), kind: PLAIN.into(), ..Default::default() };
        if !self.plus3 {
            return add;
        }
        if bytes.starts_with(b"PLUS3DOS") {
            add.kind = AS_IT_IS.into();
        } else if ending == "bas" || (ending.is_empty() && is_listing(bytes)) {
            add.kind = PROGRAM.into();
            add.autostart = is_listing(bytes).then_some(10);
            if ending.is_empty() {
                add.name = format!("{stem}.bas");
            }
        } else if ending == "scr" && bytes.len() == 6912 {
            add.kind = SCREEN.into();
        } else if matches!(ending.as_str(), "bin" | "code" | "cod") {
            add.kind = CODE.into();
            add.address = Some(32768);
        }
        add
    }

    fn add(&mut self, _folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        let name = cpm_name(&file.name)?;
        let existing = self.entries_of(&shown(&name));
        if !existing.is_empty() {
            if !file.replace {
                return Err(already(&shown(&name)));
            }
            for slot in existing {
                self.bytes[slot] = 0xE5;
            }
        }
        let bytes = match file.kind.as_str() {
            PROGRAM => {
                let program = if is_listing(&file.bytes) {
                    let text = String::from_utf8_lossy(&file.bytes);
                    let lines: Vec<&str> = text.lines().collect();
                    basic::tokenise_spectrum(&lines, basic::ZxDialect::Spectrum)?
                } else {
                    file.bytes.clone()
                };
                let length = program.len() as u16;
                plus3_header(0, &program, file.autostart.filter(|l| *l < 10000).unwrap_or(0x8000), length)
            }
            CODE => plus3_header(3, &file.bytes, file.address.unwrap_or(32768), 0x8000),
            SCREEN => {
                if file.bytes.len() != 6912 {
                    return Err(format!("A screen is 6912 bytes; {} is {}.", file.name, file.bytes.len()));
                }
                plus3_header(3, &file.bytes, 16384, 0x8000)
            }
            _ => file.bytes.clone(),
        };
        let count = bytes.len().div_ceil(self.block);
        let used = self.used();
        let free: Vec<usize> = (0..self.blocks).filter(|b| !used[*b]).take(count).collect();
        if free.len() < count {
            return Err(format!("{} needs {count}K; the disk has {}K free.", file.name, self.free_blocks() * self.block / 1024));
        }
        // An entry for each 16 blocks (16K): its extent number, records in it, and its blocks.
        let per_entry = 16;
        let extents = count.div_ceil(per_entry).max(1);
        let slots: Vec<usize> = self.slots().into_iter().filter(|s| self.bytes[*s] == 0xE5).take(extents).collect();
        if slots.len() < extents {
            return Err("The disk's directory is full.".into());
        }
        let records = bytes.len().div_ceil(128);
        let extent_size = self.block * per_entry;
        let records_per_extent = extent_size / 128;
        for (i, slot) in slots.iter().enumerate() {
            let entry_blocks = &free[(i * per_entry).min(free.len())..((i + 1) * per_entry).min(free.len())];
            // A logical extent is 16K (128 records); with 1K blocks and 8-bit block numbers each entry is one.
            let extent = i * (extent_size / 16384).max(1);
            let in_this = records.saturating_sub(i * records_per_extent).min(records_per_extent);
            let e = &mut self.bytes[*slot..*slot + 32];
            e.fill(0);
            e[0] = 0;
            e[1..12].copy_from_slice(&name);
            e[12] = (extent & 0x1F) as u8;
            e[13] = if i == extents - 1 { (bytes.len() % 128) as u8 } else { 0 };
            e[14] = (extent >> 5) as u8;
            e[15] = in_this as u8;
            for (j, b) in entry_blocks.iter().enumerate() {
                e[16 + j] = *b as u8;
            }
        }
        for (i, block) in free.iter().enumerate() {
            let part = bytes[i * self.block..((i + 1) * self.block).min(bytes.len())].to_vec();
            self.write_block(*block, &part);
        }
        Ok(())
    }

    fn delete(&mut self, _folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        let slots = self.entries_of(&entry.name);
        if slots.is_empty() {
            return Err(format!("{} isn't on the disk.", entry.name));
        }
        for slot in slots {
            self.bytes[slot] = 0xE5;
        }
        Ok(())
    }

    fn rename(&mut self, _folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        let slots = self.entries_of(&entry.name);
        if slots.is_empty() {
            return Err(format!("{} isn't on the disk.", entry.name));
        }
        let new = cpm_name(name)?;
        if !self.entries_of(&shown(&new)).iter().all(|s| slots.contains(s)) {
            return Err(already(&shown(&new)));
        }
        for slot in slots {
            // The attribute bits (read-only, system, archive) on the name's top bits are kept.
            for (i, c) in new.iter().enumerate() {
                let old = self.bytes[slot + 1 + i];
                self.bytes[slot + 1 + i] = (old & 0x80) | c;
            }
        }
        Ok(())
    }

    fn free(&self) -> Option<u64> {
        Some((self.free_blocks() * self.block) as u64)
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

    fn add(disk: &mut Disk, name: &str, bytes: &[u8]) {
        let addition = disk.guess(name, bytes);
        disk.add(None, &addition).unwrap();
    }

    #[test]
    fn files_go_on_a_plus3_disk_with_their_headers() {
        let mut disk = Disk::open(blank()).unwrap();
        assert_eq!(disk.free_blocks(), 173);
        let big: Vec<u8> = (0..40_000u32).map(|i| (i * 17) as u8).collect();
        add(&mut disk, "hello.bas", b"10 PRINT \"HELLO\"\n");
        add(&mut disk, "game.bin", &[1, 2, 3]);
        add(&mut disk, "notes.txt", &big);
        let volume = reopened(&disk, "t.dsk");
        assert_eq!(names(&*volume, None), ["GAME.BIN", "HELLO.BAS", "NOTES.TXT"], "the reader sorts by name");
        let program = basic::tokenise_spectrum(&["10 PRINT \"HELLO\""], basic::ZxDialect::Spectrum).unwrap();
        assert_eq!(volume.read(&entry(&*volume, None, "HELLO.BAS")).unwrap(), basic::plus3dos_file(&program, Some(10)));
        let code = volume.read(&entry(&*volume, None, "GAME.BIN")).unwrap();
        assert_eq!((&code[..8], code[15], &code[128..]), (&b"PLUS3DOS"[..], 3, &[1u8, 2, 3][..]));
        assert_eq!(volume.read(&entry(&*volume, None, "NOTES.TXT")).unwrap()[..40_000], big[..], "three directory entries of 16K");
        disk.rename(None, &entry(&*volume, None, "GAME.BIN"), "start.bin").unwrap();
        disk.delete(None, &entry(&*volume, None, "NOTES.TXT")).unwrap();
        let volume = reopened(&disk, "t.dsk");
        assert_eq!(names(&*volume, None), ["HELLO.BAS", "START.BIN"], "the reader sorts by name");
        assert_eq!(disk.free_blocks(), 171);
    }

    /// cpmtools lists and copies out what's written. libdsk's dsktrans turns the DSK into plain sectors first:
    /// cpmtools reading a DSK through libdsk itself gets a real +3 disk's files wrong too (its BASIC.COM doesn't
    /// start with the jump every .COM file does), and right from the plain sectors.
    #[test]
    fn cpmtools_reads_a_written_disk() {
        let (Some(cpmls), Some(cpmcp), Some(dsktrans)) = (tool("cpmls"), tool("cpmcp"), tool("dsktrans")) else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("limoniter-p3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut disk = Disk::open(blank()).unwrap();
        let big: Vec<u8> = (0..50_000u32).map(|i| (i * 7) as u8).collect();
        add(&mut disk, "big.dat", &big);
        add(&mut disk, "game.bin", &[9; 300]);
        add(&mut disk, "gone.dat", &[1; 5000]);
        disk.delete(None, &entry(&*reopened(&disk, "t.dsk"), None, "GONE.DAT")).unwrap();
        let dsk = dir.join("t.dsk");
        std::fs::write(&dsk, disk.bytes()).unwrap();
        let path = dir.join("t.raw");
        Command::new(&dsktrans).args(["-itype", "dsk", "-otype", "raw"]).arg(&dsk).arg(&path).output().unwrap();
        let listed = Command::new(&cpmls).args(["-f", "pcw"]).arg(&path).output().unwrap();
        let listed = String::from_utf8_lossy(&listed.stdout).to_lowercase();
        assert!(listed.contains("big.dat") && listed.contains("game.bin") && !listed.contains("gone"), "{listed}");
        let out = dir.join("big.out");
        let copied = Command::new(&cpmcp).args(["-f", "pcw"]).arg(&path).arg("0:big.dat").arg(&out).output().unwrap();
        assert!(copied.status.success(), "{}", String::from_utf8_lossy(&copied.stderr));
        let back = std::fs::read(&out).unwrap();
        assert_eq!(&back[..big.len()], &big[..]);
        std::fs::remove_dir_all(dir).ok();
    }
}
