//! StuffIt archives, the Mac's usual archive from 1987 until Mac OS X: Raymond Lau's classic format ("SIT!", and
//! the "ST46"-style signatures of later versions and installers) and StuffIt 5's ("StuffIt (c)1997-..."). Each
//! entry has the Finder's information and two forks, each packed by its own method; folders are entries that start
//! and end (classic) or say how many entries are in them (StuffIt 5).
//!
//! No layout was ever published. This follows what is known from reading archives, as The Unarchiver (XADMaster)
//! and macutils record it. The methods unpacked here are 0 (stored), 1 (run-length, as BinHex's), 2 (LZW, as Unix
//! `compress` with 14 bits), 3 (Huffman), 5 (LZAH: LHarc's -lh1-), 8 (MW, Miller and Wegman's LZW), 13 (LZ77
//! with Huffman codes, StuffIt 1.5's new method and StuffIt 5's fast one) and 15 (Arsenic: arithmetic coding of a
//! Burrows-Wheeler block, StuffIt 4.5's and 5's best). Each fork's CRC-16 is checked, or Arsenic's own CRC-32; an
//! entry packed another way (6, fixed Huffman, and 14, the installers' method, are seldom seen), or with a password,
//! says so rather than giving wrong bytes.

pub(super) mod compactpro;
mod tables;

use super::mac::{self, MacFile, RESOURCE_FORK};
use super::macarc::{crc16_arc, unrle90};
use super::{Entry, MOST_FILE_BYTES, Source, Volume, tidy_name};
use std::collections::HashMap;
use std::sync::Arc;

fn be(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0, |n, b| (n << 8) | u64::from(*b))
}

/// Whether bytes start like a classic StuffIt archive: "SIT!" or one of the "STxx" signatures later versions and
/// installers wrote, with Raymond Lau's "rLau" ten bytes in.
pub fn looks_like_stuffit(head: &[u8]) -> bool {
    let Some(h) = head.get(..14) else {
        return false;
    };
    let digit = |b: u8| b.is_ascii_digit();
    &h[10..14] == b"rLau" && (&h[..4] == b"SIT!" || (&h[..2] == b"ST" && ((h[2] == b'i' && (h[3] == b'n' || digit(h[3]))) || (digit(h[2]) && digit(h[3])))))
}

/// The text StuffIt 5's archives start with; the year after "1997-" varies.
const STUFFIT5_TEXT: &[u8] = b"StuffIt (c)1997-???? Aladdin Systems, Inc., http://www.aladdinsys.com/StuffIt/\r\n";

/// Whether bytes start like a StuffIt 5 archive.
pub fn looks_like_stuffit5(head: &[u8]) -> bool {
    head.len() >= 100 && STUFFIT5_TEXT.iter().zip(head).all(|(want, got)| *want == b'?' || want == got)
}

/// Where a fork's packed bytes are, and how they were packed.
#[derive(Clone, Debug, Default)]
struct Packed {
    at: u64,
    length: u64,
    method: u8,
    crc: u16,
    encrypted: bool,
}

struct Member {
    file: MacFile,
    parent: Option<usize>,
    folder: bool,
    data: Packed,
    resource: Packed,
}

pub struct StuffIt {
    source: Arc<dyn Source>,
    members: Vec<Member>,
    what: &'static str,
    notes: Vec<String>,
}

/// The name StuffIt gives a method.
fn method_name(method: u8) -> String {
    match method {
        0 => "Stored".into(),
        1 => "RLE".into(),
        2 => "LZW".into(),
        3 => "Huffman".into(),
        5 => "LZAH".into(),
        6 => "Fixed Huffman".into(),
        8 => "MW".into(),
        13 => "LZ+Huffman".into(),
        14 => "Installer".into(),
        15 => "Arsenic".into(),
        other => format!("Method {other}"),
    }
}

fn unpacks(method: u8) -> bool {
    matches!(method, 0 | 1 | 2 | 3 | 5 | 8 | 13 | 15)
}

/// A name as an archive has it: Mac Roman, unless it's UTF-8 with more than ASCII in it, as StuffIt on Mac OS X
/// wrote them. Mac Roman text is seldom also UTF-8, as its accented letters would have to fall in UTF-8's pairs.
fn archive_name(name: &[u8]) -> String {
    match std::str::from_utf8(name) {
        Ok(text) if !text.is_ascii() => text.to_string(),
        _ => mac::mac_roman(name),
    }
}

