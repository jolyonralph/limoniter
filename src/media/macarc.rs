//! Mac files wrapped to travel whole through machines that know nothing of forks, and the StuffIt archives they
//! usually carried. MacBinary (I, II and III) puts the Finder's information in a 128-byte header before the two
//! forks, each padded to 128 bytes; II added a CRC to the header and III a signature. BinHex 4.0 does the same job
//! in seven-bit text, for mail and Usenet: six bits to a character, runs packed with &90, and a CRC after the header
//! and after each fork. The layouts are Apple's and the MacBinary authors' notes and Yves Lempereur's BinHex 4.0
//! definition; the StuffIt archives themselves are in `stuffit.rs`.
//!
//! The file inside is shown as a Mac disk shows one: its data fork as the file, its resource fork as `name.rsrc`
//! beside it, with its type, creator and date. A `.sit.bin` or `.sit.hqx` holds a StuffIt archive, which opens a
//! level deeper as any image inside an image does.

use super::mac::{self, MacFile, RESOURCE_FORK};
use super::{Entry, MOST_FILE_BYTES, Slice, Source, Volume};
use std::sync::Arc;

/// A Mac format this module knows by its bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// MacBinary, of version 1, 2 or 3.
    MacBinary(u8),
    BinHex,
    StuffIt,
    StuffIt5,
}

fn be(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0, |n, b| (n << 8) | u64::from(*b))
}

/// CRC-16 as MacBinary II and BinHex use it: the CCITT polynomial, high bit first, from zero (XMODEM's).
pub(super) fn crc16_ccitt(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0u16, |crc, b| (0..8).fold(crc ^ (u16::from(*b) << 8), |c, _| if c & 0x8000 != 0 { (c << 1) ^ 0x1021 } else { c << 1 }))
}

/// CRC-16 as StuffIt uses it, and as some MacBinary writers did by mistake: the ARC polynomial, low bit first.
pub(super) fn crc16_arc(start: u16, bytes: &[u8]) -> u16 {
    bytes.iter().fold(start, |crc, b| (0..8).fold(crc ^ u16::from(*b), |c, _| if c & 1 != 0 { (c >> 1) ^ 0xA001 } else { c >> 1 }))
}

/// Which of these formats bytes are in, if any. A StuffIt archive's signature and a MacBinary II header's CRC are
/// believed whatever the name; MacBinary I has only zeros and sizes to go by, so it needs a `.bin` or no ending,
/// and BinHex, being text, needs `.hqx` or no ending.
pub fn kind_of(head: &[u8], len: u64, ending: &str) -> Option<Kind> {
    if super::stuffit::looks_like_stuffit(head) {
        return Some(Kind::StuffIt);
    }
    if super::stuffit::looks_like_stuffit5(head) {
        return Some(Kind::StuffIt5);
    }
    let plain = matches!(ending, "" | "bin" | "macbin");
    if let Some(version) = macbinary_version(head, len)
        && (version > 1 || plain)
    {
        return Some(Kind::MacBinary(version));
    }
    if matches!(ending, "" | "hqx" | "hcx") && binhex_start(head).is_some_and(|at| binhex_header_holds(&head[at..])) {
        return Some(Kind::BinHex);
    }
    None
}

/// Opens one of these formats.
pub fn open(source: Arc<dyn Source>, kind: Kind) -> Result<Box<dyn Volume>, String> {
    match kind {
        Kind::MacBinary(version) => Ok(Box::new(Wrapped::macbinary(source, version)?)),
        Kind::BinHex => Ok(Box::new(Wrapped::binhex(&*source)?)),
        Kind::StuffIt => Ok(Box::new(super::stuffit::StuffIt::open(source)?)),
        Kind::StuffIt5 => Ok(Box::new(super::stuffit::StuffIt::open5(source)?)),
    }
}

// ---------- MacBinary ----------

/// The forks' sizes rounded up to MacBinary's 128-byte blocks.
fn padded(size: u64) -> u64 {
    size.div_ceil(128) * 128
}

