//! Compact Pro archives (`.cpt`), Bill Goodman's rival to StuffIt from 1990: a byte 1, the volume number, two more
//! bytes and the offset of the directory, which is at the end. The directory is a CRC-32 of itself (zip's, but not
//! inverted at the end; the files' CRCs are kept the same way), the number of
//! entries, a comment, then the entries: a name's length (with &80 added for a folder) and the name; a folder then
//! gives how many entries are inside it, all levels down, and they follow; a file gives its volume, where its forks
//! are, its type, creator, dates, Finder flags, a CRC-32 of its forks, its flags (1 encrypted, 2 the resource fork
//! packed with LZH, 4 the data fork), and each fork's length unpacked and packed. The resource fork comes first.
//!
//! Every fork is run-length coded, with &81 as the escape: &81 &82 n repeats the byte before n-1 times in all, &81
//! &82 0 is &81 &82 itself, and &81 with anything else is itself. Over that, a fork may be packed by LZH: blocks of
//! three prefix codes (literals, match lengths, the top bits of match distances), each given as a count of bytes
//! and then a code length in each half-byte, then codes in an 8K window: a 1 bit and a literal, or a 0 bit, a
//! length, and a distance's top bits from its code and bottom six as they are. A block ends after it has counted
//! &1FFF0 (2 a literal, 3 a match), and the next starts on a byte two or three bytes on, to make up the count to
//! an even number. This follows The Unarchiver's reading of the format (XADMaster's XADCompactPro classes).

use super::super::mac::{self, MacFile, RESOURCE_FORK};
use super::super::{Entry, MOST_FILE_BYTES, Source, Volume, tidy_name};
use super::{Bits, Code, crc32_byte};
use std::sync::Arc;

struct Member {
    file: MacFile,
    parent: Option<usize>,
    folder: bool,
    /// Where the resource fork's packed bytes start; the data fork's follow them.
    at: u64,
    packed: (u64, u64),
    flags: u16,
    crc: u32,
}

pub struct CompactPro {
    source: Arc<dyn Source>,
    members: Vec<Member>,
    notes: Vec<String>,
}

/// The directory of a Compact Pro archive, if the start's offset leads to one whose CRC-32 is right: the number of
/// entries and where the first is, and the directory's whole bytes.
fn directory(source: &dyn Source) -> Option<(usize, usize, Vec<u8>)> {
    let head = source.read_vec(0, 8).ok()?;
    let offset = u64::from(u32::from_be_bytes([head[4], head[5], head[6], head[7]]));
    if head[0] != 1 || offset < 8 || offset + 7 > source.len() {
        return None;
    }
    let bytes = source.read_vec(offset, (source.len() - offset).min(4 * 1024 * 1024) as usize).ok()?;
    let count = usize::from(u16::from_be_bytes([bytes[4], bytes[5]]));
    // The CRC covers the count, the comment and each entry, as far as the entries go.
    let mut at = 7 + usize::from(bytes[6]);
    for _ in 0..count {
        let length = *bytes.get(at)?;
        at += 1 + usize::from(length & 0x7F) + if length & 0x80 != 0 { 2 } else { 45 };
    }
    let crc = bytes.get(4..at)?.iter().fold(0xFFFF_FFFF, |crc, &b| crc32_byte(crc, b));
    (crc == u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])).then(|| (count, 7 + usize::from(bytes[6]), bytes))
}