/// The Finder's information an entry gives.
fn mac_file(name: &[u8], kind: &[u8], creator: &[u8], flags: u16, sizes: (u64, u64), dates: (u64, u64)) -> MacFile {
    MacFile {
        name: archive_name(name),
        kind: kind.try_into().unwrap_or([b'?'; 4]),
        creator: creator.try_into().unwrap_or([b'?'; 4]),
        flags,
        data: mac::Fork::of_length(sizes.0),
        resource: mac::Fork::of_length(sizes.1),
        created: dates.0,
        modified: dates.1,
        parent: 0,
        id: 0,
    }
}

/// Bytes read in turn from a source, for StuffIt 5's headers, which vary in length.
struct Cursor<'a> {
    source: &'a dyn Source,
    at: u64,
}

impl Cursor<'_> {
    fn bytes(&mut self, count: usize) -> Result<Vec<u8>, String> {
        let bytes = self.source.read_vec(self.at, count).map_err(|_| "The archive ends in the middle of an entry's header.".to_string())?;
        self.at += count as u64;
        Ok(bytes)
    }

    fn number(&mut self, count: usize) -> Result<u64, String> {
        Ok(be(&self.bytes(count)?))
    }
}

impl StuffIt {
    /// A classic archive: a 22-byte header giving the archive's length, then each entry's 112-byte header (with
    /// its own CRC) followed by its resource fork's packed bytes and its data fork's.
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let head = source.read_vec(0, 22).map_err(|_| "This StuffIt archive is cut short before its first entry.".to_string())?;
        let end = be(&head[6..10]).min(source.len());
        let mut members: Vec<Member> = Vec::new();
        let mut folders: Vec<usize> = Vec::new();
        let mut notes = Vec::new();
        let mut at = 22;
        while at + 112 <= end {
            let h = source.read_vec(at, 112)?;
            if crc16_arc(0, &h[..110]) != be(&h[110..112]) as u16 {
                let problem = format!("This StuffIt archive is damaged: the header of its entry {} has the wrong CRC.", members.len() + 1);
                if members.is_empty() {
                    return Err(problem);
                }
                notes.push(format!("{problem} The entries before it are listed."));
                break;
            }
            at += 112;
            let (resource_method, data_method) = (h[0], h[1]);
            let name = &h[3..3 + usize::from(h[2].min(31))];
            let file = mac_file(name, &h[66..70], &h[70..74], be(&h[74..76]) as u16, (be(&h[88..92]), be(&h[84..88])), (be(&h[76..80]), be(&h[80..84])));
            // A folder's start and end are entries of their own, with no forks after them.
            let folder_mark = |m: u8| m & 0x6F;
            if folder_mark(resource_method) == 0x20 || folder_mark(data_method) == 0x20 {
                members.push(Member { file, parent: folders.last().copied(), folder: true, data: Packed::default(), resource: Packed::default() });
                folders.push(members.len() - 1);
                continue;
            }
            if folder_mark(resource_method) == 0x21 || folder_mark(data_method) == 0x21 {
                folders.pop();
                continue;
            }
            let (resource_length, data_length) = (be(&h[92..96]), be(&h[96..100]));
            let fork = |at: u64, length: u64, method: u8, crc: u64| Packed { at, length, method: method & 0x0F, crc: crc as u16, encrypted: method & 0x80 != 0 };
            members.push(Member {
                file,
                parent: folders.last().copied(),
                folder: false,
                resource: fork(at, resource_length, resource_method, be(&h[100..102])),
                data: fork(at + resource_length, data_length, data_method, be(&h[102..104])),
            });
            at += resource_length + data_length;
        }
        Ok(Self { source, members, what: "StuffIt archive", notes })
    }

    /// A StuffIt 5 archive: its own header, then entries whose headers say where their folder's header is.
    pub fn open5(source: Arc<dyn Source>) -> Result<Self, String> {
        let head = source.read_vec(0, 100).map_err(|_| "This StuffIt 5 archive is cut short.".to_string())?;
        if head[82] != 5 {
            return Err(format!("This is a StuffIt 5 archive of version {}, which Limoniter doesn't know.", head[82]));
        }
        let mut members = Vec::new();
        let mut notes = Vec::new();
        if let Err(problem) = Self::entries5(&*source, be(&head[92..94]), be(&head[94..98]), &mut members) {
            if members.is_empty() {
                return Err(problem);
            }
            notes.push(format!("{problem} The entries before it are listed."));
        }
        Ok(Self { source, members, what: "StuffIt 5 archive", notes })
    }

    fn entries5(source: &dyn Source, top_level: u64, first: u64, members: &mut Vec<Member>) -> Result<(), String> {
        let mut folders: HashMap<u64, usize> = HashMap::new();
        let mut cursor = Cursor { source, at: first };
        let (mut count, mut done) = (top_level, 0);
        while done < count {
            done += 1;
            if members.len() > 1_000_000 {
                return Err("This StuffIt archive has more entries than Limoniter lists.".into());
            }
            let start = cursor.at;
            if cursor.number(4)? != 0xA5A5_A5A5 {
                return Err(format!("This StuffIt 5 archive is damaged: its entry {} isn't where the one before says.", members.len() + 1));
            }
            let version = cursor.number(1)?;
            cursor.at += 1;
            let header_end = start + cursor.number(2)?;
            cursor.at += 1;
            let flags = cursor.number(1)?;
            let dates = (cursor.number(4)?, cursor.number(4)?);
            cursor.at += 8;
            let folder_at = cursor.number(4)?;
            let name_length = cursor.number(2)? as usize;
            cursor.at += 2;
            let (data_size, data_length, data_crc) = (cursor.number(4)?, cursor.number(4)?, cursor.number(2)?);
            cursor.at += 2;
            let is_folder = flags & 0x40 != 0;
            let encrypted = flags & 0x20 != 0;
            let mut data_method = 0;
            let mut inside = 0;
            if is_folder {
                inside = cursor.number(2)?;
                // Every folder is followed by an entry like this, whose purpose isn't known; it doesn't count.
                if data_size == 0xFFFF_FFFF {
                    count += 1;
                    continue;
                }
            } else {
                data_method = cursor.number(1)? as u8 & 0x0F;
                // An encrypted entry's key comes next.
                let key_length = cursor.number(1)?;
                cursor.at += key_length;
            }
            let name = cursor.bytes(name_length)?;
            if cursor.at < header_end {
                let comment = cursor.number(2)?;
                cursor.at += 2 + comment;
            }
            let has_resource = cursor.number(2)? & 1 != 0;
            cursor.at += 2;
            let (kind, creator) = (cursor.bytes(4)?, cursor.bytes(4)?);
            let finder_flags = cursor.number(2)? as u16;
            cursor.at += if version == 1 { 22 } else { 18 };
            let mut resource = Packed::default();
            let mut resource_size = 0;
            if has_resource {
                resource_size = cursor.number(4)?;
                resource.length = cursor.number(4)?;
                resource.crc = cursor.number(2)? as u16;
                cursor.at += 2;
                resource.method = cursor.number(1)? as u8 & 0x0F;
                resource.encrypted = encrypted && resource_size > 0;
                let key_length = cursor.number(1)?;
                cursor.at += key_length;
            }
            let file = mac_file(&name, &kind, &creator, finder_flags, (data_size, resource_size), dates);
            let parent = folders.get(&folder_at).copied();
            if is_folder {
                folders.insert(start, members.len());
                members.push(Member { file: MacFile { data: mac::Fork::default(), ..file }, parent, folder: true, data: Packed::default(), resource: Packed::default() });
                count += inside;
                continue;
            }
            resource.at = cursor.at;
            let data = Packed { at: cursor.at + resource.length, length: data_length, method: data_method, crc: data_crc as u16, encrypted: encrypted && data_size > 0 };
            cursor.at = data.at + data.length;
            members.push(Member { file, parent, folder: false, data, resource });
        }
        Ok(())
    }

    /// A fork's bytes, unpacked and checked.
    fn unpack(&self, packed: &Packed, size: u64) -> Result<Vec<u8>, String> {
        if packed.encrypted {
            return Err("This file is encrypted with a password, which Limoniter can't open.".into());
        }
        if size > MOST_FILE_BYTES {
            return Err("This file is larger than the 1 GB that can be opened.".into());
        }
        if !unpacks(packed.method) {
            return Err(format!("This file is packed with StuffIt's method {} ({}), which Limoniter can't unpack.", packed.method, method_name(packed.method)));
        }
        if packed.at + packed.length > self.source.len() {
            return Err("The archive ends before this file's packed data does, so it's damaged or cut short.".into());
        }
        let input = self.source.read_vec(packed.at, packed.length as usize)?;
        let size = size as usize;
        let out = match packed.method {
            _ if size == 0 => Vec::new(),
            0 => input.get(..size).ok_or("This stored file is shorter than its header says.")?.to_vec(),
            1 => unrle90(&input, Some(size))?,
            2 => lzw(&input, size)?,
            3 => huffman(&input, size)?,
            5 => lzah(&input, size)?,
            8 => mw(&input, size)?,
            13 => lz_huffman(&input, size)?,
            _ => arsenic(&input, size)?,
        };
        if out.len() != size {
            return Err(format!("This file unpacks to {} bytes where its header says {size}, so it's damaged.", out.len()));
        }
        // Arsenic checks its own CRC-32; the others have StuffIt's CRC-16 in the header.
        if packed.method != 15 && crc16_arc(0, &out) != packed.crc {
            return Err("This file unpacked, but its CRC doesn't match, so it's damaged.".into());
        }
        Ok(out)
    }

    fn entries(&self, index: usize) -> Vec<Entry> {
        let member = &self.members[index];
        let mut entries = mac::file_entries(index, &member.file);
        for entry in &mut entries {
            let packed = if entry.extra == RESOURCE_FORK { &member.resource } else { &member.data };
            let method = method_name(packed.method);
            let how = if packed.encrypted {
                format!("{method}, encrypted")
            } else if unpacks(packed.method) || (entry.size == 0 && packed.length == 0) {
                method
            } else {
                format!("{method}, not unpacked")
            };
            entry.detail = format!("{} \u{B7} {how}", entry.detail);
        }
        entries
    }
}

