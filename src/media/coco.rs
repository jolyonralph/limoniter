//! Tandy Color Computer and Dragon 32/64 disks: JVC images (.dsk, the sectors with an optional short header saying
//! the geometry), VDK images (the Dragon's, with a "dk" header) and plain OS-9 images (.os9), holding one of:
//!
//! - RS-DOS, Disk Extended Color BASIC's file system: the directory on track 17, its second sector a table of the
//!   disk's granules (half a track, 9 sectors, each), its third to eleventh 32-byte entries: an 8.3 name, a type
//!   (BASIC, data, machine code, text), whether it's ASCII, the first granule and the bytes used in the last sector.
//!   A granule's entry in the table is the next granule, or &C0 plus how many of its sectors the file uses.
//! - Dragon DOS: the directory on track 20, its first two sectors the free-sector map, the rest 25-byte entries
//!   of flags (bit 7 deleted, bit 5 continued, bit 3 the end of the directory, bit 1 protected, bit 0 a
//!   continuation), an 8.3 name padded with zeros and up to four runs of sectors (a logical sector number and a
//!   count); a continuation holds seven more runs. The last entry says how many bytes of its final sector are used.
//! - OS-9's RBF file system, used on both: sector 0 says where the root directory's file descriptor is; a
//!   descriptor gives a file's attributes, dates, length and up to 48 runs of sectors; a directory is a file of
//!   32-byte entries, a name ending with its top bit set and the sector of the entry's own descriptor.

use super::{Entry, Source, Volume, tidy_name};
use std::sync::Arc;

/// A disk's sectors, by logical sector number: tracks in order, a track's sides in turn, 256 bytes each.
struct Disk {
    source: Arc<dyn Source>,
    offset: u64,
    per_track: u64,
    heads: u64,
}

impl Disk {
    fn lsn(&self, lsn: u64) -> Option<Vec<u8>> {
        let at = self.offset + lsn * 256;
        (at + 256 <= self.source.len()).then(|| self.source.read_vec(at, 256).ok()).flatten()
    }

    /// A sector by track, side and its number on the track (from 1).
    fn sector(&self, track: u64, side: u64, sector: u64) -> Option<Vec<u8>> {
        if sector == 0 || sector > self.per_track {
            return None;
        }
        self.lsn((track * self.heads + side) * self.per_track + sector - 1)
    }

    fn sectors(&self) -> u64 {
        self.source.len().saturating_sub(self.offset) / 256
    }
}

fn be16(b: &[u8], at: usize) -> u64 {
    u64::from(u16::from_be_bytes([b[at], b[at + 1]]))
}

fn be24(b: &[u8], at: usize) -> u64 {
    u64::from(b[at]) << 16 | u64::from(b[at + 1]) << 8 | u64::from(b[at + 2])
}

/// An 8.3 name, padded with spaces (RS-DOS) or zeros (Dragon DOS).
fn name83(bytes: &[u8]) -> String {
    let text = |b: &[u8]| b.iter().map(|&c| if c == 0 { ' ' } else { char::from(c & 0x7F) }).collect::<String>().trim_end().to_string();
    let (stem, extension) = (text(&bytes[..8]), text(&bytes[8..11]));
    if extension.is_empty() { stem } else { format!("{stem}.{extension}") }
}

fn plausible_name(bytes: &[u8]) -> bool {
    bytes[0] > b' ' && bytes[0] < 0x7F && bytes.iter().all(|&c| c == 0 || (b' '..0x7F).contains(&c))
}

struct File {
    name: String,
    folder: Option<usize>,
    is_dir: bool,
    size: u64,
    /// RS-DOS: the first granule. Dragon DOS and OS-9: the runs of sectors.
    first: u64,
    runs: Vec<(u64, u64)>,
    kind: String,
    detail: String,
}

enum System {
    RsDos { fat: Vec<u8> },
    DragonDos,
    Os9,
}

pub struct Coco {
    disk: Disk,
    system: System,
    files: Vec<File>,
    what: String,
}

// ---------- RS-DOS ----------

