//! FAT12, FAT16 and FAT32 disks, PC and Atari ST floppies and the first partition of a hard disk image, written as
//! Microsoft's FAT specification says. A file gets clusters from the first free, marked in every copy of the FAT;
//! its directory entry has an 8.3 name, with the long name (VFAT) before it when the name doesn't fit 8.3, or the
//! NT lower-case flags when it only differs in being lower case. A deleted entry is marked 0xE5, as DOS does.

use super::{Addition, Entry, ImageWriter, already};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Width {
    Fat12,
    Fat16,
    Fat32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Floppy {
    Pc360,
    Pc720,
    Pc1440,
    St720,
}

pub struct Fat {
    bytes: Vec<u8>,
    /// Where the file system starts: 0, or its partition's first byte.
    base: usize,
    width: Width,
    sector: usize,
    per_cluster: usize,
    fat_start: usize,
    fat_bytes: usize,
    fats: usize,
    root_start: usize,
    root_entries: usize,
    data_start: usize,
    clusters: usize,
    root_cluster: usize,
    fsinfo: Option<usize>,
}

/// A formatted floppy, as mformat or an Atari ST's desktop leaves one, with its label in the boot sector and the
/// root directory.
pub fn blank(kind: Floppy, label: &str) -> Vec<u8> {
    // (sectors, per cluster, root entries, media, FAT sectors, sectors a track)
    let (total, per_cluster, root, media, fat_sectors, per_track) = match kind {
        Floppy::Pc360 => (720usize, 2u8, 112u16, 0xFDu8, 2u16, 9u16),
        Floppy::Pc720 => (1440, 2, 112, 0xF9, 3, 9),
        Floppy::Pc1440 => (2880, 1, 224, 0xF0, 9, 18),
        Floppy::St720 => (1440, 2, 112, 0xF9, 5, 9),
    };
    let mut bytes = vec![0u8; total * 512];
    let boot = &mut bytes[..512];
    if kind == Floppy::St720 {
        // The ST's boot sector starts with a branch and its loader's name; with no boot code its checksum isn't
        // 0x1234, so TOS won't try to run it.
        boot[..8].copy_from_slice(b"\x60\x38Loader");
        boot[8..11].copy_from_slice(&[0x4C, 0x4D, 0x31]);
    } else {
        boot[..11].copy_from_slice(b"\xEB\x3C\x90MSDOS5.0");
    }
    boot[11..13].copy_from_slice(&512u16.to_le_bytes());
    boot[13] = per_cluster;
    boot[14..16].copy_from_slice(&1u16.to_le_bytes());
    boot[16] = 2;
    boot[17..19].copy_from_slice(&root.to_le_bytes());
    boot[19..21].copy_from_slice(&(total as u16).to_le_bytes());
    boot[21] = media;
    boot[22..24].copy_from_slice(&fat_sectors.to_le_bytes());
    boot[24..26].copy_from_slice(&per_track.to_le_bytes());
    boot[26..28].copy_from_slice(&2u16.to_le_bytes());
    let name = short_label(label);
    if kind != Floppy::St720 {
        boot[36] = 0;
        boot[38] = 0x29;
        boot[39..43].copy_from_slice(&serial().to_le_bytes());
        // A disk with no label says so as DOS does, "NO NAME".
        boot[43..54].copy_from_slice(if name == [b' '; 11] { b"NO NAME    " } else { &name });
        boot[54..62].copy_from_slice(b"FAT12   ");
        boot[510] = 0x55;
        boot[511] = 0xAA;
    }
    for copy in 0..2 {
        let at = 512 + copy * usize::from(fat_sectors) * 512;
        bytes[at..at + 3].copy_from_slice(&[media, 0xFF, 0xFF]);
    }
    if name != [b' '; 11] {
        let root_at = 512 * (1 + 2 * usize::from(fat_sectors));
        bytes[root_at..root_at + 11].copy_from_slice(&name);
        bytes[root_at + 11] = 0x08;
        let (date, time) = now();
        bytes[root_at + 22..root_at + 24].copy_from_slice(&time.to_le_bytes());
        bytes[root_at + 24..root_at + 26].copy_from_slice(&date.to_le_bytes());
    }
    bytes
}

fn short_label(label: &str) -> [u8; 11] {
    let mut out = [b' '; 11];
    for (slot, c) in out.iter_mut().zip(label.chars().map(|c| c.to_ascii_uppercase()).filter(|c| short_char(*c) || *c == ' ')) {
        *slot = c as u8;
    }
    out
}

fn serial() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0x4C4D_0001, |d| d.as_secs() as u32)
}

