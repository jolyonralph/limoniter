//! The crunched data of the five crunchers, decoded: PowerPacker, Imploder, CrunchMania, StoneCracker and
//! ByteKiller. Each decoder is ported from Teemu Suutari's ancient library (github.com/temisu/ancient, BSD 2-clause
//! licence; see data/ancient-license.txt), and checked here against files crunched by the real tools. Each takes
//! the crunched stream and its sizes, and gives the bytes as they were before crunching, or Bad if the stream
//! doesn't decode to exactly that many.

use super::bits::*;

/// PowerPacker's stream: read backwards from the long before the last (the last holds the size and how many bits of
/// the first long are padding), its bits highest first. `modes` are the four widths of distance its efficiency set.
/// `key` is what each long is XORed with, for an encrypted file (0 if it isn't).
pub fn powerpacker(bytes: &[u8], start: usize, end: usize, modes: [u32; 4], key: u32) -> R<Vec<u8>> {
    powerpacker_with(bytes, start, end, modes, key, false)
}

/// PowerPacker's stream, `inverted` if a 1 rather than a 0 starts a run of literal bytes (as in PowerPacker's own
/// crunched program).
pub fn powerpacker_with(bytes: &[u8], start: usize, end: usize, modes: [u32; 4], key: u32, inverted: bool) -> R<Vec<u8>> {
    let last = be32(bytes, end)?;
    let (size, skip) = ((last >> 8) as usize, last & 0xFF);
    if size == 0 || skip >= 32 || size > super::MOST {
        return Err(Bad);
    }
    let mut input = Backward::new(bytes, start, end)?;
    let mut lsb = Lsb::default();
    let mut bits = |count: u32| -> R<u32> { Ok(reverse(lsb.read(count, || Ok((input.be32()? ^ key, 32)))?, count)) };
    bits(skip)?;
    let mut out = BackOut::new(size);
    loop {
        if bits(1)? == u32::from(inverted) {
            let mut count = 1;
            loop {
                let more = bits(2)?;
                count += more;
                if more < 3 {
                    break;
                }
            }
            for _ in 0..count {
                out.byte(bits(8)?)?;
            }
        }
        if out.done() {
            break;
        }
        let mode = bits(2)? as usize;
        let (count, distance);
        if mode == 3 {
            let width = if bits(1)? != 0 { modes[3] } else { 7 };
            distance = bits(width)? + 1;
            let mut n = 5;
            loop {
                let more = bits(3)?;
                n += more;
                if more < 7 {
                    break;
                }
            }
            count = n;
        } else {
            count = mode as u32 + 2;
            distance = bits(modes[mode])? + 1;
        }
        out.copy(distance as usize, count as usize)?;
    }
    Ok(out.bytes)
}

/// The four distance widths a PowerPacker mode long gives, if it's one of the five efficiencies.
pub fn powerpacker_modes(mode: u32) -> Option<[u32; 4]> {
    matches!(mode, 0x0909_0909 | 0x090A_0A0A | 0x090A_0B0B | 0x090A_0C0C | 0x090A_0C0D).then(|| mode.to_be_bytes().map(u32::from))
}

/// How an Imploder stream says where it stops: after so many bytes (its data files), or after so many matches
/// (its programs, whose decruncher counts them).
#[derive(Clone, Copy)]
pub enum ImploderEnd {
    Bytes(usize),
    Matches(u32),
}

/// What an Imploder stream needs besides its bytes: the literal run it starts with, the bits left over in its last
/// byte, and its mode's tables of distance (the bases of the second and third ranges, and each range's width).
pub struct ImploderTables {
    pub literals: u32,
    pub bits: (u32, u32),
    pub distances: [[u32; 4]; 2],
    /// Each range's width in bits; or, marked with $80, a byte and then its low four bits more.
    pub widths: [[u32; 4]; 3],
    /// The width of the longest literal run's count: 14, unless the cruncher chose otherwise.
    pub longest_literals: u32,
}