impl CompactPro {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let (count, first, bytes) = directory(&*source).ok_or("This isn't a Compact Pro archive, or its directory is damaged: the directory's CRC is wrong.")?;
        let mut members = Vec::new();
        // Each folder open at this point, with how many of its entries are still to come.
        let mut open: Vec<(usize, usize)> = Vec::new();
        let mut at = first;
        for _ in 0..count {
            while open.last().is_some_and(|(_, left)| *left == 0) {
                open.pop();
            }
            let parent = open.last().map(|(folder, _)| *folder);
            if let Some((_, left)) = open.last_mut() {
                *left -= 1;
            }
            let length = bytes[at];
            let name = mac::mac_roman(&bytes[at + 1..at + 1 + usize::from(length & 0x7F)]);
            at += 1 + usize::from(length & 0x7F);
            let field = |from: usize, size: usize| bytes[at + from..at + from + size].iter().fold(0u64, |n, &b| n << 8 | u64::from(b));
            let mut file = MacFile { name, kind: [0; 4], creator: [0; 4], flags: 0, data: mac::Fork::of_length(0), resource: mac::Fork::of_length(0), created: 0, modified: 0, parent: 0, id: 0 };
            if length & 0x80 != 0 {
                let inside = field(0, 2) as usize;
                at += 2;
                // Entries a folder says are inside it count against its own folders' entries too.
                for (_, left) in &mut open {
                    *left = left.saturating_sub(inside);
                }
                open.push((members.len(), inside));
                members.push(Member { file, parent, folder: true, at: 0, packed: (0, 0), flags: 0, crc: 0 });
                continue;
            }
            file.kind = bytes[at + 5..at + 9].try_into().unwrap_or([b'?'; 4]);
            file.creator = bytes[at + 9..at + 13].try_into().unwrap_or([b'?'; 4]);
            (file.created, file.modified, file.flags) = (field(13, 4), field(17, 4), field(21, 2) as u16);
            file.resource = mac::Fork::of_length(field(29, 4));
            file.data = mac::Fork::of_length(field(33, 4));
            members.push(Member { file, parent, folder: false, at: field(1, 4), packed: (field(37, 4), field(41, 4)), flags: field(27, 2) as u16, crc: field(23, 4) as u32 });
            at += 45;
        }
        Ok(Self { source, members, notes: Vec::new() })
    }

    /// A fork's bytes: the run-length coding undone, and LZH's first if it was packed with it.
    fn unpack(&self, member: &Member, resource: bool) -> Result<Vec<u8>, String> {
        if member.flags & 1 != 0 {
            return Err("This file is encrypted with a password, which Limoniter can't open.".into());
        }
        let (size, at, length, lzh) = if resource {
            (member.file.resource.length, member.at, member.packed.0, member.flags & 2 != 0)
        } else {
            (member.file.data.length, member.at + member.packed.0, member.packed.1, member.flags & 4 != 0)
        };
        if size > MOST_FILE_BYTES {
            return Err("This file is larger than the 1 GB that can be opened.".into());
        }
        if at + length > self.source.len() {
            return Err("The archive ends before this file's packed data does, so it's damaged or cut short.".into());
        }
        let input = self.source.read_vec(at, length as usize)?;
        let runs = if lzh { lzh_unpack(&input, size as usize) } else { input };
        unrle81(&runs, size as usize)
    }
}

/// Undoes Compact Pro's run-length coding, to `size` bytes.
fn unrle81(input: &[u8], size: usize) -> Result<Vec<u8>, String> {
    let short = || "This file's packed data ends early, so it's damaged or cut short.".to_string();
    let mut out = Vec::with_capacity(size);
    let (mut at, mut last) = (0, 0u8);
    // An &81 &81 is a literal &81 and then an escape: the second &81 is looked at again.
    let mut escape_pending = false;
    while out.len() < size {
        let byte = if escape_pending {
            escape_pending = false;
            0x81
        } else {
            let byte = *input.get(at).ok_or_else(short)?;
            at += 1;
            byte
        };
        if byte != 0x81 {
            out.push(byte);
            last = byte;
            continue;
        }
        let next = *input.get(at).ok_or_else(short)?;
        at += 1;
        match next {
            0x82 => {
                let count = *input.get(at).ok_or_else(short)?;
                at += 1;
                if count == 0 {
                    out.extend([0x81, 0x82]);
                    last = 0x82;
                } else {
                    // The byte before, count - 1 more times.
                    out.extend(std::iter::repeat_n(last, usize::from(count).saturating_sub(1)));
                }
            }
            0x81 => {
                out.push(0x81);
                last = 0x81;
                escape_pending = true;
            }
            other => {
                out.extend([0x81, other]);
                last = other;
            }
        }
    }
    out.truncate(size);
    Ok(out)
}

/// A prefix code as a block gives it: a count of bytes, each two code lengths, the rest of `size` symbols unused.
fn lzh_code(bits: &mut Bits, size: usize) -> Result<Code, String> {
    let count = bits.bits(8)? as usize;
    if count * 2 > size {
        return Err("This file's packed data has an impossible code table, so it's damaged.".into());
    }
    let mut lengths = vec![0i32; size];
    for pair in 0..count {
        let byte = bits.bits(8)? as i32;
        (lengths[2 * pair], lengths[2 * pair + 1]) = (byte >> 4, byte & 15);
    }
    Code::from_lengths(&lengths)
}