/// Today's date and the time now, as FAT keeps them (local time isn't known here, so it's UTC).
pub(super) fn now() -> (u16, u16) {
    let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let days = (seconds / 86_400) as i64;
    let secs = seconds % 86_400;
    // Days since 1970 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let date = (((year - 1980).clamp(0, 127) as u16) << 9) | ((month as u16) << 5) | day as u16;
    let time = (((secs / 3600) as u16) << 11) | ((((secs / 60) % 60) as u16) << 5) | ((secs % 60) / 2) as u16;
    (date, time)
}

/// A character an 8.3 name may hold as it is.
fn short_char(c: char) -> bool {
    c.is_ascii_uppercase() || c.is_ascii_digit() || "!#$%&'()-@^_`{}~".contains(c)
}

/// The checksum of a short name that each of its long name's entries carries.
fn checksum(short: &[u8; 11]) -> u8 {
    short.iter().fold(0u8, |sum, b| sum.rotate_right(1).wrapping_add(*b))
}

/// How a name is written: its 8.3 form and the NT lower-case flags, or its 8.3 form and a long name too.
struct Names {
    short: [u8; 11],
    case: u8,
    long: Option<Vec<u16>>,
}

/// A directory slot's place in the image.
type Slot = usize;

impl Fat {
    pub fn open(bytes: Vec<u8>) -> Result<Self, String> {
        let base = super::super::fat::partition_of(&bytes[..512.min(bytes.len())]).map_or(0, |(start, _)| start as usize);
        let head = bytes.get(base..base + 512).ok_or("The disk has no boot sector.")?;
        if !super::super::fat::looks_like_fat(head) {
            return Err("This doesn't start with a FAT boot sector.".into());
        }
        let word = |at: usize| usize::from(u16::from_le_bytes([head[at], head[at + 1]]));
        let long = |at: usize| u32::from_le_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]) as usize;
        let sector = word(11);
        let per_cluster = usize::from(head[13]);
        let reserved = word(14);
        let fats = usize::from(head[16]);
        let root_entries = word(17);
        let total = if word(19) != 0 { word(19) } else { long(32) };
        let fat_sectors = if word(22) != 0 { word(22) } else { long(36) };
        let root_sectors = (root_entries * 32).div_ceil(sector);
        let data_sector = reserved + fats * fat_sectors + root_sectors;
        if fat_sectors == 0 || total <= data_sector || per_cluster == 0 {
            return Err("The FAT boot sector's sizes don't add up.".into());
        }
        let clusters = (total - data_sector) / per_cluster;
        let width = if clusters < 4085 {
            Width::Fat12
        } else if clusters < 65525 {
            Width::Fat16
        } else {
            Width::Fat32
        };
        if base + total * sector > bytes.len() {
            return Err("The disk image is shorter than its boot sector says, so it isn't changed here.".into());
        }
        let fsinfo = (width == Width::Fat32 && word(48) != 0).then(|| base + word(48) * sector).filter(|at| bytes.get(*at..*at + 4) == Some(b"RRaA"));
        Ok(Self {
            width,
            sector,
            per_cluster,
            fat_start: base + reserved * sector,
            fat_bytes: fat_sectors * sector,
            fats,
            root_start: base + (reserved + fats * fat_sectors) * sector,
            root_entries,
            data_start: base + data_sector * sector,
            clusters,
            root_cluster: if width == Width::Fat32 { long(44) } else { 0 },
            fsinfo,
            base,
            bytes,
        })
    }

    fn cluster_bytes(&self) -> usize {
        self.sector * self.per_cluster
    }

    fn cluster_at(&self, cluster: usize) -> usize {
        self.data_start + (cluster - 2) * self.cluster_bytes()
    }

    fn end_mark(&self) -> usize {
        match self.width {
            Width::Fat12 => 0xFFF,
            Width::Fat16 => 0xFFFF,
            Width::Fat32 => 0x0FFF_FFFF,
        }
    }

    fn get(&self, cluster: usize) -> usize {
        let fat = &self.bytes[self.fat_start..];
        match self.width {
            Width::Fat12 => {
                let at = cluster + cluster / 2;
                let pair = usize::from(u16::from_le_bytes([fat[at], fat[at + 1]]));
                if cluster.is_multiple_of(2) { pair & 0xFFF } else { pair >> 4 }
            }
            Width::Fat16 => usize::from(u16::from_le_bytes([fat[cluster * 2], fat[cluster * 2 + 1]])),
            Width::Fat32 => u32::from_le_bytes([fat[cluster * 4], fat[cluster * 4 + 1], fat[cluster * 4 + 2], fat[cluster * 4 + 3]]) as usize & 0x0FFF_FFFF,
        }
    }

    /// Sets a cluster's entry in every copy of the FAT.
    fn set(&mut self, cluster: usize, value: usize) {
        for copy in 0..self.fats {
            let fat = self.fat_start + copy * self.fat_bytes;
            match self.width {
                Width::Fat12 => {
                    let at = fat + cluster + cluster / 2;
                    let pair = u16::from_le_bytes([self.bytes[at], self.bytes[at + 1]]);
                    let value = value as u16 & 0xFFF;
                    let pair = if cluster.is_multiple_of(2) { (pair & 0xF000) | value } else { (pair & 0x000F) | (value << 4) };
                    self.bytes[at..at + 2].copy_from_slice(&pair.to_le_bytes());
                }
                Width::Fat16 => self.bytes[fat + cluster * 2..fat + cluster * 2 + 2].copy_from_slice(&(value as u16).to_le_bytes()),
                Width::Fat32 => {
                    let at = fat + cluster * 4;
                    let old = u32::from_le_bytes([self.bytes[at], self.bytes[at + 1], self.bytes[at + 2], self.bytes[at + 3]]);
                    let new = (old & 0xF000_0000) | (value as u32 & 0x0FFF_FFFF);
                    self.bytes[at..at + 4].copy_from_slice(&new.to_le_bytes());
                }
            }
        }
    }

    fn free_clusters(&self) -> usize {
        (2..self.clusters + 2).filter(|c| self.get(*c) == 0).count()
    }

    /// The clusters a chain runs through.
    fn chain(&self, first: usize) -> Result<Vec<usize>, String> {
        let mut out = Vec::new();
        let mut cluster = first;
        while (2..self.clusters + 2).contains(&cluster) {
            if out.len() > self.clusters {
                return Err("A chain of clusters loops back on itself.".into());
            }
            out.push(cluster);
            cluster = self.get(cluster);
        }
        Ok(out)
    }

    /// Takes free clusters for a chain of this many, linked and ended.
    fn allocate(&mut self, count: usize) -> Result<Vec<usize>, String> {
        let free: Vec<usize> = (2..self.clusters + 2).filter(|c| self.get(*c) == 0).take(count).collect();
        if free.len() < count {
            return Err(format!("The disk is full: this needs {} KB, and {} KB are free.", (count * self.cluster_bytes()).div_ceil(1024), (self.free_clusters() * self.cluster_bytes()) / 1024));
        }
        for (i, cluster) in free.iter().enumerate() {
            let next = free.get(i + 1).copied().unwrap_or(self.end_mark());
            self.set(*cluster, next);
        }
        self.free_changed();
        Ok(free)
    }

    fn release(&mut self, first: usize) -> Result<(), String> {
        for cluster in self.chain(first)? {
            self.set(cluster, 0);
        }
        self.free_changed();
        Ok(())
    }

    /// FAT32 keeps a count of free clusters; it's set to unknown, which tells the system to count again.
    fn free_changed(&mut self) {
        if let Some(at) = self.fsinfo {
            self.bytes[at + 488..at + 496].copy_from_slice(&[0xFF; 8]);
        }
    }

    /// The first cluster of a folder, or None for a FAT12 or FAT16 disk's root.
    fn folder_cluster(&self, folder: Option<&Entry>) -> Option<usize> {
        match folder {
            Some(f) if f.location >= 2 => Some(f.location as usize),
            _ if self.width == Width::Fat32 => Some(self.root_cluster),
            _ => None,
        }
    }

    /// The places of a folder's directory slots.
    fn slots(&self, folder: Option<&Entry>) -> Result<Vec<Slot>, String> {
        Ok(match self.folder_cluster(folder) {
            None => (0..self.root_entries).map(|i| self.root_start + i * 32).collect(),
            Some(first) => self.chain(first)?.into_iter().flat_map(|c| (0..self.cluster_bytes() / 32).map(move |i| (c, i))).map(|(c, i)| self.cluster_at(c) + i * 32).collect(),
        })
    }

    /// The entries in a folder that are files or folders: each one's slots (its long name's and its own), its
    /// long name if it has one, and its short name.
    fn entries(&self, folder: Option<&Entry>) -> Result<Vec<(Vec<Slot>, String, String)>, String> {
        let mut out = Vec::new();
        let mut pending: Vec<Slot> = Vec::new();
        for slot in self.slots(folder)? {
            let record = &self.bytes[slot..slot + 32];
            match record[0] {
                0x00 => break,
                0xE5 => {
                    pending.clear();
                    continue;
                }
                _ => {}
            }
            if record[11] == 0x0F {
                pending.push(slot);
                continue;
            }
            if record[11] & 0x08 != 0 || record[0] == b'.' {
                pending.clear();
                continue;
            }
            let short = shown_short(record);
            let long = long_name(&self.bytes, &pending);
            pending.push(slot);
            out.push((std::mem::take(&mut pending), long, short));
        }
        Ok(out)
    }

    /// A file or folder's slots, found by the name the reader gave it.
    fn find(&self, folder: Option<&Entry>, entry: &Entry) -> Result<Vec<Slot>, String> {
        self.entries(folder)?
            .into_iter()
            .find(|(_, long, short)| {
                let shown = if long.is_empty() { short } else { long };
                crate::media::tidy_name(shown) == entry.name || shown.eq_ignore_ascii_case(&entry.name)
            })
            .map(|(slots, _, _)| slots)
            .ok_or_else(|| format!("{} isn't in this folder.", entry.name))
    }

    /// The entry a name would clash with in a folder, ignoring case as FAT does.
    fn taken(&self, folder: Option<&Entry>, name: &str) -> Result<Option<Vec<Slot>>, String> {
        Ok(self.entries(folder)?.into_iter().find(|(_, long, short)| long.eq_ignore_ascii_case(name) || short.eq_ignore_ascii_case(name)).map(|(slots, _, _)| slots))
    }

    /// How a name is to be written in a folder: as 8.3 if it fits, else with a long name and a short one made
    /// unique (NAME~1.EXT).
    fn names(&self, folder: Option<&Entry>, name: &str) -> Result<Names, String> {
        let name = name.trim().trim_end_matches('.');
        if name.is_empty() || name.chars().any(|c| c.is_control() || "\\/:*?\"<>|".contains(c)) {
            return Err(format!("\"{name}\" can't be a file name on a FAT disk."));
        }
        let (stem, ending) = match name.rfind('.') {
            Some(dot) if dot > 0 => (&name[..dot], &name[dot + 1..]),
            _ => (name, ""),
        };
        let fits = |part: &str, most: usize| !part.is_empty() && part.len() <= most && part.chars().all(|c| short_char(c.to_ascii_uppercase()));
        let uniform = |part: &str| part.chars().all(|c| !c.is_ascii_lowercase()) || part.chars().all(|c| !c.is_ascii_uppercase());
        let mut short = [b' '; 11];
        if fits(stem, 8) && (ending.is_empty() || fits(ending, 3)) && uniform(stem) && uniform(ending) && !stem.contains('.') {
            for (slot, c) in short.iter_mut().zip(stem.chars()) {
                *slot = c.to_ascii_uppercase() as u8;
            }
            for (slot, c) in short[8..].iter_mut().zip(ending.chars()) {
                *slot = c.to_ascii_uppercase() as u8;
            }
            let lower = |part: &str| part.chars().any(|c| c.is_ascii_lowercase());
            let case = if lower(stem) { 0x08 } else { 0 } | if lower(ending) { 0x10 } else { 0 };
            return Ok(Names { short, case, long: None });
        }
        // A long name, and a short one from its letters that no other entry has.
        let squash = |part: &str, most: usize| -> String { part.chars().filter(|c| *c != ' ' && *c != '.').map(|c| c.to_ascii_uppercase()).map(|c| if short_char(c) { c } else { '_' }).take(most).collect() };
        let base = squash(stem, 6);
        let ext = squash(ending, 3);
        let taken: Vec<String> = self.entries(folder)?.into_iter().map(|(_, _, short)| short).collect();
        let mut chosen = None;
        for n in 1..100_000 {
            let tail = format!("~{n}");
            let keep = 8usize.saturating_sub(tail.len()).min(base.len());
            let candidate = format!("{}{tail}", &base[..keep]);
            let shown = if ext.is_empty() { candidate.clone() } else { format!("{candidate}.{ext}") };
            if !taken.iter().any(|t| t.eq_ignore_ascii_case(&shown)) {
                chosen = Some((candidate, ext.clone()));
                break;
            }
        }
        let (stem8, ext3) = chosen.ok_or("No short name is free for this file.")?;
        for (slot, c) in short.iter_mut().zip(stem8.bytes()) {
            *slot = c;
        }
        for (slot, c) in short[8..].iter_mut().zip(ext3.bytes()) {
            *slot = c;
        }
        Ok(Names { short, case: 0, long: Some(name.encode_utf16().collect()) })
    }

    /// Finds room for this many slots in a row in a folder, growing a folder by a cluster if it's full.
    fn room(&mut self, folder: Option<&Entry>, count: usize) -> Result<Vec<Slot>, String> {
        let slots = self.slots(folder)?;
        let mut run: Vec<Slot> = Vec::new();
        for slot in &slots {
            if matches!(self.bytes[*slot], 0x00 | 0xE5) {
                run.push(*slot);
                if run.len() == count {
                    return Ok(run);
                }
            } else {
                run.clear();
            }
        }
        let Some(first) = self.folder_cluster(folder) else {
            return Err("The disk's root folder is full; put the file in a folder.".into());
        };
        let last = *self.chain(first)?.last().ok_or("The folder has no clusters.")?;
        let added = self.allocate(count.div_ceil(self.cluster_bytes() / 32))?;
        self.set(last, added[0]);
        for cluster in &added {
            let at = self.cluster_at(*cluster);
            let size = self.cluster_bytes();
            self.bytes[at..at + size].fill(0);
        }
        let mut slots = run;
        for cluster in added {
            slots.extend((0..self.cluster_bytes() / 32).map(|i| self.cluster_at(cluster) + i * 32));
        }
        slots.truncate(count);
        Ok(slots)
    }

    /// Writes a name's entries, the long name's first, then the short entry with these fields.
    fn write_entry(&mut self, folder: Option<&Entry>, names: &Names, attributes: u8, cluster: usize, size: u32, stamp: Option<[u8; 10]>) -> Result<(), String> {
        let long_parts: Vec<Vec<u16>> = names.long.as_ref().map_or(Vec::new(), |long| long.chunks(13).map(<[u16]>::to_vec).collect());
        let slots = self.room(folder, long_parts.len() + 1)?;
        let sum = checksum(&names.short);
        for (i, part) in long_parts.iter().enumerate().rev() {
            let slot = slots[long_parts.len() - 1 - i];
            let mut units = part.clone();
            if units.len() < 13 {
                units.push(0);
            }
            units.resize(13, 0xFFFF);
            let record = &mut self.bytes[slot..slot + 32];
            record.fill(0);
            record[0] = (i as u8 + 1) | if i == long_parts.len() - 1 { 0x40 } else { 0 };
            record[11] = 0x0F;
            record[13] = sum;
            for (j, unit) in units.iter().enumerate() {
                let at = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30][j];
                record[at..at + 2].copy_from_slice(&unit.to_le_bytes());
            }
        }
        let slot = *slots.last().unwrap();
        let record = &mut self.bytes[slot..slot + 32];
        record.fill(0);
        record[..11].copy_from_slice(&names.short);
        if record[0] == 0xE5 {
            record[0] = 0x05;
        }
        record[11] = attributes;
        record[12] = names.case;
        match stamp {
            Some(stamp) => record[13..23].copy_from_slice(&stamp),
            None => {
                let (date, time) = now();
                record[14..16].copy_from_slice(&time.to_le_bytes());
                record[16..18].copy_from_slice(&date.to_le_bytes());
                record[18..20].copy_from_slice(&date.to_le_bytes());
            }
        }
        if stamp.is_none() {
            let (date, time) = now();
            record[22..24].copy_from_slice(&time.to_le_bytes());
            record[24..26].copy_from_slice(&date.to_le_bytes());
        }
        record[20..22].copy_from_slice(&((cluster >> 16) as u16).to_le_bytes());
        record[26..28].copy_from_slice(&(cluster as u16).to_le_bytes());
        record[28..32].copy_from_slice(&size.to_le_bytes());
        Ok(())
    }

    fn short_entry_cluster(&self, slot: Slot) -> usize {
        let r = &self.bytes[slot..slot + 32];
        usize::from(u16::from_le_bytes([r[26], r[27]])) | if self.width == Width::Fat32 { usize::from(u16::from_le_bytes([r[20], r[21]])) << 16 } else { 0 }
    }

    /// Takes a name's entries off its folder, leaving its clusters.
    fn unlink(&mut self, slots: &[Slot]) {
        for slot in slots {
            self.bytes[*slot] = 0xE5;
        }
    }

    /// Takes a file or empty folder off, its clusters freed.
    fn remove(&mut self, folder: Option<&Entry>, slots: &[Slot]) -> Result<(), String> {
        let own = *slots.last().unwrap();
        let cluster = self.short_entry_cluster(own);
        if self.bytes[own + 11] & 0x10 != 0 {
            let inside = Entry::dir(String::new(), cluster as u64, 0);
            if !self.entries(Some(&inside))?.is_empty() {
                return Err("The folder has files in it; take them off first.".into());
            }
        }
        let _ = folder;
        self.unlink(slots);
        if cluster >= 2 {
            self.release(cluster)?;
        }
        Ok(())
    }
}

