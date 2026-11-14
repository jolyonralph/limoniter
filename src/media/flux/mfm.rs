//! The two-cells-a-bit encodings: IBM's FM and MFM, which most disk controllers wrote (PCs, the Atari ST, MSX, most
//! CP/M machines, HP, the TRS-80, the BBC Micro's DFS), and the Amiga's own MFM.
//!
//! In FM every data bit has a clock cell before it that is always a 1, so a 0 bit is "10" and a 1 is "11". MFM drops
//! most of those clock changes, to fit twice the data in the same space: a clock cell is a 1 only between two 0
//! data bits. Either way a byte is sixteen cells, its data bits in the second cell of each pair.
//!
//! Neither leaves a way to tell where a byte starts, so the controller writes marks that ordinary data can't make:
//! FM marks are bytes written with some clock cells missing (clock &C7), and MFM marks are &A1 bytes with one clock
//! cell missing (cells &4489), three of them before each mark byte. An IBM track is then, for each sector, an ID
//! field (mark &FE, then cylinder, head, sector number and size code, and a CRC) and a data field a short gap later
//! (mark &FB, or &F8 for deleted data, the sector's bytes and a CRC). The CRC is CCITT's 16-bit one, started at
//! &FFFF and taken over the marks too.
//!
//! The Amiga reads a whole track at once and wrote its own layout with the same MFM cells: after two &4489 sync
//! words, each sector's longwords are stored as all their odd bits and then all their even bits, which made them
//! quick for its blitter to decode, with an XOR checksum over the encoded header and another over the data.

use super::Found;