/// MacBinary's version by its header, or None if bytes aren't one. Every version leaves bytes 0, 74 and 82 zero and
/// names the file in 1 to 63 characters; II and III have a CRC of the header, III a signature too. A header without a
/// CRC is only taken for version I when all the bytes II added are zero and the dates are set. Either way the forks
/// must fit in the file.
fn macbinary_version(head: &[u8], len: u64) -> Option<u8> {
    let h = head.get(..128)?;
    let name_len = usize::from(h[1]);
    if h[0] != 0 || h[74] != 0 || h[82] != 0 || !(1..=63).contains(&name_len) || h[2..2 + name_len].contains(&0) {
        return None;
    }
    let (data, resource) = (be(&h[83..87]), be(&h[87..91]));
    let stored = be(&h[124..126]) as u16;
    let version = if h[108..116].iter().all(|b| *b == 0) && (crc16_ccitt(&h[..124]) == stored || !crc16_arc(0xFFFF, &h[..124]) == stored) {
        if &h[102..106] == b"mBIN" { 3 } else { 2 }
    } else if h[99..126].iter().all(|b| *b == 0) && be(&h[91..95]) != 0 && be(&h[95..99]) != 0 {
        1
    } else {
        return None;
    };
    let secondary = if version > 1 { padded(be(&h[120..122])) } else { 0 };
    let end = 128 + secondary + if resource > 0 { padded(data) + resource } else { data };
    (data < 0x8000_0000 && resource < 0x8000_0000 && end <= len).then_some(version)
}

/// A Mac file with its forks: from a MacBinary file, read where they are, or from BinHex, decoded.
struct Wrapped {
    what: String,
    file: MacFile,
    data: Arc<dyn Source>,
    resource: Arc<dyn Source>,
    /// A fork whose CRC didn't match, so isn't given out: (resource fork?, what's wrong).
    damaged: Vec<(bool, String)>,
}

/// The Finder's information about a file, as both formats give it.
fn mac_file(name: &[u8], kind: &[u8], creator: &[u8], flags: u16, data: u64, resource: u64, created: u64, modified: u64) -> MacFile {
    MacFile {
        name: mac::mac_roman(name),
        kind: kind.try_into().unwrap_or([b'?'; 4]),
        creator: creator.try_into().unwrap_or([b'?'; 4]),
        flags,
        data: mac::Fork::of_length(data),
        resource: mac::Fork::of_length(resource),
        created,
        modified,
        parent: 0,
        id: 0,
    }
}

impl Wrapped {
    fn macbinary(source: Arc<dyn Source>, version: u8) -> Result<Self, String> {
        let h = source.read_vec(0, 128)?;
        let (data, resource) = (be(&h[83..87]), be(&h[87..91]));
        let secondary = if version > 1 { padded(be(&h[120..122])) } else { 0 };
        let data_at = 128 + secondary;
        let resource_at = data_at + padded(data);
        if data_at + data > source.len() || (resource > 0 && resource_at + resource > source.len()) {
            return Err("This MacBinary file is shorter than its header says, so part of it is missing.".into());
        }
        // II keeps the low byte of the Finder's flags at 101, which I left as zero.
        let flags = (u16::from(h[73]) << 8) | if version > 1 { u16::from(h[101]) } else { 0 };
        let file = mac_file(&h[2..2 + usize::from(h[1])], &h[65..69], &h[69..73], flags, data, resource, be(&h[91..95]), be(&h[95..99]));
        let roman = ["", "I", "II", "III"][usize::from(version.min(3))];
        Ok(Self {
            what: format!("MacBinary {roman} \u{B7} {}", file.name),
            data: Arc::new(Slice { inner: source.clone(), start: data_at, len: data }),
            resource: Arc::new(Slice { inner: source, start: resource_at, len: resource }),
            file,
            damaged: Vec::new(),
        })
    }