/// A short entry's name as it's shown: NAME.EXT, its lower-case flags followed.
fn shown_short(record: &[u8]) -> String {
    let part = |bytes: &[u8], lower: bool| {
        let text: String = bytes.iter().map(|b| if *b == 0x05 { '\u{E5}' } else { char::from(*b) }).collect::<String>().trim_end().to_string();
        if lower { text.to_lowercase() } else { text }
    };
    let (base, ext) = (part(&record[..8], record[12] & 0x08 != 0), part(&record[8..11], record[12] & 0x10 != 0));
    if ext.is_empty() { base } else { format!("{base}.{ext}") }
}

/// A long name from its entries, which come before the short one, last piece first.
fn long_name(bytes: &[u8], slots: &[Slot]) -> String {
    let mut parts: Vec<(u8, Vec<u16>)> = slots
        .iter()
        .map(|slot| {
            let r = &bytes[*slot..*slot + 32];
            let units = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30].iter().map(|at| u16::from_le_bytes([r[*at], r[*at + 1]])).take_while(|u| *u != 0 && *u != 0xFFFF).collect();
            (r[0] & 0x1F, units)
        })
        .collect();
    parts.sort_by_key(|(order, _)| *order);
    String::from_utf16_lossy(&parts.into_iter().flat_map(|(_, units)| units).collect::<Vec<u16>>())
}