impl Volume for StuffIt {
    fn describe(&self) -> String {
        format!("{} \u{B7} {} file(s)", self.what, self.members.iter().filter(|m| !m.folder).count())
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        let parent = dir.map(|d| d.location as usize);
        let mut out = Vec::new();
        for (index, member) in self.members.iter().enumerate().filter(|(_, m)| m.folder && m.parent == parent) {
            let mut entry = Entry::dir(tidy_name(&member.file.name), index as u64, 0);
            entry.detail = mac::mac_date(member.file.modified);
            out.push(entry);
        }
        for (index, _) in self.members.iter().enumerate().filter(|(_, m)| !m.folder && m.parent == parent) {
            out.extend(self.entries(index));
        }
        Ok(out)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let member = self.members.get(entry.location as usize).filter(|m| !m.folder).ok_or("That file isn't in the archive.")?;
        if entry.extra == RESOURCE_FORK { self.unpack(&member.resource, member.file.resource.length) } else { self.unpack(&member.data, member.file.data.length) }
    }

    fn notes(&self) -> Vec<String> {
        self.notes.clone()
    }

    fn text_preset(&self, entry: &Entry) -> Option<&'static str> {
        (entry.extra != RESOURCE_FORK).then_some(mac::MAC_TEXT_PRESET)
    }
}

