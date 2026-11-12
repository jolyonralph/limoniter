//! FAT12, FAT16 and FAT32, as Microsoft's FAT specification (hardware white paper, 2000) describes them: PC
//! floppies and hard disk images (with an MBR partition table), Atari ST disks (.st, and .msa packed ones) and MSX
//! disks. Long file names (VFAT) are joined from their entries, and the lower-case flags Windows NT sets are kept.

use super::{Entry, Slice, Source, Volume, tidy_name};
use crate::charset;
use std::sync::Arc;

/// Whether a boot sector holds a believable BIOS parameter block. Atari ST disks have no 0x55AA mark, so the
/// fields themselves are checked. The HP 150's disks have 256-byte sectors, and some early MS-DOS machines' 128.
pub fn looks_like_fat(head: &[u8]) -> bool {
    if head.len() < 512 {
        return false;
    }
    let bytes_per_sector = u16::from_le_bytes([head[11], head[12]]);
    let per_cluster = head[13];
    let reserved = u16::from_le_bytes([head[14], head[15]]);
    let fats = head[16];
    let media = head[21];
    matches!(bytes_per_sector, 128 | 256 | 512 | 1024 | 2048 | 4096) && per_cluster.is_power_of_two() && reserved >= 1 && (1..=4).contains(&fats) && (media >= 0xF0 || media == 0)
}

/// The first FAT partition in an MBR partition table, as (first byte, length).
pub fn partition_of(head: &[u8]) -> Option<(u64, u64)> {
    if head.len() < 512 || head[510..512] != [0x55, 0xAA] || looks_like_fat(head) {
        return None;
    }
    (0..4).find_map(|i| {
        let entry = &head[446 + i * 16..462 + i * 16];
        let first = u64::from(u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]));
        let count = u64::from(u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]));
        (matches!(entry[4], 0x01 | 0x04 | 0x06 | 0x0B | 0x0C | 0x0E) && first > 0 && count > 0).then_some((first * 512, count * 512))
    })
}

/// Unpacks an Atari ST .msa image: a big-endian header (0x0E0F, sectors per track, sides less one, first track,
/// last track), then each track's sides, packed with 0xE5 runs unless they're stored whole.
pub fn unpack_msa(source: &dyn Source) -> Result<Vec<u8>, String> {
    let data = source.read_vec(0, source.len().min(8 * 1024 * 1024) as usize)?;
    let word = |at: usize| data.get(at..at + 2).map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])));
    let (Some(sectors), Some(sides), Some(first), Some(last)) = (word(2), word(4), word(6), word(8)) else {
        return Err("The MSA header is cut short.".into());
    };
    if sectors == 0 || sectors > 64 || sides > 1 || last < first || last > 255 {
        return Err("The MSA header doesn't describe a disk.".into());
    }
    let track_size = sectors * 512;
    let mut out = vec![0u8; (last + 1) * (sides + 1) * track_size];
    let mut at = 10;
    for track in first..=last {
        for side in 0..=sides {
            let length = word(at).ok_or("The MSA image ends early.")?;
            at += 2;
            let packed = data.get(at..at + length).ok_or("The MSA image ends early.")?;
            at += length;
            let start = (track * (sides + 1) + side) * track_size;
            let target = &mut out[start..start + track_size];
            if length == track_size {
                target.copy_from_slice(packed);
                continue;
            }
            let mut i = 0;
            let mut j = 0;
            while i < packed.len() && j < track_size {
                if packed[i] == 0xE5 && i + 3 < packed.len() {
                    let (value, count) = (packed[i + 1], usize::from(u16::from_be_bytes([packed[i + 2], packed[i + 3]])));
                    let end = (j + count).min(track_size);
                    target[j..end].fill(value);
                    j = end;
                    i += 4;
                } else {
                    target[j] = packed[i];
                    j += 1;
                    i += 1;
                }
            }
        }
    }
    Ok(out)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Width {
    Fat12,
    Fat16,
    Fat32,
}

pub struct Fat {
    source: Arc<dyn Source>,
    kind: &'static str,
    width: Width,
    bytes_per_sector: u64,
    per_cluster: u64,
    fat_start: u64,
    root_start: u64,
    root_entries: u64,
    data_start: u64,
    root_cluster: u64,
    clusters: u64,
    label: String,
    partition: bool,
}