impl ImageWriter for Fat {
    fn add(&mut self, folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        if file.bytes.len() as u64 > u64::from(u32::MAX) {
            return Err("A FAT file can't be 4 GB or more.".into());
        }
        if let Some(slots) = self.taken(folder, &file.name)? {
            if !file.replace {
                return Err(already(&file.name));
            }
            if self.bytes[*slots.last().unwrap() + 11] & 0x10 != 0 {
                return Err(format!("{} is a folder here, so a file can't replace it.", file.name));
            }
            self.remove(folder, &slots)?;
        }
        let names = self.names(folder, &file.name)?;
        let count = file.bytes.len().div_ceil(self.cluster_bytes());
        let clusters = self.allocate(count)?;
        for (i, cluster) in clusters.iter().enumerate() {
            let at = self.cluster_at(*cluster);
            let size = self.cluster_bytes();
            let part = &file.bytes[i * size..((i + 1) * size).min(file.bytes.len())];
            self.bytes[at..at + size].fill(0);
            self.bytes[at..at + part.len()].copy_from_slice(part);
        }
        let first = clusters.first().copied().unwrap_or(0);
        if let Err(problem) = self.write_entry(folder, &names, 0x20, first, file.bytes.len() as u32, None) {
            if first >= 2 {
                self.release(first)?;
            }
            return Err(problem);
        }
        Ok(())
    }

