//! Apple disk images (`.dmg`) in UDIF, the Universal Disk Image Format of Mac OS X's Disk Utility and `hdiutil`.
//! There's no published specification; the layout here is the one the open-source readers agree on (dmg2img,
//! libdmg-hfsplus, 7-Zip) and that Apple's own images show.
//!
//! A UDIF image ends with a 512-byte trailer, marked `koly`, that says where the image's data and its table of
//! contents are. The table is a property list (or, in the earliest images, a resource fork) with a `blkx` entry for
//! each partition of the disk inside: a `mish` block listing the partition's runs of sectors, each stored as it is,
//! compressed (zlib, bzip2, ADC or LZFSE), or left out as zeros. The disk is put back together a run at a time as
//! it's read, so a large image is never expanded whole. Its partitions are then opened as the Mac disks they are.

use super::{Slice, Source, Volume, mac};
use std::collections::BTreeSet;
use std::io::Read;
use std::sync::{Arc, Mutex};

fn be(bytes: &[u8]) -> u64 {
    mac::be(bytes)
}

/// The most a single run of sectors may expand to. Apple's tools write runs of 1MB or less.
const MOST_RUN_BYTES: u64 = 64 * 1024 * 1024;

/// How a run of sectors is kept in the image, by the `mish` block's code for it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Method {
    Zeros,
    Raw,
    Adc,
    Zlib,
    Bzip2,
    Lzfse,
    Lzma,
    Unknown(u32),
}

impl Method {
    fn of(code: u32) -> Option<Self> {
        Some(match code {
            0 | 2 => Self::Zeros,
            1 => Self::Raw,
            0x8000_0004 => Self::Adc,
            0x8000_0005 => Self::Zlib,
            0x8000_0006 => Self::Bzip2,
            0x8000_0007 => Self::Lzfse,
            0x8000_0008 => Self::Lzma,
            // Comments and the end of the list take up no sectors.
            0x7FFF_FFFE | 0xFFFF_FFFF => return None,
            other => Self::Unknown(other),
        })
    }

    fn name(self) -> String {
        match self {
            Self::Zeros => "zeros".into(),
            Self::Raw => "uncompressed".into(),
            Self::Adc => "ADC".into(),
            Self::Zlib => "zlib".into(),
            Self::Bzip2 => "bzip2".into(),
            Self::Lzfse => "LZFSE".into(),
            Self::Lzma => "LZMA".into(),
            Self::Unknown(code) => format!("method {code:#X}"),
        }
    }
}

/// A run of the disk's sectors: where it is on the disk, in bytes, and where its stored form is in the image.
#[derive(Clone, Debug)]
struct Run {
    start: u64,
    length: u64,
    method: Method,
    at: u64,
    stored: u64,
}

/// A partition, as its `blkx` entry names it, and where it is on the disk.
struct Part {
    name: String,
    start: u64,
    length: u64,
}

/// The disk inside a UDIF image, read a run at a time; the last few runs expanded are kept.
pub struct Udif {
    image: Arc<dyn Source>,
    runs: Vec<Run>,
    len: u64,
    recent: Mutex<Vec<(usize, Arc<Vec<u8>>)>>,
}

impl Udif {
    /// A run's bytes, expanded.
    fn run_bytes(&self, index: usize) -> Result<Arc<Vec<u8>>, String> {
        if let Some(found) = self.recent.lock().ok().and_then(|r| r.iter().find(|(i, _)| *i == index).map(|(_, b)| b.clone())) {
            return Ok(found);
        }
        let run = &self.runs[index];
        let stored = self.image.read_vec(run.at, run.stored as usize).map_err(|_| "The disk image's data runs past its end, so it's damaged.".to_string())?;
        let size = run.length as usize;
        let bytes = match run.method {
            Method::Zeros => vec![0; size],
            Method::Raw => stored,
            Method::Adc => unadc(&stored, size)?,
            Method::Zlib => miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&stored, size.max(1)).map_err(|_| "The disk image's zlib data is damaged.".to_string())?,
            Method::Bzip2 => {
                let mut out = Vec::with_capacity(size);
                bzip2::read::BzDecoder::new(stored.as_slice()).take(size as u64 + 1).read_to_end(&mut out).map_err(|_| "The disk image's bzip2 data is damaged.".to_string())?;
                out
            }
            Method::Lzfse => super::hfsplus::unlzfse(&stored, size).map_err(|_| "The disk image's LZFSE data is damaged.".to_string())?,
            Method::Lzma | Method::Unknown(_) => return Err(format!("This disk image is compressed with {}, which Limoniter can't read yet.", run.method.name())),
        };
        if bytes.len() < size {
            return Err("A run of the disk image's sectors comes out shorter than it says, so the image is damaged.".into());
        }
        let bytes = Arc::new(bytes);
        if let Ok(mut recent) = self.recent.lock() {
            if recent.len() >= 8 {
                recent.remove(0);
            }
            recent.push((index, bytes.clone()));
        }
        Ok(bytes)
    }
}

