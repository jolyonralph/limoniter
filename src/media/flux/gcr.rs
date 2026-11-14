//! Group-coded recording: Commodore's and Apple's way of packing more onto a disk than FM, with no clock cells at
//! all. Each group of data bits is written as a longer code chosen so that the cells never go too long without a
//! change, which keeps the drive's own clock in step.
//!
//! The 1541 writes each 4 bits as 5 cells (no code has more than two 0s in a row), in four speed zones: 21 sectors
//! on the outer tracks down to 17 on the inner. A sync is ten or more 1 cells, which no data can make. Each sector is
//! a header block (&08, an XOR checksum, sector, track, the disk's two ID bytes, two &0F fillers) and, after a
//! short gap and another sync, a data block (&07, 256 bytes, their XOR, two fillers).
//!
//! The Apple II's 6-and-2 scheme writes bytes ("nibbles") whose top bit is always set, read by shifting cells in
//! until that top bit arrives. Each sector has an address field (&D5 &AA &96, volume, track, sector and checksum
//! each as two "4-and-4" nibbles) and a data field (&D5 &AA &AD, then 342 six-bit values and a checksum, each a
//! nibble from a 64-entry table, chained by XOR). DOS 3.2's 13-sector disks use an older 5-and-3 scheme (address
//! fields &D5 &AA &B5), and the Macintosh's 400K and 800K disks the same nibbles at twice the rate, with their own
//! fields; both are decoded as the WOZ reader decodes them.

use super::super::woz;
use super::Found;

/// The 1541's 5-cell codes, by the 4 bits they stand for.
const C64_CODES: [u8; 16] = [0x0A, 0x0B, 0x12, 0x13, 0x0E, 0x0F, 0x16, 0x17, 0x09, 0x19, 0x1A, 0x1B, 0x0D, 0x1D, 0x1E, 0x15];

/// The bytes whose cells start at `at`, ten cells a byte; None past the end or for a code that isn't one.
fn c64_bytes(cells: &[u8], at: usize, count: usize) -> Option<Vec<u8>> {
    let run = cells.get(at..at + count * 10)?;
    let nibble = |five: &[u8]| -> Option<u8> {
        let code = five.iter().fold(0u8, |c, b| (c << 1) | (b & 1));
        C64_CODES.iter().position(|c| *c == code).map(|n| n as u8)
    };
    run.chunks(10).map(|ten| Some((nibble(&ten[..5])? << 4) | nibble(&ten[5..])?)).collect()
}

/// Where each block after a sync starts, from `from`: the first 0 after ten or more 1s.
fn next_sync(cells: &[u8], from: usize, until: usize) -> Option<usize> {
    let mut ones = 0;
    for (i, &cell) in cells.iter().enumerate().take(until.min(cells.len())).skip(from) {
        if cell == 1 {
            ones += 1;
        } else {
            if ones >= 10 {
                return Some(i);
            }
            ones = 0;
        }
    }
    None
}

/// Every 1541 sector on a track's cells. Its `cylinder` is the track number its header gives (from 1).
pub fn commodore(cells: &[u8]) -> Vec<Found> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(header_at) = next_sync(cells, at, cells.len()) {
        at = header_at + 1;
        // The two fillers at the end of each block aren't checked: some duplicators cut them short.
        let Some(header) = c64_bytes(cells, header_at, 6) else {
            continue;
        };
        let (sector, track) = (header[2], header[3]);
        if header[0] != 0x08 || header[1] != header[2] ^ header[3] ^ header[4] ^ header[5] || track == 0 || track > 70 || sector >= 21 {
            continue;
        }
        at = header_at + 60;
        // The data block follows within a gap of a few bytes; further than this, the header has none.
        let mut data = None;
        if let Some(data_at) = next_sync(cells, at, at + 60 * 10)
            && let Some(block) = c64_bytes(cells, data_at, 258)
        {
            if block[0] == 0x07 && block[1..257].iter().fold(0, |s, b| s ^ b) == block[257] {
                data = Some(block[1..257].to_vec());
            }
            at = data_at + 2580;
        }
        found.push(Found { cylinder: track, head: 0, id: sector, size: 256, data });
    }
    found
}

/// The 6-and-2 nibbles for each six-bit value.
const APPLE_NIBBLES: [u8; 64] = [
    0x96, 0x97, 0x9A, 0x9B, 0x9D, 0x9E, 0x9F, 0xA6, 0xA7, 0xAB, 0xAC, 0xAD, 0xAE, 0xAF, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB9, 0xBA, 0xBB, 0xBC, 0xBD, 0xBE, 0xBF, 0xCB, 0xCD, 0xCE, 0xCF, 0xD3, 0xD6, 0xD7, 0xD9, 0xDA, 0xDB, 0xDC, 0xDD, 0xDE, 0xDF, 0xE5, 0xE6, 0xE7, 0xE9, 0xEA, 0xEB, 0xEC, 0xED, 0xEE, 0xEF, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF9, 0xFA, 0xFB, 0xFC, 0xFD, 0xFE, 0xFF,
];

