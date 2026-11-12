//! LHA and LZH archives, the Amiga's usual archive (Aminet's) and common on DOS and in Japan. The headers (levels 0
//! to 3) and the unpacking (-lh0- to -lh7-, LArc's -lzs- and -lz5-, PMarc's -pm0- to -pm2-, and -lh1-) are the
//! `delharc` crate's; each file's CRC is checked once it's unpacked. Folders are worked out from the paths, as in ZIP.

use super::{Entry, MOST_FILE_BYTES, Source, Volume, tidy_name};
use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;

/// A source read in order, as the crate reads an archive.
struct Reader {
    source: Arc<dyn Source>,
    at: u64,
}

impl Read for Reader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let count = (buf.len() as u64).min(self.source.len().saturating_sub(self.at)) as usize;
        self.source.read_at(self.at, &mut buf[..count]).map_err(std::io::Error::other)?;
        self.at += count as u64;
        Ok(count)
    }
}

impl Seek for Reader {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        let at = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::Current(by) => self.at.checked_add_signed(by),
            SeekFrom::End(by) => self.source.len().checked_add_signed(by),
        };
        self.at = at.ok_or_else(|| std::io::Error::other("seek before the start"))?;
        Ok(self.at)
    }
}

struct Member {
    path: String,
    size: u64,
    method: String,
    date: String,
    supported: bool,
    /// Packed on an Amiga, as its header says. Level 0 headers don't say, and DOS and Unix archives say otherwise.
    amiga: bool,
}

pub struct Lha {
    source: Arc<dyn Source>,
    members: Vec<Member>,
}

/// Whether bytes start like an LHA archive: a method such as `-lh5-` after the first two header bytes.
pub fn looks_like_lha(head: &[u8]) -> bool {
    head.len() > 21 && head[2] == b'-' && head[6] == b'-' && matches!(&head[3..5], b"lh" | b"lz" | b"pm")
}

impl Lha {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let mut reader = delharc::LhaDecodeReader::new(Reader { source: source.clone(), at: 0 }).map_err(|e| format!("This LHA archive's first header can't be read: {e}"))?;
        let mut members = Vec::new();
        loop {
            let header = reader.header();
            let mut path = header.parse_pathname_to_str().replace('\\', "/");
            if header.is_directory() && !path.ends_with('/') {
                path.push('/');
            }
            members.push(Member {
                path,
                size: header.original_size,
                method: String::from_utf8_lossy(&header.compression).into_owned(),
                date: header.parse_last_modified().to_string(),
                supported: reader.is_decoder_supported(),
                amiga: header.level > 0 && header.os_type == b'A',
            });
            match reader.seek_next_file() {
                Ok(true) => {}
                Ok(false) => break,
                Err(e) => return Err(format!("This LHA archive is damaged after its first {} file(s): {e}", members.len())),
            }
            if members.len() > 1_000_000 {
                return Err("This LHA archive has more files than Limoniter lists.".into());
            }
        }
        Ok(Self { source, members })
    }

    fn folder_list(&self, prefix: &str) -> Vec<Entry> {
        let mut folders: Vec<String> = Vec::new();
        let mut files = Vec::new();
        for (index, member) in self.members.iter().enumerate() {
            let Some(rest) = member.path.strip_prefix(prefix) else {
                continue;
            };
            match rest.split_once('/') {
                Some((folder, _)) if !folder.is_empty() => {
                    if !folders.iter().any(|f| f == folder) {
                        folders.push(folder.to_string());
                    }
                }
                None if !rest.is_empty() => {
                    let mut entry = Entry::file(tidy_name(rest), member.size, index as u64);
                    entry.kind = if member.supported { member.method.clone() } else { format!("{} (not unpacked)", member.method) };
                    entry.detail = member.date.clone();
                    files.push(entry);
                }
                _ => {}
            }
        }
        let mut out: Vec<Entry> = folders
            .into_iter()
            .map(|folder| {
                let mut entry = Entry::dir(tidy_name(&folder), 0, 0);
                entry.open_name = format!("{prefix}{folder}");
                entry
            })
            .collect();
        out.extend(files);
        out
    }
}