impl Source for Udif {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        if offset + buf.len() as u64 > self.len {
            return Err("past the end of the image".into());
        }
        let mut done = 0;
        while done < buf.len() {
            let at = offset + done as u64;
            // The run holding this byte, if any; sectors no run covers read as zeros.
            let index = self.runs.partition_point(|r| r.start + r.length <= at);
            let want = buf.len() - done;
            match self.runs.get(index) {
                Some(run) if run.start <= at => {
                    let within = (at - run.start) as usize;
                    let take = want.min((run.length - within as u64) as usize);
                    if run.method == Method::Raw {
                        self.image.read_at(run.at + within as u64, &mut buf[done..done + take]).map_err(|_| "The disk image's data runs past its end, so it's damaged.".to_string())?;
                    } else {
                        let bytes = self.run_bytes(index)?;
                        buf[done..done + take].copy_from_slice(&bytes[within..within + take]);
                    }
                    done += take;
                }
                next => {
                    let take = want.min(next.map_or(usize::MAX, |r| (r.start - at) as usize));
                    buf[done..done + take].fill(0);
                    done += take;
                }
            }
        }
        Ok(())
    }
}

/// Where a UDIF image is within a file: the whole of it, or the data fork of a MacBinary file, as images were
/// often posted to the web, with their Finder information in a 128-byte header in front.
fn udif_within(source: &dyn Source) -> Option<(u64, u64)> {
    let len = source.len();
    let at = |offset: u64, mark: &[u8]| source.read_vec(offset, mark.len()).is_ok_and(|b| b == mark);
    if len >= 512 && at(len - 512, b"koly") {
        return Some((0, len));
    }
    // MacBinary: a zero, a name of 1 to 63 characters, zeros at 74 and 82, and the data fork's length at 83.
    let head = source.read_vec(0, 128).ok()?;
    let data = be(&head[83..87]);
    (head[0] == 0 && (1..=63).contains(&head[1]) && head[74] == 0 && head[82] == 0 && data >= 512 && 128 + data <= len && at(128 + data - 512, b"koly")).then_some((128, data))
}

/// Whether an image is a UDIF disk image by its trailer, or an encrypted disk image by its marks.
pub fn looks_like(source: &dyn Source) -> bool {
    let len = source.len();
    let at = |offset: u64, mark: &[u8]| source.read_vec(offset, mark.len()).is_ok_and(|b| b == mark);
    udif_within(source).is_some() || (len >= 512 && (at(0, b"encrcdsa") || at(len - 8, b"cdsaencr")))
}

/// Opens a UDIF disk image: its partitions, the Mac ones opened as their files.
pub fn open(image: Arc<dyn Source>) -> Result<Box<dyn Volume>, String> {
    let len = image.len();
    if image.read_vec(0, 8).is_ok_and(|b| b == b"encrcdsa") || image.read_vec(len.saturating_sub(8), 8).is_ok_and(|b| b == b"cdsaencr") {
        return Err("This disk image is encrypted, and Limoniter can't read encrypted disk images.".into());
    }
    let (start, length) = udif_within(&*image).ok_or("This isn't a UDIF disk image: it has no trailer.")?;
    let image: Arc<dyn Source> = if start == 0 { image } else { Arc::new(Slice { inner: image, start, len: length }) };
    let (disk, parts, methods) = unpack(image)?;
    let what = format!("Apple disk image, {}", methods);
    let disk: Arc<dyn Source> = Arc::new(disk);
    let mut out = Vec::new();
    let mut macs = Vec::new();
    for part in parts {
        let inner: Arc<dyn Source> = Arc::new(Slice { inner: disk.clone(), start: part.start, len: part.length.min(disk.len().saturating_sub(part.start)) });
        let kind = part.name.rsplit_once('(').map_or(part.name.as_str(), |(_, k)| k.trim_end_matches(')')).split(" : ").next().unwrap_or_default().trim().to_string();
        let mut entry = mac::Partition { name: part.name.clone(), kind, volume: None, problem: None, blocks: part.length, raw: Some(inner.clone()) };
        let head = inner.read_vec(0, inner.len().min(0x8800) as usize);
        match head {
            Err(problem) => entry.problem = Some(problem),
            Ok(head) if mac::kind_of(&head).is_some() => match mac::open_disk(inner, &what) {
                Ok(volume) => {
                    entry.volume = Some(volume);
                    macs.push(out.len());
                }
                Err(problem) => entry.problem = Some(problem),
            },
            Ok(head) if head.get(32..36) == Some(b"NXSB") || part.name.contains("Apple_APFS") || part.name.contains("7C3457EF") => {
                entry.problem = Some("APFS, which Limoniter can't read yet".into());
            }
            // Other file systems, such as the ISO 9660 half of a hybrid CD image, open as themselves.
            Ok(_) if part.length >= 64 * 1024 => {
                if let Ok(volume) = super::open_plain_disk(inner) {
                    entry.volume = Some(volume);
                }
            }
            Ok(_) => {}
        }
        out.push(entry);
    }
    if out.is_empty() {
        return Err("This disk image lists no partitions, so there's nothing in it to read.".into());
    }
    // One Mac volume opens at once, as the Finder would mount it, though the disk may also have an EFI partition.
    if let [only] = macs[..] {
        return Ok(out.swap_remove(only).volume.expect("a Mac partition has its volume"));
    }
    Ok(mac::Partitioned::new(&what, "UDIF", out).into_volume())
}