// ---------- Bits and prefix codes ----------

const CUT_SHORT: &str = "The packed data ends early, so it's damaged or cut short.";

/// Bits read in turn from bytes, from each byte's top bit down (StuffIt's Huffman and Arsenic) or its bottom bit up
/// (LZW and method 13). A few bytes of zeros may be read past the end, as decoders look ahead; more is damage.
struct Bits<'a> {
    bytes: &'a [u8],
    at: usize,
    top_first: bool,
}

impl<'a> Bits<'a> {
    fn new(bytes: &'a [u8], top_first: bool) -> Self {
        Self { bytes, at: 0, top_first }
    }

    fn bit(&mut self) -> Result<u32, String> {
        let index = self.at / 8;
        if index >= self.bytes.len() + 8 {
            return Err(CUT_SHORT.into());
        }
        let byte = u32::from(*self.bytes.get(index).unwrap_or(&0));
        let shift = if self.top_first { 7 - self.at % 8 } else { self.at % 8 };
        self.at += 1;
        Ok((byte >> shift) & 1)
    }

    /// `count` bits as a number: the first read is the highest when reading top first, the lowest otherwise.
    fn bits(&mut self, count: u32) -> Result<u32, String> {
        let mut value = 0;
        for i in 0..count {
            let bit = self.bit()?;
            value = if self.top_first { (value << 1) | bit } else { value | (bit << i) };
        }
        Ok(value)
    }
}

/// A prefix code as a tree: each node's two children, a leaf being its symbol with `LEAF` set, and 0 for none.
#[derive(Clone)]
struct Code {
    nodes: Vec<[u32; 2]>,
}

const LEAF: u32 = 0x8000_0000;
const BAD_CODE: &str = "The packed data has a code that means nothing, so it's damaged.";

impl Code {
    fn new() -> Self {
        Self { nodes: vec![[0, 0]] }
    }

    /// Adds a symbol's code, its first bit being the code's highest.
    fn add(&mut self, symbol: u32, code: u32, length: u32) -> Result<(), String> {
        let mut node = 0;
        for place in (0..length).rev() {
            let bit = ((code >> place) & 1) as usize;
            let child = self.nodes[node][bit];
            if child & LEAF != 0 || (place == 0 && child != 0) {
                return Err("The packed data's Huffman code is impossible, so it's damaged.".into());
            }
            if place == 0 {
                self.nodes[node][bit] = LEAF | symbol;
            } else if child == 0 {
                self.nodes.push([0, 0]);
                let new = (self.nodes.len() - 1) as u32;
                self.nodes[node][bit] = new;
                node = new as usize;
            } else {
                node = child as usize;
            }
        }
        Ok(())
    }

