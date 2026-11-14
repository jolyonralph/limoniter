//! StuffIt archives built here, packed by small packers written for the tests, and real archives compared with
//! what The Unarchiver's `unar` takes out of them.

use super::super::macarc::crc16_arc;
use super::super::tests::{find, names};
use super::super::{Entry, Volume, is_image, open};
use super::*;
use std::path::{Path, PathBuf};

/// Bits written in turn, low bit of each byte first (LZW, method 13) or top bit first (Huffman).
struct Writer {
    bytes: Vec<u8>,
    count: usize,
    top_first: bool,
}

impl Writer {
    fn new(top_first: bool) -> Self {
        Self { bytes: Vec::new(), count: 0, top_first }
    }

    fn bit(&mut self, bit: u32) {
        if self.count.is_multiple_of(8) {
            self.bytes.push(0);
        }
        if bit != 0 {
            let shift = if self.top_first { 7 - self.count % 8 } else { self.count % 8 };
            *self.bytes.last_mut().unwrap() |= 1 << shift;
        }
        self.count += 1;
    }

    /// A number in `count` bits, as `Bits::bits` reads one back.
    fn bits(&mut self, value: u32, count: u32) {
        for i in 0..count {
            self.bit(if self.top_first { (value >> (count - 1 - i)) & 1 } else { (value >> i) & 1 });
        }
    }

    /// A prefix code, its highest bit first.
    fn code(&mut self, (code, length): (u32, u32)) {
        for place in (0..length).rev() {
            self.bit((code >> place) & 1);
        }
    }
}

/// Run-length packing as StuffIt's method 1 and BinHex do it.
fn rle90(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let byte = data[at];
        let run = data[at..].iter().take(255).take_while(|b| **b == byte).count();
        out.push(byte);
        if byte == 0x90 {
            out.push(0);
        }
        if run > 2 {
            out.extend([0x90, run as u8]);
            at += run;
        } else {
            at += 1;
        }
    }
    out
}

/// `compress`'s LZW, without clearing: codes grow from 9 to 14 bits as the table fills.
fn lzw_pack(data: &[u8]) -> Vec<u8> {
    let mut table: HashMap<(u32, u8), u32> = HashMap::new();
    let mut out = Writer::new(false);
    let mut emitted = 0u32;
    let emit = |out: &mut Writer, code: u32, emitted: &mut u32| {
        let mut width = 9;
        while width < 14 && 256 + *emitted >= 1 << width {
            width += 1;
        }
        out.bits(code, width);
        *emitted += 1;
    };
    let Some((&first, rest)) = data.split_first() else {
        return Vec::new();
    };
    let mut current = u32::from(first);
    for &byte in rest {
        if let Some(&code) = table.get(&(current, byte)) {
            current = code;
            continue;
        }
        emit(&mut out, current, &mut emitted);
        let next = 256 + emitted;
        if next < 1 << 14 {
            table.insert((current, byte), next);
        }
        current = u32::from(byte);
    }
    emit(&mut out, current, &mut emitted);
    out.bytes
}

