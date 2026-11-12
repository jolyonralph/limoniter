//! The TRS-80's disk operating systems, as their technical references lay out the directory: TRSDOS 2.3 on the Model
//! I, and the family that grew from it: LDOS and LS-DOS, TRSDOS 6 on the Model 4, NEWDOS/80 and DOSPLUS.
//!
//! The boot sector's third byte is the directory's cylinder. Its first sector is the Granule Allocation Table, its
//! second the Hash Index Table: a byte for each directory entry, 0 if unused, otherwise a hash of its name. An
//! entry's place is its index in the HIT: the low 5 bits are the sector after the HIT, the top 3 the entry within it.
//! Entries are 32 bytes (48 on TRSDOS 2.3): attributes (bit 4 in use, bit 7 an extension of another entry, bit 6 a
//! system file, bit 3 invisible), the last sector's byte count, the record length, an 8.3 name, two password
//! hashes, the number of sectors, then the extents: a cylinder and a byte whose top 3 bits are the first granule on
//! it and the rest one less than how many follow, the run going on onto the next cylinders. &FF ends them; &FE leads
//! to an extension entry.
//!
//! A granule is a run of sectors on a track: 5 of a 10-sector single-density track's, 6 of an 18-sector
//! double-density one's. On a two-sided disk the second side's granules follow the first's.
//!
//! TRSDOS 1.3 on the Model III differs: its boot sector starts &FE then the directory's cylinder, its 48-byte entries
//! fill the sectors after the HIT in the HIT's order, a granule is 3 sectors, an extent is a starting granule and
//! how many (not one less), and a file's length is its count of whole sectors and then the last one's bytes. A
//! length longer than the granules a file was given is cut to what they hold.

use super::{Entry, Source, Volume, tidy_name};
use std::sync::Arc;

pub struct Trsdos {
    source: Arc<dyn Source>,
    per_track: u64,
    heads: u64,
    granules_per_track: u64,
    files: Vec<File>,
    what: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    /// LDOS, LS-DOS, TRSDOS 6, NEWDOS/80, DOSPLUS: 32-byte entries.
    Ldos,
    /// TRSDOS 2.3 on the Model I: 48-byte entries, placed as LDOS's are.
    Model1,
    /// TRSDOS 1.3 on the Model III.
    Model3,
}

impl Variant {
    fn entry_size(self) -> usize {
        if self == Variant::Ldos { 32 } else { 48 }
    }
}

struct File {
    name: String,
    attributes: u8,
    size: u64,
    /// Each run: cylinder, first granule on it, how many granules.
    extents: Vec<(u64, u64, u64)>,
}

impl Trsdos {
    fn sector(&self, cylinder: u64, index: u64) -> Option<Vec<u8>> {
        // A cylinder's sectors: the first side's, then the second's.
        let at = (cylinder * self.heads * self.per_track + index) * 256;
        (at + 256 <= self.source.len()).then(|| self.source.read_vec(at, 256).ok()).flatten()
    }

    pub fn open(source: Arc<dyn Source>, shape: super::cpm::Shape) -> Result<Self, String> {
        if shape.sector_size != 256 || !(10..=20).contains(&shape.per_track) {
            return Err("not a TRS-80 disk's tracks".into());
        }
        let probe = Self { source: source.clone(), per_track: shape.per_track, heads: shape.heads, granules_per_track: 2, files: Vec::new(), what: String::new() };
        let cylinders = source.len() / (256 * shape.per_track * shape.heads);
        let boot = probe.sector(0, 0).ok_or("no boot sector")?;
        let mut tried = Vec::new();
        // The Model III's boot sector starts &FE, then the directory's cylinder; the Model I's and LDOS's &00 &FE.
        let variants: &[Variant] = if boot[0] == 0xFE { &[Variant::Model3] } else { &[Variant::Ldos, Variant::Model1] };
        let named = if boot[0] == 0xFE { boot[1] } else { boot[2] };
        for directory in [u64::from(named), 17, 20] {
            if directory == 0 || directory >= cylinders || tried.contains(&directory) {
                continue;
            }
            tried.push(directory);
            let (Some(gat), Some(hit)) = (probe.sector(directory, 0), probe.sector(directory, 1)) else {
                continue;
            };
            for &variant in variants {
                let entry_size = variant.entry_size();
                if let Some(mut trs) = Self::read_directory(&source, shape, directory, variant, &gat, &hit) {
                    let dos = match variant {
                        Variant::Ldos => "LDOS, TRSDOS 6 or NEWDOS",
                        Variant::Model1 => "TRSDOS 2.3",
                        Variant::Model3 => "Model III TRSDOS 1.3",
                    };
                    let name: String = gat[0xD0..0xD8].iter().map(|&b| char::from(b & 0x7F)).filter(|c| !c.is_control()).collect::<String>().trim().to_string();
                    let density = if shape.per_track >= 18 { "double density" } else { "single density" };
                    let sides = if shape.heads == 2 { ", two sides" } else { "" };
                    let named = if entry_size == 32 && !name.is_empty() && name.chars().all(|c| c.is_ascii_graphic() || c == ' ') { format!(" \"{name}\"") } else { String::new() };
                    trs.what = format!("TRS-80 {dos} disk{named} ({cylinders} cylinders, {density}{sides})");
                    return Ok(trs);
                }
            }
        }
        Err("No TRS-80 directory was found on this disk.".into())
    }

