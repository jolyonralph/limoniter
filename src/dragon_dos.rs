//! Dragon DOS, the Dragon 32 and 64's disk system: its binary files, and its disks, made and read.
//!
//! A disk has 40 (or 80) tracks of 18 sectors of 256 bytes, on one side or two. Track 20 holds the directory, and
//! track 16 a copy of it: sectors 1 and 2 are the map of free sectors (a bit for each, set if it's free, the
//! first byte for the first eight; then at 252, the tracks and the sectors to a track, and each of them inverted,
//! to say the disk is Dragon DOS's), and sectors 3 to 18 hold ten entries of 25 bytes each: a flag (80 deleted,
//! 20 a continuation, 08 the end of the directory, 02 protected, 01 continued), the name (8) and its ending (3),
//! where the file is as up to four runs of sectors (each a logical sector number, high byte first, and a count),
//! and how many bytes of the last sector are used (or, for an entry continued, the next one's number).
//!
//! A disk is kept as a VDK file, as PC-Dragon and XRoar read it: a 12-byte header (dk, its length, versions,
//! tracks and sides) and the sectors in order, track by track, both sides of each.

/// A Dragon DOS binary: 55, its type (2), where it loads, how long it is, where it starts, AA, then the bytes.
pub fn binary(load: u16, exec: u16, bytes: &[u8]) -> Vec<u8> {
    let mut file = vec![0x55, 0x02];
    file.extend(load.to_be_bytes());
    file.extend((bytes.len() as u16).to_be_bytes());
    file.extend(exec.to_be_bytes());
    file.push(0xAA);
    file.extend_from_slice(bytes);
    file
}

/// What a binary's header says: where it loads, how long it is, and where it starts.
pub fn binary_header(file: &[u8]) -> Option<(u16, u16, u16)> {
    (file.len() >= 9 && file[0] == 0x55 && file[8] == 0xAA).then(|| (u16::from_be_bytes([file[2], file[3]]), u16::from_be_bytes([file[4], file[5]]), u16::from_be_bytes([file[6], file[7]])))
}

pub const SECTOR: usize = 256;
pub const SECTORS: usize = 18;
const DIRECTORY_TRACK: usize = 20;
const BACKUP_TRACK: usize = 16;
const ENTRY: usize = 25;
const ENTRIES_PER_SECTOR: usize = 10;
/// The directory's sectors after the two of the map: 16 of ten entries.
const MOST_ENTRIES: usize = 16 * ENTRIES_PER_SECTOR;

/// A Dragon DOS disk, its sectors in logical order: a track's 18 on side 0, then (on two sides) its 18 on side 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disk {
    pub tracks: usize,
    pub sides: usize,
    pub data: Vec<u8>,
}

/// A file on a disk: its name and ending, and its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub name: String,
    pub ending: String,
    pub bytes: Vec<u8>,
}

impl Disk {
    /// A newly formatted disk: everything free but the directory's two tracks, the map saying so, and an empty
    /// directory.
    pub fn new(tracks: usize, sides: usize) -> Disk {
        let mut disk = Disk { tracks, sides, data: vec![0xE5; tracks * sides * SECTORS * SECTOR] };
        for lsn in 0..disk.sector_count() {
            disk.set_free(lsn, true);
        }
        for track in [DIRECTORY_TRACK, BACKUP_TRACK] {
            for sector in 0..SECTORS * sides {
                disk.set_free(track * SECTORS * sides + sector, false);
            }
        }
        let map = disk.directory_lsn();
        let spt = (SECTORS * sides) as u8;
        disk.data[map * SECTOR + 0xFC..map * SECTOR + 0x100].copy_from_slice(&[tracks as u8, spt, !(tracks as u8), !spt]);
        // Every entry unused, and the first the directory's end.
        for n in 0..MOST_ENTRIES {
            let at = disk.entry_offset(n);
            disk.data[at..at + ENTRY].fill(0);
            disk.data[at] = 0x89;
        }
        for n in 0..MOST_ENTRIES / ENTRIES_PER_SECTOR {
            let at = (map + 2 + n) * SECTOR;
            disk.data[at + ENTRIES_PER_SECTOR * ENTRY..at + SECTOR].fill(0);
        }
        disk.copy_to_backup();
        disk
    }