/// Method 3: a Huffman tree made from the bytes' counts, written top bit first, then the bytes in its code.
fn huffman_pack(data: &[u8]) -> Vec<u8> {
    enum Node {
        Leaf(u8),
        Pair(Box<Node>, Box<Node>),
    }
    if data.is_empty() {
        return Vec::new();
    }
    let mut counts = [0usize; 256];
    data.iter().for_each(|b| counts[usize::from(*b)] += 1);
    let mut nodes: Vec<(usize, Node)> = (0..=255u8).filter(|b| counts[usize::from(*b)] > 0).map(|b| (counts[usize::from(b)], Node::Leaf(b))).collect();
    while nodes.len() > 1 {
        nodes.sort_by_key(|n| std::cmp::Reverse(n.0));
        let (a, b) = (nodes.pop().unwrap(), nodes.pop().unwrap());
        nodes.push((a.0 + b.0, Node::Pair(Box::new(a.1), Box::new(b.1))));
    }
    let mut out = Writer::new(true);
    let mut codes = vec![(0u32, 0u32); 256];
    fn walk(node: &Node, code: (u32, u32), out: &mut Writer, codes: &mut [(u32, u32)]) {
        match node {
            Node::Leaf(b) => {
                out.bit(1);
                out.bits(u32::from(*b), 8);
                codes[usize::from(*b)] = code;
            }
            Node::Pair(zero, one) => {
                out.bit(0);
                walk(zero, (code.0 << 1, code.1 + 1), out, codes);
                walk(one, ((code.0 << 1) | 1, code.1 + 1), out, codes);
            }
        }
    }
    walk(&nodes.pop().unwrap().1, (0, 0), &mut out, &mut codes);
    for b in data {
        out.code(codes[usize::from(*b)]);
    }
    out.bytes
}

/// Canonical codes for lengths, as `Code::from_lengths` makes them.
fn canonical(lengths: &[u8]) -> Vec<(u32, u32)> {
    let mut codes = vec![(0, 0); lengths.len()];
    let mut next = 0;
    for length in 1..=32 {
        for (symbol, _) in lengths.iter().enumerate().filter(|(_, l)| u32::from(**l) == length) {
            codes[symbol] = (next, length);
            next += 1;
        }
        next <<= 1;
    }
    codes
}

/// Method 13 with built-in code set 1: literals, and matches of 3 to 64 bytes up to 1024 back, found greedily.
fn lz_huffman_pack(data: &[u8]) -> Vec<u8> {
    let (first, second, offsets) = (canonical(tables::FIRST_LENGTHS[0]), canonical(tables::SECOND_LENGTHS[0]), canonical(tables::OFFSET_LENGTHS[0]));
    let mut out = Writer::new(false);
    out.bits(0x10, 8);
    let mut after_match = false;
    let mut at = 0;
    while at < data.len() {
        let code = if after_match { &second } else { &first };
        let best = (1..=at.min(1024))
            .map(|offset| (offset, (0..64.min(data.len() - at)).take_while(|i| data[at + i - offset] == data[at + i]).count()))
            .max_by_key(|(offset, length)| (*length, std::cmp::Reverse(*offset)))
            .filter(|(_, length)| *length >= 3);
        match best {
            Some((offset, length)) => {
                out.code(code[0x100 + length - 3]);
                let n = usize::BITS - (offset - 1).leading_zeros();
                match offset {
                    1 => out.code(offsets[0]),
                    2 => out.code(offsets[1]),
                    _ => {
                        out.code(offsets[n as usize]);
                        out.bits((offset - 1 - (1 << (n - 1))) as u32, n - 1);
                    }
                }
                at += length;
                after_match = true;
            }
            None => {
                out.code(code[usize::from(data[at])]);
                at += 1;
                after_match = false;
            }
        }
    }
    out.code(if after_match { second[0x140] } else { first[0x140] });
    out.bytes
}

fn pack(data: &[u8], method: u8) -> Vec<u8> {
    match method {
        1 => rle90(data),
        2 => lzw_pack(data),
        3 => huffman_pack(data),
        13 => lz_huffman_pack(data),
        _ => data.to_vec(),
    }
}

