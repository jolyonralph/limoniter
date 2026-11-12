//! CD images: ISO 9660 (ECMA-119), with the Joliet extension's Unicode names and Rock Ridge's POSIX names where
//! the disc has them, and raw CD images (.bin) whose 2352-byte sectors hold 2048 bytes of data each.

use super::{Entry, Source, Volume, tidy_name};
use std::sync::Arc;

const SECTOR: u64 = 2048;

/// A raw CD image read as its 2048-byte data sectors: Mode 1 sectors have their data 16 bytes in, Mode 2 Form 1's 24.
pub struct RawCd {
    inner: Arc<dyn Source>,
    skip: u64,
}

impl RawCd {
    pub fn new(inner: Arc<dyn Source>) -> Result<Self, String> {
        let mode = inner.read_vec(15, 1)?[0];
        let skip = match mode {
            1 => 16,
            2 => 24,
            _ => return Err(format!("This raw CD image's first sector is mode {mode}, which isn't a data sector.")),
        };
        Ok(Self { inner, skip })
    }
}

impl Source for RawCd {
    fn len(&self) -> u64 {
        self.inner.len() / 2352 * SECTOR
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        let mut done = 0;
        while done < buf.len() {
            let at = offset + done as u64;
            let (sector, within) = (at / SECTOR, at % SECTOR);
            let take = ((SECTOR - within) as usize).min(buf.len() - done);
            self.inner.read_at(sector * 2352 + self.skip + within, &mut buf[done..done + take])?;
            done += take;
        }
        Ok(())
    }
}

pub struct Iso {
    source: Arc<dyn Source>,
    kind: &'static str,
    label: String,
    /// The root directory's sector and length, from the descriptor whose names are best.
    root: (u64, u64),
    joliet: bool,
    rock_ridge: bool,
}

/// A directory record's fields.
struct Record {
    sector: u64,
    length: u64,
    is_dir: bool,
    name: String,
    date: String,
}

impl Iso {
    pub fn open(source: Arc<dyn Source>, kind: &'static str) -> Result<Self, String> {
        let mut primary = None;
        let mut joliet = None;
        for index in 16..64u64 {
            let Ok(descriptor) = source.read_vec(index * SECTOR, SECTOR as usize) else {
                break;
            };
            if &descriptor[1..6] != b"CD001" {
                break;
            }
            let root = (u64::from(le32(&descriptor[156 + 2..])), u64::from(le32(&descriptor[156 + 10..])));
            match descriptor[0] {
                1 => primary = Some((root, String::from_utf8_lossy(&descriptor[40..72]).trim().to_string())),
                // A supplementary descriptor with one of these escape sequences is Joliet, with UCS-2 names.
                2 if matches!(&descriptor[88..91], b"%/@" | b"%/C" | b"%/E") => joliet = Some((root, ucs2(&descriptor[40..72]).trim().to_string())),
                255 => break,
                _ => {}
            }
        }
        let Some((root, label)) = primary else {
            return Err("This CD image has no primary volume descriptor.".into());
        };
        let mut iso = Self { source, kind, label, root, joliet: false, rock_ridge: false };
        // Rock Ridge names are as good as Joliet's and keep case; it's found from the root's first record.
        iso.rock_ridge = iso.has_rock_ridge();
        if !iso.rock_ridge
            && let Some((joliet_root, joliet_label)) = joliet
        {
            iso.root = joliet_root;
            iso.joliet = true;
            if !joliet_label.is_empty() {
                iso.label = joliet_label;
            }
        }
        Ok(iso)
    }

    fn has_rock_ridge(&self) -> bool {
        let Ok(sector) = self.source.read_vec(self.root.0 * SECTOR, SECTOR as usize) else {
            return false;
        };
        let length = usize::from(sector[0]);
        if length < 34 || length > sector.len() {
            return false;
        }
        let name_length = usize::from(sector[32]);
        let system_use = 33 + name_length + (1 - name_length % 2);
        sector.get(system_use..length).is_some_and(|area| area.windows(2).any(|w| w == b"SP"))
    }