    pub fn sector_count(&self) -> usize {
        self.tracks * self.sides * SECTORS
    }

    fn directory_lsn(&self) -> usize {
        DIRECTORY_TRACK * SECTORS * self.sides
    }

    fn entry_offset(&self, n: usize) -> usize {
        (self.directory_lsn() + 2 + n / ENTRIES_PER_SECTOR) * SECTOR + (n % ENTRIES_PER_SECTOR) * ENTRY
    }

    /// The map's bit for a sector: in sector 1 for the first 1440 (180 bytes), then sector 2.
    fn map_place(&self, lsn: usize) -> usize {
        let map = self.directory_lsn() * SECTOR;
        if lsn < 1440 { map + lsn / 8 } else { map + SECTOR + (lsn - 1440) / 8 }
    }

    pub fn is_free(&self, lsn: usize) -> bool {
        self.data[self.map_place(lsn)] & (1 << (lsn % 8)) != 0
    }

    fn set_free(&mut self, lsn: usize, free: bool) {
        let at = self.map_place(lsn);
        if free {
            self.data[at] |= 1 << (lsn % 8);
        } else {
            self.data[at] &= !(1 << (lsn % 8));
        }
    }

    fn copy_to_backup(&mut self) {
        let size = SECTORS * self.sides * SECTOR;
        let (from, to) = (self.directory_lsn() * SECTOR, BACKUP_TRACK * SECTORS * self.sides * SECTOR);
        let copy = self.data[from..from + size].to_vec();
        self.data[to..to + size].copy_from_slice(&copy);
    }

    /// Free sectors, as runs of at most 255, nearest the directory first, enough for `count`.
    fn find_runs(&self, count: usize) -> Option<Vec<(usize, usize)>> {
        let middle = self.directory_lsn();
        let mut order: Vec<usize> = (middle..self.sector_count()).collect();
        order.extend((0..middle).rev());
        let mut runs: Vec<(usize, usize)> = Vec::new();
        let mut left = count;
        for lsn in order {
            if left == 0 {
                break;
            }
            if !self.is_free(lsn) {
                continue;
            }
            match runs.last_mut() {
                Some((start, length)) if *start + *length == lsn && *length < 255 => *length += 1,
                _ => runs.push((lsn, 1)),
            }
            left -= 1;
        }
        (left == 0).then_some(runs)
    }

    /// Puts a file on the disk, in place of any of the same name.
    pub fn add(&mut self, name: &str, ending: &str, bytes: &[u8]) -> Result<(), String> {
        let (name, ending) = (clean(name, 8), clean(ending, 3));
        if name.is_empty() {
            return Err(crate::i18n::tr("A Dragon DOS file needs a name: up to eight letters and digits.").into());
        }
        let _ = self.remove(&name, &ending);
        let count = bytes.len().div_ceil(SECTOR).max(1);
        let runs = self.find_runs(count).ok_or_else(|| crate::i18n::tf!("{name}.{ending} doesn't fit: there isn't room left on the disk."))?;
        // Four runs to an entry; more go in continuation entries.
        let entries_needed = runs.len().div_ceil(4).max(1);
        let free_entries: Vec<usize> = (0..MOST_ENTRIES).filter(|n| self.data[self.entry_offset(*n)] & 0x80 != 0).take(entries_needed).collect();
        if free_entries.len() < entries_needed {
            return Err(crate::i18n::tr("The disk's directory is full.").into());
        }
        let mut written = 0;
        for &(start, length) in &runs {
            for i in 0..length {
                let take = (bytes.len() - written.min(bytes.len())).min(SECTOR);
                let at = (start + i) * SECTOR;
                self.data[at..at + SECTOR].fill(0);
                self.data[at..at + take].copy_from_slice(&bytes[written..written + take]);
                written += take;
                self.set_free(start + i, false);
            }
        }
        let used_in_last = (bytes.len() % SECTOR) as u8;
        for (k, (&n, chunk)) in free_entries.iter().zip(runs.chunks(4)).enumerate() {
            let at = self.entry_offset(n);
            let continued = k + 1 < free_entries.len();
            let mut entry = [0u8; ENTRY];
            entry[0] = if k > 0 { 0x20 } else { 0 } | if continued { 0x01 } else { 0 };
            if k == 0 {
                entry[1..9].copy_from_slice(&padded(&name, 8));
                entry[9..12].copy_from_slice(&padded(&ending, 3));
            }
            // A continuation entry's runs start at its second byte.
            let runs_at = if k == 0 { 12 } else { 1 };
            for (j, (start, length)) in chunk.iter().enumerate() {
                let place = runs_at + j * 3;
                entry[place..place + 2].copy_from_slice(&(*start as u16).to_be_bytes());
                entry[place + 2] = *length as u8;
            }
            entry[24] = if continued { free_entries[k + 1] as u8 } else { used_in_last };
            self.data[at..at + ENTRY].copy_from_slice(&entry);
        }
        self.mark_end();
        self.copy_to_backup();
        Ok(())
    }