    fn delete(&mut self, folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        let slots = self.find(folder, entry)?;
        self.remove(folder, &slots)
    }

    fn rename(&mut self, folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        let slots = self.find(folder, entry)?;
        if self.taken(folder, name)?.is_some_and(|other| other != slots) {
            return Err(already(name));
        }
        let own = *slots.last().unwrap();
        let record: Vec<u8> = self.bytes[own..own + 32].to_vec();
        let mut stamp = [0u8; 10];
        stamp.copy_from_slice(&record[13..23]);
        self.unlink(&slots);
        let names = match self.names(folder, name) {
            Ok(names) => names,
            Err(problem) => {
                for slot in &slots {
                    self.bytes[*slot] = record[0];
                }
                return Err(problem);
            }
        };
        let cluster = self.short_entry_cluster(own);
        let size = u32::from_le_bytes([record[28], record[29], record[30], record[31]]);
        self.write_entry(folder, &names, record[11], cluster, size, Some(stamp))?;
        // The written time and date are kept as they were.
        let new = self.find(folder, &Entry::file(crate::media::tidy_name(name.trim()), 0, 0)).ok().and_then(|s| s.last().copied());
        if let Some(new) = new {
            self.bytes[new + 22..new + 26].copy_from_slice(&record[22..26]);
        }
        Ok(())
    }