    fn read_directory(source: &Arc<dyn Source>, shape: super::cpm::Shape, directory: u64, variant: Variant, gat: &[u8], hit: &[u8]) -> Option<Self> {
        let entry_size = variant.entry_size();
        let per_sector = 256 / entry_size;
        // LDOS keeps granules per track less one in the GAT's configuration byte; the Model III's are 3 sectors; others
        // are worked out.
        let from_gat = u64::from(gat[0xCD] & 7) + 1;
        let granules_per_track = if variant == Variant::Model3 {
            shape.per_track / 3
        } else if variant == Variant::Ldos && from_gat >= 2 && shape.per_track.is_multiple_of(from_gat) && (5..=10).contains(&(shape.per_track / from_gat)) {
            from_gat
        } else {
            [2, 3, 4].into_iter().find(|g| shape.per_track.is_multiple_of(*g) && (5..=8).contains(&(shape.per_track / g)))?
        };
        let mut trs = Self { source: source.clone(), per_track: shape.per_track, heads: shape.heads, granules_per_track, files: Vec::new(), what: String::new() };
        let entry_at = |dec: usize| -> Option<Vec<u8>> {
            let (index, slot) = if variant == Variant::Model3 { (dec / per_sector, dec % per_sector) } else { (dec & 0x1F, dec >> 5) };
            // Only the directory cylinder's own sectors hold entries.
            if slot >= per_sector || index as u64 + 2 >= shape.per_track * shape.heads {
                return None;
            }
            let data = trs.sector(directory, index as u64 + 2)?;
            Some(data[slot * entry_size..(slot + 1) * entry_size].to_vec())
        };
        let (mut good, mut bad) = (0, 0);
        let mut files = Vec::new();
        // The Model III's HIT has room for more than its directory holds; the rest of it isn't entries.
        let capacity = if variant == Variant::Model3 { (shape.per_track * shape.heads - 2) as usize * per_sector } else { hit.len() };
        for (dec, &hash) in hit.iter().enumerate().take(capacity) {
            if hash == 0 {
                continue;
            }
            let Some(entry) = entry_at(dec) else {
                bad += 1;
                continue;
            };
            let attributes = entry[0];
            // Not in use, or an extension of another entry: only a file's first entry starts it.
            if attributes & 0x10 == 0 || attributes & 0x80 != 0 {
                continue;
            }
            let text = |range: std::ops::Range<usize>| entry[range].iter().map(|&b| char::from(b)).collect::<String>().trim_end().to_string();
            let (stem, extension) = (text(5..13), text(13..16));
            if !stem.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) || !stem.chars().chain(extension.chars()).all(|c| c.is_ascii_alphanumeric()) {
                bad += 1;
                continue;
            }
            let records = u64::from(u16::from_le_bytes([entry[20], entry[21]]));
            let end = u64::from(entry[3]);
            // LDOS counts the last sector, part-filled or not; TRSDOS 1.3 counts the whole sectors before it.
            let size = if variant == Variant::Model3 {
                records * 256 + end
            } else if end == 0 {
                records * 256
            } else {
                records.saturating_sub(1) * 256 + end
            };
            // The extents, following extension entries where they lead.
            let mut extents = Vec::new();
            let mut current = entry.clone();
            let mut hops = 0;
            'chain: loop {
                for pair in current[22..entry_size].as_chunks::<2>().0 {
                    match pair[0] {
                        0xFF => break 'chain,
                        0xFE => {
                            hops += 1;
                            match entry_at(usize::from(pair[1])).filter(|_| hops < 16) {
                                Some(next) => {
                                    current = next;
                                    continue 'chain;
                                }
                                None => break 'chain,
                            }
                        }
                        cylinder => extents.push((u64::from(cylinder), u64::from(pair[1] >> 5), u64::from(pair[1] & 0x1F) + u64::from(variant != Variant::Model3))),
                    }
                }
                break;
            }
            good += 1;
            files.push(File { name: if extension.is_empty() { stem } else { format!("{stem}/{extension}") }, attributes, size, extents });
        }
        if good == 0 || bad * 4 > good {
            return None;
        }
        trs.files = files;
        Some(trs)
    }

    fn read_file(&self, file: &File) -> Result<Vec<u8>, String> {
        let per_granule = self.per_track / self.granules_per_track;
        let per_cylinder = self.granules_per_track * self.heads;
        let mut out = Vec::with_capacity(file.size as usize);
        for &(start_cylinder, first, count) in &file.extents {
            for granule in first..first + count {
                // A run goes on onto the next cylinder; a granule on the second side follows the first side's.
                let (cylinder, granule) = (start_cylinder + granule / per_cylinder, granule % per_cylinder);
                let (side, on_track) = (granule / self.granules_per_track, granule % self.granules_per_track);
                for n in 0..per_granule {
                    if out.len() as u64 >= file.size {
                        break;
                    }
                    let sector = self.sector(cylinder, side * self.per_track + on_track * per_granule + n).ok_or_else(|| format!("cylinder {cylinder} isn't on the disk"))?;
                    out.extend(sector);
                }
            }
        }
        out.truncate(file.size as usize);
        Ok(out)
    }
}