/// Imploder's stream, read backwards by `byte`; the bytes come out in the order they're made, which is the
/// order a program's decruncher hands them on (and the reverse of a data file's).
pub fn imploder_stream(mut byte: impl FnMut() -> R<u8>, t: &ImploderTables, end: ImploderEnd) -> R<Vec<u8>> {
    let mut msb = Msb::default();
    msb.reset(t.bits.0, t.bits.1);
    let first = Huffman::new(&[(1, 0b0, 0u32), (2, 0b10, 1), (3, 0b110, 2), (4, 0b1110, 3), (5, 0b11110, 4), (5, 0b11111, 5)])?;
    let second = Huffman::new(&[(1, 0b0, 0u32), (2, 0b10, 1), (2, 0b11, 2)])?;
    const LITERAL_LENGTHS: [u32; 4] = [6, 10, 10, 18];
    let literal_bits: [[u32; 4]; 3] = [[1, 1, 1, 1], [2, 3, 3, 4], [4, 5, 7, t.longest_literals]];
    let (limit, mut matches) = match end {
        ImploderEnd::Bytes(size) => (size, u32::MAX),
        ImploderEnd::Matches(count) => (MOST_IMPLODED, count),
    };
    let mut out: Vec<u8> = Vec::new();
    let mut literals = t.literals;
    // The bit reader and the byte reader share the stream, so both go through one closure.
    let mut read = |what: u32| -> R<u32> {
        if what == u32::MAX {
            return byte().map(u32::from);
        }
        msb.read(what, || Ok((u32::from(byte()?), 8)))
    };
    loop {
        if out.len() + literals as usize > limit {
            return Err(Bad);
        }
        for _ in 0..literals {
            out.push(read(u32::MAX)? as u8);
        }
        if let ImploderEnd::Bytes(size) = end
            && out.len() == size
        {
            break;
        }
        matches -= 1;
        if matches == 0 {
            break;
        }
        let i0 = first.decode(|| read(1))?;
        let selector = i0.min(3) as usize;
        let mut count = i0 + 2;
        if count == 6 {
            count += read(3)?;
        } else if count == 7 {
            count = read(u32::MAX)?;
            if count == 0 {
                return Err(Bad);
            }
        }
        let i1 = second.decode(|| read(1))? as usize;
        literals = (i1 + i1) as u32;
        if literals == 4 {
            literals = LITERAL_LENGTHS[selector];
        }
        literals += read(literal_bits[i1][selector])?;
        let i2 = second.decode(|| read(1))? as usize;
        let base = if i2 > 0 { t.distances[i2 - 1][selector] } else { 0 };
        // A program's decruncher marks widths of 8 and more with $80: a whole byte from the stream, then the rest
        // of the width (its low four bits) as bits.
        let width = t.widths[i2][selector];
        let offset = if width & 0x80 != 0 { read(u32::MAX)? << (width & 0x0F) | read(width & 0x0F)? } else { read(width)? };
        let distance = (1 + base + offset) as usize;
        if distance > out.len() || out.len() + count as usize > limit {
            return Err(Bad);
        }
        for _ in 0..count {
            out.push(out[out.len() - distance]);
        }
    }
    Ok(out)
}

/// The most an imploded program's stream may make here.
const MOST_IMPLODED: usize = super::MOST;

/// Imploder's data files. `bytes` holds the header's 12 bytes at its start (the first 12 of the stream, which the
/// header took the place of, are kept at `end`, after the stream), the stream up to `end`, then its tables.
pub fn imploder(bytes: &[u8], end: usize, size: usize) -> R<Vec<u8>> {
    if end & 1 != 0 || end < 12 || end + 0x32 > bytes.len() || size == 0 || size > super::MOST {
        return Err(Bad);
    }
    // The stream is read backwards from `end`, its first 12 bytes taken from where they were moved to.
    let mut at = end;
    if bytes[end + 16] & 0x80 == 0 {
        at -= 1;
    }
    let byte = || -> R<u8> {
        if at == 0 {
            return Err(Bad);
        }
        at -= 1;
        let place = match at {
            0..4 => end + at + 8,
            4..8 => end + at,
            8..12 => end + at - 8,
            _ => at,
        };
        bytes.get(place).copied().ok_or(Bad)
    };
    // The bits left over in the last byte, above its marker bit.
    let half = u32::from(bytes[end + 17]);
    let bits = (0..7).find(|i| half & (1 << i) != 0).map_or((0, 0), |i| (half >> (i + 1), 7 - i));
    let mut distances = [[0u32; 4]; 2];
    for i in 0..8 {
        distances[i >> 2][i & 3] = be16(bytes, end + 18 + i * 2)?;
    }
    let mut widths = [[0u32; 4]; 3];
    for i in 0..12 {
        widths[i >> 2][i & 3] = u32::from(bytes[end + 34 + i]);
    }
    let tables = ImploderTables { literals: be32(bytes, end + 12)?, bits, distances, widths, longest_literals: 14 };
    let mut out = imploder_stream(byte, &tables, ImploderEnd::Bytes(size))?;
    out.reverse();
    Ok(out)
}