impl Fat {
    pub fn open(source: Arc<dyn Source>, kind: &'static str) -> Result<Self, String> {
        let head = source.read_vec(0, 512.min(source.len() as usize))?;
        if let Some((start, len)) = partition_of(&head) {
            let len = len.min(source.len().saturating_sub(start));
            let inner: Arc<dyn Source> = Arc::new(Slice { inner: source, start, len });
            let mut fat = Self::open(inner, "FAT hard disk image")?;
            fat.partition = true;
            return Ok(fat);
        }
        if !looks_like_fat(&head) {
            return Err("This doesn't start with a FAT boot sector.".into());
        }
        let word = |at: usize| u64::from(u16::from_le_bytes([head[at], head[at + 1]]));
        let long = |at: usize| u64::from(u32::from_le_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]));
        let bytes_per_sector = word(11);
        let per_cluster = u64::from(head[13]);
        let reserved = word(14);
        let fats = u64::from(head[16]);
        let root_entries = word(17);
        let total = if word(19) != 0 { word(19) } else { long(32) };
        let fat_sectors = if word(22) != 0 { word(22) } else { long(36) };
        let root_sectors = (root_entries * 32).div_ceil(bytes_per_sector);
        let data_sector = reserved + fats * fat_sectors + root_sectors;
        let clusters = total.saturating_sub(data_sector) / per_cluster.max(1);
        let width = if clusters < 4085 {
            Width::Fat12
        } else if clusters < 65525 {
            Width::Fat16
        } else {
            Width::Fat32
        };
        if fat_sectors == 0 || total == 0 || data_sector >= total {
            return Err("The FAT boot sector's sizes don't add up.".into());
        }
        let label_at = if width == Width::Fat32 { 71 } else { 43 };
        let has_label = head[if width == Width::Fat32 { 66 } else { 38 }] == 0x29;
        let label = if has_label { String::from_utf8_lossy(&head[label_at..label_at + 11]).trim().to_string() } else { String::new() };
        let mut fat = Self {
            source,
            kind,
            width,
            bytes_per_sector,
            per_cluster,
            fat_start: reserved * bytes_per_sector,
            root_start: (reserved + fats * fat_sectors) * bytes_per_sector,
            root_entries,
            data_start: data_sector * bytes_per_sector,
            root_cluster: if width == Width::Fat32 { long(44) } else { 0 },
            clusters,
            label: if label == "NO NAME" { String::new() } else { label },
            partition: false,
        };
        // DOS before 4.0 has no label in the boot sector; it's an entry in the root directory marked as one.
        if fat.label.is_empty()
            && width != Width::Fat32
            && let Ok(root) = fat.source.read_vec(fat.root_start, (root_entries * 32) as usize)
        {
            let entry = root.as_chunks::<32>().0.iter().take_while(|e| e[0] != 0).find(|e| e[0] != 0xE5 && e[11] & 0x08 != 0 && e[11] != 0x0F);
            if let Some(entry) = entry {
                fat.label = String::from_utf8_lossy(&entry[..11]).trim().to_string();
            }
        }
        Ok(fat)
    }

    fn cluster_bytes(&self) -> u64 {
        self.bytes_per_sector * self.per_cluster
    }

    fn next_cluster(&self, cluster: u64) -> Result<Option<u64>, String> {
        let value = self.fat_entry(cluster)?;
        let end = match self.width {
            Width::Fat12 => 0xFF8,
            Width::Fat16 => 0xFFF8,
            Width::Fat32 => 0x0FFF_FFF8,
        };
        Ok((value >= 2 && value < end && value < self.clusters + 2).then_some(value))
    }

    /// A cluster's entry in the first FAT, as it is.
    fn fat_entry(&self, cluster: u64) -> Result<u64, String> {
        // FAT12 packs two entries into three bytes; the others are two or four bytes each.
        Ok(match self.width {
            Width::Fat12 => {
                let b = self.source.read_vec(self.fat_start + cluster + cluster / 2, 2)?;
                let pair = u64::from(u16::from_le_bytes([b[0], b[1]]));
                if cluster.is_multiple_of(2) { pair & 0xFFF } else { pair >> 4 }
            }
            Width::Fat16 => {
                let b = self.source.read_vec(self.fat_start + cluster * 2, 2)?;
                u64::from(u16::from_le_bytes([b[0], b[1]]))
            }
            Width::Fat32 => {
                let b = self.source.read_vec(self.fat_start + cluster * 4, 4)?;
                u64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) & 0x0FFF_FFFF)
            }
        })
    }

    /// A chain of clusters' bytes, up to `limit`. A loop or a chain longer than the disk ends it.
    fn read_chain(&self, first: u64, limit: u64) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        let mut cluster = Some(first).filter(|c| *c >= 2 && *c < self.clusters + 2);
        let mut steps = 0;
        while let Some(c) = cluster {
            if out.len() as u64 >= limit || steps > self.clusters {
                break;
            }
            let take = self.cluster_bytes().min(limit - out.len() as u64);
            out.extend(self.source.read_vec(self.data_start + (c - 2) * self.cluster_bytes(), take as usize)?);
            cluster = self.next_cluster(c)?;
            steps += 1;
        }
        Ok(out)
    }

    fn entries(&self, raw: &[u8]) -> Vec<Entry> {
        let dos = charset::known("cp437");
        let mut out = Vec::new();
        let mut long_parts: Vec<(u8, String)> = Vec::new();
        for record in raw.as_chunks::<32>().0.iter() {
            match record[0] {
                0x00 => break,
                0xE5 => {
                    long_parts.clear();
                    continue;
                }
                _ => {}
            }
            let attributes = record[11];
            if attributes == 0x0F {
                // A long name's pieces come before its short entry, last piece first, 13 UTF-16 units each.
                let units: Vec<u16> = [1..11, 14..26, 28..32].iter().flat_map(|r| record[r.clone()].as_chunks::<2>().0.iter().map(|p| u16::from_le_bytes([p[0], p[1]]))).take_while(|u| *u != 0 && *u != 0xFFFF).collect();
                long_parts.push((record[0] & 0x1F, String::from_utf16_lossy(&units)));
                continue;
            }
            if attributes & 0x08 != 0 {
                long_parts.clear();
                continue;
            }
            let short = |bytes: &[u8], lower: bool| -> String {
                let text: String = bytes.iter().map(|b| crate::ansi_art::glyph(dos, if *b == 0x05 { 0xE5 } else { *b })).collect::<String>().trim_end().to_string();
                if lower { text.to_lowercase() } else { text }
            };
            let (base, extension) = (short(&record[0..8], record[12] & 0x08 != 0), short(&record[8..11], record[12] & 0x10 != 0));
            if base == "." || base == ".." {
                long_parts.clear();
                continue;
            }
            let short_name = if extension.is_empty() { base } else { format!("{base}.{extension}") };
            long_parts.sort_by_key(|(order, _)| *order);
            let long: String = long_parts.drain(..).map(|(_, part)| part).collect();
            let name = tidy_name(if long.is_empty() { &short_name } else { &long });
            let cluster = u64::from(u16::from_le_bytes([record[26], record[27]])) | u64::from(u16::from_le_bytes([record[20], record[21]])) << 16;
            let size = u64::from(u32::from_le_bytes([record[28], record[29], record[30], record[31]]));
            let (time, date) = (u16::from_le_bytes([record[22], record[23]]), u16::from_le_bytes([record[24], record[25]]));
            let detail = format!("{}-{:02}-{:02} {:02}:{:02}", 1980 + (date >> 9), (date >> 5) & 0xF, date & 0x1F, time >> 11, (time >> 5) & 0x3F);
            let mut entry = if attributes & 0x10 != 0 { Entry::dir(name, cluster, 0) } else { Entry::file(name, size, cluster) };
            let flags: String = [(0x01, 'R'), (0x02, 'H'), (0x04, 'S'), (0x20, 'A')].iter().filter(|(bit, _)| attributes & bit != 0).map(|(_, c)| *c).collect();
            entry.kind = if entry.is_dir { "Folder".into() } else if flags.is_empty() { "File".into() } else { format!("File, {flags}") };
            entry.detail = if long.is_empty() || long == short_name { detail } else { format!("{detail}, short name {short_name}") };
            out.push(entry);
        }
        out
    }
}