    fn binhex(source: &dyn Source) -> Result<Self, String> {
        if source.len() > 4 * MOST_FILE_BYTES / 3 {
            return Err("This BinHex file is larger than the 1 GB that can be opened.".into());
        }
        let text = source.read_vec(0, source.len() as usize)?;
        let start = binhex_start(&text).ok_or("This isn't BinHex 4.0: its \"(This file must be converted with BinHex\" line or the ':' after it is missing.")?;
        let bytes = unrle90(&binhex_bits(&text[start..])?, None)?;
        let name_len = usize::from(*bytes.first().ok_or("This BinHex file holds nothing.")?);
        let header_end = 1 + name_len + 19;
        let h = bytes.get(..header_end + 2).ok_or("This BinHex file ends inside its header.")?;
        if crc16_ccitt(&h[..header_end]) != be(&h[header_end..]) as u16 {
            return Err("This BinHex file's header is damaged: its CRC doesn't match.".into());
        }
        let at = 1 + name_len + 1;
        let (data, resource) = (be(&h[at + 10..at + 14]), be(&h[at + 14..at + 18]));
        let file = mac_file(&h[1..1 + name_len], &h[at..at + 4], &h[at + 4..at + 8], be(&h[at + 8..at + 10]) as u16, data, resource, 0, 0);
        let data_at = header_end as u64 + 2;
        let resource_at = data_at + data + 2;
        if resource_at + resource + 2 > bytes.len() as u64 {
            return Err("This BinHex file ends before its forks do, so part of it is missing.".into());
        }
        let mut damaged = Vec::new();
        for (is_resource, at, length) in [(false, data_at, data), (true, resource_at, resource)] {
            let (at, length) = (at as usize, length as usize);
            if crc16_ccitt(&bytes[at..at + length]) != be(&bytes[at + length..at + length + 2]) as u16 {
                damaged.push((is_resource, format!("This file's {} fork is damaged: its BinHex CRC doesn't match.", if is_resource { "resource" } else { "data" })));
            }
        }
        let bytes: Arc<dyn Source> = Arc::new(bytes);
        Ok(Self {
            what: format!("BinHex 4.0 \u{B7} {}", file.name),
            data: Arc::new(Slice { inner: bytes.clone(), start: data_at, len: data }),
            resource: Arc::new(Slice { inner: bytes, start: resource_at, len: resource }),
            file,
            damaged,
        })
    }
}

impl Volume for Wrapped {
    fn describe(&self) -> String {
        self.what.clone()
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        Ok(if dir.is_some() { Vec::new() } else { mac::file_entries(0, &self.file) })
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let resource = entry.extra == RESOURCE_FORK;
        if let Some((_, problem)) = self.damaged.iter().find(|(r, _)| *r == resource) {
            return Err(problem.clone());
        }
        let fork = if resource { &self.resource } else { &self.data };
        if fork.len() > MOST_FILE_BYTES {
            return Err("This file is larger than the 1 GB that can be opened.".into());
        }
        fork.read_vec(0, fork.len() as usize)
    }

    fn notes(&self) -> Vec<String> {
        self.damaged.iter().map(|(_, problem)| problem.clone()).collect()
    }

    fn text_preset(&self, entry: &Entry) -> Option<&'static str> {
        (entry.extra != RESOURCE_FORK).then_some(mac::MAC_TEXT_PRESET)
    }
}

// ---------- BinHex ----------

const BINHEX_MARK: &[u8] = b"(This file must be converted with BinHex";

/// The 64 characters BinHex 4.0 writes, each standing for six bits by its place here.
const BINHEX_CHARACTERS: &[u8; 64] = b"!\"#$%&'()*+,-012345689@ABCDEFGHIJKLMNPQRSTUVXYZ[`abcdefhijklmpqr";

/// Where BinHex's encoded data starts: just after the ':' that begins a line (or follows a space) after the
/// "(This file must be converted with BinHex 4.0)" line. Mail headers and the like may come before.
fn binhex_start(text: &[u8]) -> Option<usize> {
    let mark = text.windows(BINHEX_MARK.len()).position(|w| w == BINHEX_MARK)?;
    let line_end = mark + text[mark..].iter().position(|b| matches!(b, b'\r' | b'\n'))?;
    let mut previous = b'\n';
    for (at, byte) in text.iter().enumerate().skip(line_end) {
        if *byte == b':' && matches!(previous, b'\n' | b'\r' | b'\t' | b' ') {
            return Some(at + 1);
        }
        previous = *byte;
    }
    None
}