/// The nibbles on a track's cells, as the Disk II's controller reads them: 0 cells are skipped until a 1, which is
/// the nibble's top bit, then seven more cells. With each, the cell it starts at.
fn apple_nibbles(cells: &[u8]) -> Vec<(usize, u8)> {
    let mut out = Vec::with_capacity(cells.len() / 8);
    let mut i = 0;
    while i < cells.len() {
        if cells[i] == 0 {
            i += 1;
            continue;
        }
        let Some(run) = cells.get(i..i + 8) else {
            break;
        };
        out.push((i, run.iter().fold(0u8, |b, c| (b << 1) | (c & 1))));
        i += 8;
    }
    out
}

/// Every Apple II 6-and-2 sector on a track's cells, by the physical sector number its address field gives. Its
/// `cylinder` is the address field's track.
pub fn apple(cells: &[u8]) -> Vec<Found> {
    let mut decode = [0xFFu8; 256];
    for (value, nibble) in APPLE_NIBBLES.iter().enumerate() {
        decode[usize::from(*nibble)] = value as u8;
    }
    let nibbles: Vec<u8> = apple_nibbles(cells).into_iter().map(|(_, n)| n).collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i + 3 <= nibbles.len() {
        if nibbles[i..i + 3] != [0xD5, 0xAA, 0x96] {
            i += 1;
            continue;
        }
        let Some(field) = nibbles.get(i + 3..i + 11) else {
            break;
        };
        i += 11;
        let four = |n: usize| ((field[n] << 1) | 1) & field[n + 1];
        let (volume, track, sector, check) = (four(0), four(2), four(4), four(6));
        if volume ^ track ^ sector != check || sector >= 16 || track >= 80 {
            continue;
        }
        // The data field's prologue follows within a gap of a few nibbles.
        let Some(start) = (i..(i + 40).min(nibbles.len().saturating_sub(2))).find(|&j| nibbles[j..j + 3] == [0xD5, 0xAA, 0xAD]) else {
            found.push(Found { cylinder: track, head: 0, id: sector, size: 256, data: None });
            continue;
        };
        let mut data = None;
        if let Some(raw) = nibbles.get(start + 3..start + 3 + 343) {
            let mut values = [0u8; 342];
            let mut chain = 0u8;
            let mut good = true;
            for (n, nibble) in raw.iter().enumerate() {
                let value = decode[usize::from(*nibble)];
                if value == 0xFF {
                    good = false;
                    break;
                }
                chain ^= value;
                if n < 342 {
                    values[n] = chain;
                }
            }
            // The last nibble is the checksum: the chain comes back to 0 after it.
            if good && chain == 0 {
                let bytes = (0..256)
                    .map(|j| {
                        let low = values[j % 86] >> (2 * (j / 86));
                        (values[86 + j] << 2) | ((low & 1) << 1) | ((low >> 1) & 1)
                    })
                    .collect();
                data = Some(bytes);
            }
            i = start + 3 + 343;
        }
        found.push(Found { cylinder: track, head: 0, id: sector, size: 256, data });
    }
    found
}

/// Every DOS 3.2 sector on a track's cells: thirteen to a track, in the older 5-and-3 scheme, whose address field
/// starts &D5 &AA &B5 and whose data field is 410 five-bit values and a checksum, as the WOZ reader decodes them.
pub fn apple13(cells: &[u8]) -> Vec<Found> {
    let table = woz::inverse(&woz::FIVE_AND_THREE);
    let nibbles: Vec<u8> = apple_nibbles(cells).into_iter().map(|(_, n)| n).collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i + 11 <= nibbles.len() {
        if nibbles[i..i + 3] != [0xD5, 0xAA, 0xB5] {
            i += 1;
            continue;
        }
        let four = |k: usize| woz::four_and_four(nibbles[i + 3 + 2 * k], nibbles[i + 4 + 2 * k]);
        let (volume, track, sector, check) = (four(0), four(1), four(2), four(3));
        i += 11;
        if volume ^ track ^ sector != check || sector >= 13 || track >= 80 {
            continue;
        }
        let data = woz::data_field(&nibbles, i).and_then(|at| woz::decode_53(&nibbles[at + 3..], &table));
        found.push(Found { cylinder: track, head: 0, id: sector, size: 256, data });
    }
    found
}