/// The trailer's table of contents, read into the disk's runs and partitions, with the methods its runs use.
fn unpack(image: Arc<dyn Source>) -> Result<(Udif, Vec<Part>, String), String> {
    let len = image.len();
    let trailer = image.read_vec(len.checked_sub(512).ok_or("The disk image is too short for its trailer.")?, 512)?;
    if &trailer[..4] != b"koly" {
        return Err("This isn't a UDIF disk image: it has no trailer.".into());
    }
    let (data_at, xml_at, xml_length) = (be(&trailer[24..32]), be(&trailer[216..224]), be(&trailer[224..232]));
    let (fork_at, fork_length) = (be(&trailer[40..48]), be(&trailer[48..56]));
    let blocks: Vec<(String, Vec<u8>)> = if xml_length > 0 {
        if xml_at.saturating_add(xml_length) > len || xml_length > 64 * 1024 * 1024 {
            return Err("The disk image's table of contents is past its end, so it's damaged.".into());
        }
        plist_blocks(&image.read_vec(xml_at, xml_length as usize)?)?
    } else if fork_length > 0 {
        if fork_at.saturating_add(fork_length) > len || fork_length > 64 * 1024 * 1024 {
            return Err("The disk image's table of contents is past its end, so it's damaged.".into());
        }
        fork_blocks(&image.read_vec(fork_at, fork_length as usize)?)?
    } else {
        return Err("The disk image has no table of contents, so its disk can't be put together.".into());
    };
    let mut runs = Vec::new();
    let mut parts = Vec::new();
    let mut methods = BTreeSet::new();
    for (name, block) in blocks {
        // The mish block: its mark, version, first sector and count, where its data starts, and (past checksums)
        // the count of its runs, each forty bytes.
        if block.len() < 204 || &block[..4] != b"mish" {
            return Err("A partition's table of runs isn't one a disk image has, so the image is damaged.".into());
        }
        let (first, count, base) = (be(&block[8..16]), be(&block[16..24]), be(&block[24..32]));
        let entries = be(&block[200..204]) as usize;
        if 204 + entries * 40 > block.len() || first.checked_add(count).is_none_or(|end| end > u64::MAX / 512) {
            return Err("A partition's table of runs is cut short, so the image is damaged.".into());
        }
        for i in 0..entries {
            let entry = &block[204 + i * 40..244 + i * 40];
            let Some(method) = Method::of(be(&entry[0..4]) as u32) else {
                continue;
            };
            let (sector, sectors, at, stored) = (be(&entry[8..16]), be(&entry[16..24]), be(&entry[24..32]), be(&entry[32..40]));
            // Runs of zeros are left out: what no run covers reads as zeros, however long it is.
            if method == Method::Zeros || sectors == 0 {
                continue;
            }
            // A run stored as it is, is read a part at a time; only one to be expanded need fit in memory.
            let length = sectors.checked_mul(512).filter(|l| *l <= MOST_RUN_BYTES || method == Method::Raw).ok_or("A run of the disk image's sectors is larger than any it could hold, so the image is damaged.")?;
            let at = data_at.saturating_add(base).saturating_add(at);
            if at.saturating_add(stored) > len || stored > MOST_RUN_BYTES * 2 {
                return Err("A run of the disk image's sectors is past its end, so the image is damaged.".into());
            }
            if method == Method::Raw && stored < length {
                return Err("A run of the disk image's sectors is stored shorter than it is, so the image is damaged.".into());
            }
            let start = first.checked_add(sector).and_then(|s| s.checked_mul(512)).filter(|s| s.checked_add(length).is_some()).ok_or("A run of the disk image's sectors is past any disk's end, so the image is damaged.")?;
            methods.insert(method.name());
            runs.push(Run { start, length, method, at, stored });
        }
        parts.push(Part { name, start: first * 512, length: count * 512 });
    }
    runs.sort_by_key(|r| r.start);
    if runs.windows(2).any(|w| w[0].start + w[0].length > w[1].start) {
        return Err("The disk image's runs of sectors overlap, so the image is damaged.".into());
    }
    let sectors = be(&trailer[492..500]);
    let len = sectors.saturating_mul(512).max(runs.last().map_or(0, |r| r.start + r.length)).max(parts.iter().map(|p| p.start + p.length).max().unwrap_or(0));
    let methods = if methods.is_empty() { "empty".to_string() } else { methods.into_iter().collect::<Vec<_>>().join(", ") };
    Ok((Udif { image, runs, len, recent: Mutex::new(Vec::new()) }, parts, methods))
}