/// BinHex text decoded to bytes, still run-length packed: six bits from each of its characters until the closing
/// ':'. Line breaks and anything else that isn't one of its characters are passed over, as BinHex's own did.
fn binhex_bits(text: &[u8]) -> Result<Vec<u8>, String> {
    let mut value = [0xFFu8; 256];
    for (index, c) in BINHEX_CHARACTERS.iter().enumerate() {
        value[usize::from(*c)] = index as u8;
    }
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut bits, mut count) = (0u32, 0);
    for byte in text {
        if *byte == b':' {
            return Ok(out);
        }
        let six = value[usize::from(*byte)];
        if six == 0xFF {
            continue;
        }
        bits = (bits << 6) | u32::from(six);
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    Err("This BinHex file ends without its closing ':', so part of it is missing.".into())
}

/// Whether text starting where BinHex's data does has a whole header, with a name of 1 to 63 characters and the
/// right CRC. Only the start is decoded, so it's quick enough to tell a file by.
fn binhex_header_holds(text: &[u8]) -> bool {
    let end = text.iter().position(|b| *b == b':').unwrap_or(text.len());
    let mut slice = text[..end].to_vec();
    slice.push(b':');
    let Ok(bytes) = binhex_bits(&slice).and_then(|b| unrle90(&b, Some(1 + 63 + 21)).or_else(|_| unrle90(&b, None))) else {
        return false;
    };
    let name_len = usize::from(*bytes.first().unwrap_or(&0));
    let header_end = 1 + name_len + 19;
    (1..=63).contains(&name_len) && bytes.len() >= header_end + 2 && crc16_ccitt(&bytes[..header_end]) == be(&bytes[header_end..header_end + 2]) as u16
}