/// Whether an Imploder file's checksum (after its tables) is right: the sum of its words up to there, plus 7.
pub fn imploder_checksum(bytes: &[u8], end: usize) -> bool {
    let Ok(sum) = be32(bytes, end + 0x2E) else { return false };
    let words = bytes[..end + 0x2E].as_chunks::<2>().0;
    words.iter().fold(7u32, |s, w| s.wrapping_add(u32::from(u16::from_be_bytes(*w)))) == sum
}

/// CrunchMania's stream, which ends 14 bytes plus `packed` into `bytes`: its last six bytes say how many bits of
/// the long before them are used. `lzh` for its LZ-Huffman mode, `sampled` for sound crunched as differences.
pub fn crunchmania(bytes: &[u8], packed: usize, size: usize, lzh: bool, sampled: bool) -> R<Vec<u8>> {
    if size == 0 || size > super::MOST || packed < 6 || packed + 14 > bytes.len() {
        return Err(Bad);
    }
    let top = packed + 14 - 6;
    let mut input = Backward::new(bytes, 14, top)?;
    let mut lsb = Lsb::default();
    let (content, shift) = (be32(bytes, top)?, be16(bytes, top + 4)?);
    if shift > 16 {
        return Err(Bad);
    }
    lsb.reset(content >> (16 - shift), shift + 16);
    let mut bits = |count: u32| -> R<u32> { lsb.read(count, || Ok((u32::from(input.byte()?), 8))) };
    let mut out = BackOut::new(size);
    if lzh {
        loop {
            let lengths = crunchmania_table(&mut bits, 9)?;
            let distances = crunchmania_table(&mut bits, 4)?;
            let items = bits(16)? + 1;
            for _ in 0..items {
                let count = lengths.decode(|| bits(1))?;
                if count & 0x100 != 0 {
                    out.byte(count)?;
                } else {
                    let width = distances.decode(|| bits(1))?;
                    let distance = if width == 0 { bits(1)? + 1 } else { (bits(width)? | 1 << width) + 1 };
                    out.copy(distance as usize, count as usize + 3)?;
                }
            }
            if bits(1)? == 0 {
                break;
            }
        }
    } else {
        let lengths = Huffman::new(&[(1, 0b0, 0u32), (2, 0b10, 1), (3, 0b110, 2), (3, 0b111, 3)])?;
        let distances = Huffman::new(&[(1, 0b0, 1u32), (2, 0b10, 0), (2, 0b11, 2)])?;
        let length_ranges = Ranges::new(&[1, 2, 4, 8]);
        let distance_ranges = Ranges::new(&[5, 9, 14]);
        while !out.done() {
            if bits(1)? != 0 {
                out.byte(bits(8)?)?;
            } else {
                let index = lengths.decode(|| bits(1))?;
                let mut count = length_ranges.decode(&mut bits, index)? + 2;
                if count == 23 {
                    count = if bits(1)? != 0 { bits(5)? + 15 } else { bits(14)? + 15 };
                    for _ in 0..count {
                        out.byte(bits(8)?)?;
                    }
                } else {
                    if count > 23 {
                        count -= 1;
                    }
                    let index = distances.decode(|| bits(1))?;
                    let distance = distance_ranges.decode(&mut bits, index)?;
                    out.copy(distance as usize, count as usize)?;
                }
            }
        }
    }
    if !out.done() {
        return Err(Bad);
    }
    let mut bytes = out.bytes;
    if sampled {
        let mut sum = 0u8;
        for b in &mut bytes {
            sum = sum.wrapping_add(*b);
            *b = sum;
        }
    }
    Ok(bytes)
}

