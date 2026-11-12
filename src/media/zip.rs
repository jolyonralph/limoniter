//! ZIP archives, from PKWARE's APPNOTE: the central directory at the end gives every file's name, sizes and where
//! its local header is. Stored and deflated files can be read; others are listed and say why they can't be.
//! Folders are worked out from the names, as many archives don't list them.

use super::{Entry, Source, Volume, tidy_name};
use crate::charset;
use std::sync::Arc;

struct Member {
    path: String,
    method: u16,
    compressed: u64,
    size: u64,
    header: u64,
    encrypted: bool,
    detail: String,
}

pub struct Zip {
    source: Arc<dyn Source>,
    members: Vec<Member>,
    zip64: bool,
}

impl Zip {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let len = source.len();
        let tail_len = len.min(65_536 + 22);
        let tail = source.read_vec(len - tail_len, tail_len as usize)?;
        let end = (0..tail.len().saturating_sub(21)).rev().find(|&i| tail[i..i + 4] == *b"PK\x05\x06").ok_or("This ZIP archive has no central directory at its end.")?;
        let record = &tail[end..];
        let word = |b: &[u8], at: usize| u64::from(u16::from_le_bytes([b[at], b[at + 1]]));
        let long = |b: &[u8], at: usize| u64::from(u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]));
        let mut count = word(record, 10);
        let mut size = long(record, 12);
        let mut start = long(record, 16);
        let mut zip64 = false;
        // A ZIP64 archive's real numbers are in its own end record, found by the locator just before.
        if (count == 0xFFFF || start == 0xFFFF_FFFF) && end >= 20 && tail[end - 20..end - 16] == *b"PK\x06\x07" {
            let at = u64::from_le_bytes(tail[end - 12..end - 4].try_into().unwrap_or([0; 8]));
            let big = source.read_vec(at, 56)?;
            if big[..4] == *b"PK\x06\x06" {
                let quad = |at: usize| u64::from_le_bytes(big[at..at + 8].try_into().unwrap_or([0; 8]));
                (count, size, start) = (quad(32), quad(40), quad(48));
                zip64 = true;
            }
        }
        if start + size > len || size > 256 * 1024 * 1024 {
            return Err("This ZIP archive's directory isn't where it says it is.".into());
        }
        let directory = source.read_vec(start, size as usize)?;
        let dos = charset::known("cp437");
        let mut members = Vec::new();
        let mut at = 0;
        while members.len() < count as usize && at + 46 <= directory.len() && directory[at..at + 4] == *b"PK\x01\x02" {
            let d = &directory[at..];
            let (flags, method) = (word(d, 8), word(d, 10) as u16);
            let (name_len, extra_len, comment_len) = (word(d, 28) as usize, word(d, 30) as usize, word(d, 32) as usize);
            let Some(raw_name) = d.get(46..46 + name_len) else {
                break;
            };
            // Names are UTF-8 when the archive says so, and otherwise the DOS set.
            let path = if flags & 0x800 != 0 { String::from_utf8_lossy(raw_name).into_owned() } else { raw_name.iter().map(|b| crate::ansi_art::glyph(dos, *b)).collect() };
            let (mut compressed, mut size, mut header) = (long(d, 20), long(d, 24), long(d, 42));
            if let Some(extra) = d.get(46 + name_len..46 + name_len + extra_len) {
                let mut e = 0;
                while e + 4 <= extra.len() {
                    let (id, n) = (word(extra, e), word(extra, e + 2) as usize);
                    if id == 1 {
                        let mut field = extra.get(e + 4..e + 4 + n).unwrap_or(&[]).as_chunks::<8>().0.iter().map(|c| u64::from_le_bytes(*c));
                        if size == 0xFFFF_FFFF {
                            size = field.next().unwrap_or(size);
                        }
                        if compressed == 0xFFFF_FFFF {
                            compressed = field.next().unwrap_or(compressed);
                        }
                        if header == 0xFFFF_FFFF {
                            header = field.next().unwrap_or(header);
                        }
                    }
                    e += 4 + n;
                }
            }
            let (time, date) = (word(d, 12), word(d, 14));
            let detail = format!("{}-{:02}-{:02} {:02}:{:02}", 1980 + (date >> 9), (date >> 5) & 0xF, date & 0x1F, time >> 11, (time >> 5) & 0x3F);
            members.push(Member { path: path.replace('\\', "/"), method, compressed, size, header, encrypted: flags & 1 != 0, detail });
            at += 46 + name_len + extra_len + comment_len;
        }
        Ok(Self { source, members, zip64 })
    }
}