/// A property list dictionary's value for a key: the element after the <key>.
fn value<'a, 'input>(dict: roxmltree::Node<'a, 'input>, name: &str) -> Option<roxmltree::Node<'a, 'input>> {
    let mut children = dict.children().filter(|n| n.is_element());
    while let Some(child) = children.next() {
        if child.has_tag_name("key") && child.text() == Some(name) {
            return children.next();
        }
    }
    None
}

/// The `blkx` entries of the property list: each partition's name and its mish block, base64-encoded.
fn plist_blocks(xml: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let damaged = || "The disk image's table of contents can't be read, so it's damaged.".to_string();
    let text = std::str::from_utf8(xml).map_err(|_| damaged())?;
    let document = roxmltree::Document::parse_with_options(text, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() }).map_err(|_| damaged())?;
    let top = document.root_element().children().find(|n| n.has_tag_name("dict")).ok_or_else(damaged)?;
    let fork = value(top, "resource-fork").ok_or_else(damaged)?;
    let list = value(fork, "blkx").ok_or_else(damaged)?;
    let mut out = Vec::new();
    for (i, entry) in list.children().filter(|n| n.has_tag_name("dict")).enumerate() {
        let name = value(entry, "Name").or_else(|| value(entry, "CFName")).and_then(|n| n.text()).map_or_else(|| format!("Partition {}", i + 1), str::to_string);
        let data = value(entry, "Data").and_then(|n| n.text()).ok_or_else(damaged)?;
        out.push((name, base64(data).ok_or_else(damaged)?));
    }
    Ok(out)
}

/// The `blkx` resources of the resource fork the earliest UDIF images keep their table in, in the Resource
/// Manager's layout: a header, the data, and a map of types, references and names.
fn fork_blocks(fork: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let damaged = || "The disk image's table of contents can't be read, so it's damaged.".to_string();
    let half = |at: usize| fork.get(at..at + 2).map(be).ok_or_else(damaged);
    let word = |at: usize| fork.get(at..at + 4).map(be).ok_or_else(damaged);
    let (data_at, map_at) = (word(0)? as usize, word(4)? as usize);
    let (types_at, names_at) = (map_at + half(map_at + 24)? as usize, map_at + half(map_at + 26)? as usize);
    let types = (half(types_at)? + 1) & 0xFFFF;
    let mut out = Vec::new();
    for t in 0..types as usize {
        let entry = types_at + 2 + t * 8;
        if fork.get(entry..entry + 4) != Some(b"blkx") {
            continue;
        }
        let (count, list) = (half(entry + 4)? + 1, types_at + half(entry + 6)? as usize);
        for r in 0..count as usize {
            let reference = list + r * 12;
            let name_offset = half(reference + 2)?;
            let data = data_at + (word(reference + 4)? & 0xFF_FFFF) as usize;
            let length = word(data)? as usize;
            let block = fork.get(data + 4..data + 4 + length).ok_or_else(damaged)?.to_vec();
            let name = if name_offset == 0xFFFF {
                format!("Partition {}", out.len() + 1)
            } else {
                let at = names_at + name_offset as usize;
                let length = usize::from(*fork.get(at).ok_or_else(damaged)?);
                String::from_utf8_lossy(fork.get(at + 1..at + 1 + length).ok_or_else(damaged)?).into_owned()
            };
            out.push((name, block));
        }
    }
    Ok(out)
}

/// Base64, as a property list's <data> holds it, with its spacing and line breaks ignored.
fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut bits, mut count) = (0u32, 0);
    for c in text.bytes() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            c if c.is_ascii_whitespace() => continue,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    Some(out)
}