/// A CrunchMania LZ-Huffman table: how many codes of each length, then each code's value.
fn crunchmania_table(bits: &mut dyn FnMut(u32) -> R<u32>, width: u32) -> R<Huffman<u32>> {
    let deepest = bits(4)?;
    if deepest == 0 {
        return Err(Bad);
    }
    let mut counts = [0u32; 15];
    for (i, count) in counts.iter_mut().enumerate().take(deepest as usize) {
        *count = bits((i as u32 + 1).min(width))?;
    }
    let mut tree = Huffman::empty();
    let mut code = 0u32;
    for depth in 1..=deepest {
        for _ in 0..counts[depth as usize - 1] {
            let value = bits(width)?;
            tree.insert(depth, code >> (deepest - depth), value)?;
            code += 1 << (deepest - depth);
        }
    }
    Ok(tree)
}

/// StoneCracker's formats, by version: 2.92 and 2.99 (with no mark of their own); 3.00 (S300); 3.10 (S310); before
/// 4.00 (S400); 4.01 (S401); 4.02a (S403); and 4.10 (S404). (2.69 to 2.81 had no mark at all, nothing to know them by
/// in a data file, so they're left out.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stone {
    V292,
    V300,
    V310,
    V400,
    V401,
    V403,
    V404,
}

impl Stone {
    pub fn name(self) -> &'static str {
        match self {
            Stone::V292 => "StoneCracker 2.9",
            Stone::V300 => "StoneCracker 3.00",
            Stone::V310 => "StoneCracker 3.10",
            Stone::V400 => "StoneCracker 4.00",
            Stone::V401 => "StoneCracker 4.01",
            Stone::V403 => "StoneCracker 4.02",
            Stone::V404 => "StoneCracker 4.10",
        }
    }
}

/// A StoneCracker stream's version, sizes and where its data starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoneHeader {
    pub version: Stone,
    /// The widths of distance its efficiency set, for versions before 4.
    pub modes: [u32; 4],
    pub size: usize,
    /// Where the stream starts, and where it ends (the whole of the crunched data, its header included).
    pub start: usize,
    pub end: usize,
}

/// Reads a StoneCracker header at the start of `bytes`, as `version` writes it.
pub fn stone_header(bytes: &[u8], version: Stone) -> R<StoneHeader> {
    let id = be32(bytes, 0)?;
    let mut header = StoneHeader { version, modes: [0; 4], size: 0, start: 0, end: 0 };
    let read_modes = |value: u32| -> R<[u32; 4]> {
        let modes = value.to_be_bytes().map(u32::from);
        if modes.iter().any(|m| !(8..=14).contains(m)) { Err(Bad) } else { Ok(modes) }
    };
    let packed = match version {
        Stone::V292 | Stone::V310 | Stone::V400 | Stone::V401 => {
            if version == Stone::V292 {
                header.modes = read_modes(id)?;
            }
            header.start = 12;
            header.size = be32(bytes, 4)? as usize;
            be32(bytes, 8)? as usize
        }
        Stone::V300 => {
            header.start = 16;
            header.modes = read_modes(be32(bytes, 4)?)?;
            header.size = be32(bytes, 8)? as usize;
            be32(bytes, 12)? as usize
        }
        Stone::V403 | Stone::V404 => {
            header.start = 16;
            header.size = be32(bytes, 8)? as usize;
            be32(bytes, 12)? as usize + 2
        }
    };
    header.end = packed.checked_add(header.start).ok_or(Bad)?;
    if header.end > bytes.len() || header.size > super::MOST {
        return Err(Bad);
    }
    Ok(header)
}

/// Decodes a StoneCracker stream whose header has been read.
pub fn stonecracker(bytes: &[u8], h: &StoneHeader) -> R<Vec<u8>> {
    if h.size == 0 {
        return Ok(Vec::new());
    }
    match h.version {
        Stone::V292 | Stone::V300 => stone_292(bytes, h),
        Stone::V310 | Stone::V400 | Stone::V401 => stone_310(bytes, h),
        Stone::V403 => stone_403(bytes, h),
        Stone::V404 => stone_404(bytes, h),
    }
}

/// The highest set bit of the first long read marks where the stream's bits begin.
fn anchored(value: u32) -> (u32, u32) {
    match (1..32).rev().find(|i| value & (1 << i) != 0) {
        Some(i) => (value & ((1 << i) - 1), i),
        None => (0, 0),
    }
}

/// A count made of `width`-bit pieces, read until one isn't `threshold`; reversed first, if `reversed`.
fn count_of(bits: &mut dyn FnMut(u32) -> R<u32>, threshold: u32, width: u32, reversed: bool) -> R<u32> {
    let mut total = 0u32;
    loop {
        let read = bits(width)?;
        let n = if reversed { reverse(read, width) } else { read };
        total = total.checked_add(n).ok_or(Bad)?;
        if n != threshold {
            return Ok(total);
        }
    }
}