    /// The canonical code for symbols' lengths, shortest first and in symbol order within a length, the first
    /// code being all zeros. A length outside 1 to 32 means the symbol isn't used.
    fn from_lengths(lengths: &[i32]) -> Result<Self, String> {
        let mut code = Self::new();
        let mut next: u64 = 0;
        for length in 1..=32 {
            for (symbol, _) in lengths.iter().enumerate().filter(|(_, l)| **l == length) {
                if next >> length != 0 {
                    return Err("The packed data's Huffman code is impossible, so it's damaged.".into());
                }
                code.add(symbol as u32, next as u32, length as u32)?;
                next += 1;
            }
            next <<= 1;
        }
        Ok(code)
    }

    fn read(&self, bits: &mut Bits) -> Result<u32, String> {
        let mut node = 0;
        loop {
            let child = self.nodes[node][bits.bit()? as usize];
            if child == 0 {
                return Err(BAD_CODE.into());
            }
            if child & LEAF != 0 {
                return Ok(child & !LEAF);
            }
            node = child as usize;
        }
    }
}

// ---------- Method 2: LZW ----------

/// Method 2: LZW as Unix `compress` does it, in "block mode" with codes of up to 14 bits, low bit first. Code 256
/// clears the table, after which `compress` skips to the end of the current group of eight codes.
fn lzw(input: &[u8], size: usize) -> Result<Vec<u8>, String> {
    const MOST: usize = 1 << 14;
    let mut parent = vec![u16::MAX; MOST];
    let mut last_byte: Vec<u8> = (0..MOST).map(|i| i as u8).collect();
    let mut bits = Bits::new(input, false);
    let (mut width, mut next, mut previous, mut counted) = (9u32, 257usize, None::<usize>, 0u32);
    let mut out = Vec::with_capacity(size);
    let mut string = Vec::new();
    let first_byte = |parent: &[u16], last_byte: &[u8], mut code: usize| {
        while parent[code] != u16::MAX {
            code = usize::from(parent[code]);
        }
        last_byte[code]
    };
    while out.len() < size && bits.at / 8 < input.len() {
        let code = bits.bits(width)? as usize;
        counted += 1;
        if code == 256 {
            if counted % 8 != 0 {
                bits.at += (width * (8 - counted % 8)) as usize;
            }
            (width, next, previous, counted) = (9, 257, None, 0);
            continue;
        }
        if let Some(previous) = previous {
            let first = if code < next {
                first_byte(&parent, &last_byte, code)
            } else if code == next {
                first_byte(&parent, &last_byte, previous)
            } else {
                return Err(BAD_CODE.into());
            };
            if next < MOST {
                parent[next] = previous as u16;
                last_byte[next] = first;
                next += 1;
                if next < MOST && next.is_power_of_two() {
                    width += 1;
                }
            }
        } else if code >= next {
            return Err(BAD_CODE.into());
        }
        previous = Some(code);
        string.clear();
        let mut at = code;
        loop {
            string.push(last_byte[at]);
            if parent[at] == u16::MAX {
                break;
            }
            at = usize::from(parent[at]);
        }
        out.extend(string.iter().rev().take(size - out.len()));
    }
    Ok(out)
}

// ---------- Method 3: Huffman ----------

/// Method 3: one Huffman code for bytes, its tree written first (top bit first): 1 and a byte for a leaf, 0 and
/// the two branches for a node.
fn huffman(input: &[u8], size: usize) -> Result<Vec<u8>, String> {
    fn branch(bits: &mut Bits, code: &mut Code, depth: u32) -> Result<u32, String> {
        if bits.bit()? == 1 {
            return Ok(LEAF | bits.bits(8)?);
        }
        if depth > 256 || code.nodes.len() > 512 {
            return Err("The packed data's Huffman tree is impossible, so it's damaged.".into());
        }
        code.nodes.push([0, 0]);
        let node = code.nodes.len() - 1;
        let zero = branch(bits, code, depth + 1)?;
        let one = branch(bits, code, depth + 1)?;
        code.nodes[node] = [zero, one];
        Ok(node as u32)
    }
    let mut bits = Bits::new(input, true);
    let mut code = Code { nodes: Vec::new() };
    let root = branch(&mut bits, &mut code, 0)?;
    if root & LEAF != 0 {
        return Ok(vec![root as u8; size]);
    }
    let mut out = Vec::with_capacity(size);
    while out.len() < size {
        out.push(code.read(&mut bits)? as u8);
    }
    Ok(out)
}

// ---------- Method 5: LZAH ----------