/// Apple Data Compression, from the first Disk Copy images: each byte starts either a run of up to 128 bytes as
/// they are, or a copy of 3 to 18 bytes from up to 1K back (two bytes), or 4 to 67 from up to 64K back (three).
fn unadc(input: &[u8], size: usize) -> Result<Vec<u8>, String> {
    let damaged = || "The disk image's ADC data is damaged.".to_string();
    let mut out = Vec::with_capacity(size);
    let mut at = 0;
    while at < input.len() && out.len() < size {
        let first = input[at];
        let (length, back) = if first & 0x80 != 0 {
            let length = usize::from(first & 0x7F) + 1;
            out.extend_from_slice(input.get(at + 1..at + 1 + length).ok_or_else(damaged)?);
            at += 1 + length;
            continue;
        } else if first & 0x40 != 0 {
            let back = input.get(at + 1..at + 3).ok_or_else(damaged)?;
            at += 3;
            (usize::from(first & 0x3F) + 4, (usize::from(back[0]) << 8) | usize::from(back[1]))
        } else {
            let back = *input.get(at + 1).ok_or_else(damaged)?;
            at += 2;
            (usize::from((first & 0x3F) >> 2) + 3, (usize::from(first & 3) << 8) | usize::from(back))
        };
        let from = out.len().checked_sub(back + 1).ok_or_else(damaged)?;
        for i in 0..length {
            out.push(out[from + i]);
        }
    }
    Ok(out)
}

#[cfg(test)]
pub mod tests {
    use super::super::{is_image, open};
    use super::*;
    use std::io::Write;

    pub const RAW: u32 = 1;
    pub const ZEROS: u32 = 2;
    pub const ADC: u32 = 0x8000_0004;
    pub const ZLIB: u32 = 0x8000_0005;
    pub const BZIP2: u32 = 0x8000_0006;
    pub const LZFSE: u32 = 0x8000_0007;
    pub const LZMA: u32 = 0x8000_0008;

    fn base64_of(bytes: &[u8]) -> String {
        const LETTERS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for (i, chunk) in bytes.chunks(3).enumerate() {
            let n = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
            for j in 0..4 {
                out.push(if j <= chunk.len() { char::from(LETTERS[(n >> (18 - 6 * j)) as usize & 63]) } else { '=' });
            }
            if i % 13 == 12 {
                out.push_str("\n\t\t\t");
            }
        }
        out
    }

    /// ADC with no copies: runs of up to 128 bytes as they are.
    fn adc_literals(bytes: &[u8]) -> Vec<u8> {
        bytes.chunks(128).flat_map(|c| std::iter::once(0x80 | (c.len() - 1) as u8).chain(c.iter().copied())).collect()
    }