fn stone_292(bytes: &[u8], h: &StoneHeader) -> R<Vec<u8>> {
    let mut input = Backward::new(bytes, h.start, h.end)?;
    let mut lsb = Lsb::default();
    let (content, length) = anchored(input.be32()?);
    lsb.reset(content, length);
    let mut bits = |n: u32| -> R<u32> { lsb.read(n, || Ok((input.be32()?, 32))) };
    let later = h.version == Stone::V300;
    let mut out = BackOut::new(h.size);
    while !out.done() {
        if bits(1)? != 0 {
            let mut count = count_of(&mut bits, 7, 3, true)?;
            if later {
                count += 1;
            }
            if count == 0 {
                return Err(Bad);
            }
            for _ in 0..count {
                out.byte(bits(8)?)?;
            }
        } else {
            let mode = reverse(bits(2)?, 2) as usize;
            let mut width = h.modes[mode] + 1;
            let count = if mode == 3 {
                if bits(1)? != 0 {
                    if later {
                        width = 8;
                    }
                    count_of(&mut bits, 7, 3, true)? + 5
                } else {
                    count_of(&mut bits, 127, 7, true)? + if later { 5 } else { 19 }
                }
            } else {
                mode as u32 + 2
            };
            let distance = reverse(bits(width)?, width) + 1;
            out.copy(distance as usize, count as usize)?;
        }
    }
    Ok(out.bytes)
}

fn stone_310(bytes: &[u8], h: &StoneHeader) -> R<Vec<u8>> {
    let mut input = Forward::new(bytes, h.start, h.end)?;
    let mut msb = Msb::default();
    let longs = h.version == Stone::V310;
    let mut bits = |n: u32| -> R<u32> { msb.read(n, || if longs { Ok((input.be32()?, 32)) } else { Ok((input.be16()?, 16)) }) };
    const MODES: [u32; 4] = [10, 11, 12, 8];
    let mut out = ForwardOut::new(h.size);
    while !out.done() {
        if bits(1)? != 0 {
            let mode = bits(2)? as usize;
            let distance = bits(MODES[mode])?;
            let mut count = if mode >= 2 {
                if longs { count_of(&mut bits, 15, 4, false)? + 3 } else { count_of(&mut bits, 7, 3, false)? + 3 }
            } else {
                mode as u32 + 3
            };
            // The crunchers could run a copy past the end; their decrunchers stop at it.
            count = count.min((h.size - out.at) as u32);
            out.copy(distance as usize, count as usize)?;
        } else {
            let mut count = count_of(&mut bits, 7, 3, false)?;
            if h.version == Stone::V401 {
                count += 1;
            }
            if count == 0 {
                return Err(Bad);
            }
            for _ in 0..count {
                out.byte(bits(8)?)?;
            }
        }
    }
    Ok(out.bytes)
}

fn stone_403(bytes: &[u8], h: &StoneHeader) -> R<Vec<u8>> {
    let mut input = Backward::new(bytes, h.start, h.end - 2)?;
    let mut lsb = Lsb::default();
    let count = be16(bytes, h.end - 2)?;
    if count > 16 {
        return Err(Bad);
    }
    let value = input.be16()?;
    lsb.reset(value, count);
    let mut bits = |n: u32| -> R<u32> { lsb.read(n, || Ok((input.be16()?, 16))) };
    let ranges = Ranges::new(&[5, 8, 10, 12]);
    let mut out = BackOut::new(h.size);
    while !out.done() {
        if bits(1)? != 0 {
            let index = bits(2)?;
            let distance = ranges.decode(&mut bits, index)? + 1;
            let mut count = 2;
            if bits(1)? == 0 {
                count += 1;
                if bits(1)? == 0 {
                    count += 1;
                    if bits(1)? == 0 {
                        count += 1;
                        loop {
                            let n = bits(3)?;
                            count += n;
                            if n != 7 {
                                break;
                            }
                        }
                    }
                }
            }
            out.copy(distance as usize, count as usize)?;
        } else {
            out.byte(bits(8)?)?;
        }
    }
    Ok(out.bytes)
}