    /// Takes a file off the disk, freeing its sectors. Whether it was there.
    pub fn remove(&mut self, name: &str, ending: &str) -> bool {
        let Some(n) = (0..MOST_ENTRIES).find(|n| {
            let at = self.entry_offset(*n);
            self.data[at] & 0xA0 == 0 && trimmed(&self.data[at + 1..at + 9]) == clean(name, 8) && trimmed(&self.data[at + 9..at + 12]) == clean(ending, 3)
        }) else {
            return false;
        };
        let mut next = Some(n);
        while let Some(n) = next {
            let at = self.entry_offset(n);
            let runs_at = if self.data[at] & 0x20 != 0 { 1 } else { 12 };
            for j in 0..if runs_at == 1 { 7 } else { 4 } {
                let place = at + runs_at + j * 3;
                if place + 3 > at + 24 {
                    break;
                }
                let (start, length) = (usize::from(u16::from_be_bytes([self.data[place], self.data[place + 1]])), usize::from(self.data[place + 2]));
                for lsn in start..(start + length).min(self.sector_count()) {
                    self.set_free(lsn, true);
                }
            }
            next = (self.data[at] & 0x01 != 0).then(|| usize::from(self.data[at + 24]));
            self.data[at] = 0x80;
        }
        self.mark_end();
        self.copy_to_backup();
        true
    }

    /// Gives a file a new name and ending, as RENAME does, keeping its place on the disk. Its new name mustn't
    /// be another file's.
    pub fn rename(&mut self, name: &str, ending: &str, new_name: &str, new_ending: &str) -> Result<(), String> {
        let (new_name, new_ending) = (clean(new_name, 8), clean(new_ending, 3));
        if new_name.is_empty() {
            return Err(crate::i18n::tr("A Dragon DOS file needs a name: up to eight letters and digits.").into());
        }
        let find = |disk: &Disk, name: &str, ending: &str| {
            (0..MOST_ENTRIES).find(|n| {
                let at = disk.entry_offset(*n);
                disk.data[at] & 0xA0 == 0 && trimmed(&disk.data[at + 1..at + 9]) == name && trimmed(&disk.data[at + 9..at + 12]) == ending
            })
        };
        let Some(n) = find(self, &clean(name, 8), &clean(ending, 3)) else {
            return Err(crate::i18n::tf!("{name}.{ending} isn't on the disk."));
        };
        if find(self, &new_name, &new_ending).is_some_and(|other| other != n) {
            return Err(crate::i18n::tf!("There's already a {new_name}.{new_ending} on the disk."));
        }
        let at = self.entry_offset(n);
        self.data[at + 1..at + 9].copy_from_slice(&padded(&new_name, 8));
        self.data[at + 9..at + 12].copy_from_slice(&padded(&new_ending, 3));
        self.copy_to_backup();
        Ok(())
    }

    /// Sets the end-of-directory mark on the entry after the last one used.
    fn mark_end(&mut self) {
        let last_used = (0..MOST_ENTRIES).rev().find(|n| self.data[self.entry_offset(*n)] & 0x80 == 0);
        for n in 0..MOST_ENTRIES {
            let at = self.entry_offset(n);
            if self.data[at] & 0x80 != 0 {
                self.data[at] = if last_used.is_none_or(|l| n > l) { 0x89 } else { 0x80 };
            }
        }
    }