/// An entry of a classic archive to build.
pub enum Item<'a> {
    File { name: &'a str, data: &'a [u8], resource: &'a [u8], method: u8 },
    Folder(&'a str),
    End,
}

/// A classic StuffIt archive of the items, each fork packed with the method given.
pub fn archive_of(items: &[Item]) -> Vec<u8> {
    let mut body = Vec::new();
    let header = |name: &str, methods: (u8, u8)| {
        let mut h = vec![0u8; 112];
        (h[0], h[1], h[2]) = (methods.0, methods.1, name.len() as u8);
        h[3..3 + name.len()].copy_from_slice(name.as_bytes());
        h[66..70].copy_from_slice(b"TEXT");
        h[70..74].copy_from_slice(b"LMNT");
        h[76..80].copy_from_slice(&0xA000_0000u32.to_be_bytes());
        h[80..84].copy_from_slice(&0xA000_0E10u32.to_be_bytes());
        h
    };
    for item in items {
        let mut h;
        let mut packed = Vec::new();
        match item {
            Item::File { name, data, resource, method } => {
                h = header(name, (*method, *method));
                let (r, d) = (pack(resource, *method), pack(data, *method));
                h[84..88].copy_from_slice(&(resource.len() as u32).to_be_bytes());
                h[88..92].copy_from_slice(&(data.len() as u32).to_be_bytes());
                h[92..96].copy_from_slice(&(r.len() as u32).to_be_bytes());
                h[96..100].copy_from_slice(&(d.len() as u32).to_be_bytes());
                h[100..102].copy_from_slice(&crc16_arc(0, resource).to_be_bytes());
                h[102..104].copy_from_slice(&crc16_arc(0, data).to_be_bytes());
                packed = [r, d].concat();
            }
            Item::Folder(name) => h = header(name, (0x20, 0x20)),
            Item::End => h = header("", (0x21, 0x21)),
        }
        let crc = crc16_arc(0, &h[..110]);
        h[110..112].copy_from_slice(&crc.to_be_bytes());
        body.extend(h);
        body.extend(packed);
    }
    let mut out = b"SIT!".to_vec();
    out.extend((items.len() as u16).to_be_bytes());
    out.extend((22 + body.len() as u32).to_be_bytes());
    out.extend(b"rLau\x01\x00");
    out.extend([0u8; 6]);
    out.extend(body);
    out
}

/// A classic archive of files at the top level, each (name, data fork, resource fork, method).
pub fn archive(files: &[(&str, &[u8], &[u8], u8)]) -> Vec<u8> {
    archive_of(&files.iter().map(|(name, data, resource, method)| Item::File { name, data, resource, method: *method }).collect::<Vec<_>>())
}

/// Text with repeats in it, as files mostly are, and some bytes of every value.
fn sample(length: usize) -> Vec<u8> {
    let words: [&[u8]; 9] = [b"the ", b"Mac ", b"StuffIt ", b"archive ", b"fork ", b"\r", b"\x90", b"resource ", b"data "];
    let mut state = 12345u32;
    let mut out = Vec::new();
    while out.len() < length {
        state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
        if state >> 28 == 0 {
            out.extend((0..=255u8).take((state >> 8) as usize % 40));
        } else if state >> 28 == 1 {
            out.extend(std::iter::repeat_n(b'-', 3 + (state >> 8) as usize % 200));
        } else {
            out.extend(words[(state >> 16) as usize % words.len()]);
        }
    }
    out.truncate(length);
    out
}

#[test]
fn the_tables_are_whole() {
    assert!(tables::FIRST_LENGTHS.iter().chain(&tables::SECOND_LENGTHS).all(|set| set.len() == 321));
    assert_eq!(tables::OFFSET_LENGTHS.map(|set| set.len()), [11, 13, 14, 11, 11]);
    for set in tables::FIRST_LENGTHS.iter().chain(&tables::SECOND_LENGTHS).chain(&tables::OFFSET_LENGTHS) {
        assert!(Code::from_lengths(&set.iter().map(|l| i32::from(*l)).collect::<Vec<_>>()).is_ok());
    }
}

#[test]
fn classic_archives_list_folders_and_unpack_every_method_written_here() {
    let resource = mac::tests::small_resource_fork();
    let text = sample(40_000);
    let archive = archive_of(&[
        Item::File { name: "Stored", data: b"Hello from StuffIt\r", resource: &resource, method: 0 },
        Item::Folder("Docs"),
        Item::File { name: "Runs", data: &text, resource: &[], method: 1 },
        Item::Folder("Inner"),
        Item::File { name: "Packed", data: &text, resource: &resource, method: 2 },
        Item::End,
        Item::File { name: "Squeezed", data: &text, resource: &[], method: 3 },
        Item::End,
        Item::File { name: "New method", data: &text, resource: &resource, method: 13 },
    ]);
    assert!(is_image("Stuff.sit", &archive, archive.len() as u64));
    assert!(is_image("Stuff", &archive, archive.len() as u64), "known by its signature with no ending");
    let volume = open(Arc::new(archive), "Stuff.sit").unwrap();
    assert_eq!(volume.describe(), "StuffIt archive \u{B7} 5 file(s)");
    let top = volume.list(None).unwrap();
    assert_eq!(names(&top), ["Docs", "Stored", "Stored.rsrc", "New method", "New method.rsrc"]);
    assert_eq!(volume.read(find(&top, "Stored")).unwrap(), b"Hello from StuffIt\r");
    assert_eq!(volume.read(find(&top, "Stored.rsrc")).unwrap(), resource);
    assert!(find(&top, "Stored").detail.starts_with("TEXT \u{B7} LMNT \u{B7} 1989-"), "{}", find(&top, "Stored").detail);
    assert_eq!(volume.read(find(&top, "New method")).unwrap(), text);
    assert_eq!(volume.read(find(&top, "New method.rsrc")).unwrap(), resource);
    let docs = volume.list(Some(find(&top, "Docs"))).unwrap();
    assert_eq!(names(&docs), ["Inner", "Runs", "Squeezed"]);
    assert_eq!(volume.read(find(&docs, "Runs")).unwrap(), text);
    assert_eq!(volume.read(find(&docs, "Squeezed")).unwrap(), text);
    let inner = volume.list(Some(find(&docs, "Inner"))).unwrap();
    assert_eq!(volume.read(find(&inner, "Packed")).unwrap(), text);
    assert_eq!(volume.read(find(&inner, "Packed.rsrc")).unwrap(), resource);
    assert_eq!(volume.text_preset(find(&inner, "Packed")), Some(mac::MAC_TEXT_PRESET));
}

#[test]
fn unsupported_encrypted_and_damaged_entries_say_why() {
    let text = sample(5000);
    let mut archive = archive(&[("Odd", &text[..], &[][..], 6), ("Good", &text[..], &[][..], 2)]);
    let volume = open(Arc::new(archive.clone()), "x.sit").unwrap();
    let top = volume.list(None).unwrap();
    let problem = volume.read(find(&top, "Odd")).unwrap_err();
    assert!(problem.contains("method 6 (Fixed Huffman)"), "{problem}");
    assert!(find(&top, "Odd").detail.ends_with("Fixed Huffman, not unpacked"));
    // Spoiling a packed byte is caught by the CRC (or by the packed data making no sense).
    let good = find(&top, "Good").clone();
    let end = archive.len() - 100;
    archive[end] ^= 0x55;
    let volume = open(Arc::new(archive.clone()), "x.sit").unwrap();
    assert!(volume.read(&good).is_err());
    // An encrypted fork is named as such.
    let locked = super::tests::archive(&[("Locked", &text[..], &[][..], 0x80)]);
    let volume = open(Arc::new(locked), "x.sit").unwrap();
    assert!(volume.read(&volume.list(None).unwrap()[0]).unwrap_err().contains("encrypted"));
    // A spoiled header stops the listing there.
    archive[22 + 5] ^= 1;
    assert!(open(Arc::new(archive), "x.sit").err().unwrap().contains("CRC"));
}

#[test]
fn damaged_stuffit_archives_give_errors_never_panics() {
    let resource = mac::tests::small_resource_fork();
    let text = sample(3000);
    let files: Vec<(&str, &[u8], &[u8], u8)> = [0, 1, 2, 3, 13].iter().map(|m| ("F", &text[..], &resource[..], *m)).collect();
    let good = archive(&files);
    for seed in 0..300u32 {
        let mut bytes = good.clone();
        let mut state = seed.wrapping_mul(2_654_435_761);
        for _ in 0..1 + seed % 8 {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let at = 22 + (state as usize >> 4) % (bytes.len() - 22);
            bytes[at] ^= (state >> 24) as u8 | 1;
        }
        if let Ok(volume) = open(Arc::new(bytes), "x.sit") {
            for entry in volume.list(None).unwrap() {
                let _ = volume.read(&entry);
            }
        }
    }
    // Packed data of each kind, wrong from the start, is refused without a panic.
    for method in [1, 2, 3, 5, 8, 13, 15] {
        for seed in 0..50u8 {
            let junk: Vec<u8> = (0..200u32).map(|i| (i.wrapping_mul(u32::from(seed) * 2 + 7) >> 3) as u8 ^ seed).collect();
            let packed = Packed { at: 0, length: junk.len() as u64, method, crc: 0, encrypted: false };
            let volume = StuffIt { source: Arc::new(junk), members: Vec::new(), what: "", notes: Vec::new() };
            let _ = volume.unpack(&packed, 5000);
        }
    }
}

/// A StuffIt 5 archive of files in a folder, as its headers are laid out, with stored forks.
fn archive5(files: &[(&str, &[u8], &[u8])]) -> Vec<u8> {
    let mut out = STUFFIT5_TEXT.iter().map(|b| if *b == b'?' { b'9' } else { *b }).collect::<Vec<u8>>();
    out.resize(100, 0);
    out[82] = 5;
    out[92..94].copy_from_slice(&1u16.to_be_bytes());
    out[94..98].copy_from_slice(&100u32.to_be_bytes());
    let entry = |name: &str, flags: u8, folder_at: u32, sizes: [u32; 2], extra: &[u8], second: &[u8]| {
        let mut h = vec![0u8; 48];
        h[..4].copy_from_slice(&0xA5A5_A5A5u32.to_be_bytes());
        h[4] = 1;
        h[6..8].copy_from_slice(&(48 + name.len() as u16).to_be_bytes());
        h[9] = flags;
        h[10..14].copy_from_slice(&0xA000_0000u32.to_be_bytes());
        h[14..18].copy_from_slice(&0xA000_0E10u32.to_be_bytes());
        h[26..30].copy_from_slice(&folder_at.to_be_bytes());
        h[30..32].copy_from_slice(&(name.len() as u16).to_be_bytes());
        h[34..38].copy_from_slice(&sizes[0].to_be_bytes());
        h[38..42].copy_from_slice(&sizes[1].to_be_bytes());
        h[42..44].copy_from_slice(&extra[..2]);
        h[46..48].copy_from_slice(&extra[2..4]);
        h.extend(name.as_bytes());
        h.extend(second);
        h
    };
    let second = |resource: Option<&[u8]>| {
        let mut s = vec![0u8; 4];
        s[1] = u8::from(resource.is_some());
        s.extend(b"TEXTLMNT");
        s.extend([0u8; 2 + 22]);
        if let Some(r) = resource {
            s.extend((r.len() as u32).to_be_bytes());
            s.extend((r.len() as u32).to_be_bytes());
            s.extend(crc16_arc(0, r).to_be_bytes());
            s.extend([0, 0, 0, 0]);
        }
        s
    };
    let folder_at = out.len() as u32;
    out.extend(entry("Folder", 0x40, 0, [0, 0], &[0, 0, 0, files.len() as u8], &second(None)));
    for (name, data, resource) in files {
        let crc = crc16_arc(0, data).to_be_bytes();
        let resource = (!resource.is_empty()).then_some(*resource);
        out.extend(entry(name, 0, folder_at, [data.len() as u32, data.len() as u32], &[crc[0], crc[1], 0, 0], &second(resource)));
        out.extend(resource.unwrap_or_default());
        out.extend(*data);
    }
    out
}

#[test]
fn stuffit5_archives_follow_their_folders() {
    let resource = mac::tests::small_resource_fork();
    let archive = archive5(&[("Read Me", b"Hello from StuffIt 5", &resource), ("Notes", b"More", &[])]);
    assert!(is_image("x.sit", &archive, archive.len() as u64));
    let volume = open(Arc::new(archive), "x.sit").unwrap();
    assert_eq!(volume.describe(), "StuffIt 5 archive \u{B7} 2 file(s)");
    let top = volume.list(None).unwrap();
    assert_eq!(names(&top), ["Folder"]);
    let inside = volume.list(Some(find(&top, "Folder"))).unwrap();
    assert_eq!(names(&inside), ["Read Me", "Read Me.rsrc", "Notes"]);
    assert_eq!(volume.read(find(&inside, "Read Me")).unwrap(), b"Hello from StuffIt 5");
    assert_eq!(volume.read(find(&inside, "Read Me.rsrc")).unwrap(), resource);
    assert_eq!(volume.read(find(&inside, "Notes")).unwrap(), b"More");
}

// ---------- Compared with unar ----------

/// A resource fork from an AppleDouble file, as `unar` writes one beside each file that has a fork.
fn apple_double_fork(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.get(..4)? != [0, 5, 0x16, 0x07] {
        return None;
    }
    let count = usize::from(u16::from_be_bytes(bytes.get(24..26)?.try_into().ok()?));
    (0..count).find_map(|i| {
        let e = bytes.get(26 + i * 12..38 + i * 12)?;
        let at = |r: std::ops::Range<usize>| u32::from_be_bytes(e[r].try_into().unwrap()) as usize;
        (at(0..4) == 2).then(|| bytes.get(at(4..8)..at(4..8) + at(8..12)).map(<[u8]>::to_vec))?
    })
}

/// Every file in a volume, with its path, read or with why not; images inside aren't opened.
fn files_of(volume: &dyn Volume, dir: Option<&Entry>, prefix: &str, out: &mut Vec<(String, String, Result<Vec<u8>, String>)>) {
    for entry in volume.list(dir).unwrap_or_default() {
        let path = format!("{prefix}{}", entry.name);
        if entry.is_dir {
            files_of(volume, Some(&entry), &format!("{path}/"), out);
        } else {
            let how = entry.detail.rsplit(" \u{B7} ").next().unwrap_or_default().to_string();
            out.push((path, how, volume.read(&entry)));
        }
    }
}

/// A path made comparable whatever each side did with characters a file name can't have.
fn plain(path: &str) -> String {
    path.split('/').map(|part| part.trim().chars().map(|c| if c.is_alphanumeric() || c == '.' { c } else { '_' }).collect::<String>()).collect::<Vec<_>>().join("/")
}

/// What `unar` takes out of an archive, one level deep: each file's data, and each resource fork as `name.rsrc`.
fn unar_files(unar: &Path, archive: &Path, scratch: &Path) -> Option<HashMap<String, Vec<u8>>> {
    // unar 1.10 now and then crashes part way, so it's given a few goes.
    let mut done = None;
    for _ in 0..5 {
        let _ = std::fs::remove_dir_all(scratch);
        std::fs::create_dir_all(scratch).ok()?;
        let output = std::process::Command::new(unar).args(["-nr", "-f", "-D", "-k", "visible", "-o"]).arg(scratch).arg(archive).output().ok()?;
        if output.status.code().is_some() {
            done = Some(output);
            break;
        }
    }
    let done = done?;
    // Files it can't take out are named in its report ("  name  (126 B)... Failed!"), and left out.
    let report = String::from_utf8_lossy(&done.stdout).into_owned();
    let failed: Vec<String> = report
        .lines()
        .filter(|l| l.contains("... Failed!"))
        .filter_map(|l| l.rsplit_once("  ("))
        .map(|(name, size)| plain(&if size.contains(", rsrc)") { format!("{name}.rsrc") } else { name.to_string() }))
        .collect();
    let mut out = HashMap::new();
    let mut folders = vec![scratch.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for item in std::fs::read_dir(&folder).ok()?.flatten() {
            let path = item.path();
            if path.is_dir() {
                folders.push(path);
                continue;
            }
            let bytes = std::fs::read(&path).ok()?;
            let name = plain(&path.strip_prefix(scratch).ok()?.to_string_lossy());
            match apple_double_fork(&bytes).filter(|_| name.ends_with(".rsrc")) {
                Some(fork) => out.insert(name, fork),
                None => out.insert(name, bytes),
            };
        }
    }
    out.retain(|name, _| !failed.contains(name));
    Some(out)
}

/// What macutils' `macunpack -3` takes out of a StuffIt archive: `name.data` is the data fork, `name.rsrc` the
/// resource fork.
fn macunpack_files(macunpack: &Path, archive: &Path, scratch: &Path) -> Option<HashMap<String, Vec<u8>>> {
    let _ = std::fs::remove_dir_all(scratch);
    std::fs::create_dir_all(scratch).ok()?;
    std::process::Command::new(macunpack).arg("-3").arg(archive).current_dir(scratch).output().ok()?;
    let mut out = HashMap::new();
    let mut folders = vec![scratch.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for item in std::fs::read_dir(&folder).ok()?.flatten() {
            let path = item.path();
            if path.is_dir() {
                folders.push(path);
                continue;
            }
            let name = path.strip_prefix(scratch).ok()?.to_string_lossy().into_owned();
            if let Some(data) = name.strip_suffix(".data") {
                out.insert(plain(data), std::fs::read(&path).ok()?);
            } else if name.ends_with(".rsrc") {
                out.insert(plain(&name), std::fs::read(&path).ok()?);
            }
        }
    }
    Some(out)
}

/// Real archives (StuffIt, MacBinary, BinHex) from ../testmedia/mac-archives, or the folder LIMONITER_MAC_ARCHIVES
/// names, opened and every fork compared byte for byte with what `unar` takes out. Skipped without the folder or
/// `unar`. Whatever either side can't read is reported; anything read differently fails.
#[test]
fn real_mac_archives_match_unar() {
    let root = std::env::var_os("LIMONITER_MAC_ARCHIVES").map_or_else(|| PathBuf::from("../testmedia/mac-archives"), PathBuf::from);
    let unar = std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|p| p.join("unar")).find(|p| p.is_file()));
    let (true, Some(unar)) = (root.is_dir(), unar) else {
        return;
    };
    let scratch = std::env::temp_dir().join(format!("limoniter-unar-{}", std::process::id()));
    let mut archives = Vec::new();
    let mut folders = vec![root.clone()];
    while let Some(folder) = folders.pop() {
        for item in std::fs::read_dir(&folder).unwrap().flatten() {
            let path = item.path();
            if path.is_dir() {
                folders.push(path);
            } else {
                archives.push(path);
            }
        }
    }
    archives.sort();
    let (mut same, mut same_second, mut ours_only, mut theirs_only, mut neither) = (0, 0, Vec::new(), Vec::new(), Vec::new());
    let macunpack = std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|p| p.join("macunpack")).find(|p| p.is_file()));
    let mut wrong = Vec::new();
    let mut methods: HashMap<String, usize> = HashMap::new();
    // Archives inside MacBinary and BinHex files are written out and compared in turn.
    let inner = std::env::temp_dir().join(format!("limoniter-inner-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&inner);
    let mut next = 0;
    while next < archives.len() {
        let path = archives[next].clone();
        next += 1;
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).unwrap();
        let head = &bytes[..bytes.len().min(0x8800)];
        let ending = Path::new(&name).extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
        if super::super::macarc::kind_of(head, bytes.len() as u64, &ending).is_none() {
            continue;
        }
        let shown = path.strip_prefix(&root).or_else(|_| path.strip_prefix(&inner)).unwrap().to_string_lossy().into_owned();
        // Damaged copies give errors, never a panic.
        for seed in 0..3u32 {
            let mut damaged = bytes.clone();
            let mut state = seed.wrapping_add(damaged.len() as u32).wrapping_mul(2_654_435_761);
            for _ in 0..8 {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let at = (state as usize >> 3) % damaged.len();
                damaged[at] ^= (state >> 24) as u8 | 1;
            }
            if let Ok(volume) = open(Arc::new(damaged), &name) {
                files_of(&*volume, None, "", &mut Vec::new());
            }
        }
        let volume = match open(Arc::new(bytes), &name) {
            Ok(volume) => volume,
            Err(problem) => {
                ours_only.push(format!("{shown}: {problem}"));
                continue;
            }
        };
        let mut ours = Vec::new();
        files_of(&*volume, None, "", &mut ours);
        let theirs = unar_files(&unar, &path, &scratch).unwrap_or_default();
        // macutils' macunpack is asked about what unar couldn't take out.
        let mut second: Option<HashMap<String, Vec<u8>>> = None;
        // Two files of the same name in a folder can't both be written out, so aren't compared.
        let mut seen: HashMap<String, usize> = HashMap::new();
        ours.iter().for_each(|(file, _, _)| *seen.entry(plain(file)).or_default() += 1);
        for (file, how, result) in ours {
            let key = plain(&file);
            if seen[&key] > 1 {
                continue;
            }
            if let Ok(bytes) = &result
                && (super::super::stuffit::looks_like_stuffit(bytes) || super::super::stuffit::looks_like_stuffit5(bytes))
            {
                let at = inner.join(format!("{} {}", archives.len(), plain(&file).replace('/', " ")));
                std::fs::write(&at, bytes).unwrap();
                archives.push(at);
            }
            *methods.entry(how).or_default() += 1;
            match (result, theirs.get(&key)) {
                (Ok(mine), Some(reference)) if &mine == reference => same += 1,
                (Ok(mine), None) if mine.is_empty() => same += 1,
                // unar guesses names' character sets, so a name it read another way is found by its bytes.
                (Ok(mine), None) if theirs.values().any(|reference| *reference == mine) => same += 1,
                (Ok(mine), Some(reference)) => wrong.push(format!("{shown} {file}: {} bytes, unar {} bytes", mine.len(), reference.len())),
                (Ok(mine), None) => {
                    let second = second.get_or_insert_with(|| macunpack.as_deref().and_then(|m| macunpack_files(m, &path, &scratch)).unwrap_or_default());
                    match second.get(&key) {
                        Some(reference) if *reference == mine => same_second += 1,
                        Some(reference) => wrong.push(format!("{shown} {file}: {} bytes, macunpack {} bytes", mine.len(), reference.len())),
                        None if second.values().any(|reference| *reference == mine) => same_second += 1,
                        None => theirs_only.push(format!("{shown} {file}: neither unar nor macunpack wrote it")),
                    }
                }
                (Err(problem), Some(_)) => ours_only.push(format!("{shown} {file}: {problem}")),
                (Err(problem), None) => neither.push(problem),
            }
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
    let _ = std::fs::remove_dir_all(&inner);
    eprintln!("{} archive(s): {same} fork(s) the same as unar's, {same_second} more the same as macunpack's; {} neither could read", archives.len(), neither.len());
    let mut why: HashMap<&str, usize> = HashMap::new();
    neither.iter().for_each(|problem| *why.entry(problem.as_str()).or_default() += 1);
    eprintln!("Why neither could: {why:#?}");
    eprintln!("Forks by how they were packed: {methods:?}");
    eprintln!("Limoniter couldn't read, unar could ({}):\n{}", ours_only.len(), ours_only.join("\n"));
    eprintln!("unar couldn't read ({}):\n{}", theirs_only.len(), theirs_only.join("\n"));
    assert!(wrong.is_empty(), "read differently from unar:\n{}", wrong.join("\n"));
}