/// CCITT's CRC-16 (polynomial &1021), as IBM's controllers check their fields.
pub fn crc16(start: u16, bytes: &[u8]) -> u16 {
    let mut crc = start;
    for &byte in bytes {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

/// Sixteen cells from `at` as a number, first cell highest; None past the end.
fn word(cells: &[u8], at: usize) -> Option<u16> {
    let run = cells.get(at..at + 16)?;
    Some(run.iter().fold(0u16, |w, c| (w << 1) | u16::from(*c & 1)))
}

/// The byte whose sixteen cells start at `at`: its data bits are the second of each pair.
fn byte(cells: &[u8], at: usize) -> Option<u8> {
    let run = cells.get(at..at + 16)?;
    Some((0..8).fold(0u8, |b, i| (b << 1) | (run[i * 2 + 1] & 1)))
}

fn bytes(cells: &[u8], at: usize, count: usize) -> Option<Vec<u8>> {
    (0..count).map(|i| byte(cells, at + i * 16)).collect()
}

/// The IBM marks, as the cells they're written with.
const MFM_SYNC: u16 = 0x4489;
const FM_ID: u16 = 0xF57E;
/// FM data marks: &FB, &FA, &F9 and &F8 (deleted), each with clock &C7.
const FM_DATA: [u16; 4] = [0xF56F, 0xF56E, 0xF56B, 0xF56A];

/// How far after an ID field its data field may start, in cells: IBM's gap is 22 bytes and 12 of zeros before the
/// marks; some formats leave more. Further than this, the data is taken to belong to no ID.
const MOST_DATA_GAP: usize = 90 * 16;

/// Every IBM MFM sector on a track's cells. A sector's `cylinder` is the one its ID field gives; its head is left
/// for the caller, who knows which side was read.
pub fn ibm_mfm(cells: &[u8]) -> Vec<Found> {
    ibm(cells, false)
}

/// Every IBM FM (single density) sector on a track's cells.
pub fn ibm_fm(cells: &[u8]) -> Vec<Found> {
    ibm(cells, true)
}

/// Where the next mark after `from` is, and the mark byte: for MFM the byte after one or more &A1 syncs, for FM the
/// mark itself. `at` is where the field's bytes start, after the mark.
fn next_mark(cells: &[u8], from: usize, fm: bool) -> Option<(usize, u8)> {
    let mut window = 0u16;
    for (i, &cell) in cells.iter().enumerate().skip(from) {
        window = (window << 1) | u16::from(cell & 1);
        if i < from + 15 {
            continue;
        }
        if fm {
            if window == FM_ID {
                return Some((i + 1, 0xFE));
            }
            if let Some(n) = FM_DATA.iter().position(|m| *m == window) {
                return Some((i + 1, 0xFB - n as u8));
            }
        } else if window == MFM_SYNC {
            let mut at = i + 1;
            while word(cells, at) == Some(MFM_SYNC) {
                at += 16;
            }
            return Some((at + 16, byte(cells, at)?));
        }
    }
    None
}

fn ibm(cells: &[u8], fm: bool) -> Vec<Found> {
    // The CRC starts at &FFFF and takes in the three &A1s first in MFM, so the field's own bytes start from this.
    let start = if fm { 0xFFFF } else { crc16(0xFFFF, &[0xA1; 3]) };
    let mut found = Vec::new();
    let mut at = 0;
    while let Some((field, mark)) = next_mark(cells, at, fm) {
        at = field;
        if mark != 0xFE {
            continue;
        }
        let Some(id) = bytes(cells, field, 6) else {
            break;
        };
        if crc16(crc16(start, &[0xFE]), &id) != 0 {
            continue;
        }
        let (cylinder, head, sector, code) = (id[0], id[1], id[2], id[3]);
        at = field + 6 * 16;
        // Size codes past 7 (16K) are copy protection's, with no sector of that size behind them.
        if code > 7 {
            continue;
        }
        let size = 128usize << code;
        // The data field is the first mark after the ID whose data checks: something that looks like a mark can
        // be left in the gap by an earlier write, so one whose data doesn't check isn't the end of the search.
        let mut data = None;
        let mut search = at;
        let mut end = at;
        while let Some((data_at, data_mark)) = next_mark(cells, search, fm).filter(|(data_at, _)| *data_at - at <= MOST_DATA_GAP) {
            if !(0xF8..=0xFB).contains(&data_mark) {
                break;
            }
            let Some(field) = bytes(cells, data_at, size + 2) else {
                break;
            };
            if end == at {
                end = data_at;
            }
            if crc16(crc16(start, &[data_mark]), &field) == 0 {
                data = Some(field[..size].to_vec());
                end = data_at + size * 16;
                break;
            }
            search = data_at - 15;
        }
        at = end;
        found.push(Found { cylinder, head, id: sector, size, data });
    }
    found
}

/// The 32 cells from `at` as a longword.
fn long(cells: &[u8], at: usize) -> Option<u32> {
    let run = cells.get(at..at + 32)?;
    Some(run.iter().fold(0u32, |w, c| (w << 1) | u32::from(*c & 1)))
}

/// Every Amiga sector on a track's cells. Its `cylinder` and `head` come from its header's track number.
pub fn amiga(cells: &[u8]) -> Vec<Found> {
    const ODD: u32 = 0x5555_5555;
    let mut found = Vec::new();
    let mut window = 0u32;
    let mut i = 0;
    while i < cells.len() {
        window = (window << 1) | u32::from(cells[i] & 1);
        i += 1;
        if window != 0x4489_4489 {
            continue;
        }
        // Some tracks have a third sync word; the sector starts after the last.
        let mut at = i;
        while word(cells, at) == Some(MFM_SYNC) {
            at += 16;
        }
        // The sector as encoded longwords: header 2, label 8, header checksum 2, data checksum 2, data 256.
        let Some(raw) = (0..270).map(|n| long(cells, at + n * 32)).collect::<Option<Vec<u32>>>() else {
            break;
        };
        let pair = |n: usize| ((raw[n] & ODD) << 1) | (raw[n + 1] & ODD);
        let header_sum = raw[..10].iter().fold(0, |s, l| s ^ l) & ODD;
        if header_sum != pair(10) {
            continue;
        }
        let info = pair(0).to_be_bytes();
        let (format, track, sector) = (info[0], info[1], info[2]);
        if format != 0xFF || sector >= 22 {
            continue;
        }
        let data_sum = raw[14..270].iter().fold(0, |s, l| s ^ l) & ODD;
        let data = (data_sum == pair(12)).then(|| (0..128).flat_map(|n| (((raw[14 + n] & ODD) << 1) | (raw[142 + n] & ODD)).to_be_bytes()).collect());
        found.push(Found { cylinder: track / 2, head: track % 2, id: sector, size: 512, data });
        i = at + 270 * 32;
        window = 0;
    }
    found
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// MFM cells for data bytes, following on from the last cell written.
    pub fn mfm_bytes(out: &mut Vec<u8>, data: &[u8]) {
        for &b in data {
            for bit in (0..8).rev() {
                let d = (b >> bit) & 1;
                let previous = out.last().copied().unwrap_or(0);
                // The clock cell is a 1 only between two 0 data bits; `previous` is the last data cell.
                out.push(u8::from(d == 0 && previous == 0));
                out.push(d);
            }
        }
    }

    fn cells_of(word: u16) -> impl Iterator<Item = u8> {
        (0..16).rev().map(move |i| ((word >> i) & 1) as u8)
    }

    /// A sector for the encoders: cylinder, head, number, size code, its bytes.
    pub struct Write<'a> {
        pub cylinder: u8,
        pub head: u8,
        pub id: u8,
        pub code: u8,
        pub data: &'a [u8],
    }

    /// An IBM MFM track of the sectors, laid out as a PC's controller formats it.
    pub fn ibm_mfm_track(sectors: &[Write]) -> Vec<u8> {
        let mut out = Vec::new();
        mfm_bytes(&mut out, &[0x4E; 80]);
        for s in sectors {
            mfm_bytes(&mut out, &[0; 12]);
            out.extend((0..3).flat_map(|_| cells_of(MFM_SYNC)));
            let mut id = vec![0xFE, s.cylinder, s.head, s.id, s.code];
            id.extend(crc16(0xCDB4, &id).to_be_bytes());
            mfm_bytes(&mut out, &id);
            mfm_bytes(&mut out, &[0x4E; 22]);
            mfm_bytes(&mut out, &[0; 12]);
            out.extend((0..3).flat_map(|_| cells_of(MFM_SYNC)));
            let mut field = vec![0xFB];
            field.extend(s.data);
            field.extend(crc16(0xCDB4, &field).to_be_bytes());
            mfm_bytes(&mut out, &field);
            mfm_bytes(&mut out, &[0x4E; 40]);
        }
        mfm_bytes(&mut out, &[0x4E; 200]);
        out
    }

    /// FM cells for bytes, each clock cell a 1, or the given clock for a mark.
    fn fm_bytes(out: &mut Vec<u8>, data: &[u8], clock: u8) {
        for &b in data {
            for bit in (0..8).rev() {
                out.push((clock >> bit) & 1);
                out.push((b >> bit) & 1);
            }
        }
    }

    /// An IBM FM track of the sectors.
    pub fn ibm_fm_track(sectors: &[Write]) -> Vec<u8> {
        let mut out = Vec::new();
        fm_bytes(&mut out, &[0xFF; 40], 0xFF);
        for s in sectors {
            fm_bytes(&mut out, &[0; 6], 0xFF);
            fm_bytes(&mut out, &[0xFE], 0xC7);
            let mut id = vec![s.cylinder, s.head, s.id, s.code];
            id.extend(crc16(crc16(0xFFFF, &[0xFE]), &id).to_be_bytes());
            fm_bytes(&mut out, &id, 0xFF);
            fm_bytes(&mut out, &[0xFF; 11], 0xFF);
            fm_bytes(&mut out, &[0; 6], 0xFF);
            fm_bytes(&mut out, &[0xFB], 0xC7);
            let mut field = s.data.to_vec();
            field.extend(crc16(crc16(0xFFFF, &[0xFB]), s.data).to_be_bytes());
            fm_bytes(&mut out, &field, 0xFF);
            fm_bytes(&mut out, &[0xFF; 27], 0xFF);
        }
        fm_bytes(&mut out, &[0xFF; 100], 0xFF);
        out
    }

    /// An Amiga track: eleven (or 22) 512-byte sectors after a gap.
    pub fn amiga_track(track: u8, sectors: &[&[u8]]) -> Vec<u8> {
        const ODD: u32 = 0x5555_5555;
        let mut out = Vec::new();
        mfm_bytes(&mut out, &[0; 300]);
        let count = sectors.len() as u8;
        for (n, data) in sectors.iter().enumerate() {
            mfm_bytes(&mut out, &[0, 0]);
            out.extend((0..2).flat_map(|_| cells_of(MFM_SYNC)));
            // Each longword split into its odd bits then its even bits, then MFM-encoded.
            let info = u32::from_be_bytes([0xFF, track, n as u8, count - n as u8]);
            let mut halves: Vec<u32> = vec![(info >> 1) & ODD, info & ODD];
            halves.extend([0u32; 8]);
            let longs: Vec<u32> = data.as_chunks::<4>().0.iter().map(|c| u32::from_be_bytes(*c)).collect();
            let odd: Vec<u32> = longs.iter().map(|l| (l >> 1) & ODD).collect();
            let even: Vec<u32> = longs.iter().map(|l| l & ODD).collect();
            // The checksums are over the encoded longwords' data bits, which are just these halves.
            let header_sum = halves.iter().fold(0, |s, l| s ^ l);
            let data_sum = odd.iter().chain(&even).fold(0, |s, l| s ^ l);
            halves.extend([(header_sum >> 1) & ODD, header_sum & ODD, (data_sum >> 1) & ODD, data_sum & ODD]);
            halves.extend(odd);
            halves.extend(even);
            // Each half's data bits sit at the odd positions; the encoder works out the clocks between them.
            let bytes: Vec<u8> = halves.iter().flat_map(|h| h.to_be_bytes()).collect();
            for b in bytes {
                for bit in (0..4).rev() {
                    let d = (b >> (bit * 2)) & 1;
                    let previous = out.last().copied().unwrap_or(0);
                    out.push(u8::from(d == 0 && previous == 0));
                    out.push(d);
                }
            }
        }
        mfm_bytes(&mut out, &[0; 600]);
        out
    }

    #[test]
    fn ibm_and_amiga_sectors_decode_from_their_own_cells() {
        let data: Vec<u8> = (0..512u32).map(|n| (n * 7 + 3) as u8).collect();
        let sectors: Vec<Write> = (1..=9).map(|id| Write { cylinder: 5, head: 1, id, code: 2, data: &data }).collect();
        let found = ibm_mfm(&ibm_mfm_track(&sectors));
        assert_eq!(found.len(), 9);
        assert!(found.iter().all(|f| f.data.as_deref() == Some(&data[..]) && f.cylinder == 5 && f.size == 512));
        let small: Vec<Write> = (0..10).map(|id| Write { cylinder: 3, head: 0, id, code: 1, data: &data[..256] }).collect();
        let found = ibm_fm(&ibm_fm_track(&small));
        assert_eq!(found.iter().map(|f| f.id).collect::<Vec<_>>(), (0..10).collect::<Vec<_>>());
        assert!(found.iter().all(|f| f.data.as_deref() == Some(&data[..256])));
        let amiga_sectors: Vec<&[u8]> = (0..11).map(|_| &data[..]).collect();
        let found = amiga(&amiga_track(37, &amiga_sectors));
        assert_eq!(found.len(), 11);
        assert!(found.iter().all(|f| f.cylinder == 18 && f.head == 1 && f.data.as_deref() == Some(&data[..])));
        // A flipped cell in a sector's data fails its check, and only that sector.
        let mut cells = ibm_mfm_track(&sectors);
        let at = cells.len() / 2;
        cells[at] ^= 1;
        let found = ibm_mfm(&cells);
        assert!(found.iter().filter(|f| f.data.is_none()).count() <= 1);
    }
}