/// Undoes Compact Pro's LZH, giving the run-length coded bytes, enough for `size` bytes once they're undone.
fn lzh_unpack(input: &[u8], size: usize) -> Vec<u8> {
    const BLOCK: usize = 0x1FFF0;
    const WINDOW: usize = 8192;
    let mut bits = Bits::new(input, true);
    let mut out: Vec<u8> = Vec::new();
    // The run-length coding can only make bytes more: the packed data is never more than twice what it gives, and
    // a run of 255 from three bytes is the most, so this is a safe limit to stop at.
    let most = size.saturating_mul(3).saturating_add(16);
    let (mut counted, mut block_start) = (BLOCK, None::<usize>);
    let (mut literals, mut lengths, mut distances) = (Code::new(), Code::new(), Code::new());
    // Each literal or match in turn, while there's more to read. The packed data has no end of its own: what's
    // after the last code the run-length coding needs is padding, so trouble there ends it, the CRC of what was
    // unpacked being the judge.
    let mut step = || -> Result<bool, String> {
        if bits.at / 8 >= input.len() || out.len() >= most {
            return Ok(false);
        }
        if counted >= BLOCK {
            if let Some(start) = block_start {
                let byte = bits.at.div_ceil(8);
                bits.at = (byte + if (byte - start) % 2 == 1 { 3 } else { 2 }) * 8;
                if bits.at / 8 >= input.len() {
                    return Ok(false);
                }
            }
            literals = lzh_code(&mut bits, 256)?;
            lengths = lzh_code(&mut bits, 64)?;
            distances = lzh_code(&mut bits, 128)?;
            counted = 0;
            block_start = Some(bits.at / 8);
        }
        if bits.bit()? == 1 {
            out.push(literals.read(&mut bits)? as u8);
            counted += 2;
        } else {
            let length = lengths.read(&mut bits)? as usize;
            let distance = (distances.read(&mut bits)? as usize) << 6 | bits.bits(6)? as usize;
            counted += 3;
            for _ in 0..length {
                // The window is 8K round, so a distance of 0 is the whole window back; before the start, it's zeros.
                let back = if distance == 0 { WINDOW } else { distance };
                let byte = if back > out.len() { 0 } else { out[out.len() - back] };
                out.push(byte);
            }
        }
        Ok(true)
    };
    while let Ok(true) = step() {}
    out
}

impl Volume for CompactPro {
    fn describe(&self) -> String {
        format!("Compact Pro archive \u{B7} {} file(s)", self.members.iter().filter(|m| !m.folder).count())
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        let parent = dir.map(|d| d.location as usize);
        let mut out = Vec::new();
        for (index, member) in self.members.iter().enumerate().filter(|(_, m)| m.folder && m.parent == parent) {
            let mut entry = Entry::dir(tidy_name(&member.file.name), index as u64, 0);
            entry.detail = mac::mac_date(member.file.modified);
            out.push(entry);
        }
        for (index, member) in self.members.iter().enumerate().filter(|(_, m)| !m.folder && m.parent == parent) {
            for mut entry in mac::file_entries(index, &member.file) {
                let packed = if entry.extra == RESOURCE_FORK { member.flags & 2 != 0 } else { member.flags & 4 != 0 };
                let how = match (member.flags & 1 != 0, packed) {
                    (true, _) => "encrypted",
                    (false, true) => "LZH",
                    (false, false) => "RLE",
                };
                entry.detail = format!("{} \u{B7} {how}", entry.detail);
                out.push(entry);
            }
        }
        Ok(out)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let member = self.members.get(entry.location as usize).filter(|m| !m.folder).ok_or("That file isn't in the archive.")?;
        let resource = entry.extra == RESOURCE_FORK;
        let fork = self.unpack(member, resource)?;
        // The CRC-32 is of both forks, the resource fork first, so it's checked with the other fork too.
        let (first, second) = if resource { (fork.clone(), self.unpack(member, false)?) } else { (self.unpack(member, true)?, fork.clone()) };
        let crc = first.iter().chain(&second).fold(0xFFFF_FFFF, |crc, &b| crc32_byte(crc, b));
        if crc != member.crc {
            return Err("This file's CRC is wrong once it's unpacked, so the archive is damaged.".into());
        }
        Ok(fork)
    }

    fn notes(&self) -> Vec<String> {
        self.notes.clone()
    }

    fn text_preset(&self, entry: &Entry) -> Option<&'static str> {
        (entry.extra != RESOURCE_FORK).then_some(mac::MAC_TEXT_PRESET)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crc(bytes: &[u8]) -> u32 {
        bytes.iter().fold(0xFFFF_FFFF, |crc, &b| crc32_byte(crc, b))
    }

    /// Compact Pro's run-length coding of bytes: runs of four or more as the byte and &81 &82 n. An &81 followed by
    /// anything but &81 or &82 stands for itself, which is all these tests need.
    fn rle81(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut at = 0;
        while at < data.len() {
            let byte = data[at];
            let run = data[at..].iter().take(255).take_while(|&&b| b == byte).count();
            if run >= 4 && byte != 0x81 {
                out.extend([byte, 0x81, 0x82, run as u8]);
                at += run;
            } else {
                out.push(byte);
                at += 1;
            }
        }
        out
    }