/// Method 5: LZ77 with an adaptive Huffman code, the same as LHarc's -lh1-.
fn lzah(input: &[u8], size: usize) -> Result<Vec<u8>, String> {
    use delharc::decode::{Decoder, Lh1Decoder};
    let mut out = vec![0; size];
    Lh1Decoder::new(input).fill_buffer(&mut out).map_err(|e| format!("The file couldn't be unpacked: {e}"))?;
    Ok(out)
}

// ---------- Method 8: MW ----------

/// Method 8: Miller and Wegman's variant of LZW, low bit first. Each new string is the previous two read, joined,
/// rather than the previous one and a byte; codes grow from 9 bits as the table doubles, and the next free code
/// itself starts the table again.
fn mw(input: &[u8], size: usize) -> Result<Vec<u8>, String> {
    const MOST: usize = 16_384;
    // Entry n (from 256) is entry `halves[n - 1]` followed by entry `halves[n]`.
    let mut halves = vec![0u16; MOST + 1];
    let mut bits = Bits::new(input, false);
    let mut out = Vec::with_capacity(size);
    let mut pending = Vec::new();
    let mut put = |out: &mut Vec<u8>, halves: &[u16], code: usize| {
        pending.clear();
        pending.push(code);
        while let Some(mut code) = pending.pop() {
            while code >= 256 {
                pending.push(usize::from(halves[code]));
                code = usize::from(halves[code - 1]);
            }
            if out.len() == size {
                return;
            }
            out.push(code as u8);
        }
    };
    while out.len() < size {
        let (mut next, mut width) = (256usize, 9);
        let mut code = bits.bits(width)? as usize;
        if code >= next {
            return Err(BAD_CODE.into());
        }
        halves[255] = code as u16;
        put(&mut out, &halves, code);
        while out.len() < size {
            code = bits.bits(width)? as usize;
            if code >= next {
                break;
            }
            if next >= MOST {
                return Err("This file's MW data has more strings than StuffIt makes, so it's damaged.".into());
            }
            halves[next] = code as u16;
            next += 1;
            if next.is_power_of_two() {
                width += 1;
            }
            put(&mut out, &halves, code);
        }
        // The next free code starts again; anything above it is damage.
        if out.len() < size && code > next {
            return Err(BAD_CODE.into());
        }
    }
    Ok(out)
}

// ---------- Method 13: LZ77 and Huffman ----------

/// Method 13: LZ77 over a 64K window, with Huffman codes for literals and lengths (one code after a literal, another
/// after a match) and for offsets' bit counts. Its first byte picks one of five built-in sets of codes, or says the
/// codes follow, their lengths written with a fixed code of their own.
fn lz_huffman(input: &[u8], size: usize) -> Result<Vec<u8>, String> {
    let first_byte = *input.first().ok_or(CUT_SHORT)?;
    let mut bits = Bits::new(input, false);
    bits.at = 8;
    let set = usize::from(first_byte >> 4);
    let widen = |lengths: &[u8]| lengths.iter().map(|l| i32::from(*l)).collect::<Vec<i32>>();
    let (first, second, offsets) = match set {
        0 => {
            let mut meta = Code::new();
            for (symbol, (code, length)) in tables::META_CODES.iter().zip(tables::META_LENGTHS).enumerate() {
                let length = u32::from(length);
                meta.add(symbol as u32, u32::from(*code).reverse_bits() >> (32 - length), length)?;
            }
            let first = written_code(&mut bits, &meta, 321)?;
            let second = if first_byte & 8 != 0 { first.clone() } else { written_code(&mut bits, &meta, 321)? };
            (first, second, written_code(&mut bits, &meta, usize::from(first_byte & 7) + 10)?)
        }
        1..=5 => (Code::from_lengths(&widen(tables::FIRST_LENGTHS[set - 1]))?, Code::from_lengths(&widen(tables::SECOND_LENGTHS[set - 1]))?, Code::from_lengths(&widen(tables::OFFSET_LENGTHS[set - 1]))?),
        _ => return Err(format!("This file's packed data starts with a code set ({set}) StuffIt doesn't have, so it's damaged.")),
    };
    let mut out = Vec::with_capacity(size);
    let mut after_match = false;
    while out.len() < size {
        let symbol = if after_match { second.read(&mut bits)? } else { first.read(&mut bits)? };
        if symbol < 0x100 {
            out.push(symbol as u8);
            after_match = false;
            continue;
        }
        after_match = true;
        let length = match symbol {
            0x100..0x13E => symbol - 0x100 + 3,
            0x13E => bits.bits(10)? + 65,
            0x13F => bits.bits(15)? + 65,
            _ => break,
        } as usize;
        let offset = match offsets.read(&mut bits)? {
            0 => 1,
            1 => 2,
            n => (1 << (n - 1)) + bits.bits(n - 1)? as usize + 1,
        };
        // The window starts as zeros, so a match from before the start gives zeros.
        for _ in 0..length.min(size - out.len()) {
            let byte = out.len().checked_sub(offset).map_or(0, |from| out[from]);
            out.push(byte);
        }
    }
    Ok(out)
}