    fn make_folder(&mut self, folder: Option<&Entry>, name: &str) -> Result<(), String> {
        if self.taken(folder, name)?.is_some() {
            return Err(already(name));
        }
        let names = self.names(folder, name)?;
        let cluster = self.allocate(1)?[0];
        let at = self.cluster_at(cluster);
        let size = self.cluster_bytes();
        self.bytes[at..at + size].fill(0);
        let parent = match folder {
            Some(f) if f.location >= 2 && f.location as usize != self.root_cluster => f.location as usize,
            _ => 0,
        };
        let (date, time) = now();
        for (i, (dots, target)) in [(*b".          ", cluster), (*b"..         ", parent)].into_iter().enumerate() {
            let r = &mut self.bytes[at + i * 32..at + i * 32 + 32];
            r[..11].copy_from_slice(&dots);
            r[11] = 0x10;
            r[22..24].copy_from_slice(&time.to_le_bytes());
            r[24..26].copy_from_slice(&date.to_le_bytes());
            r[20..22].copy_from_slice(&((target >> 16) as u16).to_le_bytes());
            r[26..28].copy_from_slice(&(target as u16).to_le_bytes());
        }
        if let Err(problem) = self.write_entry(folder, &names, 0x10, cluster, 0, None) {
            self.release(cluster)?;
            return Err(problem);
        }
        Ok(())
    }

