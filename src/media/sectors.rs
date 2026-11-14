//! Sector-by-sector floppy images: ImageDisk (`.imd`, Dave Dunfield's) and Teledisk (`.td0`, Sydex's), as his
//! documents for both lay them out. Each records every track's sectors as the controller read them, with their
//! numbers, sizes and whether they read cleanly, which keeps odd formats such as the HP 150's (sixteen 256-byte
//! sectors numbered from 0 on one side) that a plain image can't say anything about.
//!
//! The sectors are laid out in order, as a plain image of the disk, using the geometry most of the tracks have.
//! Sectors that couldn't be read when the image was made are left as zeros and listed, and the disk inside is opened
//! as whatever file system it holds. The plain image itself is offered too, so a disk whose file system Limoniter
//! doesn't know can still be looked at in hex or saved for another tool.

use super::{Entry, Source, Volume};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// A sector as the image recorded it.
pub(super) struct Sector {
    pub id: u8,
    pub size: usize,
    /// None when the image has no data for it: it couldn't be read.
    pub data: Option<Vec<u8>>,
    pub error: bool,
}

/// A track's sectors, by cylinder and head.
pub(super) type Tracks = BTreeMap<(u8, u8), Vec<Sector>>;

/// What's known of a disk once its tracks are laid out.
pub(super) struct Laid {
    pub bytes: Vec<u8>,
    pub shape: String,
    pub notes: Vec<String>,
    /// The tracks' shape, for a file system such as CP/M's that has to be told it.
    pub tracks: super::cpm::Shape,
}