impl Volume for Lha {
    fn describe(&self) -> String {
        format!("LHA archive \u{B7} {} file(s)", self.members.iter().filter(|m| !m.path.ends_with('/')).count())
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        Ok(self.folder_list(&dir.map_or(String::new(), |d| format!("{}/", d.open_name))))
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let index = entry.location as usize;
        let member = self.members.get(index).ok_or("That file isn't in the archive.")?;
        if member.size > MOST_FILE_BYTES {
            return Err("This file is larger than the 1 GB that can be opened.".into());
        }
        if !member.supported {
            return Err(format!("This file is packed with {}, which Limoniter can't unpack.", member.method));
        }
        let mut reader = delharc::LhaDecodeReader::new(Reader { source: self.source.clone(), at: 0 }).map_err(|e| e.to_string())?;
        for _ in 0..index {
            if !reader.seek_next_file().map_err(|e| e.to_string())? {
                return Err("The archive ended before that file.".into());
            }
        }
        let mut out = Vec::with_capacity(member.size as usize);
        reader.read_to_end(&mut out).map_err(|e| format!("The file couldn't be unpacked: {e}"))?;
        reader.crc_check().map_err(|e| format!("The file unpacked, but its CRC doesn't match, so it's damaged: {e}"))?;
        Ok(out)
    }

    fn text_preset(&self, entry: &Entry) -> Option<&'static str> {
        self.members.get(entry.location as usize).filter(|m| m.amiga).map(|_| super::AMIGA_TEXT_PRESET)
    }
}

#[cfg(test)]
pub mod tests {
    use super::super::tests::{find, names};
    use super::super::open;
    use std::sync::Arc;

    /// CRC-16 as LHA uses it (the ARC polynomial, reflected).
    fn crc16(data: &[u8]) -> u16 {
        data.iter().fold(0u16, |crc, b| (0..8).fold(crc ^ u16::from(*b), |c, _| if c & 1 != 0 { (c >> 1) ^ 0xA001 } else { c >> 1 }))
    }

    /// A level 0 header and stored (-lh0-) data, as LHarc writes them.
    pub fn stored(path: &str, data: &[u8]) -> Vec<u8> {
        let mut header = b"-lh0-".to_vec();
        header.extend((data.len() as u32).to_le_bytes());
        header.extend((data.len() as u32).to_le_bytes());
        header.extend(0x5A2B_6000u32.to_le_bytes());
        header.extend([0x20, 0, path.len() as u8]);
        header.extend(path.as_bytes());
        header.extend(crc16(data).to_le_bytes());
        let sum = header.iter().fold(0u8, |s, b| s.wrapping_add(*b));
        let mut out = vec![header.len() as u8, sum];
        out.extend(header);
        out.extend(data);
        out
    }

    #[test]
    fn an_lha_archive_lists_its_folders_and_unpacks_its_files() {
        let mut archive = stored("readme.txt", b"Hello from LHA\n");
        archive.extend(stored("s\\startup-sequence", b"echo hi\n"));
        archive.push(0);
        let lha = open(Arc::new(archive.clone()), "test.lha").unwrap();
        assert_eq!(lha.describe(), "LHA archive \u{B7} 2 file(s)");
        let top = lha.list(None).unwrap();
        assert_eq!(names(&top), ["s", "readme.txt"]);
        assert_eq!(lha.read(find(&top, "readme.txt")).unwrap(), b"Hello from LHA\n");
        let s = lha.list(Some(find(&top, "s"))).unwrap();
        assert_eq!(lha.read(find(&s, "startup-sequence")).unwrap(), b"echo hi\n");
        // A damaged file is caught by its CRC.
        let at = archive.windows(5).position(|w| w == b"Hello").unwrap();
        archive[at] = b'J';
        let damaged = open(Arc::new(archive), "test.lha").unwrap();
        assert!(damaged.read(&damaged.list(None).unwrap()[1]).unwrap_err().contains("CRC"));
    }
}