    fn records(&self, sector: u64, length: u64) -> Result<Vec<Record>, String> {
        let length = length.min(64 * 1024 * 1024);
        let data = self.source.read_vec(sector * SECTOR, length as usize)?;
        let mut out = Vec::new();
        let mut at = 0;
        while at < data.len() {
            let size = usize::from(data[at]);
            if size == 0 {
                // Records don't cross sectors: a zero length means the rest of this sector is empty.
                at = (at / SECTOR as usize + 1) * SECTOR as usize;
                continue;
            }
            let Some(record) = data.get(at..at + size).filter(|r| r.len() >= 34) else {
                break;
            };
            at += size;
            let name_length = usize::from(record[32]);
            let Some(raw_name) = record.get(33..33 + name_length) else {
                continue;
            };
            // The first two records are the directory itself and its parent.
            if raw_name == [0] || raw_name == [1] {
                continue;
            }
            let mut name = if self.joliet { ucs2(raw_name) } else { String::from_utf8_lossy(raw_name).into_owned() };
            if self.rock_ridge {
                let system_use = 33 + name_length + (1 - name_length % 2);
                if let Some(alternate) = record.get(system_use..).and_then(rock_ridge_name) {
                    name = alternate;
                }
            } else {
                // ISO 9660 names end with a version number, and a name with no extension with a full stop.
                if let Some(cut) = name.rfind(';') {
                    name.truncate(cut);
                }
                if name.ends_with('.') {
                    name.pop();
                }
            }
            let d = &record[18..25];
            let date = format!("{}-{:02}-{:02} {:02}:{:02}", 1900 + u32::from(d[0]), d[1], d[2], d[3], d[4]);
            out.push(Record { sector: u64::from(le32(&record[2..])), length: u64::from(le32(&record[10..])), is_dir: record[25] & 0x02 != 0, name: tidy_name(&name), date });
        }
        Ok(out)
    }
}

/// Rock Ridge's NM entries, joined, from a directory record's system use area.
fn rock_ridge_name(area: &[u8]) -> Option<String> {
    let mut name = Vec::new();
    let mut at = 0;
    while at + 4 <= area.len() {
        let (signature, length) = (&area[at..at + 2], usize::from(area[at + 2]));
        if length < 4 || at + length > area.len() {
            break;
        }
        if signature == b"NM" && length > 5 {
            name.extend_from_slice(&area[at + 5..at + length]);
        }
        at += length;
    }
    (!name.is_empty()).then(|| String::from_utf8_lossy(&name).into_owned())
}

fn le32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn ucs2(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|p| u16::from_be_bytes([p[0], p[1]])).collect();
    String::from_utf16_lossy(&units)
}

impl Volume for Iso {
    fn describe(&self) -> String {
        let names = if self.rock_ridge {
            ", Rock Ridge names"
        } else if self.joliet {
            ", Joliet names"
        } else {
            ""
        };
        format!("{}{} \u{B7} {}", self.kind, names, if self.label.is_empty() { "no label" } else { &self.label })
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        let (sector, length) = dir.map_or(self.root, |d| (d.location, d.extra));
        let mut entries: Vec<Entry> = self
            .records(sector, length)?
            .into_iter()
            .map(|r| {
                let mut entry = if r.is_dir { Entry::dir(r.name, r.sector, r.length) } else { Entry::file(r.name, r.length, r.sector) };
                entry.detail = r.date;
                if !entry.is_dir {
                    entry.kind = "File".into();
                }
                entry
            })
            .collect();
        entries.sort_by_key(|e| !e.is_dir);
        Ok(entries)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        if entry.size > super::MOST_FILE_BYTES {
            return Err("This file is larger than the 1 GB that can be opened.".into());
        }
        self.source.read_vec(entry.location * SECTOR, entry.size as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{find, names, volume};

    #[test]
    fn a_cd_image_lists_its_folders_and_files_with_their_long_names() {
        let cd = volume("test.iso");
        assert!(cd.describe().contains("TESTCD") && cd.describe().contains("Rock Ridge"), "{}", cd.describe());
        let top = cd.list(None).unwrap();
        assert_eq!(names(&top)[0], "Deep", "folders first");
        assert!(names(&top).contains(&"A long file name.txt".to_string()), "{:?}", names(&top));
        assert_eq!(cd.read(find(&top, "README.TXT")).unwrap(), b"Hello from a disk image.\r\n");
        let deeper = cd.list(Some(find(&cd.list(Some(find(&top, "Deep"))).unwrap(), "Deeper"))).unwrap();
        assert_eq!(cd.read(find(&deeper, "inner.txt")).unwrap(), b"nested\r\n");
        assert_eq!(cd.read(find(&top, "binary.dat")).unwrap().len(), 10240);
    }

    #[test]
    fn a_plain_iso_has_its_short_names_and_a_raw_one_reads_the_same() {
        let plain = volume("plain.iso");
        let top = plain.list(None).unwrap();
        assert!(names(&top).contains(&"README.TXT".to_string()), "{:?}", names(&top));
        assert!(names(&top).iter().any(|n| n.len() <= 12 && n.starts_with('A')), "the long name is cut to 8.3 there: {:?}", names(&top));
        let raw = volume("test.bin");
        assert!(raw.describe().contains("raw CD"));
        assert_eq!(raw.read(find(&raw.list(None).unwrap(), "README.TXT")).unwrap(), b"Hello from a disk image.\r\n");
    }
}