/// Undoes the run-length packing BinHex and StuffIt's method 1 share: &90 followed by a count repeats the byte
/// before it to make that many in all, and &90 followed by 0 is &90 itself. With `most`, stops at that many bytes.
pub(super) fn unrle90(packed: &[u8], most: Option<usize>) -> Result<Vec<u8>, String> {
    let most = most.unwrap_or(usize::MAX);
    let mut out = Vec::with_capacity(packed.len().min(most));
    let mut last = 0u8;
    let mut at = 0;
    while at < packed.len() && out.len() < most {
        let byte = packed[at];
        at += 1;
        if byte != 0x90 {
            out.push(byte);
            last = byte;
            continue;
        }
        let count = *packed.get(at).ok_or("The packed data ends in the middle of a run.")?;
        at += 1;
        match count {
            0 => {
                out.push(0x90);
                last = 0x90;
            }
            1 => return Err("The packed data has a run of one, which no packer writes, so it's damaged.".into()),
            _ => {
                let more = (usize::from(count) - 1).min(most - out.len());
                out.extend(std::iter::repeat_n(last, more));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
pub mod tests {
    use super::super::tests::{find, names};
    use super::super::{is_image, is_nested_image, open as open_image};
    use super::*;

    /// A MacBinary II file, as MacBinary writers make one: the header with its CRC, then each fork padded.
    pub fn macbinary(name: &str, kind: &[u8; 4], data: &[u8], resource: &[u8], three: bool) -> Vec<u8> {
        let mut h = vec![0u8; 128];
        h[1] = name.len() as u8;
        h[2..2 + name.len()].copy_from_slice(name.as_bytes());
        h[65..69].copy_from_slice(kind);
        h[69..73].copy_from_slice(b"LMNT");
        h[83..87].copy_from_slice(&(data.len() as u32).to_be_bytes());
        h[87..91].copy_from_slice(&(resource.len() as u32).to_be_bytes());
        h[91..95].copy_from_slice(&0xA000_0000u32.to_be_bytes());
        h[95..99].copy_from_slice(&0xA000_0E10u32.to_be_bytes());
        if three {
            h[102..106].copy_from_slice(b"mBIN");
        }
        h[122] = 129;
        h[123] = 129;
        let crc = crc16_ccitt(&h[..124]);
        h[124..126].copy_from_slice(&crc.to_be_bytes());
        let mut out = h;
        for fork in [data, resource] {
            out.extend(fork);
            out.resize(padded(out.len() as u64) as usize, 0);
        }
        out
    }

    /// BinHex 4.0 text for a file, packed and encoded as BinHex writes it, 64 characters to a line.
    pub fn binhex(name: &str, kind: &[u8; 4], data: &[u8], resource: &[u8]) -> Vec<u8> {
        let mut plain = vec![name.len() as u8];
        plain.extend(name.as_bytes());
        plain.push(0);
        plain.extend(kind);
        plain.extend(b"LMNT");
        plain.extend([1, 0]);
        plain.extend((data.len() as u32).to_be_bytes());
        plain.extend((resource.len() as u32).to_be_bytes());
        plain.extend(crc16_ccitt(&plain).to_be_bytes());
        for fork in [data, resource] {
            plain.extend(fork);
            plain.extend(crc16_ccitt(fork).to_be_bytes());
        }
        // Runs of more than three are packed; &90 itself is escaped.
        let mut packed = Vec::new();
        let mut at = 0;
        while at < plain.len() {
            let byte = plain[at];
            let run = plain[at..].iter().take(255).take_while(|b| **b == byte).count();
            if byte == 0x90 {
                packed.extend([0x90, 0]);
                at += 1;
            } else if run > 3 {
                packed.extend([byte, 0x90, run as u8]);
                at += run;
            } else {
                packed.push(byte);
                at += 1;
            }
        }
        let mut text = b"(This file must be converted with BinHex 4.0)\r\n:".to_vec();
        let mut column = 1;
        for chunk in packed.chunks(3) {
            let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
            for i in 0..=chunk.len() {
                text.push(BINHEX_CHARACTERS[((n >> (18 - 6 * i)) & 63) as usize]);
                column += 1;
                if column == 64 {
                    text.extend(b"\r\n");
                    column = 0;
                }
            }
        }
        text.extend(b":\r\n");
        text
    }

    #[test]
    fn macbinary_files_show_both_forks_and_the_finders_information() {
        let resource = mac::tests::small_resource_fork();
        for three in [false, true] {
            let file = macbinary("Read Me", b"TEXT", b"Hello\rMac\r", &resource, three);
            assert!(is_image("Read Me.bin", &file, file.len() as u64));
            assert!(is_image("Read Me", &file, file.len() as u64), "known by its header with no ending");
            let volume = open_image(Arc::new(file), "Read Me.bin").unwrap();
            assert_eq!(volume.describe(), if three { "MacBinary III \u{B7} Read Me" } else { "MacBinary II \u{B7} Read Me" });
            let files = volume.list(None).unwrap();
            assert_eq!(names(&files), ["Read Me", "Read Me.rsrc"]);
            assert!(find(&files, "Read Me").detail.starts_with("TEXT \u{B7} LMNT \u{B7} 1989-"), "{}", find(&files, "Read Me").detail);
            assert_eq!(volume.read(find(&files, "Read Me")).unwrap(), b"Hello\rMac\r");
            assert_eq!(volume.read(find(&files, "Read Me.rsrc")).unwrap(), resource);
            assert_eq!(volume.text_preset(find(&files, "Read Me")), Some(mac::MAC_TEXT_PRESET));
        }
        // MacBinary I has no CRC: zeros where II's fields are, and its dates, are all it has.
        let mut one = macbinary("Game", b"APPL", b"", &resource, false);
        one[99..128].fill(0);
        assert_eq!(macbinary_version(&one, one.len() as u64), Some(1));
        assert!(!is_image("Game.txt", &one, one.len() as u64), "only with .bin or no ending");
        let volume = open_image(Arc::new(one.clone()), "Game.bin").unwrap();
        assert_eq!(volume.describe(), "MacBinary I \u{B7} Game");
        assert_eq!(names(&volume.list(None).unwrap()), ["Game", "Game.rsrc"]);
        // A header whose forks don't fit in the file isn't MacBinary.
        assert_eq!(macbinary_version(&one[..300], 300), None);
    }

    #[test]
    fn binhex_files_decode_check_their_crcs_and_show_both_forks() {
        let resource = mac::tests::small_resource_fork();
        let data: Vec<u8> = b"BinHex\x90 runs: ".iter().copied().chain(std::iter::repeat_n(b'z', 300)).chain(*b"\x90\x90 end").collect();
        let mut text = b"From: someone\r\nSubject: a file\r\n\r\n".to_vec();
        text.extend(binhex("Notes", b"TEXT", &data, &resource));
        assert!(is_image("notes.hqx", &text, text.len() as u64));
        let volume = open_image(Arc::new(text.clone()), "notes.hqx").unwrap();
        assert_eq!(volume.describe(), "BinHex 4.0 \u{B7} Notes");
        let files = volume.list(None).unwrap();
        assert_eq!(names(&files), ["Notes", "Notes.rsrc"]);
        assert_eq!(volume.read(find(&files, "Notes")).unwrap(), data);
        assert_eq!(volume.read(find(&files, "Notes.rsrc")).unwrap(), resource);
        // A changed character spoils the data fork's CRC, and only that fork is refused.
        let at = text.len() - 40;
        text[at] = if text[at] == b'A' { b'B' } else { b'A' };
        let volume = open_image(Arc::new(text), "notes.hqx").unwrap();
        let files = volume.list(None).unwrap();
        let problems: Vec<bool> = files.iter().map(|f| volume.read(f).is_err()).collect();
        assert_eq!(problems.iter().filter(|p| **p).count(), 1, "one fork is damaged");
        assert!(!volume.notes().is_empty());
    }

    #[test]
    fn wrapped_stuffit_archives_open_a_level_deeper() {
        let archive = super::super::stuffit::tests::archive(&[("Read Me", &b"Hello"[..], &[][..], 0)]);
        let wrapped = macbinary("Stuff.sit", b"SIT!", &archive, &[], false);
        let volume = open_image(Arc::new(wrapped), "Stuff.sit.bin").unwrap();
        let inner = volume.read(&volume.list(None).unwrap()[0]).unwrap();
        assert!(is_nested_image("Stuff.sit", &inner));
        let hqx = binhex("Stuff", b"SIT!", &archive, &[]);
        let volume = open_image(Arc::new(hqx), "Stuff.sit.hqx").unwrap();
        let entry = &volume.list(None).unwrap()[0];
        assert_eq!(entry.open_name, "Stuff.sit", "a StuffIt archive's type gives it the ending to open by");
        assert!(is_nested_image(&entry.open_name, &volume.read(entry).unwrap()));
    }

    #[test]
    fn damaged_wrappers_give_errors_never_panics() {
        let resource = mac::tests::small_resource_fork();
        let good = [macbinary("Read Me", b"TEXT", b"Hello", &resource, true), binhex("Read Me", b"TEXT", b"Hello", &resource)];
        for (index, file) in good.iter().enumerate() {
            let name = if index == 0 { "x.bin" } else { "x.hqx" };
            for cut in [0, 1, 50, 127, 128, 200, file.len() - 1] {
                if let Ok(volume) = open_image(Arc::new(file[..cut.min(file.len())].to_vec()), name) {
                    for entry in volume.list(None).unwrap() {
                        let _ = volume.read(&entry);
                    }
                }
            }
            for seed in 0..200u32 {
                let mut bytes = file.clone();
                let at = (seed.wrapping_mul(2_654_435_761) as usize) % bytes.len();
                bytes[at] ^= (seed as u8) | 1;
                if let Ok(volume) = open_image(Arc::new(bytes), name) {
                    for entry in volume.list(None).unwrap() {
                        let _ = volume.read(&entry);
                    }
                }
            }
        }
        assert!(unrle90(b"a\x90", None).is_err());
        assert!(unrle90(b"a\x90\x01", None).is_err());
        assert_eq!(unrle90(b"a\x90\x04\x90\x00\x90\x03", None).unwrap(), b"aaaa\x90\x90\x90");
    }
}