fn rsdos(disk: &Disk, tracks: u64) -> Option<(System, Vec<File>)> {
    let fat = disk.sector(17, 0, 2)?;
    let granules = (tracks.min(80) - 1) * 2;
    let granules = granules.min(fat.len() as u64);
    // The table's entries are free (&FF), the next granule, or the last with its sectors used.
    if !fat[..granules as usize].iter().all(|&g| g == 0xFF || u64::from(g) < granules || (0xC0..=0xC9).contains(&g)) {
        return None;
    }
    let mut files = Vec::new();
    let mut bad = 0;
    'sectors: for sector in 3..=11 {
        let data = disk.sector(17, 0, sector)?;
        for entry in data.as_chunks::<32>().0 {
            match entry[0] {
                0xFF => break 'sectors,
                0 => continue,
                _ => {}
            }
            if !plausible_name(&entry[..11]) || entry[11] > 3 || !matches!(entry[12], 0 | 0xFF) || u64::from(entry[13]) >= granules {
                bad += 1;
                continue;
            }
            let kind = ["BASIC program", "BASIC data", "Machine code", "Text"][usize::from(entry[11])];
            let ascii = if entry[12] == 0xFF { ", ASCII" } else { "" };
            // The length: the granules in the chain, the last one's sectors used, and the last sector's bytes.
            let last_bytes = be16(entry, 14).min(256);
            let (mut granule, mut whole, mut size) = (u64::from(entry[13]), 0u64, None);
            for _ in 0..granules {
                match fat.get(granule as usize).copied() {
                    Some(next @ 0xC0..=0xC9) => {
                        let used = u64::from(next & 0x0F);
                        size = Some(whole * 2304 + used.saturating_sub(1) * 256 + if used == 0 { 0 } else { last_bytes });
                        break;
                    }
                    Some(next) if u64::from(next) < granules => {
                        whole += 1;
                        granule = u64::from(next);
                    }
                    _ => break,
                }
            }
            let Some(size) = size else {
                bad += 1;
                continue;
            };
            files.push(File { name: name83(&entry[..11]), folder: None, is_dir: false, size, first: u64::from(entry[13]), runs: Vec::new(), kind: format!("{kind}{ascii}"), detail: String::new() });
        }
    }
    if files.is_empty() || bad * 4 > files.len() {
        return None;
    }
    Some((System::RsDos { fat: fat[..granules as usize].to_vec() }, files))
}

/// An RS-DOS granule's place: half a track, skipping the directory's track 17.
fn rsdos_granule(granule: u64) -> Vec<(u64, u64)> {
    let track = granule / 2 + u64::from(granule >= 34);
    let first = if granule.is_multiple_of(2) { 1 } else { 10 };
    (first..first + 9).map(|s| (track, s)).collect()
}

fn rsdos_read(disk: &Disk, fat: &[u8], file: &File) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut granule = file.first;
    for _ in 0..fat.len() {
        let next = *fat.get(granule as usize).ok_or("the file's chain of granules leaves the disk")?;
        let places = rsdos_granule(granule);
        // The last granule says how many of its sectors are used; the length cuts the last of those.
        let (count, last) = if next >= 0xC0 { (u64::from(next & 0x0F).clamp(1, 9), true) } else { (9, false) };
        for &(track, sector) in places.iter().take(count as usize) {
            out.extend(disk.sector(track, 0, sector).ok_or_else(|| format!("track {track} isn't on the disk"))?);
        }
        if last {
            out.truncate(file.size as usize);
            return Ok(out);
        }
        if next == 0xFF {
            return Err("the file's chain of granules runs into a free one".into());
        }
        granule = u64::from(next);
    }
    Err("the file's chain of granules loops".into())
}

// ---------- Dragon DOS ----------

fn dragon_dos(disk: &Disk) -> Option<(System, Vec<File>)> {
    // The directory's entries, in order, from track 20's third sector on.
    let mut entries = Vec::new();
    for sector in 3..=disk.per_track.min(18) {
        let data = disk.sector(20, 0, sector)?;
        entries.extend(data[..250].as_chunks::<25>().0.iter().copied());
    }
    let mut files = Vec::new();
    let mut bad = 0;
    for entry in &entries {
        let flags = entry[0];
        if flags & 0x08 != 0 {
            break;
        }
        // Deleted (bit 7), or a continuation of another entry (bit 0).
        if flags & 0x81 != 0 {
            continue;
        }
        if !plausible_name(&entry[1..12]) {
            bad += 1;
            continue;
        }
        let mut runs = Vec::new();
        let mut current = *entry;
        let mut extents: Vec<[u8; 3]> = current[12..24].as_chunks::<3>().0.to_vec();
        let mut hops = 0;
        loop {
            runs.extend(extents.iter().filter(|e| e[2] > 0).map(|e| (u64::from(u16::from_be_bytes([e[0], e[1]])), u64::from(e[2]))));
            // Bit 5: continued in the entry byte 24 names; otherwise byte 24 is the last sector's bytes used.
            if current[0] & 0x20 == 0 || hops > 160 {
                break;
            }
            hops += 1;
            match entries.get(usize::from(current[24])) {
                Some(next) if next[0] & 0x01 != 0 => {
                    current = *next;
                    extents = current[1..22].as_chunks::<3>().0.to_vec();
                }
                _ => break,
            }
        }
        let sectors: u64 = runs.iter().map(|r| r.1).sum();
        let last = u64::from(current[24]);
        let size = if sectors == 0 { 0 } else { (sectors - 1) * 256 + if last == 0 { 256 } else { last } };
        let protected = if flags & 0x02 != 0 { "File, protected" } else { "File" };
        files.push(File { name: name83(&entry[1..12]), folder: None, is_dir: false, size, first: 0, runs, kind: protected.into(), detail: String::new() });
    }
    (!files.is_empty() && bad * 4 <= files.len() && files.iter().all(|f| f.runs.iter().all(|&(lsn, n)| lsn + n <= disk.sectors()))).then_some((System::DragonDos, files))
}