    /// The files on the disk, in the directory's order.
    pub fn files(&self) -> Vec<File> {
        let mut out = Vec::new();
        for n in 0..MOST_ENTRIES {
            let at = self.entry_offset(n);
            let flag = self.data[at];
            if flag & 0x08 != 0 && flag & 0x80 != 0 {
                break;
            }
            if flag & 0xA0 != 0 {
                continue;
            }
            let mut bytes = Vec::new();
            let mut entry = n;
            let mut runs_at = 12;
            let used_in_last = loop {
                let at = self.entry_offset(entry);
                for j in 0..if runs_at == 12 { 4 } else { 7 } {
                    let place = at + runs_at + j * 3;
                    let (start, length) = (usize::from(u16::from_be_bytes([self.data[place], self.data[place + 1]])), usize::from(self.data[place + 2]));
                    for lsn in start..(start + length).min(self.sector_count()) {
                        bytes.extend_from_slice(&self.data[lsn * SECTOR..(lsn + 1) * SECTOR]);
                    }
                }
                if self.data[at] & 0x01 == 0 {
                    break self.data[at + 24];
                }
                entry = usize::from(self.data[at + 24]);
                runs_at = 1;
                if entry >= MOST_ENTRIES {
                    break 0;
                }
            };
            if used_in_last != 0 && !bytes.is_empty() {
                bytes.truncate(bytes.len() - SECTOR + usize::from(used_in_last));
            }
            out.push(File { name: trimmed(&self.data[at + 1..at + 9]), ending: trimmed(&self.data[at + 9..at + 12]), bytes });
        }
        out
    }

    /// The disk as a VDK file.
    pub fn to_vdk(&self) -> Vec<u8> {
        let mut file = vec![b'd', b'k', 12, 0, 0x10, 0x10, b'L', 0, self.tracks as u8, self.sides as u8, 0, 0];
        file.extend_from_slice(&self.data);
        file
    }

    /// A VDK file's disk, if it's a Dragon DOS one.
    pub fn from_vdk(file: &[u8]) -> Option<Disk> {
        if !file.starts_with(b"dk") || file.len() < 12 {
            return None;
        }
        let header = usize::from(u16::from_le_bytes([file[2], file[3]]));
        let (tracks, sides) = (usize::from(file[8]), usize::from(file[9]).max(1));
        let data = file.get(header..header + tracks * sides * SECTORS * SECTOR)?.to_vec();
        let disk = Disk { tracks, sides, data };
        let map = disk.directory_lsn() * SECTOR;
        let marks = &disk.data[map + 0xFC..map + 0x100];
        (marks[0] == !marks[2] && marks[1] == !marks[3]).then_some(disk)
    }
}

/// A name as Dragon DOS keeps it: capitals, letters and digits, at most `most`.
fn clean(name: &str, most: usize) -> String {
    name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').take(most).collect::<String>().to_ascii_uppercase()
}

fn padded(text: &str, size: usize) -> Vec<u8> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.resize(size, 0);
    bytes
}