    /// An archive of one folder holding one file, both its forks run-length coded only.
    fn archive(data: &[u8], resource: &[u8]) -> Vec<u8> {
        let (packed_data, packed_resource) = (rle81(data), rle81(resource));
        let mut out = vec![1, 1, 0, 0, 0, 0, 0, 0];
        out.extend(&packed_resource);
        out.extend(&packed_data);
        let directory = out.len() as u32;
        out[4..8].copy_from_slice(&directory.to_be_bytes());
        let mut entries = vec![0, 2, 0];
        entries.push(0x80 | 6);
        entries.extend(b"Folder");
        entries.extend(1u16.to_be_bytes());
        entries.push(9);
        entries.extend(b"Read Me 1");
        entries.push(1);
        entries.extend(8u32.to_be_bytes());
        entries.extend(b"TEXTttxt");
        entries.extend([0; 8]);
        entries.extend([0, 0]);
        let both: Vec<u8> = resource.iter().chain(data).copied().collect();
        entries.extend(crc(&both).to_be_bytes());
        entries.extend([0, 0]);
        for length in [resource.len(), data.len(), packed_resource.len(), packed_data.len()] {
            entries.extend((length as u32).to_be_bytes());
        }
        out.extend(crc(&entries).to_be_bytes());
        out.extend(entries);
        out
    }

    #[test]
    fn a_compact_pro_archive_gives_back_its_forks() {
        let data: Vec<u8> = b"Hello \x81 world".iter().copied().chain(std::iter::repeat_n(b'!', 300)).collect();
        let resource = vec![7u8; 40];
        let bytes = archive(&data, &resource);
        assert_eq!(super::super::super::macarc::kind_of(&bytes, bytes.len() as u64, "cpt"), Some(super::super::super::macarc::Kind::CompactPro));
        let volume = super::super::super::open(Arc::new(bytes.clone()), "test.cpt").unwrap();
        assert_eq!(volume.describe(), "Compact Pro archive \u{B7} 1 file(s)");
        let top = volume.list(None).unwrap();
        assert_eq!((top.len(), top[0].name.as_str(), top[0].is_dir), (1, "Folder", true));
        let inside = volume.list(Some(&top[0])).unwrap();
        assert_eq!(inside.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["Read Me 1", "Read Me 1.rsrc"]);
        assert_eq!(volume.read(&inside[0]).unwrap(), data);
        assert_eq!(volume.read(&inside[1]).unwrap(), resource);

        // A changed byte in a fork is caught by the CRC; one in the directory, by its own.
        let mut spoilt = bytes.clone();
        spoilt[8] ^= 1;
        let volume = super::super::super::open(Arc::new(spoilt), "test.cpt").unwrap();
        let inside = volume.list(Some(&volume.list(None).unwrap()[0])).unwrap();
        assert!(volume.read(&inside[0]).unwrap_err().contains("CRC"));
        let mut spoilt = bytes.clone();
        let last = spoilt.len() - 5;
        spoilt[last] ^= 1;
        assert!(super::super::super::open(Arc::new(spoilt), "test.cpt").is_err());
        for cut in 0..bytes.len() {
            if let Ok(volume) = super::super::super::open(Arc::new(bytes[..cut].to_vec()), "cut.cpt") {
                for dir in volume.list(None).unwrap() {
                    for entry in volume.list(Some(&dir)).unwrap_or_default() {
                        let _ = volume.read(&entry);
                    }
                }
            }
        }
    }

    #[test]
    fn lzh_codes_and_matches_unpack() {
        // A block whose literal code gives 'A' and 'B' a bit each, its length code length 2, and its distance code
        // distance 0 (the top bits): "AB", then a match of length 2, distance 2.
        let mut bits = Vec::new();
        let mut push = |value: u32, count: u32| {
            for i in (0..count).rev() {
                bits.push((value >> i) & 1);
            }
        };
        // Literals: 34 bytes of lengths, for symbols 0 to 67, 'A' (65) and 'B' (66) one bit each.
        push(34, 8);
        for pair in 0..34 {
            let (a, b) = (2 * pair, 2 * pair + 1);
            let length = |s: u32| if s == 65 || s == 66 { 1 } else { 0 };
            push(length(a) << 4 | length(b), 8);
        }
        // Lengths: symbol 2 alone, one bit. Distances: symbol 0 alone, one bit.
        push(2, 8);
        push(0x00, 8);
        push(0x10, 8);
        push(1, 8);
        push(0x10, 8);
        push(1, 1);
        push(0, 1);
        push(1, 1);
        push(1, 1);
        push(0, 1);
        push(0, 1);
        push(0, 1);
        push(2, 6);
        let bytes: Vec<u8> = bits.chunks(8).map(|c| c.iter().enumerate().fold(0u8, |b, (i, bit)| b | ((*bit as u8) << (7 - i)))).collect();
        assert_eq!(&lzh_unpack(&bytes, 4)[..4], b"ABAB");
    }
}