/// The sectors in order, in the geometry most tracks have: sector size, first sector number and sectors per track.
pub(super) fn lay_out(tracks: &Tracks) -> Result<Laid, String> {
    let mut sizes: HashMap<usize, usize> = HashMap::new();
    for sector in tracks.values().flatten().filter(|s| s.data.is_some()) {
        *sizes.entry(sector.size).or_default() += 1;
    }
    let size = sizes.iter().max_by_key(|(size, count)| (**count, std::cmp::Reverse(**size))).map(|(size, _)| *size).ok_or("The image holds no sectors with data.")?;
    // The run of numbers each track has: from its lowest numbered sector of that size, while the numbers go on.
    let mut runs: HashMap<(u8, usize), usize> = HashMap::new();
    for sectors in tracks.values() {
        let mut ids: Vec<u8> = sectors.iter().filter(|s| s.size == size).map(|s| s.id).collect();
        ids.sort_unstable();
        ids.dedup();
        if let Some(&first) = ids.first() {
            let length = ids.iter().enumerate().take_while(|(i, id)| usize::from(**id) == usize::from(first) + i).count();
            *runs.entry((first, length)).or_default() += 1;
        }
    }
    let (first, per_track) = runs.iter().max_by_key(|(_, count)| **count).map(|(run, _)| *run).ok_or("The image holds no tracks.")?;
    let heads: Vec<u8> = {
        let mut heads: Vec<u8> = tracks.keys().map(|(_, head)| *head).collect();
        heads.sort_unstable();
        heads.dedup();
        heads
    };
    // A track recorded with no sectors at all, as Teledisk can end with, isn't part of the disk.
    let cylinders = tracks.iter().filter(|(_, sectors)| !sectors.is_empty()).map(|((cylinder, _), _)| *cylinder).max().unwrap_or(0) as usize + 1;
    let mut bytes = Vec::with_capacity(cylinders * heads.len() * per_track * size);
    let (mut missing, mut errors, mut extra): (Vec<String>, Vec<String>, usize) = (Vec::new(), Vec::new(), 0);
    for cylinder in 0..cylinders as u8 {
        for &head in &heads {
            let sectors = tracks.get(&(cylinder, head)).map(Vec::as_slice).unwrap_or_default();
            let place = if heads.len() > 1 { format!("track {cylinder} side {head}") } else { format!("track {cylinder}") };
            let mut gone = Vec::new();
            for number in 0..per_track {
                let id = first as usize + number;
                match sectors.iter().find(|s| usize::from(s.id) == id && s.size == size && s.data.is_some()) {
                    Some(sector) => {
                        bytes.extend_from_slice(sector.data.as_deref().unwrap_or_default());
                        if sector.error {
                            errors.push(format!("{place} sector {id}"));
                        }
                    }
                    None => {
                        bytes.resize(bytes.len() + size, 0);
                        gone.push(id);
                    }
                }
            }
            extra += sectors.iter().filter(|s| s.data.is_some() && (s.size != size || usize::from(s.id) < first as usize || usize::from(s.id) >= first as usize + per_track)).count();
            match gone.len() {
                0 => {}
                n if n == per_track => missing.push(format!("{place} (all {n})")),
                _ => missing.push(format!("{place} sector{} {}", if gone.len() > 1 { "s" } else { "" }, gone.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "))),
            }
        }
    }
    let list = |places: &[String]| if places.len() > 8 { format!("{} and {} more", places[..8].join("; "), places.len() - 8) } else { places.join("; ") };
    let mut notes = Vec::new();
    if !missing.is_empty() {
        notes.push(format!("Sectors that couldn't be read when the image was made are zeros here: {}.", list(&missing)));
    }
    if !errors.is_empty() {
        notes.push(format!("Sectors read with a data error, so they may be wrong: {}.", list(&errors)));
    }
    if extra > 0 {
        notes.push(format!("{extra} sector(s) outside the tracks' usual numbering or size aren't part of the disk here."));
    }
    let sides = if heads.len() == 1 { "1 side".to_string() } else { format!("{} sides", heads.len()) };
    let shape = format!("{cylinders} tracks, {sides}, {per_track} \u{D7} {size}-byte sectors numbered from {first}");
    Ok(Laid { bytes, shape, notes, tracks: super::cpm::Shape { sector_size: size as u64, per_track: per_track as u64, heads: heads.len() as u64 } })
}

// ---------- ImageDisk ----------

pub fn looks_like_imd(head: &[u8]) -> bool {
    head.starts_with(b"IMD ") && head.iter().take(8192).any(|b| *b == 0x1A)
}

/// An ImageDisk file's comment and tracks. After the text header, ended by &1A, each track is its mode, cylinder,
/// head (with flags for the optional maps), sector count and size code, the sectors' numbers, then each sector as a
/// type byte and its data: none, a whole sector, or one byte filling it.
fn read_imd(file: &[u8]) -> Result<(String, Tracks), String> {
    let end = file.iter().position(|b| *b == 0x1A).ok_or("This ImageDisk file's header never ends.")?;
    let text = String::from_utf8_lossy(&file[..end]).replace("\r\n", "\n");
    let comment = text.split_once('\n').map_or("", |(_, rest)| rest).trim().to_string();
    let mut tracks = Tracks::new();
    let mut at = end + 1;
    let take = |at: &mut usize, n: usize| -> Result<&[u8], String> {
        let bytes = file.get(*at..*at + n).ok_or("The ImageDisk file ends partway through a track.")?;
        *at += n;
        Ok(bytes)
    };
    while at < file.len() {
        let head = take(&mut at, 5)?;
        let (cylinder, side, count, size_code) = (head[1], head[2], usize::from(head[3]), head[4]);
        let numbers = take(&mut at, count)?.to_vec();
        if side & 0x80 != 0 {
            take(&mut at, count)?;
        }
        if side & 0x40 != 0 {
            take(&mut at, count)?;
        }
        let sizes: Vec<usize> = if size_code == 0xFF {
            take(&mut at, count * 2)?.chunks(2).map(|c| usize::from(u16::from_le_bytes([c[0], c[1]]))).collect()
        } else if size_code <= 6 {
            vec![128 << size_code; count]
        } else {
            return Err(format!("Track {cylinder} has a sector size code ({size_code}) ImageDisk doesn't use."));
        };
        let mut sectors = Vec::with_capacity(count);
        for (number, size) in numbers.into_iter().zip(sizes) {
            let kind = take(&mut at, 1)?[0];
            let data = match kind {
                0 => None,
                1 | 3 | 5 | 7 => Some(take(&mut at, size)?.to_vec()),
                2 | 4 | 6 | 8 => Some(vec![take(&mut at, 1)?[0]; size]),
                _ => return Err(format!("Track {cylinder} has a sector record of a kind ({kind}) ImageDisk doesn't write.")),
            };
            sectors.push(Sector { id: number, size, data, error: kind >= 5 });
        }
        tracks.entry((cylinder, side & 1)).or_default().extend(sectors);
    }
    Ok((comment, tracks))
}

// ---------- DMK, JV3 and JV1 (TRS-80) ----------

/// A DMK image (David Keil's): a 16-byte header (write protect, tracks, each track's length, options: bit 4 one side,
/// bit 6 single-density bytes kept once), then each track as recorded: a table of 64 pointers to its sectors' ID
/// marks (bit 15 set for a double-density one), then the track's bytes. Single-density bytes are kept twice unless
/// the header says otherwise.
pub fn looks_like_dmk(head: &[u8], len: u64) -> bool {
    if head.len() < 16 || !matches!(head[0], 0 | 0xFF) || head[1] == 0 || head[1] > 96 || head[5..12].iter().any(|&b| b != 0) {
        return false;
    }
    let track = u64::from(u16::from_le_bytes([head[2], head[3]]));
    let sides = if head[4] & 0x10 != 0 { 1 } else { 2 };
    (0x0C00..=0x4000).contains(&track) && len >= 16 + u64::from(head[1]) * track * sides
}

fn read_dmk(file: &[u8]) -> Result<Tracks, String> {
    let track_length = usize::from(u16::from_le_bytes([file[2], file[3]]));
    let sides = if file[4] & 0x10 != 0 { 1 } else { 2 };
    let single_kept_once = file[4] & 0x40 != 0;
    let mut tracks = Tracks::new();
    for cylinder in 0..usize::from(file[1]) {
        for side in 0..sides {
            let at = 16 + (cylinder * sides + side) * track_length;
            let Some(track) = file.get(at..at + track_length) else {
                break;
            };
            let mut sectors = Vec::new();
            for slot in 0..64 {
                let pointer = usize::from(u16::from_le_bytes([track[slot * 2], track[slot * 2 + 1]]));
                if pointer == 0 {
                    break;
                }
                let double = pointer & 0x8000 != 0;
                let step = if double || single_kept_once { 1 } else { 2 };
                let id = pointer & 0x3FFF;
                let byte = |n: usize| track.get(id + n * step).copied();
                if byte(0) != Some(0xFE) {
                    continue;
                }
                let (Some(number), Some(code)) = (byte(3), byte(4)) else {
                    continue;
                };
                let size = 128usize << (code & 3);
                // The data mark (&F8 to &FB) follows the ID field's CRC within a gap of a few dozen bytes.
                let data = (7..7 + if double { 60 } else { 40 }).find(|&n| byte(n).is_some_and(|b| (0xF8..=0xFB).contains(&b))).and_then(|mark| (0..size).map(|n| byte(mark + 1 + n)).collect::<Option<Vec<u8>>>());
                sectors.push(Sector { id: number, size, data, error: false });
            }
            if !sectors.is_empty() {
                tracks.entry((cylinder as u8, side as u8)).or_default().extend(sectors);
            }
        }
    }
    Ok(tracks)
}

/// A JV3 image (Jeff Vavasour's): a table of 2901 sectors (track, sector, flags: bit 7 double density, bit 4 the
/// second side, bit 3 a CRC error, bits 0-1 the size: 256, 128, 1024 or 512), a write-protect byte, then those
/// sectors' data in the same order. A larger disk has a second table after the first one's data.
pub fn looks_like_jv3(head: &[u8], len: u64) -> bool {
    if head.len() < 8704 || len <= 8704 {
        return false;
    }
    let entries: Vec<&[u8; 3]> = head[..8703].as_chunks::<3>().0.iter().collect();
    let used: Vec<&&[u8; 3]> = entries.iter().filter(|e| e[0] != 0xFF).collect();
    !used.is_empty() && used.iter().all(|e| e[0] < 96 && e[1] < 64) && used.iter().any(|e| e[0] == 0) && matches!(head[8703], 0 | 0xFF)
}

fn read_jv3(file: &[u8]) -> Result<Tracks, String> {
    const SIZES: [usize; 4] = [256, 128, 1024, 512];
    let mut tracks = Tracks::new();
    let mut table = 0;
    while table + 8704 <= file.len() {
        let mut at = table + 8704;
        let mut any = false;
        for entry in file[table..table + 8703].as_chunks::<3>().0 {
            if entry[0] == 0xFF {
                continue;
            }
            any = true;
            let size = SIZES[usize::from(entry[2] & 3)];
            let data = file.get(at..at + size).map(<[u8]>::to_vec);
            at += size;
            tracks.entry((entry[0], (entry[2] >> 4) & 1)).or_default().push(Sector { id: entry[1], size, data, error: entry[2] & 0x08 != 0 });
        }
        if !any {
            break;
        }
        table = at;
    }
    Ok(tracks)
}

/// A JV1 image: a TRS-80 Model I disk as it is, ten 256-byte sectors a track numbered from 0, one side. Only one
/// whose first sector is a TRS-80 boot sector (starting &00 &FE, then the directory's track) is taken for one.
pub fn looks_like_jv1(head: &[u8], len: u64) -> bool {
    len.is_multiple_of(2560) && (35 * 2560..=96 * 2560).contains(&len) && head.len() >= 3 && head[0] == 0 && head[1] == 0xFE && (10..=40).contains(&head[2])
}

fn read_jv1(file: &[u8]) -> Tracks {
    let mut tracks = Tracks::new();
    for (cylinder, track) in file.as_chunks::<2560>().0.iter().enumerate() {
        let sectors = track.as_chunks::<256>().0.iter().enumerate().map(|(n, data)| Sector { id: n as u8, size: 256, data: Some(data.to_vec()), error: false }).collect();
        tracks.insert((cylinder as u8, 0), sectors);
    }
    tracks
}

/// The sector-by-sector image formats.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Imd,
    Td0,
    Dmk,
    Jv3,
    Jv1,
}

// ---------- Teledisk ----------

pub fn looks_like_td0(head: &[u8]) -> bool {
    head.len() >= 12 && (head.starts_with(b"TD") || head.starts_with(b"td")) && head[2] == 0 && (10..=30).contains(&head[4])
}

/// Teledisk's "advanced compression": Okumura and Yoshizaki's LZHUF, a 4K window of LZSS matches and literals coded
/// by an adaptive Huffman tree, the window starting as spaces.
fn lzhuf(input: &[u8], most: usize) -> Vec<u8> {
    const N: usize = 4096;
    const F: usize = 60;
    const THRESHOLD: usize = 2;
    const N_CHAR: usize = 256 - THRESHOLD + F;
    const T: usize = N_CHAR * 2 - 1;
    const R: usize = T - 1;
    const MAX_FREQ: u32 = 0x8000;

    struct Bits<'a> {
        input: &'a [u8],
        at: usize,
        buffer: u32,
        count: u32,
    }
    impl Bits<'_> {
        fn fill(&mut self) {
            while self.count <= 8 {
                let byte = self.input.get(self.at).copied().unwrap_or(0);
                self.at += 1;
                self.buffer |= u32::from(byte) << (8 - self.count);
                self.count += 8;
            }
        }
        fn bit(&mut self) -> usize {
            self.fill();
            let bit = (self.buffer >> 15) & 1;
            self.buffer = (self.buffer << 1) & 0xFFFF;
            self.count -= 1;
            bit as usize
        }
        fn byte(&mut self) -> usize {
            self.fill();
            let byte = (self.buffer >> 8) & 0xFF;
            self.buffer = (self.buffer << 8) & 0xFFFF;
            self.count -= 8;
            byte as usize
        }
        fn ended(&self) -> bool {
            self.at > self.input.len() + 2
        }
    }

    let mut freq = vec![0u32; T + 1];
    let mut son = vec![0usize; T];
    let mut prnt = vec![0usize; T + N_CHAR];
    for i in 0..N_CHAR {
        freq[i] = 1;
        son[i] = i + T;
        prnt[i + T] = i;
    }
    let (mut i, mut j) = (0, N_CHAR);
    while j <= R {
        freq[j] = freq[i] + freq[i + 1];
        son[j] = i;
        prnt[i] = j;
        prnt[i + 1] = j;
        i += 2;
        j += 1;
    }
    freq[T] = 0xFFFF;
    prnt[R] = 0;

    let reconstruct = |freq: &mut Vec<u32>, son: &mut Vec<usize>, prnt: &mut Vec<usize>| {
        let mut j = 0;
        for i in 0..T {
            if son[i] >= T {
                freq[j] = freq[i].div_ceil(2);
                son[j] = son[i];
                j += 1;
            }
        }
        let (mut i, mut j) = (0, N_CHAR);
        while j < T {
            let f = freq[i] + freq[i + 1];
            freq[j] = f;
            let mut k = j - 1;
            while f < freq[k] {
                k -= 1;
            }
            k += 1;
            freq.copy_within(k..j, k + 1);
            freq[k] = f;
            son.copy_within(k..j, k + 1);
            son[k] = i;
            i += 2;
            j += 1;
        }
        for (i, &k) in son.iter().enumerate().take(T) {
            prnt[k] = i;
            if k < T {
                prnt[k + 1] = i;
            }
        }
    };
    let update = |c: usize, freq: &mut Vec<u32>, son: &mut Vec<usize>, prnt: &mut Vec<usize>| {
        if freq[R] == MAX_FREQ {
            reconstruct(freq, son, prnt);
        }
        let mut c = prnt[c + T];
        loop {
            freq[c] += 1;
            let k = freq[c];
            let mut l = c + 1;
            if k > freq[l] {
                while k > freq[l + 1] {
                    l += 1;
                }
                freq[c] = freq[l];
                freq[l] = k;
                let i = son[c];
                prnt[i] = l;
                if i < T {
                    prnt[i + 1] = l;
                }
                let j = son[l];
                son[l] = i;
                prnt[j] = c;
                if j < T {
                    prnt[j + 1] = c;
                }
                son[c] = j;
                c = l;
            }
            c = prnt[c];
            if c == 0 {
                break;
            }
        }
    };
    // The top six bits of a match's distance: 32 codes of 3 bits, then 48 of 4, 64 of 5, 48 of 6, 48 of 7, 16 of 8.
    let mut d_code = [0usize; 256];
    let mut d_len = [0usize; 256];
    let mut at = 0;
    for (code_count, codes_each, bits) in [(1, 32, 3), (3, 16, 4), (8, 8, 5), (12, 4, 6), (24, 2, 7), (16, 1, 8)] {
        for _ in 0..code_count {
            for _ in 0..codes_each {
                d_len[at] = bits;
                at += 1;
            }
        }
    }
    let mut code = 0;
    let mut at = 0;
    for (code_count, codes_each) in [(1, 32), (3, 16), (8, 8), (12, 4), (24, 2), (16, 1)] {
        for _ in 0..code_count {
            for _ in 0..codes_each {
                d_code[at] = code;
                at += 1;
            }
            code += 1;
        }
    }

    let mut bits = Bits { input, at: 0, buffer: 0, count: 0 };
    let mut window = vec![b' '; N + F - 1];
    let mut r = N - F;
    let mut out = Vec::new();
    while out.len() < most && !bits.ended() {
        let mut c = son[R];
        while c < T {
            c = son[c + bits.bit()];
        }
        c -= T;
        update(c, &mut freq, &mut son, &mut prnt);
        if c < 256 {
            out.push(c as u8);
            window[r] = c as u8;
            r = (r + 1) & (N - 1);
        } else {
            let mut i = bits.byte();
            let high = d_code[i] << 6;
            for _ in 0..d_len[i] - 2 {
                i = (i << 1) + bits.bit();
            }
            let position = high | (i & 0x3F);
            let from = (r + N - position - 1) & (N - 1);
            for k in 0..c - 255 + THRESHOLD {
                let byte = window[(from + k) & (N - 1)];
                out.push(byte);
                window[r] = byte;
                r = (r + 1) & (N - 1);
            }
        }
    }
    out
}