/// One of method 13's own codes: each symbol's length, written as changes to the length before with the fixed code.
fn written_code(bits: &mut Bits, meta: &Code, count: usize) -> Result<Code, String> {
    let mut lengths = vec![0i32; count + 80];
    let mut length = 0i32;
    let mut i = 0;
    let set = |lengths: &mut Vec<i32>, i: usize, length: i32| -> Result<(), String> {
        *lengths.get_mut(i).ok_or("The packed data's code lengths run past the end, so it's damaged.")? = length;
        Ok(())
    };
    while i < count {
        match meta.read(bits)? {
            31 => length = -1,
            32 => length += 1,
            33 => length -= 1,
            34 => {
                if bits.bit()? == 1 {
                    set(&mut lengths, i, length)?;
                    i += 1;
                }
            }
            // The length again, 2 to 9 or 10 to 73 more times.
            repeat @ (35 | 36) => {
                let repeats = if repeat == 35 { bits.bits(3)? + 2 } else { bits.bits(6)? + 10 };
                for _ in 0..repeats {
                    set(&mut lengths, i, length)?;
                    i += 1;
                }
            }
            symbol => length = symbol as i32 + 1,
        }
        set(&mut lengths, i, length)?;
        i += 1;
    }
    lengths.truncate(count);
    Code::from_lengths(&lengths)
}

// ---------- Method 15: Arsenic ----------

/// An adaptive model of symbols' frequencies for Arsenic's arithmetic coder.
struct Model {
    first: u32,
    step: u32,
    most: u32,
    frequencies: Vec<u32>,
    total: u32,
}

impl Model {
    fn new(first: u32, last: u32, step: u32, most: u32) -> Self {
        let mut model = Self { first, step, most, frequencies: vec![0; (last - first + 1) as usize], total: 0 };
        model.reset();
        model
    }

    fn reset(&mut self) {
        self.frequencies.fill(self.step);
        self.total = self.step * self.frequencies.len() as u32;
    }

    fn seen(&mut self, index: usize) {
        self.frequencies[index] += self.step;
        self.total += self.step;
        if self.total > self.most {
            for f in &mut self.frequencies {
                *f = (*f + 1) >> 1;
            }
            self.total = self.frequencies.iter().sum();
        }
    }
}

/// Arsenic's arithmetic decoder: 26 bits of precision, read top bit first.
struct Arithmetic<'a> {
    bits: Bits<'a>,
    range: i32,
    code: i32,
}

const HALF: i32 = 1 << 24;

impl<'a> Arithmetic<'a> {
    fn new(input: &'a [u8]) -> Result<Self, String> {
        let mut bits = Bits::new(input, true);
        let code = bits.bits(26)? as i32;
        Ok(Self { bits, range: 1 << 25, code })
    }

    fn symbol(&mut self, model: &mut Model) -> Result<u32, String> {
        let total = model.total as i32;
        let scale = self.range / total;
        if scale == 0 {
            return Err(CUT_SHORT.into());
        }
        let target = self.code / scale;
        let mut low = 0;
        let mut index = 0;
        while index < model.frequencies.len() - 1 && low + model.frequencies[index] as i32 <= target {
            low += model.frequencies[index] as i32;
            index += 1;
        }
        let size = model.frequencies[index] as i32;
        self.code = self.code.wrapping_sub(scale.wrapping_mul(low));
        self.range = if low + size == total { self.range.wrapping_sub(scale.wrapping_mul(low)) } else { size.wrapping_mul(scale) };
        if self.range <= 0 {
            return Err("The packed data is damaged: its arithmetic coding goes wrong.".into());
        }
        while self.range <= HALF {
            self.range <<= 1;
            self.code = (self.code.wrapping_shl(1)) | self.bits.bit()? as i32;
        }
        model.seen(index);
        Ok(model.first + index as u32)
    }

    /// A number of `count` bits, each a symbol, lowest first.
    fn number(&mut self, model: &mut Model, count: u32) -> Result<u32, String> {
        let mut value = 0;
        for i in 0..count {
            value |= self.symbol(model)? << i;
        }
        Ok(value)
    }
}