    fn has_folders(&self) -> bool {
        true
    }

    fn free(&self) -> Option<u64> {
        Some((self.free_clusters() * self.cluster_bytes()) as u64)
    }

    fn bytes(&self) -> &[u8] {
        let _ = self.base;
        &self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{entry, names, reopened, tool};
    use super::*;
    use std::process::Command;

    fn add(fat: &mut Fat, folder: Option<&Entry>, name: &str, bytes: &[u8]) {
        fat.add(folder, &Addition { name: name.into(), bytes: bytes.to_vec(), ..Default::default() }).unwrap();
    }

    #[test]
    fn files_and_folders_go_on_a_fat_disk_with_long_names_where_needed() {
        for kind in [Floppy::Pc360, Floppy::Pc720, Floppy::Pc1440, Floppy::St720] {
            let mut fat = Fat::open(blank(kind, "work")).unwrap();
            let big: Vec<u8> = (0..70_000u32).map(|i| (i % 251) as u8).collect();
            add(&mut fat, None, "README.TXT", b"Hello\r\n");
            add(&mut fat, None, "notes.txt", b"lower");
            add(&mut fat, None, "A long file name.text", &big);
            fat.make_folder(None, "Games").unwrap();
            let volume = reopened(&fat, "t.img");
            assert_eq!(names(&*volume, None), ["Games", "README.TXT", "notes.txt", "A long file name.text"], "{kind:?}");
            assert_eq!(volume.read(&entry(&*volume, None, "A long file name.text")).unwrap(), big);
            assert!(entry(&*volume, None, "A long file name.text").detail.contains("ALONGF~1.TEX"));
            let games = entry(&*volume, None, "Games");
            for n in 0..40 {
                add(&mut fat, Some(&games), &format!("level number {n}.dat"), &[n as u8; 100]);
            }
            let volume = reopened(&fat, "t.img");
            let inside = volume.list(Some(&games)).unwrap();
            assert_eq!(inside.len(), 40, "the folder grew past its first cluster");
            assert_eq!(volume.read(&entry(&*volume, Some(&games), "level number 39.dat")).unwrap(), [39; 100]);
            assert!(fat.delete(None, &games).unwrap_err().contains("files in it"));
            fat.rename(None, &entry(&*volume, None, "notes.txt"), "Shopping list.txt").unwrap();
            fat.delete(None, &entry(&*volume, None, "A long file name.text")).unwrap();
            let volume = reopened(&fat, "t.img");
            assert_eq!(names(&*volume, None), ["Games", "README.TXT", "Shopping list.txt"]);
            assert_eq!(volume.read(&entry(&*volume, None, "Shopping list.txt")).unwrap(), b"lower");
        }
    }

    /// dosfstools' fsck.fat finds nothing wrong with what's written, and mtools reads it back.
    #[test]
    fn fsck_and_mtools_agree_with_written_disks() {
        let (Some(fsck), Some(mtype)) = (tool("fsck.fat").or_else(|| Some("/usr/sbin/fsck.fat".into()).filter(|p: &std::path::PathBuf| p.is_file())), tool("mtype")) else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("limoniter-fat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut images = vec![blank(Floppy::Pc1440, "CHECK"), blank(Floppy::Pc360, "")];
        for size in [("16", "20"), ("32", "40")] {
            let path = dir.join(format!("made{}.img", size.0));
            std::fs::remove_file(&path).ok();
            Command::new(tool("mkfs.fat").or_else(|| Some("/usr/sbin/mkfs.fat".into())).unwrap()).args(["-C", "-F", size.0]).arg(&path).arg(format!("{}000", size.1)).output().unwrap();
            images.push(std::fs::read(&path).unwrap());
        }
        for (index, image) in images.into_iter().enumerate() {
            let mut fat = Fat::open(image).unwrap();
            let big: Vec<u8> = (0..50_000u32).map(|i| (i * 31) as u8).collect();
            add(&mut fat, None, "Big file.bin", &big);
            add(&mut fat, None, "SMALL.TXT", b"small\r\n");
            fat.make_folder(None, "Sub folder").unwrap();
            let sub = entry(&*reopened(&fat, "t.img"), None, "Sub folder");
            for n in 0..30 {
                add(&mut fat, Some(&sub), &format!("inner file {n}.txt"), format!("{n}").as_bytes());
            }
            add(&mut fat, None, "gone.txt", &[7; 3000]);
            fat.delete(None, &entry(&*reopened(&fat, "t.img"), None, "gone.txt")).unwrap();
            let path = dir.join(format!("w{index}.img"));
            std::fs::write(&path, fat.bytes()).unwrap();
            let checked = Command::new(&fsck).arg("-n").arg(&path).output().unwrap();
            let said = format!("{}{}", String::from_utf8_lossy(&checked.stdout), String::from_utf8_lossy(&checked.stderr));
            assert!(checked.status.success(), "image {index}: {said}");
            let typed = Command::new(&mtype).env("MTOOLS_SKIP_CHECK", "1").arg("-i").arg(&path).arg("::/Sub folder/inner file 29.txt").output().unwrap();
            assert_eq!(String::from_utf8_lossy(&typed.stdout), "29", "image {index}");
            let typed = Command::new(&mtype).env("MTOOLS_SKIP_CHECK", "1").arg("-i").arg(&path).arg("::/Big file.bin").output().unwrap();
            assert_eq!(typed.stdout, big, "image {index}");
        }
        std::fs::remove_dir_all(dir).ok();
    }
}

#[cfg(test)]
mod next_sd {
    use super::super::tests::{entry, reopened};
    use super::*;

    /// A Next's SD card image (ZEsarUX's, a FAT16 partition after an MBR) takes a dot command in /dot.
    #[test]
    fn a_dot_command_goes_on_a_next_sd_card() {
        let Ok(card) = std::fs::read("/root/emu/zsrc/ZEsarUX-13.0/tbblue.mmc") else {
            return;
        };
        let mut fat = Fat::open(card).unwrap();
        let volume = reopened(&fat, "tbblue.mmc");
        let dot = entry(&*volume, None, "dot");
        fat.add(Some(&dot), &Addition { name: "HIDOT".into(), bytes: vec![0x3E, b'H', 0xD7, 0xC9], ..Default::default() }).unwrap();
        let volume = reopened(&fat, "tbblue.mmc");
        let dot = entry(&*volume, None, "dot");
        assert_eq!(volume.read(&entry(&*volume, Some(&dot), "HIDOT")).unwrap(), [0x3E, b'H', 0xD7, 0xC9]);
        let Some(fsck) = Some(std::path::PathBuf::from("/usr/sbin/fsck.fat")).filter(|p| p.is_file()) else {
            return;
        };
        // fsck.fat checks the partition on its own, before and after: it finds nothing new (the card's label
        // was already only in its boot sector).
        let path = std::env::temp_dir().join(format!("limoniter-sd-{}.img", std::process::id()));
        let check = |bytes: &[u8]| -> Vec<String> {
            std::fs::write(&path, bytes).unwrap();
            let out = std::process::Command::new(&fsck).arg("-n").arg(&path).output().unwrap();
            String::from_utf8_lossy(&out.stdout).lines().filter(|l| !l.contains(" files, ")).map(str::to_string).collect()
        };
        let original = std::fs::read("/root/emu/zsrc/ZEsarUX-13.0/tbblue.mmc").unwrap();
        let before = check(&original[fat.base..]);
        let after = check(&fat.bytes()[fat.base..]);
        std::fs::remove_file(&path).ok();
        assert_eq!(after, before);
    }
}