/// A Teledisk file's comment and tracks. After the 12-byte header (and, for `td`, everything after it compressed),
/// an optional comment, then each track's sector count, cylinder and head, then each sector's header and data
/// block: the bytes as they are, a repeated pair, or runs and literals.
fn read_td0(file: &[u8]) -> Result<(String, Tracks), String> {
    let header = file.get(..12).ok_or("This Teledisk file is too short for its header.")?;
    let body = if header.starts_with(b"td") {
        if header[4] < 20 {
            return Err("This Teledisk file uses the compression of versions before 2.0, which Limoniter can't unpack yet.".into());
        }
        lzhuf(&file[12..], 16 * 1024 * 1024)
    } else {
        file[12..].to_vec()
    };
    let mut at = 0;
    let take = |at: &mut usize, n: usize| -> Result<&[u8], String> {
        let bytes = body.get(*at..*at + n).ok_or("The Teledisk file ends partway through a track.")?;
        *at += n;
        Ok(bytes)
    };
    let mut comment = String::new();
    if header[7] & 0x80 != 0 {
        let block = take(&mut at, 10)?;
        let length = usize::from(u16::from_le_bytes([block[2], block[3]]));
        let text = take(&mut at, length)?;
        comment = text.split(|b| *b == 0).map(|line| String::from_utf8_lossy(line).trim().to_string()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
    }
    let mut tracks = Tracks::new();
    loop {
        let track = take(&mut at, 1)?;
        if track[0] == 0xFF {
            break;
        }
        let rest = take(&mut at, 3)?;
        let (count, cylinder, side) = (usize::from(track[0]), rest[0], rest[1]);
        let mut sectors = Vec::with_capacity(count);
        for _ in 0..count {
            let head = take(&mut at, 6)?.to_vec();
            let (id, size_code, flags) = (head[2], head[3], head[4]);
            let size = 128usize << size_code.min(6);
            // A sector Teledisk skipped as unused, or whose ID it found without data, has no data block.
            if flags & 0x30 != 0 || size_code > 6 {
                sectors.push(Sector { id, size, data: None, error: false });
                continue;
            }
            let length = usize::from(u16::from_le_bytes(take(&mut at, 2)?.try_into().unwrap()));
            let block = take(&mut at, length)?;
            let (&method, rest) = block.split_first().ok_or("A Teledisk sector's data block is empty.")?;
            let mut data = Vec::with_capacity(size);
            match method {
                0 => data.extend_from_slice(rest),
                1 => {
                    let mut i = 0;
                    while i + 4 <= rest.len() {
                        let times = usize::from(u16::from_le_bytes([rest[i], rest[i + 1]]));
                        for _ in 0..times {
                            data.extend_from_slice(&rest[i + 2..i + 4]);
                        }
                        i += 4;
                    }
                }
                2 => {
                    let mut i = 0;
                    while i < rest.len() && data.len() < size {
                        let kind = usize::from(rest[i]);
                        if kind == 0 {
                            let n = usize::from(*rest.get(i + 1).unwrap_or(&0));
                            data.extend_from_slice(rest.get(i + 2..i + 2 + n).unwrap_or_default());
                            i += 2 + n;
                        } else {
                            // The pattern is 2 to the power of the code (Dunfield's TD02IMD; his notes say twice it).
                            let Some(span) = 1usize.checked_shl(kind as u32).filter(|span| *span <= size) else {
                                return Err(format!("Track {cylinder} has a sector whose runs don't make sense, so the image is damaged."));
                            };
                            let times = usize::from(*rest.get(i + 1).unwrap_or(&0));
                            let pattern = rest.get(i + 2..i + 2 + span).unwrap_or_default();
                            for _ in 0..times {
                                data.extend_from_slice(pattern);
                            }
                            i += 2 + span;
                        }
                    }
                }
                _ => return Err(format!("Track {cylinder} has a sector stored a way ({method}) Teledisk doesn't use.")),
            }
            data.resize(size, 0);
            sectors.push(Sector { id, size, data: Some(data), error: flags & 0x02 != 0 });
        }
        tracks.entry((cylinder, side & 1)).or_default().extend(sectors);
    }
    Ok((comment, tracks))
}

// ---------- The disk inside ----------

/// A sector image's disk: its file system if Limoniter knows it, and the plain image of its sectors either way.
pub struct SectorDisk {
    what: String,
    inner: Option<Box<dyn Volume>>,
    plain: Arc<Vec<u8>>,
    notes: Vec<String>,
}

/// An entry's `extra` for the plain image of the whole disk.
const WHOLE_DISK: u64 = u64::MAX;

pub fn open(source: Arc<dyn Source>, format: Kind) -> Result<Box<dyn Volume>, String> {
    let file = source.read_vec(0, source.len().min(32 * 1024 * 1024) as usize)?;
    let (comment, tracks) = match format {
        Kind::Imd => read_imd(&file)?,
        Kind::Td0 => read_td0(&file)?,
        Kind::Dmk => (String::new(), read_dmk(&file)?),
        Kind::Jv3 => (String::new(), read_jv3(&file)?),
        Kind::Jv1 => (String::new(), read_jv1(&file)),
    };
    let laid = lay_out(&tracks)?;
    let kind = match format {
        Kind::Imd => "ImageDisk (IMD)",
        Kind::Td0 => "Teledisk (TD0)",
        Kind::Dmk => "DMK",
        Kind::Jv3 => "JV3",
        Kind::Jv1 => "JV1",
    };
    let plain = Arc::new(laid.bytes);
    let mut notes = Vec::new();
    if !comment.is_empty() {
        notes.push(format!("The image's comment: {}", comment.replace('\n', " \u{B7} ")));
    }
    notes.extend(laid.notes);
    let disk: Arc<dyn Source> = Arc::new(plain.as_ref().clone());
    let inner = match open_inner(disk, laid.tracks) {
        Ok(volume) => Some(volume),
        Err(problem) => {
            notes.push(format!("{problem} The disk's sectors are offered as a plain image to look at in hex or save."));
            None
        }
    };
    Ok(Box::new(SectorDisk { what: format!("{kind} \u{B7} {}", laid.shape), inner, plain, notes }))
}

/// The file system on a disk laid out from its sectors: whatever its bytes show, then the TRS-80's, the Color
/// Computer's and CP/M's, which need the tracks' shape to judge it by.
pub(super) fn open_inner(disk: Arc<dyn Source>, tracks: super::cpm::Shape) -> Result<Box<dyn Volume>, String> {
    super::open_plain_disk(disk.clone())
        .or_else(|problem| super::trsdos::Trsdos::open(disk.clone(), tracks).map(|trs| Box::new(trs) as Box<dyn Volume>).map_err(|_| problem))
        .or_else(|problem| super::coco::Coco::open_flat(disk.clone(), tracks).map(|coco| Box::new(coco) as Box<dyn Volume>).map_err(|_| problem))
        .or_else(|problem| super::cpm::open_guessed(disk, Some(tracks), "").map(|cpm| Box::new(cpm) as Box<dyn Volume>).map_err(|_| problem))
}

impl Volume for SectorDisk {
    fn describe(&self) -> String {
        match &self.inner {
            Some(inner) => format!("{} \u{B7} {}", self.what, inner.describe()),
            None => self.what.clone(),
        }
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        if let Some(dir) = dir {
            return self.inner.as_ref().ok_or("This disk has no folders Limoniter can read.")?.list(Some(dir));
        }
        let mut entries = match &self.inner {
            Some(inner) => inner.list(None)?,
            None => Vec::new(),
        };
        let mut whole = Entry::file("Whole disk (sectors).raw".into(), self.plain.len() as u64, 0);
        whole.extra = WHOLE_DISK;
        whole.kind = "Disk image".into();
        whole.detail = "every sector in order, as a plain image".into();
        // Opened, it's bytes to look at, not an image to browse again; saved, a plain sector image.
        whole.open_name = "Whole disk (sectors).raw".into();
        entries.push(whole);
        Ok(entries)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        if entry.extra == WHOLE_DISK {
            return Ok(self.plain.as_ref().clone());
        }
        self.inner.as_ref().ok_or("That isn't on this disk.")?.read(entry)
    }

    fn notes(&self) -> Vec<String> {
        let mut notes = self.notes.clone();
        if let Some(inner) = &self.inner {
            notes.extend(inner.notes());
        }
        notes
    }

    fn text_preset(&self, entry: &Entry) -> Option<&'static str> {
        self.inner.as_ref().filter(|_| entry.extra != WHOLE_DISK).and_then(|inner| inner.text_preset(entry))
    }

    fn reading_bytes(&self, entry: &Entry, bytes: Vec<u8>) -> Vec<u8> {
        match &self.inner {
            Some(inner) if entry.extra != WHOLE_DISK => inner.reading_bytes(entry, bytes),
            _ => bytes,
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::path::Path;

    /// An HP 150 disk: FAT12 with 256-byte sectors, 66 tracks of 16 on one side, its label in the root directory as
    /// DOS 2 keeps it, a text file, and a document whose cluster the FAT calls free.
    pub fn hp150_disk() -> Vec<u8> {
        let mut disk = vec![0u8; 66 * 16 * 256];
        disk[..3].copy_from_slice(&[0xEB, 0x1C, 0x90]);
        disk[3..11].copy_from_slice(b"HP150   ");
        disk[11..13].copy_from_slice(&256u16.to_le_bytes());
        disk[13] = 4;
        disk[14..16].copy_from_slice(&2u16.to_le_bytes());
        disk[16] = 2;
        disk[17..19].copy_from_slice(&128u16.to_le_bytes());
        disk[19..21].copy_from_slice(&1056u16.to_le_bytes());
        disk[21] = 0xFA;
        disk[22..24].copy_from_slice(&3u16.to_le_bytes());
        disk[24..26].copy_from_slice(&16u16.to_le_bytes());
        disk[26..28].copy_from_slice(&1u16.to_le_bytes());
        // Both FATs: the media byte, then cluster 2 the end of its chain; cluster 3 left free.
        for fat in [512, 512 + 768] {
            disk[fat..fat + 5].copy_from_slice(&[0xFA, 0xFF, 0xFF, 0xFF, 0x0F]);
        }
        let root = 512 + 2 * 768;
        let mut entry = |slot: usize, name: &[u8; 11], attributes: u8, cluster: u16, size: u32| {
            let at = root + slot * 32;
            disk[at..at + 11].copy_from_slice(name);
            disk[at + 11] = attributes;
            disk[at + 26..at + 28].copy_from_slice(&cluster.to_le_bytes());
            disk[at + 28..at + 32].copy_from_slice(&size.to_le_bytes());
        };
        entry(0, b"MAESTRO_APL", 0x08, 0, 0);
        entry(1, b"HELLO   TXT", 0x20, 2, 14);
        entry(2, b"NAMELESS   ", 0x20, 3, 9);
        let data = root + 128 * 32;
        disk[data..data + 14].copy_from_slice(b"Hello, HP 150\r");
        disk[data + 1024..data + 1033].copy_from_slice(b"My letter");
        disk
    }

    /// An ImageDisk file of a disk of sixteen 256-byte sectors a track: interleaved, a filled sector stored as one
    /// byte, a sector that couldn't be read, one read with an error, and an extra sector 17 on each track.
    pub fn imd_of(disk: &[u8], unreadable: (u8, u8), with_error: (u8, u8)) -> Vec<u8> {
        let mut out = b"IMD 1.18: 11/11/2026 10:00:00\r\nHP 150 test disk\x1A".to_vec();
        let order: Vec<u8> = (0..16).map(|i| (i * 7) % 16).chain([17]).collect();
        for track in 0..disk.len() / 4096 {
            out.extend([5, track as u8, 0, 17, 1]);
            out.extend(&order);
            for &id in &order {
                let at = track * 4096 + usize::from(id) * 256;
                if id == 17 || (track as u8, id) == unreadable {
                    out.push(0);
                    continue;
                }
                let sector = &disk[at..at + 256];
                let error = (track as u8, id) == with_error;
                if sector.iter().all(|b| *b == sector[0]) {
                    out.extend([if error { 6 } else { 2 }, sector[0]]);
                } else {
                    out.push(if error { 5 } else { 1 });
                    out.extend(sector);
                }
            }
        }
        out
    }

    /// A Teledisk file of the same kind of disk, its sectors stored all three ways, with a comment and an empty track
    /// at the end, as Teledisk writes them, before any compression.
    pub fn td0_body(disk: &[u8]) -> Vec<u8> {
        let comment = b"HP 150 test disk\0";
        let mut out = vec![0, 0, comment.len() as u8, 0, 126, 11, 11, 10, 0, 0];
        out.extend(comment);
        for track in 0..disk.len() / 4096 {
            out.extend([17, track as u8, 0, 0]);
            for id in (0..16u8).chain([17]) {
                if id == 17 {
                    out.extend([track as u8, 0, 17, 1, 0x20, 0]);
                    continue;
                }
                out.extend([track as u8, 0, id, 1, 0, 0]);
                let sector = &disk[track * 4096 + usize::from(id) * 256..][..256];
                let block: Vec<u8> = if sector.iter().all(|b| *b == sector[0]) {
                    // A repeated pair: 128 times.
                    [&[1u8, 128, 0][..], &[sector[0], sector[0]]].concat()
                } else if id % 2 == 0 {
                    // Runs and literals: a literal of the first 12 bytes, then the rest as a 4-byte pattern (code 2)
                    // where it repeats, else literals of up to 255.
                    let mut block = vec![2u8, 0, 12];
                    block.extend(&sector[..12]);
                    let mut at = 12;
                    while at < 256 {
                        let pattern = &sector[at..at + 4];
                        let times = (0..(256 - at) / 4).take_while(|k| &sector[at + k * 4..at + k * 4 + 4] == pattern).count();
                        if times > 1 {
                            block.extend([2, times as u8]);
                            block.extend(pattern);
                            at += times * 4;
                        } else {
                            let n = (256 - at).min(4);
                            block.extend([0, n as u8]);
                            block.extend(&sector[at..at + n]);
                            at += n;
                        }
                    }
                    block
                } else {
                    [&[0u8][..], sector].concat()
                };
                out.extend((block.len() as u16).to_le_bytes());
                out.extend(block);
            }
        }
        out.extend([0, 66, 0, 0, 0xFF]);
        out
    }

    pub fn td0_header(compressed: bool) -> Vec<u8> {
        [if compressed { b"td" } else { b"TD" }, &[0u8, 0x31, 21, 0, 4, 0x80, 0, 1, 0, 0][..]].concat()
    }

    /// LZHUF's encoder, for the tests: each byte as a literal, except runs of a repeated byte, which are a literal
    /// and then a match one back, so both the tree and the window are used.
    pub fn lzhuf_encode(input: &[u8]) -> Vec<u8> {
        const N: usize = 4096;
        const F: usize = 60;
        const N_CHAR: usize = 256 - 2 + F;
        const T: usize = N_CHAR * 2 - 1;
        const R: usize = T - 1;
        let mut freq = vec![0u32; T + 1];
        let mut son = vec![0usize; T];
        let mut prnt = vec![0usize; T + N_CHAR];
        for i in 0..N_CHAR {
            freq[i] = 1;
            son[i] = i + T;
            prnt[i + T] = i;
        }
        let (mut i, mut j) = (0, N_CHAR);
        while j <= R {
            freq[j] = freq[i] + freq[i + 1];
            son[j] = i;
            prnt[i] = j;
            prnt[i + 1] = j;
            i += 2;
            j += 1;
        }
        freq[T] = 0xFFFF;
        prnt[R] = 0;
        let mut bits: Vec<bool> = Vec::new();
        let put = |c: usize, freq: &mut Vec<u32>, son: &mut Vec<usize>, prnt: &mut Vec<usize>, bits: &mut Vec<bool>| {
            // From the symbol's leaf up to the root, each node's own place says which child it is.
            let mut code = Vec::new();
            let mut k = prnt[c + T];
            loop {
                code.push(k & 1 == 1);
                k = prnt[k];
                if k == R {
                    break;
                }
            }
            bits.extend(code.iter().rev());
            // The same update the decoder makes.
            let mut c = prnt[c + T];
            loop {
                freq[c] += 1;
                let k = freq[c];
                let mut l = c + 1;
                if k > freq[l] {
                    while k > freq[l + 1] {
                        l += 1;
                    }
                    freq[c] = freq[l];
                    freq[l] = k;
                    let i = son[c];
                    prnt[i] = l;
                    if i < T {
                        prnt[i + 1] = l;
                    }
                    let j = son[l];
                    son[l] = i;
                    prnt[j] = c;
                    if j < T {
                        prnt[j + 1] = c;
                    }
                    son[c] = j;
                    c = l;
                }
                c = prnt[c];
                if c == 0 {
                    break;
                }
            }
        };
        let mut at = 0;
        while at < input.len() {
            put(usize::from(input[at]), &mut freq, &mut son, &mut prnt, &mut bits);
            let run = input[at + 1..].iter().take(F).take_while(|b| **b == input[at]).count();
            at += 1;
            if run >= 3 {
                // A match of `run` bytes starting one back: distance 0. Its upper six bits are code 0, sent as 000,
                // then the low six bits.
                put(run + 253, &mut freq, &mut son, &mut prnt, &mut bits);
                bits.extend([false; 3]);
                bits.extend([false; 6]);
                at += run;
            }
        }
        assert!(freq[R] < 0x8000, "the tests' input is too long to need the tree rebuilt");
        let _ = N;
        bits.chunks(8).map(|byte| byte.iter().enumerate().fold(0u8, |v, (i, b)| v | (u8::from(*b) << (7 - i)))).collect()
    }

    fn names(entries: &[Entry]) -> Vec<String> {
        entries.iter().map(|e| e.name.clone()).collect()
    }

    #[test]
    fn imagedisk_images_lay_out_their_sectors_and_open_the_disk_inside() {
        let disk = hp150_disk();
        let image = imd_of(&disk, (40, 5), (41, 2));
        assert!(super::super::is_image("APPLATM1.IMD", &image[..0x8800.min(image.len())], image.len() as u64));
        let volume = super::super::open(Arc::new(image), "APPLATM1.IMD").unwrap();
        assert_eq!(volume.describe(), "ImageDisk (IMD) \u{B7} 66 tracks, 1 side, 16 \u{D7} 256-byte sectors numbered from 0 \u{B7} FAT12 \u{B7} MAESTRO_APL");
        assert_eq!(volume.notes(), [
            "The image's comment: HP 150 test disk".to_string(),
            "Sectors that couldn't be read when the image was made are zeros here: track 40 sector 5.".into(),
            "Sectors read with a data error, so they may be wrong: track 41 sector 2.".into(),
        ]);
        let files = volume.list(None).unwrap();
        assert_eq!(names(&files), ["HELLO.TXT", "NAMELESS", "Whole disk (sectors).raw"]);
        assert_eq!(volume.read(&files[0]).unwrap(), b"Hello, HP 150\r");
        assert_eq!(volume.read(&files[1]).unwrap(), b"My letter", "a file the FAT calls free is still read");
        assert!(files[1].detail.contains("the FAT marks its clusters free"));
        let mut whole = disk.clone();
        whole[40 * 4096 + 5 * 256..][..256].fill(0);
        assert_eq!(volume.read(&files[2]).unwrap(), whole, "every sector in order, the unreadable one as zeros");
        assert!(!super::super::is_nested_image(&files[2].open_name, &whole), "the whole disk opens as bytes, not as the disk again");
    }

    #[test]
    fn teledisk_images_decode_all_three_ways_compressed_or_not() {
        let disk = hp150_disk();
        let body = td0_body(&disk);
        for compressed in [false, true] {
            let mut image = td0_header(compressed);
            image.extend(if compressed { lzhuf_encode(&body) } else { body.clone() });
            let volume = super::super::open(Arc::new(image), "MEMOUSR.TD0").unwrap_or_else(|e| panic!("compressed {compressed}: {e}"));
            assert_eq!(volume.describe(), "Teledisk (TD0) \u{B7} 66 tracks, 1 side, 16 \u{D7} 256-byte sectors numbered from 0 \u{B7} FAT12 \u{B7} MAESTRO_APL", "compressed {compressed}");
            assert_eq!(volume.notes(), ["The image's comment: HP 150 test disk"], "the empty last track and the sectors without data aren't the disk's");
            let files = volume.list(None).unwrap();
            assert_eq!(volume.read(&files[2]).unwrap(), disk, "compressed {compressed}");
        }
        assert_eq!(lzhuf(&lzhuf_encode(&body), body.len()), body);
    }

    #[test]
    fn a_disk_with_no_known_file_system_is_still_its_sectors() {
        let mut disk = vec![0x5Au8; 70 * 4096];
        disk[..16].copy_from_slice(b"HP WORD PROCESSR");
        let volume = super::super::open(Arc::new(imd_of(&disk, (99, 99), (99, 99))), "letters.imd").unwrap();
        let files = volume.list(None).unwrap();
        assert_eq!(names(&files), ["Whole disk (sectors).raw"]);
        assert_eq!(volume.read(&files[0]).unwrap(), disk);
        assert!(volume.notes().iter().any(|n| n.contains("No file system Limoniter knows")));
    }

    #[test]
    fn damaged_sector_images_give_errors_never_panics() {
        let disk = hp150_disk();
        let mut td0 = td0_header(true);
        td0.extend(lzhuf_encode(&td0_body(&disk)));
        for (name, image) in [("d.imd", imd_of(&disk, (1, 1), (2, 2))), ("d.td0", td0)] {
            for seed in 0..60u32 {
                let mut bytes = image.clone();
                let mut state = seed.wrapping_mul(2_654_435_761);
                for _ in 0..24 {
                    state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    let at = 12 + state as usize % (bytes.len() - 12).min(20_000);
                    bytes[at] = (state >> 16) as u8;
                }
                if let Ok(volume) = super::super::open(Arc::new(bytes), name) {
                    for entry in volume.list(None).unwrap_or_default() {
                        let _ = volume.read(&entry);
                    }
                }
            }
        }
    }

    /// The real sector images in ..\testmedia, when it's there: each lays out into a plain image, written to
    /// limoniter-unpacked so it can be compared with other tools' reading and with the same disk in other forms.
    #[test]
    fn real_sector_images_lay_out() {
        let root = Path::new("../testmedia");
        let Ok(folders) = std::fs::read_dir(root) else {
            return;
        };
        let out = root.join("limoniter-unpacked");
        std::fs::create_dir_all(&out).unwrap();
        for folder in folders.flatten().map(|f| f.path()).filter(|p| p.is_dir()) {
            for file in std::fs::read_dir(&folder).unwrap().flatten().map(|f| f.path()) {
                let name = file.file_name().unwrap().to_string_lossy().into_owned();
                let lower = name.to_lowercase();
                let imd = lower.ends_with(".imd");
                if !imd && !lower.ends_with(".td0") {
                    continue;
                }
                let bytes = std::fs::read(&file).unwrap();
                let (_, tracks) = if imd { read_imd(&bytes) } else { read_td0(&bytes) }.unwrap_or_else(|e| panic!("{name}: {e}"));
                let laid = lay_out(&tracks).unwrap_or_else(|e| panic!("{name}: {e}"));
                std::fs::write(out.join(format!("{name}.raw")), &laid.bytes).unwrap();
            }
        }
    }
}