// ---------- OS-9 ----------

fn os9_name(bytes: &[u8]) -> Option<String> {
    let end = bytes.iter().position(|&b| b & 0x80 != 0)?;
    let name: String = bytes[..=end].iter().map(|&b| char::from(b & 0x7F)).collect();
    name.chars().all(|c| c.is_ascii_graphic()).then_some(name)
}

/// What a file descriptor says: attributes, length, runs of sectors and when the file was last written.
type Descriptor = (u8, u64, Vec<(u64, u64)>, String);

fn os9_descriptor(disk: &Disk, lsn: u64) -> Option<Descriptor> {
    let fd = disk.lsn(lsn)?;
    let size = u64::from(u32::from_be_bytes([fd[9], fd[10], fd[11], fd[12]]));
    let runs: Vec<(u64, u64)> = fd[16..16 + 48 * 5].as_chunks::<5>().0.iter().map(|s| (be24(s, 0), be16(s, 3))).take_while(|r| r.1 > 0).collect();
    let date = if fd[4] > 0 { format!("{}-{:02}-{:02} {:02}:{:02}", 1900 + u64::from(fd[3]), fd[4], fd[5], fd[6], fd[7]) } else { String::new() };
    Some((fd[0], size, runs, date))
}

fn os9_read(disk: &Disk, runs: &[(u64, u64)], size: u64) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(size as usize);
    'runs: for &(lsn, count) in runs {
        for n in 0..count {
            if out.len() as u64 >= size {
                break 'runs;
            }
            out.extend(disk.lsn(lsn + n).ok_or_else(|| format!("sector {} isn't on the disk", lsn + n))?);
        }
    }
    out.truncate(size as usize);
    Ok(out)
}

fn os9(disk: &Disk) -> Option<(System, Vec<File>, String)> {
    let lsn0 = disk.lsn(0)?;
    let total = be24(&lsn0, 0);
    let root = be24(&lsn0, 8);
    if total == 0 || total > disk.sectors() + disk.per_track * disk.heads || root == 0 || root >= total.min(disk.sectors()) {
        return None;
    }
    let (attributes, size, runs, _) = os9_descriptor(disk, root)?;
    if attributes & 0x80 == 0 || runs.is_empty() || size % 32 != 0 {
        return None;
    }
    let name = os9_name(&lsn0[31..63]).unwrap_or_default();
    let mut files = Vec::new();
    os9_directory(disk, &runs, size, None, &mut files, 0)?;
    Some((System::Os9, files, name))
}

fn os9_directory(disk: &Disk, runs: &[(u64, u64)], size: u64, folder: Option<usize>, files: &mut Vec<File>, depth: usize) -> Option<()> {
    if depth > 16 {
        return Some(());
    }
    let data = os9_read(disk, runs, size.min(256 * 1024)).ok()?;
    for entry in data.as_chunks::<32>().0 {
        if entry[0] == 0 {
            continue;
        }
        let name = os9_name(&entry[..29])?;
        if name == "." || name == ".." {
            continue;
        }
        let Some((attributes, size, runs, date)) = os9_descriptor(disk, be24(entry, 29)) else {
            continue;
        };
        let is_dir = attributes & 0x80 != 0;
        let permissions: String = [(0x01, 'r'), (0x02, 'w'), (0x04, 'e')].iter().map(|&(bit, c)| if attributes & bit != 0 { c } else { '-' }).collect();
        files.push(File { name, folder, is_dir, size, first: 0, runs: runs.clone(), kind: if is_dir { "Folder".into() } else { format!("File, {permissions}") }, detail: date });
        if is_dir {
            let index = files.len() - 1;
            os9_directory(disk, &runs, size, Some(index), files, depth + 1);
        }
    }
    Some(())
}