    fn pack(method: u32, bytes: &[u8]) -> Vec<u8> {
        match method {
            RAW => bytes.to_vec(),
            ZEROS => Vec::new(),
            ADC => adc_literals(bytes),
            ZLIB => miniz_oxide::deflate::compress_to_vec_zlib(bytes, 6),
            BZIP2 => {
                let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::best());
                encoder.write_all(bytes).unwrap();
                encoder.finish().unwrap()
            }
            LZFSE => {
                let mut out = Vec::new();
                lzfse_rust::encode_bytes(bytes, &mut out).unwrap();
                out
            }
            _ => vec![0x5D, 0, 0, 0x80, 0],
        }
    }

    /// A partition's runs, `per_run` sectors each, stored by the methods given in turn (zeros where a run is all
    /// zeros): its mish block, and the stored bytes, placed from `data_at` in the image.
    fn mish(disk: &[u8], first: u64, sectors: u64, per_run: u64, methods: &[u32], data_at: u64) -> (Vec<u8>, Vec<u8>) {
        let mut stored = Vec::new();
        let mut entries = Vec::new();
        let mut sector = 0;
        let mut turn = 0;
        while sector < sectors {
            let count = per_run.min(sectors - sector);
            let bytes = &disk[((first + sector) * 512) as usize..((first + sector + count) * 512) as usize];
            let method = if bytes.iter().all(|b| *b == 0) { ZEROS } else { methods[turn % methods.len()] };
            turn += 1;
            let packed = pack(method, bytes);
            let mut entry = vec![0u8; 40];
            entry[0..4].copy_from_slice(&method.to_be_bytes());
            entry[8..16].copy_from_slice(&sector.to_be_bytes());
            entry[16..24].copy_from_slice(&count.to_be_bytes());
            entry[24..32].copy_from_slice(&(data_at + stored.len() as u64).to_be_bytes());
            entry[32..40].copy_from_slice(&(packed.len() as u64).to_be_bytes());
            entries.push(entry);
            stored.extend(packed);
            sector += count;
        }
        let mut end = vec![0u8; 40];
        end[0..4].copy_from_slice(&0xFFFF_FFFFu32.to_be_bytes());
        end[8..16].copy_from_slice(&sectors.to_be_bytes());
        entries.push(end);
        let mut block = vec![0u8; 204];
        block[0..4].copy_from_slice(b"mish");
        block[4..8].copy_from_slice(&1u32.to_be_bytes());
        block[8..16].copy_from_slice(&first.to_be_bytes());
        block[16..24].copy_from_slice(&sectors.to_be_bytes());
        block[200..204].copy_from_slice(&(entries.len() as u32).to_be_bytes());
        block.extend(entries.concat());
        (block, stored)
    }

    /// A UDIF image of a disk, with a blkx entry for each of the partitions given, (name, first sector, sectors).
    pub fn udif(disk: &[u8], parts: &[(&str, u64, u64)], methods: &[u32]) -> Vec<u8> {
        let mut data = Vec::new();
        let mut blocks = Vec::new();
        for (name, first, sectors) in parts {
            let (block, stored) = mish(disk, *first, *sectors, 8, methods, data.len() as u64);
            data.extend(stored);
            blocks.push((name.to_string(), block));
        }
        let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n\t<key>resource-fork</key>\n\t<dict>\n\t\t<key>blkx</key>\n\t\t<array>\n");
        for (i, (name, block)) in blocks.iter().enumerate() {
            xml.push_str(&format!("\t\t\t<dict>\n\t\t\t\t<key>Attributes</key>\n\t\t\t\t<string>0x0050</string>\n\t\t\t\t<key>CFName</key>\n\t\t\t\t<string>{name}</string>\n\t\t\t\t<key>Data</key>\n\t\t\t\t<data>\n\t\t\t{}\n\t\t\t\t</data>\n\t\t\t\t<key>ID</key>\n\t\t\t\t<string>{}</string>\n\t\t\t\t<key>Name</key>\n\t\t\t\t<string>{name}</string>\n\t\t\t</dict>\n", base64_of(block), i as i32 - 1));
        }
        xml.push_str("\t\t</array>\n\t</dict>\n</dict>\n</plist>\n");
        let mut image = data;
        let (data_length, xml_at) = (image.len() as u64, image.len() as u64);
        image.extend(xml.as_bytes());
        image.extend(trailer(data_length, xml_at, xml.len() as u64, 0, 0, (disk.len() / 512) as u64));
        image
    }

    fn trailer(data_length: u64, xml_at: u64, xml_length: u64, fork_at: u64, fork_length: u64, sectors: u64) -> Vec<u8> {
        let mut koly = vec![0u8; 512];
        koly[0..4].copy_from_slice(b"koly");
        koly[4..8].copy_from_slice(&4u32.to_be_bytes());
        koly[8..12].copy_from_slice(&512u32.to_be_bytes());
        koly[12..16].copy_from_slice(&1u32.to_be_bytes());
        koly[32..40].copy_from_slice(&data_length.to_be_bytes());
        koly[40..48].copy_from_slice(&fork_at.to_be_bytes());
        koly[48..56].copy_from_slice(&fork_length.to_be_bytes());
        koly[216..224].copy_from_slice(&xml_at.to_be_bytes());
        koly[224..232].copy_from_slice(&xml_length.to_be_bytes());
        koly[488..492].copy_from_slice(&1u32.to_be_bytes());
        koly[492..500].copy_from_slice(&sectors.to_be_bytes());
        koly
    }

    /// The disk's partitions as `hdiutil` lists those of a disk with an Apple partition map.
    fn mapped_parts(disk: &[u8]) -> Vec<(&'static str, u64, u64)> {
        vec![("Driver Descriptor Map (DDM : 0)", 0, 1), ("Apple (Apple_partition_map : 1)", 1, 63), ("disk image (Apple_HFS : 2)", 64, (disk.len() / 512) as u64 - 64)]
    }

    fn names(entries: &[super::super::Entry]) -> Vec<String> {
        entries.iter().map(|e| e.name.clone()).collect()
    }

    #[test]
    fn udif_images_open_as_the_disks_inside() {
        let inner = super::super::hfsplus::tests::sample(b"H+");
        let disk = mac::tests::partitioned(&[&inner]);
        let image = udif(&disk, &mapped_parts(&disk), &[ZLIB, RAW, BZIP2, ADC, LZFSE]);
        assert!(is_image("Sample.dmg", &image[..image.len().min(0x8800)], image.len() as u64));
        assert!(looks_like(&image));
        // The disk comes back byte for byte, read across runs in pieces of every size.
        let (udif, parts, methods) = unpack(Arc::new(image.clone())).unwrap();
        assert_eq!(methods, "ADC, LZFSE, bzip2, uncompressed, zlib");
        assert_eq!(parts.len(), 3);
        assert_eq!(udif.len(), disk.len() as u64);
        let mut at = 0;
        for step in [1usize, 511, 4096, 5000, 77_777].iter().cycle() {
            if at >= disk.len() {
                break;
            }
            let length = (*step).min(disk.len() - at);
            assert_eq!(udif.read_vec(at as u64, length).unwrap(), disk[at..at + length], "at {at}");
            at += length;
        }
        // The one HFS+ partition opens as its files, the same as the disk itself does.
        let volume = open(Arc::new(image), "Sample.dmg").unwrap();
        assert!(volume.describe().starts_with("Apple disk image, ADC, LZFSE, bzip2, uncompressed, zlib \u{B7} HFS+ \u{B7} Sample"), "{}", volume.describe());
        let plain = open(Arc::new(disk), "disk.img").unwrap();
        let (mut ours, mut theirs) = (Vec::new(), Vec::new());
        super::super::hfsplus::tests::walk(&*volume, None, 0, &mut |path, bytes| ours.push((path, bytes)));
        super::super::hfsplus::tests::walk(&*plain, None, 0, &mut |path, bytes| theirs.push((path, bytes)));
        assert!(ours.len() > 60);
        assert_eq!(ours, theirs);
    }

    #[test]
    fn whole_disk_images_and_several_partitions() {
        let hfs = super::super::tests::fixture("hfs.img");
        let whole = udif(&hfs, &[("whole disk (Apple_HFS : 0)", 0, (hfs.len() / 512) as u64)], &[ZLIB]);
        let volume = open(Arc::new(whole), "Old.dmg").unwrap();
        assert!(volume.describe().starts_with("Apple disk image, zlib \u{B7} HFS \u{B7} Fixture"), "{}", volume.describe());
        // Two file systems: the partitions are listed, each opened.
        let inner = super::super::hfsplus::tests::sample(b"HX");
        let disk = mac::tests::partitioned(&[&hfs, &inner]);
        let hfs_sectors = (hfs.len() / 512) as u64;
        let parts = [("Apple (Apple_partition_map : 1)", 1, 63), ("Disk 1 (Apple_HFS : 2)", 64, hfs_sectors), ("Disk 2 (Apple_HFS : 3)", 64 + hfs_sectors, (inner.len() / 512) as u64)];
        let volume = open(Arc::new(udif(&disk, &parts, &[RAW, ZLIB])), "Two.dmg").unwrap();
        let top = volume.list(None).unwrap();
        assert_eq!(names(&top), ["Apple (Apple_partition_map _ 1)", "Disk 1 (Apple_HFS _ 2)", "Disk 2 (Apple_HFS _ 3)"]);
        assert!(!top[0].is_dir && top[0].kind == "Apple_partition_map");
        let map = volume.read(&top[0]).unwrap();
        assert!(map.len() == 63 * 512 && map.starts_with(b"PM"), "a partition with no file system opens as its bytes");
        assert!(top[2].detail.contains("HFSX \u{B7} Sample"), "{}", top[2].detail);
        let inside = volume.list(Some(&top[2])).unwrap();
        let big = inside.iter().find(|e| e.name == "Big").unwrap();
        assert_eq!(volume.read(big).unwrap().len(), 12 * 512 - 100);
    }

    #[test]
    fn early_images_with_their_table_in_a_resource_fork() {
        let hfs = super::super::tests::fixture("hfs.img");
        let (block, stored) = mish(&hfs, 0, (hfs.len() / 512) as u64, 64, &[ADC], 0);
        // A resource fork with one blkx resource, named, as the Resource Manager lays it out.
        let data = [(block.len() as u32).to_be_bytes().to_vec(), block].concat();
        let name = b"\x0Bwhole disk ";
        let types = [&0u16.to_be_bytes()[..], b"blkx", &0u16.to_be_bytes(), &10u16.to_be_bytes()].concat();
        let reference = [&0i16.to_be_bytes()[..], &0u16.to_be_bytes(), &[0, 0, 0, 0], &[0; 4]].concat();
        let map = [&[0u8; 24][..], &28u16.to_be_bytes(), &(28 + types.len() as u16 + reference.len() as u16).to_be_bytes(), &types, &reference, name].concat();
        let mut fork = [256u32, 256 + data.len() as u32, data.len() as u32, map.len() as u32].iter().flat_map(|n| n.to_be_bytes()).collect::<Vec<u8>>();
        fork.resize(256, 0);
        fork.extend(&data);
        fork.extend(&map);
        let mut image = stored;
        let fork_at = image.len() as u64;
        image.extend(&fork);
        image.extend(trailer(fork_at, 0, 0, fork_at, fork.len() as u64, (hfs.len() / 512) as u64));
        let volume = open(Arc::new(image), "Early.dmg").unwrap();
        assert!(volume.describe().starts_with("Apple disk image, ADC \u{B7} HFS \u{B7} Fixture"), "{}", volume.describe());
        let top = volume.list(None).unwrap();
        assert_eq!(volume.read(top.iter().find(|e| e.name == "F1").unwrap()).unwrap(), vec![1u8; 512]);
    }

    #[test]
    fn adc_copies_from_what_came_before() {
        // "abc" as it is; three bytes from three back (a two-byte copy); four from six back (a three-byte copy).
        let packed = [0x82, b'a', b'b', b'c', 0x00, 0x02, 0x40, 0x00, 0x05];
        assert_eq!(unadc(&packed, 10).unwrap(), b"abcabcabca");
        assert!(unadc(&[0x00, 0x05], 10).is_err(), "a copy from before the start");
        assert!(unadc(&[0x85, b'a'], 10).is_err(), "a run cut short");
    }

    #[test]
    fn encrypted_unknown_and_damaged_images_give_reasons() {
        let mut encrypted = b"encrcdsa".to_vec();
        encrypted.resize(4096, 0);
        assert!(open(Arc::new(encrypted), "secret.dmg").err().unwrap().contains("encrypted"));
        let hfs = super::super::tests::fixture("hfs.img");
        let lzma = udif(&hfs, &[("whole disk (Apple_HFS : 0)", 0, (hfs.len() / 512) as u64)], &[LZMA]);
        let refused = open(Arc::new(lzma), "new.dmg").map(|v| v.list(None).map(|l| l.iter().map(|e| e.detail.clone()).collect::<Vec<_>>()));
        assert!(format!("{refused:?}").contains("LZMA, which Limoniter can't read yet"), "{refused:?}");
        let image = udif(&mac::tests::partitioned(&[&hfs]), &mapped_parts(&mac::tests::partitioned(&[&hfs])), &[ZLIB, ADC, BZIP2, LZFSE, RAW]);
        for seed in 0..300u32 {
            let mut bytes = image.clone();
            let mut state = seed.wrapping_mul(2_654_435_761);
            for _ in 0..8 {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let at = (state >> 4) as usize % bytes.len();
                bytes[at] = (state >> 16) as u8;
            }
            if let Ok(volume) = open(Arc::new(bytes), "d.dmg") {
                super::super::hfsplus::tests::walk(&*volume, None, 0, &mut |_, _| {});
            }
        }
        for cut in [10usize, 600, image.len() / 2] {
            let _ = open(Arc::new(image[cut..].to_vec()), "cut.dmg");
        }
    }

    /// The disk images in ..\testmedia\hfsplus, when it's there: each disk put back together byte for byte as dmg2img
    /// does (for the methods it knows), and each partition as 7-Zip's `7zz` takes it out, where they're installed.
    #[test]
    fn real_disk_images_unpack_as_dmg2img_and_7zip_do() {
        let Ok(items) = std::fs::read_dir("../testmedia/hfsplus") else {
            return;
        };
        let quiet = || std::process::Stdio::null();
        let (mut by_dmg2img, mut by_7zip) = (0, 0);
        for path in items.flatten().map(|i| i.path()).filter(|p| p.extension().is_some_and(|e| e == "dmg")) {
            let file: Arc<dyn Source> = Arc::new(super::super::FileSource::open(&path).unwrap());
            let Some((start, len)) = udif_within(&*file) else {
                continue;
            };
            let (disk, parts, methods) = unpack(Arc::new(Slice { inner: file, start, len })).unwrap();
            let out = std::env::temp_dir().join(format!("limoniter-dmg-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&out);
            std::fs::create_dir_all(&out).unwrap();
            // dmg2img knows neither LZFSE nor MacBinary.
            let whole = out.join("whole.img");
            if start == 0 && !methods.contains("LZFSE") && std::process::Command::new("dmg2img").arg("-s").arg("-i").arg(&path).arg("-o").arg(&whole).stdout(quiet()).stderr(quiet()).status().is_ok_and(|s| s.success()) {
                let theirs = std::fs::read(&whole).unwrap();
                let ours = disk.read_vec(0, disk.len() as usize).unwrap();
                assert!(ours == theirs, "{}: different bytes from dmg2img's", path.display());
                by_dmg2img += 1;
            }
            // 7-Zip takes each partition out as a file named by its number, then its kind.
            if let Some(seven) = super::super::hfsplus::tests::modern_7zip()
                && std::process::Command::new(seven).arg("x").arg("-y").arg("-tdmg").arg(format!("-o{}", out.display())).arg(&path).stdout(quiet()).stderr(quiet()).status().is_ok_and(|s| s.success())
            {
                let made: Vec<_> = std::fs::read_dir(&out).unwrap().flatten().map(|i| i.file_name().to_string_lossy().into_owned()).collect();
                for (i, part) in parts.iter().enumerate() {
                    let Some(name) = made.iter().find(|n| n.starts_with(&format!("{i}."))) else {
                        continue;
                    };
                    let theirs = std::fs::read(out.join(name)).unwrap();
                    assert!(disk.read_vec(part.start, part.length as usize).unwrap() == theirs, "{} {name}: different bytes from 7-Zip's", path.display());
                }
                by_7zip += 1;
            }
            let _ = std::fs::remove_dir_all(&out);
        }
        eprintln!("{by_dmg2img} disk images compared with dmg2img, {by_7zip} with 7-Zip");
    }
}