/// Method 15, Arsenic: blocks of up to 2^24 bytes, each sorted by the Burrows-Wheeler transform, moved to the
/// front, the runs of zeros counted, and all of it arithmetic-coded with adaptive models. Some blocks have bits
/// flipped at set places to stop the sort going slow ("randomised"); a run of four bytes the same is followed by a
/// count of more. A CRC-32 of the whole file ends it.
fn arsenic(input: &[u8], size: usize) -> Result<Vec<u8>, String> {
    let damaged = || "This file's Arsenic data is damaged.".to_string();
    let mut coder = Arithmetic::new(input)?;
    let mut initial = Model::new(0, 1, 1, 256);
    let mut selector = Model::new(0, 10, 8, 1024);
    let mut moved: Vec<Model> = [(2, 3, 8), (4, 7, 4), (8, 15, 4), (16, 31, 4), (32, 63, 2), (64, 127, 2), (128, 255, 1)].iter().map(|&(first, last, step)| Model::new(first, last, step, 1024)).collect();
    if coder.number(&mut initial, 8)? != u32::from(b'A') || coder.number(&mut initial, 8)? != u32::from(b's') {
        return Err("This file's Arsenic data doesn't start as Arsenic does, so it's damaged.".into());
    }
    let block_bits = coder.number(&mut initial, 4)? + 9;
    let block_size = 1usize << block_bits;
    let mut out = Vec::with_capacity(size);
    let mut crc = 0xFFFF_FFFFu32;
    let mut stored_crc = 0;
    let mut end = coder.symbol(&mut initial)? != 0;
    let mut block = Vec::new();
    let mut transform = Vec::new();
    while !end {
        let mut order: [u8; 256] = std::array::from_fn(|i| i as u8);
        let mut to_front = |symbol: usize| {
            let byte = order[symbol];
            order.copy_within(..symbol, 1);
            order[0] = byte;
            byte
        };
        let randomised = coder.symbol(&mut initial)? != 0;
        let primary = coder.number(&mut initial, block_bits)? as usize;
        block.clear();
        loop {
            let mut choice = coder.symbol(&mut selector)?;
            if choice < 2 {
                // A run of zeros (the byte at the front), its length written in bijective base 2.
                let (mut weight, mut zeros) = (1usize, 0usize);
                while choice < 2 {
                    zeros += weight * (choice as usize + 1);
                    weight *= 2;
                    if zeros > block_size {
                        return Err(damaged());
                    }
                    choice = coder.symbol(&mut selector)?;
                }
                if block.len() + zeros > block_size {
                    return Err(damaged());
                }
                let byte = to_front(0);
                block.extend(std::iter::repeat_n(byte, zeros));
            }
            if choice == 10 {
                break;
            }
            let symbol = if choice == 2 { 1 } else { coder.symbol(&mut moved[choice as usize - 3])? };
            if block.len() >= block_size {
                return Err(damaged());
            }
            block.push(to_front(symbol as usize));
        }
        if primary >= block.len() {
            return Err(damaged());
        }
        selector.reset();
        moved.iter_mut().for_each(Model::reset);
        if coder.symbol(&mut initial)? != 0 {
            stored_crc = coder.number(&mut initial, 32)?;
            end = true;
        }
        // Undo the sort: each byte's place in the sorted block leads to the next.
        let mut counts = [0usize; 256];
        for byte in &block {
            counts[usize::from(*byte)] += 1;
        }
        let mut starts = [0usize; 256];
        let mut total = 0;
        for (start, count) in starts.iter_mut().zip(counts) {
            *start = total;
            total += count;
        }
        transform.clear();
        transform.resize(block.len(), 0);
        for (index, byte) in block.iter().enumerate() {
            transform[starts[usize::from(*byte)]] = index;
            starts[usize::from(*byte)] += 1;
        }
        let (mut at, mut same, mut last) = (primary, 0, 0u8);
        let (mut step, mut flip_at) = (0, usize::from(tables::RANDOMISATION[0]));
        for count in 0..block.len() {
            at = transform[at];
            let mut byte = block[at];
            if randomised && count == flip_at {
                byte ^= 1;
                step = (step + 1) & 255;
                flip_at += usize::from(tables::RANDOMISATION[step]);
            }
            let run = if same == 4 {
                same = 0;
                usize::from(byte)
            } else {
                if byte == last {
                    same += 1;
                } else {
                    (same, last) = (1, byte);
                }
                1
            };
            if out.len() + run > size {
                return Err(format!("This file's Arsenic data unpacks to more than the {size} bytes its header says, so it's damaged."));
            }
            for _ in 0..run {
                out.push(last);
                crc = crc32_byte(crc, last);
            }
        }
    }
    if !crc == stored_crc { Ok(out) } else { Err("This file unpacked, but its CRC doesn't match, so it's damaged.".into()) }
}

/// The CRC-32 of zip and Ethernet, a byte at a time.
fn crc32_byte(crc: u32, byte: u8) -> u32 {
    (0..8).fold(crc ^ u32::from(byte), |c, _| if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 })
}

#[cfg(test)]
pub mod tests;