// ---------- Opening ----------

/// Whether an image's name and start make it one to try: a VDK by its header, a JVC or OS-9 image by its size.
pub fn looks_like(head: &[u8], len: u64, ending: &str) -> bool {
    if ending == "vdk" {
        return head.starts_with(b"dk");
    }
    let header = len % 256;
    let body = len - header;
    match ending {
        "os9" => body.is_multiple_of(256) && body >= 256 * 18 && head.len() > 256,
        "dsk" | "jvc" => header <= 5 && body.is_multiple_of(18 * 256) && (35..=80).contains(&(body / (18 * 256))) || (header <= 5 && body.is_multiple_of(36 * 256) && (35..=80).contains(&(body / (36 * 256)))),
        _ => false,
    }
}

impl Coco {
    pub fn open(source: Arc<dyn Source>, ending: &str) -> Result<Self, String> {
        let len = source.len();
        let head = source.read_vec(0, len.min(16) as usize)?;
        let (offset, per_track, heads, container) = if head.starts_with(b"dk") && head.len() >= 12 {
            let header = u64::from(u16::from_le_bytes([head[2], head[3]]));
            (header, 18, u64::from(head[9]).clamp(1, 2), "VDK")
        } else {
            // A JVC header, its length being what's left over from whole sectors: sectors a track, sides.
            let header = len % 256;
            let per_track = if header >= 1 { u64::from(head[0]).max(1) } else { 18 };
            let heads = if header >= 2 { u64::from(head[1]).clamp(1, 2) } else { 1 };
            (header, per_track, heads, if ending == "os9" { "OS-9 image" } else { "JVC" })
        };
        let disk = Disk { source: source.clone(), offset, per_track, heads };
        let mut coco = Self::on(disk, container)?;
        if coco.files.is_empty() && matches!(coco.system, System::Os9) {
            coco.what.push_str(" (empty)");
        }
        Ok(coco)
    }

    /// A disk laid out plainly, as the sectors of a DMK or other sector image come.
    pub fn open_flat(source: Arc<dyn Source>, shape: super::cpm::Shape) -> Result<Self, String> {
        if shape.sector_size != 256 || shape.per_track != 18 {
            return Err("not a CoCo or Dragon disk's tracks".into());
        }
        Self::on(Disk { source, offset: 0, per_track: 18, heads: shape.heads }, "")
    }

    fn on(disk: Disk, container: &str) -> Result<Self, String> {
        let tracks = disk.sectors() / (disk.per_track * disk.heads);
        let within = if container.is_empty() { String::new() } else { format!(" ({container}, {tracks} tracks{})", if disk.heads == 2 { ", two sides" } else { "" }) };
        if let Some((system, files, name)) = os9(&disk) {
            let named = if name.is_empty() { String::new() } else { format!(" \"{name}\"") };
            return Ok(Self { disk, system, files, what: format!("OS-9 disk{named}{within}") });
        }
        if let Some((system, files)) = rsdos(&disk, tracks) {
            return Ok(Self { disk, system, files, what: format!("Color Computer RS-DOS disk{within}") });
        }
        if let Some((system, files)) = dragon_dos(&disk) {
            return Ok(Self { disk, system, files, what: format!("Dragon DOS disk{within}") });
        }
        Err("This disk has no RS-DOS, Dragon DOS or OS-9 directory: it may boot a program of its own. It can still be shown in hex.".into())
    }
}