impl Volume for Zip {
    fn describe(&self) -> String {
        format!("ZIP archive{} \u{B7} {} file(s)", if self.zip64 { " (ZIP64)" } else { "" }, self.members.iter().filter(|m| !m.path.ends_with('/')).count())
    }

    /// A folder is the members whose path starts with its own; the folders inside it come from the paths too.
    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        let prefix = dir.map_or(String::new(), |d| format!("{}/", self.folder_path(d)));
        let mut folders: Vec<String> = Vec::new();
        let mut files = Vec::new();
        for (index, member) in self.members.iter().enumerate() {
            let Some(rest) = member.path.strip_prefix(&prefix) else {
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
                    entry.kind = match (member.encrypted, member.method) {
                        (true, _) => "Encrypted".into(),
                        (_, 0) => "Stored".into(),
                        (_, 8) => "Deflated".into(),
                        (_, other) => format!("Method {other}"),
                    };
                    entry.detail = member.detail.clone();
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
        Ok(out)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let member = self.members.get(entry.location as usize).ok_or("That file isn't in the archive.")?;
        if member.encrypted {
            return Err("This file is encrypted, and Limoniter doesn't take passwords.".into());
        }
        if member.size > super::MOST_FILE_BYTES || member.compressed > super::MOST_FILE_BYTES {
            return Err("This file is larger than the 1 GB that can be opened.".into());
        }
        let local = self.source.read_vec(member.header, 30)?;
        if local[..4] != *b"PK\x03\x04" {
            return Err("The archive's directory points somewhere that isn't a file.".into());
        }
        let skip = 30 + u64::from(u16::from_le_bytes([local[26], local[27]])) + u64::from(u16::from_le_bytes([local[28], local[29]]));
        let data = self.source.read_vec(member.header + skip, member.compressed as usize)?;
        match member.method {
            0 => Ok(data),
            8 => {
                let out = miniz_oxide::inflate::decompress_to_vec_with_limit(&data, member.size as usize).map_err(|e| format!("The file couldn't be unpacked: {e:?}"))?;
                if out.len() as u64 != member.size {
                    return Err("The file unpacked to a different size than the archive says.".into());
                }
                Ok(out)
            }
            other => Err(format!("This file is packed with method {other}; Limoniter reads stored and deflated files.")),
        }
    }
}

impl Zip {
    /// A folder entry keeps its whole path in its open name.
    fn folder_path(&self, entry: &Entry) -> String {
        entry.open_name.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{find, fixture, names, volume};
    use super::super::{Volume, open};
    use std::sync::Arc;

    #[test]
    fn a_zip_lists_its_folders_and_unpacks_its_files() {
        let zip = volume("nested.zip");
        let top = zip.list(None).unwrap();
        assert_eq!(names(&top), ["disks"]);
        let inside = zip.list(Some(find(&top, "disks"))).unwrap();
        let disk = zip.read(find(&inside, "test.d64")).unwrap();
        assert_eq!(disk, fixture("test.d64"), "it unpacks to the same bytes");
        // And what's in it opens in turn.
        let d64: Box<dyn Volume> = open(Arc::new(disk), "test.d64").unwrap();
        assert_eq!(names(&d64.list(None).unwrap()), ["HELLO", "NOTES"]);
    }
}