fn stone_404(bytes: &[u8], h: &StoneHeader) -> R<Vec<u8>> {
    let mut input = Backward::new(bytes, h.start, h.end - 2)?;
    let mut msb = Msb::default();
    let count = be16(bytes, h.end - 2)? & 15;
    let value = input.be16()?;
    msb.reset(value >> (16 - count), count);
    let widest = input.be16()?;
    if !(10..=14).contains(&widest) {
        return Err(Bad);
    }
    let mut bits = |n: u32| -> R<u32> { msb.read(n, || Ok((input.be16()?, 16))) };
    let counts = Ranges::new(&[0, 1, 2, 3, 2, 1, 0, 8, -3, 2, 0, 5]);
    let distances = Ranges::new(&[5, 9, widest as i32]);
    let count_code = Huffman::new(&[(1, 0b0, 0x80u32), (2, 0b11, 1), (3, 0b101, 2), (4, 0b1000, 7), (5, 0b10010, 3), (6, 0b100110, 4), (7, 0b1001110, 5), (8, 0b10011110, 6), (8, 0b10011111, 0x8B)])?;
    let distance_code = Huffman::new(&[(1, 0b1, 2u32), (2, 0b00, 1), (2, 0b01, 0)])?;
    let mut out = BackOut::new(h.size);
    while !out.done() {
        let index = count_code.decode(|| bits(1))?;
        let mut count = counts.decode(&mut bits, index & 0x7F)? + 1;
        if count == 278 {
            loop {
                let n = bits(8)?;
                count += n;
                if n != 0xFF {
                    break;
                }
            }
        }
        if index & 0x80 != 0 {
            for _ in 0..count {
                out.byte(bits(8)?)?;
            }
        } else {
            let which = distance_code.decode(|| bits(1))?;
            let distance = distances.decode(&mut bits, which)? + 1;
            out.copy(distance as usize, count as usize)?;
        }
    }
    Ok(out.bytes)
}

/// `n` bits taken one at a time, the first read becoming the highest.
fn bits(n: u32, bit: &mut dyn FnMut() -> R<u32>) -> R<u32> {
    let mut value = 0;
    for _ in 0..n {
        value = value << 1 | bit()?;
    }
    Ok(value)
}

/// ByteKiller's stream, from `start` to `end`, read backwards from its end, whose first long's highest set bit
/// marks where its bits begin; and the XOR of all its longs, which it carries to check it by.
pub fn bytekiller(bytes: &[u8], start: usize, end: usize, size: usize, check: u32) -> R<Vec<u8>> {
    if size == 0 || size > super::MOST || end < start || !(end - start).is_multiple_of(4) || end > bytes.len() {
        return Err(Bad);
    }
    let sum = bytes[start..end].as_chunks::<4>().0.iter().fold(0, |s, l| s ^ u32::from_be_bytes(*l));
    if sum != check {
        return Err(Bad);
    }
    let mut input = Backward::new(bytes, start, end)?;
    let mut lsb = Lsb::default();
    let (content, length) = anchored(input.be32()?);
    lsb.reset(content, length);
    let mut bit = || -> R<u32> { lsb.read(1, || Ok((input.be32()?, 32))) };
    let code = Huffman::new(&[(2, 0b000, 0u32), (2, 0b001, 1), (3, 0b100, 2), (3, 0b101, 3), (3, 0b110, 4), (3, 0b111, 5)])?;
    let mut out = BackOut::new(size);
    while !out.done() {
        match code.decode(&mut bit)? {
            0 => {
                let count = bits(3, &mut bit)? + 1;
                for _ in 0..count {
                    out.byte(bits(8, &mut bit)?)?;
                }
            }
            1 => out.copy(bits(8, &mut bit)? as usize, 2)?,
            2 => out.copy(bits(9, &mut bit)? as usize, 3)?,
            3 => out.copy(bits(10, &mut bit)? as usize, 4)?,
            4 => {
                let count = bits(8, &mut bit)? + 1;
                let distance = bits(12, &mut bit)?;
                out.copy(distance as usize, count as usize)?;
            }
            _ => {
                let count = bits(8, &mut bit)? + 9;
                for _ in 0..count {
                    out.byte(bits(8, &mut bit)?)?;
                }
            }
        }
    }
    // Every bit is used: a stream with bits to spare isn't the one its header describes.
    if lsb.length != 0 || input.left() != 0 {
        return Err(Bad);
    }
    Ok(out.bytes)
}