impl Volume for Fat {
    fn describe(&self) -> String {
        let width = match self.width {
            Width::Fat12 => "FAT12",
            Width::Fat16 => "FAT16",
            Width::Fat32 => "FAT32",
        };
        let kind = if self.kind == "FAT" { String::new() } else { format!("{}, ", self.kind) };
        let part = if self.partition { ", first partition" } else { "" };
        format!("{kind}{width}{part} \u{B7} {}", if self.label.is_empty() { "no label" } else { &self.label })
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        let raw = match dir {
            Some(d) if d.location >= 2 => self.read_chain(d.location, 16 * 1024 * 1024)?,
            // A folder at cluster 0 is the root: a fixed area before the data on FAT12 and FAT16.
            _ if self.width == Width::Fat32 => self.read_chain(self.root_cluster, 16 * 1024 * 1024)?,
            _ => self.source.read_vec(self.root_start, (self.root_entries * 32) as usize)?,
        };
        let mut entries = self.entries(&raw);
        // A file whose first cluster the FAT calls free: its bytes are read as the directory says, which is how the
        // file can be recovered, but they may be cut short or belong to a file written there since.
        for entry in entries.iter_mut().filter(|e| !e.is_dir && e.size > 0 && e.location >= 2) {
            if self.fat_entry(entry.location).is_ok_and(|value| value == 0) {
                entry.detail.push_str(" \u{B7} the FAT marks its clusters free, so it may be incomplete or overwritten");
            }
        }
        entries.sort_by_key(|e| !e.is_dir);
        Ok(entries)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        if entry.size > super::MOST_FILE_BYTES {
            return Err("This file is larger than the 1 GB that can be opened.".into());
        }
        let bytes = self.read_chain(entry.location, entry.size)?;
        if (bytes.len() as u64) < entry.size {
            return Err(format!("Only {} of this file's {} bytes could be found on the disk.", bytes.len(), entry.size));
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{find, names, volume};

    #[test]
    fn fat12_floppies_have_their_long_names_folders_and_files() {
        let disk = volume("fat12.img");
        assert_eq!(disk.describe(), "FAT12 \u{B7} TESTVOL");
        let top = disk.list(None).unwrap();
        assert_eq!(names(&top), ["DEEP", "README.TXT", "A long file name.txt", "binary.dat"], "mdir's order, folders first, NT's lower case kept");
        assert!(find(&top, "A long file name.txt").detail.contains("ALONGF~1.TXT"));
        assert_eq!(disk.read(find(&top, "README.TXT")).unwrap(), b"Hello from a disk image.\r\n");
        let binary = disk.read(find(&top, "binary.dat")).unwrap();
        assert_eq!(binary.len(), 10240);
        assert_eq!(binary[..256], (0..=255u8).collect::<Vec<_>>()[..]);
        let deeper = disk.list(Some(find(&disk.list(Some(find(&top, "DEEP"))).unwrap(), "DEEPER"))).unwrap();
        assert_eq!(disk.read(find(&deeper, "inner.txt")).unwrap(), b"nested\r\n");
    }

    #[test]
    fn fat16_fat32_and_atari_msa_disks_read_too() {
        for (name, describe) in [("fat16.img", "FAT16 \u{B7} SIXTEEN"), ("fat32.img", "FAT32 \u{B7} THIRTYTWO")] {
            let disk = volume(name);
            assert_eq!(disk.describe(), describe);
            let top = disk.list(None).unwrap();
            assert_eq!(disk.read(find(&top, "A long file name.txt")).unwrap(), b"A file with a long name.\r\n", "{name}");
        }
        let st = volume("test.msa");
        assert!(st.describe().starts_with("Atari ST disk (MSA), FAT12"), "{}", st.describe());
        assert_eq!(st.read(find(&st.list(None).unwrap(), "README.TXT")).unwrap(), b"Hello from a disk image.\r\n");
    }
}