/// Every sector of a Macintosh 400K or 800K disk on a track's cells: Apple's 6-and-2 nibbles at twice the 5.25-inch
/// rate, an address field (&D5 &AA &96, then the track's low six bits, the sector, the side with the track's top
/// bits, the format and a checksum, each a 6-bit value) and a data field of 524 bytes, 12 of tags and 512 of data,
/// as the Sony driver writes it and the WOZ reader decodes it. Its `cylinder` is the address field's track.
pub fn sony(cells: &[u8]) -> Vec<Found> {
    let six = woz::inverse(&woz::SIX_AND_TWO);
    let nibbles: Vec<u8> = apple_nibbles(cells).into_iter().map(|(_, n)| n).collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i + 8 <= nibbles.len() {
        if nibbles[i..i + 3] != [0xD5, 0xAA, 0x96] {
            i += 1;
            continue;
        }
        let v: Vec<u8> = nibbles[i + 3..i + 8].iter().map(|&b| six[usize::from(b)]).collect();
        i += 3;
        if v.contains(&0xFF) || (v[0] ^ v[1] ^ v[2] ^ v[3]) & 0x3F != v[4] || v[1] >= 12 {
            continue;
        }
        let (track, side) = (v[0] | (v[2] & 1) << 6, v[2] >> 5 & 1);
        let data = woz::data_field(&nibbles, i + 5).and_then(|at| woz::decode_sony(&nibbles[(at + 4).min(nibbles.len())..], &six)).map(|bytes| bytes[12..].to_vec());
        found.push(Found { cylinder: track, head: side, id: v[1], size: 512, data });
    }
    found
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    fn c64_cells(out: &mut Vec<u8>, bytes: &[u8]) {
        for &b in bytes {
            for nibble in [b >> 4, b & 15] {
                let code = C64_CODES[usize::from(nibble)];
                out.extend((0..5).rev().map(|i| (code >> i) & 1));
            }
        }
    }

    /// A 1541 track of the given sectors.
    pub fn commodore_track(track: u8, sectors: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for (n, data) in sectors.iter().enumerate() {
            out.extend([1u8; 40]);
            let (id1, id2) = (0x41, 0x42);
            c64_cells(&mut out, &[0x08, n as u8 ^ track ^ id2 ^ id1, n as u8, track, id2, id1, 0x0F, 0x0F]);
            c64_cells(&mut out, &[0x55; 9]);
            out.extend([1u8; 40]);
            let mut block = vec![0x07];
            block.extend(data);
            block.extend([data.iter().fold(0, |s, b| s ^ b), 0, 0]);
            c64_cells(&mut out, &block);
            c64_cells(&mut out, &[0x55; 9]);
        }
        c64_cells(&mut out, &[0x55; 100]);
        out
    }

    fn apple_cells(out: &mut Vec<u8>, nibbles: &[u8]) {
        for &n in nibbles {
            out.extend((0..8).rev().map(|i| (n >> i) & 1));
        }
    }

    /// An Apple II 6-and-2 track of 16 sectors, by physical sector number.
    pub fn apple_track(track: u8, sectors: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for (sector, data) in sectors.iter().enumerate() {
            // Self-sync bytes: &FF and two 0 cells.
            for _ in 0..16 {
                apple_cells(&mut out, &[0xFF]);
                out.extend([0, 0]);
            }
            let four = |x: u8| [(x >> 1) | 0xAA, x | 0xAA];
            let volume = 254u8;
            let mut address = vec![0xD5, 0xAA, 0x96];
            for x in [volume, track, sector as u8, volume ^ track ^ sector as u8] {
                address.extend(four(x));
            }
            address.extend([0xDE, 0xAA, 0xEB]);
            apple_cells(&mut out, &address);
            for _ in 0..6 {
                apple_cells(&mut out, &[0xFF]);
                out.extend([0, 0]);
            }
            let mut values = [0u8; 342];
            for j in 0..256 {
                let b = data[j];
                values[86 + j] = b >> 2;
                let low = ((b & 1) << 1) | ((b >> 1) & 1);
                values[j % 86] |= low << (2 * (j / 86));
            }
            let mut field = vec![0xD5, 0xAA, 0xAD];
            let mut previous = 0;
            for v in values {
                field.push(APPLE_NIBBLES[usize::from((v & 0x3F) ^ previous)]);
                previous = v & 0x3F;
            }
            field.push(APPLE_NIBBLES[usize::from(previous)]);
            field.extend([0xDE, 0xAA, 0xEB]);
            apple_cells(&mut out, &field);
        }
        out
    }

    #[test]
    fn commodore_and_apple_sectors_decode_from_their_own_cells() {
        let sectors: Vec<Vec<u8>> = (0..21u32).map(|s| (0..256u32).map(|n| (n * 13 + s) as u8).collect()).collect();
        let found = commodore(&commodore_track(7, &sectors));
        assert_eq!(found.len(), 21);
        assert!(found.iter().all(|f| f.cylinder == 7 && f.data.as_ref() == Some(&sectors[usize::from(f.id)])));
        let found = apple(&apple_track(17, &sectors[..16]));
        assert_eq!(found.len(), 16);
        assert!(found.iter().all(|f| f.cylinder == 17 && f.data.as_ref() == Some(&sectors[usize::from(f.id)])));
    }
}