impl Volume for Trsdos {
    fn describe(&self) -> String {
        self.what.clone()
    }

    fn list(&self, _dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        Ok(self
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                // TRS-80 names are NAME/EXT; opened, the slash becomes a dot so the ending is understood.
                let mut entry = Entry::file(tidy_name(&file.name), file.size, index as u64);
                entry.open_name = tidy_name(&file.name.replace('/', "."));
                let mut kind = vec!["File"];
                if file.attributes & 0x40 != 0 {
                    kind.push("system");
                }
                if file.attributes & 0x08 != 0 {
                    kind.push("invisible");
                }
                entry.kind = kind.join(", ");
                entry
            })
            .collect())
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let file = self.files.get(entry.location as usize).ok_or("That file isn't on the disk.")?;
        self.read_file(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Model I JV1 disk with LDOS's layout: the directory on cylinder 17, one file of 3 bytes whose single granule
    /// is the last on cylinder 5, and a second file whose run of two granules goes on to cylinder 6.
    #[test]
    fn a_jv1_disk_is_read_through_its_hit_and_granules() {
        let mut disk = vec![0u8; 35 * 2560];
        let sector = |cylinder: usize, index: usize| (cylinder * 10 + index) * 256;
        disk[..3].copy_from_slice(&[0x00, 0xFE, 17]);
        disk[sector(17, 0) + 0xCD] = 0x01; // two granules a track
        disk[sector(17, 1)] = 0x55;
        disk[sector(17, 1) + 0x20] = 0x66; // the second entry: sector 2, slot 1
        let entry = |disk: &mut Vec<u8>, at: usize, name: &[u8; 11], end: u8, records: u16, extent: [u8; 2]| {
            disk[at] = 0x10;
            disk[at + 3] = end;
            disk[at + 5..at + 16].copy_from_slice(name);
            disk[at + 20..at + 22].copy_from_slice(&records.to_le_bytes());
            disk[at + 22..at + 24].copy_from_slice(&extent);
            disk[at + 24] = 0xFF;
        };
        entry(&mut disk, sector(17, 2), b"HELLO   TXT", 3, 1, [5, 0x20]);
        entry(&mut disk, sector(17, 2) + 32, b"LONG    DAT", 0, 6, [5, 0x21]);
        disk[sector(5, 5)..sector(5, 5) + 3].copy_from_slice(b"Hi!");
        disk[sector(6, 0)] = 0x77;
        let image = super::super::open(Arc::new(disk), "model1.dsk").unwrap();
        assert!(image.describe().contains("JV1") && image.describe().contains("TRS-80"), "{}", image.describe());
        let files = image.list(None).unwrap();
        assert_eq!(files[0].name, "HELLO_TXT");
        assert_eq!(files[0].open_name, "HELLO.TXT");
        assert_eq!(image.read(&files[0]).unwrap(), b"Hi!");
        let long = image.read(&files[1]).unwrap();
        assert_eq!(long.len(), 6 * 256);
        assert_eq!(long[5 * 256], 0x77, "the run's second granule is the next cylinder's first");
    }
}