fn trimmed(bytes: &[u8]) -> String {
    bytes.iter().take_while(|b| **b != 0).map(|b| *b as char).collect::<String>().trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binaries_have_dragon_doss_header() {
        let file = binary(0x3F00, 0x3F02, &[1, 2, 3]);
        assert_eq!(file, [0x55, 0x02, 0x3F, 0x00, 0x00, 0x03, 0x3F, 0x02, 0xAA, 1, 2, 3]);
        assert_eq!(binary_header(&file), Some((0x3F00, 3, 0x3F02)));
    }

    #[test]
    fn files_go_on_a_disk_and_come_back_off_it() {
        let mut disk = Disk::new(40, 1);
        let free_at_first = (0..disk.sector_count()).filter(|l| disk.is_free(*l)).count();
        assert_eq!(free_at_first, 720 - 36, "all but the directory's two tracks");
        let small = binary(0x3F00, 0x3F00, &[0x39]);
        // Bigger than one run of 255 sectors: two runs.
        let big: Vec<u8> = (0..70_000u32).map(|i| (i * 7) as u8).collect();
        disk.add("hello", "bin", &small).unwrap();
        disk.add("bigfile", "dat", &big).unwrap();
        let files = disk.files();
        assert_eq!(files.iter().map(|f| format!("{}.{}", f.name, f.ending)).collect::<Vec<_>>(), ["HELLO.BIN", "BIGFILE.DAT"]);
        assert_eq!(files[0].bytes, small);
        assert_eq!(files[1].bytes, big);
        // Kept as a VDK, and read back the same.
        let again = Disk::from_vdk(&disk.to_vdk()).unwrap();
        assert_eq!(again, disk);
        // The backup directory matches.
        let size = SECTORS * SECTOR;
        assert_eq!(disk.data[16 * size..17 * size], disk.data[20 * size..21 * size]);
        // Taken off, its sectors are free again.
        assert!(disk.remove("BIGFILE", "DAT"));
        assert_eq!(disk.files().len(), 1);
        assert_eq!((0..disk.sector_count()).filter(|l| disk.is_free(*l)).count(), free_at_first - 1);
        // Too big for a disk.
        assert!(disk.add("huge", "dat", &vec![0; 200_000]).is_err());
    }

    #[test]
    fn files_are_renamed_where_they_are() {
        let mut disk = Disk::new(40, 1);
        let program = binary(0x3000, 0x3000, &[0x39]);
        disk.add("first", "bin", &program).unwrap();
        disk.add("second", "dat", b"words").unwrap();
        let free = (0..disk.sector_count()).filter(|l| disk.is_free(*l)).count();
        disk.rename("FIRST", "BIN", "game!", "bin").unwrap();
        let files = disk.files();
        assert_eq!(files.iter().map(|f| format!("{}.{}", f.name, f.ending)).collect::<Vec<_>>(), ["GAME.BIN", "SECOND.DAT"], "in its place, its name cleaned");
        assert_eq!(files[0].bytes, program);
        assert_eq!((0..disk.sector_count()).filter(|l| disk.is_free(*l)).count(), free, "no sectors moved");
        // The backup directory has the new name too.
        let size = SECTORS * SECTOR;
        assert_eq!(disk.data[16 * size..17 * size], disk.data[20 * size..21 * size]);
        // Not to another file's name, not to nothing, and not a file that isn't there.
        assert!(disk.rename("GAME", "BIN", "second", "dat").unwrap_err().contains("SECOND.DAT"));
        assert!(disk.rename("GAME", "BIN", "", "bin").is_err());
        assert!(disk.rename("NOTHERE", "BIN", "X", "BIN").is_err());
        // To the same name, with only its ending changed, is fine.
        disk.rename("GAME", "BIN", "game", "old").unwrap();
        assert_eq!(disk.files()[0].ending, "OLD");
        assert_eq!(Disk::from_vdk(&disk.to_vdk()).unwrap().files(), disk.files());
    }
}

#[cfg(test)]
mod real_disks {
    /// With LIMONITER_VDKS naming a folder of real Dragon DOS disks: each is read, and every binary on it has a
    /// header whose length is the file's.
    #[test]
    #[ignore]
    fn real_disks_are_read() {
        let Ok(folder) = std::env::var("LIMONITER_VDKS") else { return };
        let (mut disks, mut dragon, mut binaries, mut good) = (0, 0, 0, 0);
        for entry in std::fs::read_dir(folder).unwrap().flatten() {
            disks += 1;
            let Some(disk) = super::Disk::from_vdk(&std::fs::read(entry.path()).unwrap()) else { continue };
            dragon += 1;
            for file in disk.files() {
                if let Some((_, length, _)) = super::binary_header(&file.bytes) {
                    binaries += 1;
                    if usize::from(length) + 9 == file.bytes.len() {
                        good += 1;
                    } else {
                        eprintln!("{:?} {}.{}: header says {length}, file has {}", entry.path(), file.name, file.ending, file.bytes.len());
                    }
                }
            }
        }
        eprintln!("{disks} disks, {dragon} Dragon DOS, {binaries} binaries, {good} whose lengths match");
        assert!(dragon > 0 && good * 10 >= binaries * 9);
    }
}