impl Volume for Coco {
    fn describe(&self) -> String {
        self.what.clone()
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        let folder = dir.map(|d| d.location as usize);
        Ok(self
            .files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.folder == folder)
            .map(|(index, file)| {
                let mut entry = if file.is_dir { Entry::dir(tidy_name(&file.name), index as u64, 0) } else { Entry::file(tidy_name(&file.name), file.size, index as u64) };
                entry.kind.clone_from(&file.kind);
                entry.detail.clone_from(&file.detail);
                entry
            })
            .collect())
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let file = self.files.get(entry.location as usize).ok_or("That file isn't on the disk.")?;
        match &self.system {
            System::RsDos { fat } => rsdos_read(&self.disk, fat, file),
            System::DragonDos | System::Os9 => os9_read(&self.disk, &file.runs, file.size),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 35-track RS-DOS disk with one 300-byte file in granules 0 and 1 (track 0), the second granule's first sector
    /// holding its last 44 bytes.
    #[test]
    fn rsdos_follows_its_granule_table() {
        let mut disk = vec![0u8; 35 * 18 * 256];
        let sector = |track: usize, s: usize| (track * 18 + s - 1) * 256;
        let fat = sector(17, 2);
        disk[fat..fat + 68].fill(0xFF);
        disk[fat] = 1;
        disk[fat + 1] = 0xC1;
        let directory = sector(17, 3);
        disk[directory..directory + 9 * 256].fill(0xFF);
        disk[directory..directory + 11].copy_from_slice(b"HELLO   TXT");
        disk[directory + 11] = 3;
        disk[directory + 12] = 0xFF;
        disk[directory + 13] = 0;
        disk[directory + 14..directory + 16].copy_from_slice(&44u16.to_be_bytes());
        disk[directory + 16..directory + 32].fill(0);
        disk[directory + 32] = 0xFF;
        for n in 0..9 {
            disk[sector(0, 1 + n)..sector(0, 1 + n) + 256].fill(b'A' + n as u8);
        }
        disk[sector(0, 10)..sector(0, 10) + 44].fill(b'Z');
        assert!(looks_like(&[], disk.len() as u64, "dsk"));
        let coco = Coco::open(Arc::new(disk), "dsk").unwrap();
        assert!(coco.describe().starts_with("Color Computer RS-DOS"), "{}", coco.describe());
        let files = coco.list(None).unwrap();
        assert_eq!(files[0].name, "HELLO.TXT");
        assert_eq!(files[0].kind, "Text, ASCII");
        let data = coco.read(&files[0]).unwrap();
        assert_eq!(data.len(), 9 * 256 + 44);
        assert_eq!((data[0], data[8 * 256], data[9 * 256]), (b'A', b'I', b'Z'));
    }

    /// An OS-9 disk: the root directory with one file and one folder, the folder with one file.
    #[test]
    fn os9_walks_its_directories() {
        let mut disk = vec![0u8; 630 * 256];
        let at = |lsn: usize| lsn * 256;
        disk[0..3].copy_from_slice(&[0, 2, 0x76]); // 630 sectors
        disk[8..11].copy_from_slice(&[0, 0, 2]); // the root's descriptor at 2
        disk[31..35].copy_from_slice(b"DIS\xCB");
        let descriptor = |disk: &mut Vec<u8>, lsn: usize, dir: bool, size: u32, data: usize| {
            disk[at(lsn)] = if dir { 0x80 | 0x3F } else { 0x1B };
            disk[at(lsn) + 9..at(lsn) + 13].copy_from_slice(&size.to_be_bytes());
            disk[at(lsn) + 16..at(lsn) + 19].copy_from_slice(&(data as u32).to_be_bytes()[1..]);
            disk[at(lsn) + 19..at(lsn) + 21].copy_from_slice(&1u16.to_be_bytes());
        };
        let entry = |disk: &mut Vec<u8>, lsn: usize, slot: usize, name: &[u8], fd: usize| {
            let e = at(lsn) + slot * 32;
            disk[e..e + name.len()].copy_from_slice(name);
            disk[e + name.len() - 1] |= 0x80;
            disk[e + 29..e + 32].copy_from_slice(&(fd as u32).to_be_bytes()[1..]);
        };
        descriptor(&mut disk, 2, true, 4 * 32, 3);
        entry(&mut disk, 3, 0, b"..", 2);
        entry(&mut disk, 3, 1, b".", 2);
        entry(&mut disk, 3, 2, b"startup", 4);
        entry(&mut disk, 3, 3, b"CMDS", 6);
        descriptor(&mut disk, 4, false, 5, 5);
        disk[at(5)..at(5) + 5].copy_from_slice(b"hello");
        descriptor(&mut disk, 6, true, 3 * 32, 7);
        entry(&mut disk, 7, 2, b"dir", 8);
        descriptor(&mut disk, 8, false, 2, 9);
        disk[at(9)..at(9) + 2].copy_from_slice(b"ok");
        let coco = Coco::open(Arc::new(disk), "os9").unwrap();
        assert!(coco.describe().contains("OS-9 disk \"DISK\""), "{}", coco.describe());
        let top = coco.list(None).unwrap();
        assert_eq!(top.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["startup", "CMDS"]);
        assert_eq!(coco.read(&top[0]).unwrap(), b"hello");
        let inside = coco.list(Some(&top[1])).unwrap();
        assert_eq!(coco.read(&inside[0]).unwrap(), b"ok");
    }
}
