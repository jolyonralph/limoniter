//! Apple II WOZ disk images (1.0, 2.0 and 2.1), as Applesauce makes them: the bits a floppy drive read off each
//! track, rather than the sectors a DOS saw, so that copy-protected disks are kept whole. Here the bits are read the
//! way the Apple's disk controller read them, back into sectors, and the sectors handed to the DOS 3.3, DOS 3.2,
//! ProDOS, CP/M or Macintosh reader as if they'd come from a sector image.
//!
//! The file: a 12-byte header ("WOZ1" or "WOZ2", $FF, LF CR LF, a CRC), then chunks of a 4-letter name and a
//! length. INFO says whether it's a 5.25-inch or 3.5-inch disk; TMAP maps each quarter track (5.25-inch) or each
//! track and side (3.5-inch) to one of the tracks in TRKS, or $FF for none. WOZ 1 keeps each track in 6656 bytes
//! with its length in bits at the end; WOZ 2 keeps a table of where each track's 512-byte blocks start and how many
//! bits it has. WOZ 2.1 may keep a track as flux timings instead (FLUX), a byte per transition counting 125ns ticks.
//!
//! The bits are read as the controller does: a 1 starts a byte, and a byte is done when its top bit is set, so the
//! timing bits between bytes fall away and the stream comes back into step after a slip on its own. A track is
//! read round twice, so a sector that runs over the place the track was cut is found whole.
//!
//! 5.25-inch: each sector is an address field (D5 AA 96, then volume, track, sector and their checksum, each in
//! 4-and-4 encoding) and a data field (D5 AA AD, then 342 bytes of 6-and-2 encoding and a checksum) for the 16
//! sectors of DOS 3.3 and ProDOS; DOS 3.2's 13-sector disks have D5 AA B5 address fields and 410 bytes of 5-and-3
//! encoding. Sectors are put in the order a DOS-order (.do) image keeps them. 3.5-inch: Sony's GCR, 80 tracks of 12
//! sectors falling to 8 towards the middle, each 524 bytes (12 of tags, then 512 of data) in 699 6-bit bytes with a
//! 3-byte checksum, read into ProDOS-order 512-byte blocks.
//!
//! A sector whose fields can't be found, or whose checksum is wrong, is left out and said so; anything that needs
//! it gets an error rather than made-up bytes. A disk with no file system on it gives its readable sectors as a
//! disk image to look at in hex.

use super::{Entry, InfoSection, Source, Volume};
use std::sync::Arc;

/// The 64 bytes a 6-and-2 (and Sony) encoded 6-bit value can be written as, in order.
const SIX_AND_TWO: [u8; 64] = [
    0x96, 0x97, 0x9A, 0x9B, 0x9D, 0x9E, 0x9F, 0xA6, 0xA7, 0xAB, 0xAC, 0xAD, 0xAE, 0xAF, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6, 0xB7, 0xB9, 0xBA, 0xBB, 0xBC, 0xBD, 0xBE, 0xBF, 0xCB, 0xCD, 0xCE, 0xCF, 0xD3, 0xD6, 0xD7, 0xD9, 0xDA, 0xDB, 0xDC, 0xDD, 0xDE, 0xDF, 0xE5, 0xE6, 0xE7, 0xE9, 0xEA, 0xEB, 0xEC, 0xED, 0xEE, 0xEF, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6, 0xF7, 0xF9, 0xFA, 0xFB, 0xFC, 0xFD, 0xFE, 0xFF,
];
/// The 32 bytes a 5-and-3 encoded 5-bit value can be written as, in order.
const FIVE_AND_THREE: [u8; 32] = [
    0xAB, 0xAD, 0xAE, 0xAF, 0xB5, 0xB6, 0xB7, 0xBA, 0xBB, 0xBD, 0xBE, 0xBF, 0xD6, 0xD7, 0xDA, 0xDB, 0xDD, 0xDE, 0xDF, 0xEA, 0xEB, 0xED, 0xEE, 0xEF, 0xF5, 0xF6, 0xF7, 0xFA, 0xFB, 0xFD, 0xFE, 0xFF,
];
/// Where each sector of a DOS-order image is on the track: DOS 3.3 numbers its sectors in a different order from
/// the address fields, so that it has time to deal with one before the next comes round.
const DOS_TO_PHYSICAL: [usize; 16] = [0, 13, 11, 9, 7, 5, 3, 1, 14, 12, 10, 8, 6, 4, 2, 15];
/// How many sectors a 3.5-inch track has, in each of its five speed zones of 16 tracks.
const SONY_SECTORS: [usize; 5] = [12, 11, 10, 9, 8];
/// The length of a WOZ 1 track's record.
const WOZ1_TRACK: usize = 6656;
/// How many 5.25-inch tracks are looked at: 35, and the 40 some disks have.
const MOST_TRACKS: usize = 40;

/// Whether a file starts as a WOZ does.
pub fn looks_like(head: &[u8]) -> bool {
    (head.starts_with(b"WOZ1") || head.starts_with(b"WOZ2")) && head.get(4..8) == Some(b"\xFF\x0A\x0D\x0A")
}

/// What a WOZ file's chunks say: the disk's kind, its track map, and each track's bits.
struct Woz {
    version: u8,
    /// 1 for 5.25-inch, 2 for 3.5-inch.
    disk_type: u8,
    sides: u8,
    creator: String,
    /// The bit cell, in 125ns ticks: 32 (4µs) for a 5.25-inch disk, 16 for 3.5-inch.
    bit_timing: u8,
    tmap: Vec<u8>,
    flux_map: Vec<u8>,
    /// Where each track's data is in the file, and how many bits (or for a flux track, bytes) it has.
    tracks: Vec<(usize, usize)>,
    meta: Vec<(String, String)>,
}

fn le16(b: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([b[at], b[at + 1]]))
}

fn le32(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize
}

impl Woz {
    fn parse(file: &[u8]) -> Result<Self, String> {
        if !looks_like(file) || file.len() < 12 {
            return Err("This isn't a WOZ disk image: it doesn't start with \"WOZ1\" or \"WOZ2\".".into());
        }
        let version = if file[3] == b'1' { 1 } else { 2 };
        let mut woz = Woz { version, disk_type: 1, sides: 1, creator: String::new(), bit_timing: 32, tmap: Vec::new(), flux_map: Vec::new(), tracks: Vec::new(), meta: Vec::new() };
        let mut at = 12;
        let mut info = false;
        while at + 8 <= file.len() {
            let id = &file[at..at + 4];
            let size = le32(file, at + 4);
            let start = at + 8;
            let Some(data) = start.checked_add(size).and_then(|end| file.get(start..end)) else {
                // A chunk cut short at the end of the file: what's before it is still read.
                break;
            };
            match id {
                b"INFO" if data.len() >= 37 => {
                    info = true;
                    woz.disk_type = data[1];
                    woz.creator = String::from_utf8_lossy(&data[5..37]).trim().to_string();
                    if data[0] >= 2 && data.len() >= 40 {
                        woz.sides = data[37].max(1);
                        if data[39] != 0 {
                            woz.bit_timing = data[39];
                        }
                    }
                    if data[0] < 2 && woz.disk_type == 2 {
                        woz.bit_timing = 16;
                    }
                }
                b"TMAP" => woz.tmap = data.to_vec(),
                b"FLUX" => woz.flux_map = data.to_vec(),
                b"TRKS" if version == 1 => {
                    woz.tracks = (0..data.len() / WOZ1_TRACK)
                        .map(|t| {
                            let record = start + t * WOZ1_TRACK;
                            (record, le16(file, record + 6648))
                        })
                        .collect();
                }
                b"TRKS" => {
                    woz.tracks = data.as_chunks::<8>().0.iter().take(160).map(|trk| (le16(trk, 0) * 512, le32(trk, 4))).collect();
                }
                b"META" => {
                    woz.meta = String::from_utf8_lossy(data).lines().filter_map(|line| line.split_once('\t')).filter(|(_, v)| !v.trim().is_empty()).map(|(k, v)| (k.to_string(), v.replace('|', ", "))).collect();
                }
                _ => {}
            }
            at = start + size;
        }
        if !info || woz.tmap.is_empty() || woz.tracks.is_empty() {
            return Err("This WOZ image is missing its INFO, TMAP or TRKS chunk, so its tracks can't be found.".into());
        }
        if !matches!(woz.disk_type, 1 | 2) {
            return Err(format!("This WOZ image says it's a disk of type {}, which is neither 5.25-inch nor 3.5-inch.", woz.disk_type));
        }
        Ok(woz)
    }

    /// The bits of the track a map entry names, one to a byte, or None for no track. A flux track is turned into
    /// bits by counting how many bit cells each gap between transitions is.
    fn bits(&self, file: &[u8], map_index: usize) -> Option<Vec<u8>> {
        if let Some(&index) = self.flux_map.get(map_index).filter(|&&i| i != 0xFF) {
            let &(start, length) = self.tracks.get(usize::from(index))?;
            let flux = file.get(start..start.checked_add(length)?)?;
            let cell = u32::from(self.bit_timing.max(1));
            let mut bits = Vec::new();
            let mut ticks = 0u32;
            for &b in flux {
                ticks += u32::from(b);
                if b == 255 {
                    continue;
                }
                let cells = ((ticks + cell / 2) / cell).clamp(1, 64);
                bits.extend(std::iter::repeat_n(0, cells as usize - 1));
                bits.push(1);
                ticks = 0;
            }
            return (!bits.is_empty()).then_some(bits);
        }
        let index = *self.tmap.get(map_index).filter(|&&i| i != 0xFF)?;
        let &(start, count) = self.tracks.get(usize::from(index))?;
        if count == 0 {
            return None;
        }
        let bytes = file.get(start..start.checked_add(count.div_ceil(8))?)?;
        Some((0..count).map(|i| bytes[i / 8] >> (7 - i % 8) & 1).collect())
    }
}

/// A track's bits as the disk controller would read them into bytes, round the track twice.
fn nibbles(bits: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bits.len() / 4);
    let mut latch = 0u8;
    for &bit in bits.iter().chain(bits) {
        if latch == 0 && bit == 0 {
            continue;
        }
        latch = latch << 1 | bit;
        if latch & 0x80 != 0 {
            out.push(latch);
            latch = 0;
        }
    }
    out
}

fn inverse(table: &[u8]) -> [u8; 256] {
    let mut out = [0xFF; 256];
    for (value, &byte) in table.iter().enumerate() {
        out[usize::from(byte)] = value as u8;
    }
    out
}

/// A 4-and-4 encoded byte: its odd bits in the first, its even bits in the second, each with the rest set.
fn four_and_four(a: u8, b: u8) -> u8 {
    (a << 1 | 1) & b
}

/// Where the next data field's D5 AA AD is, a little after an address field.
fn data_field(n: &[u8], from: usize) -> Option<usize> {
    (from..from + 64).find(|&i| n.get(i..i + 3) == Some(&[0xD5, 0xAA, 0xAD]))
}

/// A 6-and-2 data field's 342 bytes and checksum, as 256 bytes, or None if they don't add up.
fn decode_62(field: &[u8], table: &[u8; 256]) -> Option<Vec<u8>> {
    let field = field.get(..343)?;
    let mut values = [0u8; 342];
    let mut running = 0u8;
    for (i, &byte) in field[..342].iter().enumerate() {
        let value = table[usize::from(byte)];
        if value == 0xFF {
            return None;
        }
        running ^= value;
        values[i] = running;
    }
    if table[usize::from(field[342])] != running {
        return None;
    }
    // The first 86 hold each byte's lowest two bits, swapped; the rest its top six.
    let (aux, main) = values.split_at(86);
    Some(
        (0..256)
            .map(|i| {
                let two = aux[i % 86] >> (2 * (i / 86)) & 3;
                main[i] << 2 | (two & 1) << 1 | two >> 1
            })
            .collect(),
    )
}

/// A 5-and-3 data field's 410 bytes and checksum, as 256 bytes, or None if they don't add up. As DOS 3.2's RWTS
/// writes it: 154 values of each byte's low three bits, stored backwards, then 256 of their top five.
fn decode_53(field: &[u8], table: &[u8; 256]) -> Option<Vec<u8>> {
    let field = field.get(..411)?;
    let mut running = 0u8;
    let mut value = |byte: u8| -> Option<u8> {
        let v = table[usize::from(byte)];
        if v == 0xFF {
            return None;
        }
        running ^= v;
        Some(running)
    };
    let mut three = [0u8; 154];
    for i in (0..154).rev() {
        three[i] = value(field[153 - i])?;
    }
    let mut five = [0u8; 256];
    for (i, slot) in five.iter_mut().enumerate() {
        *slot = value(field[154 + i])? << 3;
    }
    if table[usize::from(field[410])] != running {
        return None;
    }
    let mut out = Vec::with_capacity(256);
    for i in (0..51).rev() {
        let (a, b, c) = (three[i], three[51 + i], three[102 + i]);
        let d = (a & 2) << 1 | (b & 2) | (c & 2) >> 1;
        let e = (a & 1) << 2 | (b & 1) << 1 | (c & 1);
        out.push(five[i] | (a >> 2 & 7));
        out.push(five[51 + i] | (b >> 2 & 7));
        out.push(five[102 + i] | (c >> 2 & 7));
        out.push(five[153 + i] | (d & 7));
        out.push(five[204 + i] | (e & 7));
    }
    out.push(five[255] | (three[153] & 7));
    Some(out)
}

/// What's found of one kind of sector on a track: each sector's bytes by its address field's number, and which have
/// an address field with no data field after it, as a sector never written has.
struct Found {
    sectors: Vec<Option<Vec<u8>>>,
    unwritten: Vec<bool>,
}

impl Found {
    fn none(count: usize) -> Self {
        Self { sectors: vec![None; count], unwritten: vec![false; count] }
    }
}

/// The sectors found on a 5.25-inch track, 16-sector and 13-sector apart.
fn read_525_track(n: &[u8], six: &[u8; 256], five: &[u8; 256]) -> (Found, Found) {
    let mut sixteen = Found::none(16);
    let mut thirteen = Found::none(13);
    let mut i = 0;
    while i + 14 <= n.len() {
        if n[i] != 0xD5 || n[i + 1] != 0xAA || !matches!(n[i + 2], 0x96 | 0xB5) {
            i += 1;
            continue;
        }
        let field = |k: usize| four_and_four(n[i + 3 + 2 * k], n[i + 4 + 2 * k]);
        let (volume, track, sector, checksum) = (field(0), field(1), field(2), field(3));
        let after = i + 11;
        let sixteen_sector = n[i + 2] == 0x96;
        i += 3;
        if volume ^ track ^ sector != checksum {
            continue;
        }
        let sector = usize::from(sector);
        let (found, decode): (&mut Found, fn(&[u8], &[u8; 256]) -> Option<Vec<u8>>) = if sixteen_sector { (&mut sixteen, decode_62) } else { (&mut thirteen, decode_53) };
        if sector >= found.sectors.len() || found.sectors[sector].is_some() {
            continue;
        }
        match data_field(n, after) {
            None => found.unwritten[sector] = true,
            Some(data) => {
                found.sectors[sector] = decode(&n[data + 3..], if sixteen_sector { six } else { five });
                if found.sectors[sector].is_some() {
                    found.unwritten[sector] = false;
                }
            }
        }
    }
    (sixteen, thirteen)
}

/// A Sony data field's 699 6-bit values and 4 of checksum, as its 524 bytes (12 of tags, 512 of data), or None if
/// the checksum doesn't match. The three running sums are as the Macintosh's Sony driver keeps them.
fn decode_sony(field: &[u8], table: &[u8; 256]) -> Option<Vec<u8>> {
    let field = field.get(..703)?;
    let mut values = Vec::with_capacity(703);
    for &byte in field {
        let v = table[usize::from(byte)];
        if v == 0xFF {
            return None;
        }
        values.push(v);
    }
    let (mut b1, mut b2, mut b3) = ([0u8; 175], [0u8; 175], [0u8; 175]);
    let mut at = 0;
    for i in 0..175 {
        let w4 = values[at];
        let (w1, w2) = (values[at + 1], values[at + 2]);
        let w3 = if i != 174 { values[at + 3] } else { 0 };
        at += if i != 174 { 4 } else { 3 };
        b1[i] = w1 & 0x3F | (w4 << 2 & 0xC0);
        b2[i] = w2 & 0x3F | (w4 << 4 & 0xC0);
        b3[i] = w3 & 0x3F | (w4 << 6 & 0xC0);
    }
    let (mut c1, mut c2, mut c3) = (0u32, 0u32, 0u32);
    let mut out = Vec::with_capacity(524);
    let mut count = 0;
    loop {
        c1 = (c1 & 0xFF) << 1;
        if c1 & 0x100 != 0 {
            c1 += 1;
        }
        let value = b1[count] ^ c1 as u8;
        c3 += u32::from(value);
        if c1 & 0x100 != 0 {
            c3 += 1;
            c1 &= 0xFF;
        }
        out.push(value);
        let value = b2[count] ^ c3 as u8;
        c2 += u32::from(value);
        if c3 > 0xFF {
            c2 += 1;
            c3 &= 0xFF;
        }
        out.push(value);
        if out.len() == 524 {
            break;
        }
        let value = b3[count] ^ c2 as u8;
        c1 += u32::from(value);
        if c2 > 0xFF {
            c1 += 1;
            c2 &= 0xFF;
        }
        out.push(value);
        count += 1;
    }
    let c4 = ((c1 & 0xC0) >> 6 | (c2 & 0xC0) >> 4 | (c3 & 0xC0) >> 2) as u8;
    let sums = [c4 & 0x3F, (c3 & 0x3F) as u8, (c2 & 0x3F) as u8, (c1 & 0x3F) as u8];
    (values[699..703] == sums).then_some(out)
}

/// The sectors found on a 3.5-inch track, by number.
fn read_sony_track(n: &[u8], wanted: usize, six: &[u8; 256]) -> Vec<Option<Vec<u8>>> {
    let mut sectors = vec![None; wanted];
    let mut i = 0;
    while i + 10 <= n.len() {
        if n[i..i + 3] != [0xD5, 0xAA, 0x96] {
            i += 1;
            continue;
        }
        let v: Vec<u8> = n[i + 3..i + 8].iter().map(|&b| six[usize::from(b)]).collect();
        i += 3;
        if v.contains(&0xFF) || (v[0] ^ v[1] ^ v[2] ^ v[3]) & 0x3F != v[4] {
            continue;
        }
        let sector = usize::from(v[1]);
        let Some(data) = data_field(n, i + 5) else { continue };
        if sector < wanted
            && sectors[sector].is_none()
            && let Some(bytes) = decode_sony(&n[(data + 4).min(n.len())..], six)
        {
            sectors[sector] = Some(bytes);
        }
    }
    sectors
}

/// The disk's sectors laid out as a sector image, with which of them could be read. Reading a part that wasn't
/// gives an error saying where it is on the disk, never made-up bytes.
struct Sectors {
    bytes: Vec<u8>,
    unit: usize,
    found: Vec<bool>,
    /// Where each unit is on the disk, said plainly.
    places: Vec<String>,
    /// How many sectors the disk has: fewer than the units for DOS 3.2's 13 to a track.
    on_disk: usize,
}

impl Source for Sectors {
    fn len(&self) -> u64 {
        self.bytes.len() as u64
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        let start = usize::try_from(offset).map_err(|_| "past the end of the disk".to_string())?;
        let bytes = start.checked_add(buf.len()).and_then(|end| self.bytes.get(start..end)).ok_or("past the end of the disk")?;
        if !buf.is_empty()
            && let Some(unit) = (start / self.unit..=(start + buf.len() - 1) / self.unit).find(|&u| !self.found[u])
        {
            return Err(format!("{} couldn't be read off the disk (it may be copy-protected or damaged), so this can't be read.", self.places[unit]));
        }
        buf.copy_from_slice(bytes);
        Ok(())
    }
}

/// Lists what couldn't be read, a track to a phrase: "track 3", or "track 17 sectors 2, 5".
fn missing_summary(per_track: &[(String, Vec<usize>, usize)], heading: &str, after: &str) -> Option<String> {
    let phrases: Vec<String> = per_track
        .iter()
        .filter(|(_, missing, _)| !missing.is_empty())
        .map(|(track, missing, all)| {
            if missing.len() == *all {
                format!("{track} (all of it)")
            } else {
                format!("{track} sector{} {}", if missing.len() == 1 { "" } else { "s" }, missing.iter().map(usize::to_string).collect::<Vec<_>>().join(", "))
            }
        })
        .collect();
    if phrases.is_empty() {
        return None;
    }
    let shown = phrases.len().min(24);
    let more = if phrases.len() > shown { format!(", and {} more tracks", phrases.len() - shown) } else { String::new() };
    Some(format!("{heading}: {}{more}. {after}", phrases[..shown].join("; ")))
}

/// A WOZ disk, opened as the file system its sectors hold.
pub struct WozDisk {
    inner: Option<Box<dyn Volume>>,
    /// The readable sectors as a sector image, for a disk with no file system to read.
    image: Option<(String, Vec<u8>)>,
    what: String,
    notes: Vec<String>,
    info: Vec<InfoSection>,
}

/// A WOZ image's disk read back into sectors: the sectors, what kind of disk it is, and what couldn't be read.
struct Decoded {
    woz: Woz,
    sectors: Sectors,
    label: String,
    notes: Vec<String>,
}

fn decode(file: &[u8]) -> Result<Decoded, String> {
    let woz = Woz::parse(file)?;
    let six = inverse(&SIX_AND_TWO);
    let mut notes = Vec::new();
    let (sectors, label) = if woz.disk_type == 1 {
        let five = inverse(&FIVE_AND_THREE);
        let mut sixteen = Vec::new();
        let mut thirteen = Vec::new();
        for track in 0..MOST_TRACKS {
            // The quarter track the head sits on for a whole track; a disk imaged with only its neighbours mapped is
            // read from them, as the head is wider than the track.
            let bits = [track * 4, track * 4 + 1, (track * 4).wrapping_sub(1)].iter().find_map(|&q| woz.bits(file, q));
            let (a, b) = bits.map_or_else(|| (Found::none(16), Found::none(13)), |bits| read_525_track(&nibbles(&bits), &six, &five));
            sixteen.push(a);
            thirteen.push(b);
        }
        let count = |tracks: &[Found]| tracks.iter().flat_map(|t| &t.sectors).filter(|s| s.is_some()).count();
        let use_13 = count(&thirteen) > count(&sixteen);
        let per_track = if use_13 { 13 } else { 16 };
        let mut tracks = if use_13 { thirteen } else { sixteen };
        // A 40-track disk is kept as one only if its last 5 tracks have something on them.
        let last = if tracks[35..].iter().flat_map(|t| &t.sectors).any(Option::is_some) { 40 } else { 35 };
        tracks.truncate(last);
        let mut bytes = vec![0u8; last * 16 * 256];
        let mut found = vec![false; last * 16];
        let mut places = Vec::with_capacity(last * 16);
        let mut missing = Vec::new();
        let mut unwritten = Vec::new();
        for (t, track) in tracks.iter().enumerate() {
            for logical in 0..16 {
                // A 13-sector disk's sectors are numbered as DOS 3.2 numbers them; they keep their own places.
                let physical = if use_13 { logical } else { DOS_TO_PHYSICAL[logical] };
                places.push(format!("Track {t} sector {physical}"));
                if let Some(Some(data)) = track.sectors.get(physical).filter(|_| physical < per_track) {
                    let at = (t * 16 + logical) * 256;
                    bytes[at..at + 256].copy_from_slice(data);
                    found[t * 16 + logical] = true;
                }
            }
            let lost: Vec<usize> = (0..per_track).filter(|&s| track.sectors[s].is_none() && !track.unwritten[s]).collect();
            missing.push((format!("track {t}"), lost, per_track));
            let blank: Vec<usize> = (0..per_track).filter(|&s| track.sectors[s].is_none() && track.unwritten[s]).collect();
            unwritten.push((format!("track {t}"), blank, per_track));
        }
        if !found.iter().any(|f| *f) {
            return Err("No sectors could be read from this WOZ image: its tracks don't hold the address and data fields DOS 3.3, ProDOS or DOS 3.2 write, as a copy-protected disk's often don't.".into());
        }
        notes.extend(missing_summary(&missing, "Sectors that couldn't be read, so files using them can't be opened", "Copy-protected disks often have tracks no DOS can read."));
        notes.extend(missing_summary(&unwritten, "Sectors never written (an address field with no data after it, as a freshly formatted DOS 3.2 disk has)", "Like any other unread sector, nothing can be read from them."));
        let kind = if use_13 { "13-sector (DOS 3.2)" } else { "16-sector" };
        (Sectors { bytes, unit: 256, found, places, on_disk: last * per_track }, format!("5.25-inch, {kind}, {last} tracks"))
    } else {
        let sides = usize::from(woz.sides.clamp(1, 2));
        let mut bytes = Vec::new();
        let mut found = Vec::new();
        let mut places = Vec::new();
        let mut missing = Vec::new();
        for track in 0..80 {
            let wanted = SONY_SECTORS[track / 16];
            for side in 0..sides {
                // WOZ 2 maps a 3.5-inch disk's track and side as (track << 1) + side; WOZ 1 kept one side's tracks
                // first, then the other's.
                let map_index = if woz.version == 1 { side * 80 + track } else { track * 2 + side };
                let read = woz.bits(file, map_index).map_or_else(|| vec![None; wanted], |bits| read_sony_track(&nibbles(&bits), wanted, &six));
                let mut lost = Vec::new();
                for (sector, data) in read.into_iter().enumerate() {
                    places.push(format!("Block {} (track {track} side {side} sector {sector})", found.len()));
                    match data {
                        Some(data) => {
                            bytes.extend(&data[12..]);
                            found.push(true);
                        }
                        None => {
                            bytes.extend([0u8; 512]);
                            found.push(false);
                            lost.push(sector);
                        }
                    }
                }
                missing.push((if sides == 2 { format!("track {track} side {side}") } else { format!("track {track}") }, lost, wanted));
            }
        }
        if !found.iter().any(|f| *f) {
            return Err("No sectors could be read from this 3.5-inch WOZ image: its tracks don't hold the Sony format's address and data fields.".into());
        }
        notes.extend(missing_summary(&missing, "Sectors that couldn't be read, so files using them can't be opened", "Copy-protected disks often have tracks no DOS can read."));
        (Sectors { on_disk: found.len(), bytes, unit: 512, found, places }, format!("3.5-inch, {}K", if sides == 2 { 800 } else { 400 }))
    };
    Ok(Decoded { woz, sectors, label, notes })
}

/// Opens a WOZ image.
pub fn open(source: Arc<dyn Source>) -> Result<Box<dyn Volume>, String> {
    let file = source.read_vec(0, usize::try_from(source.len()).map_err(|_| "This WOZ image is too large.")?.min(64 * 1024 * 1024))?;
    let Decoded { woz, sectors, label, mut notes } = decode(&file)?;
    let mut info_rows = vec![("WOZ version".to_string(), woz.version.to_string()), ("Made by".to_string(), woz.creator.clone())];
    info_rows.extend(woz.meta.iter().cloned());
    let readable = sectors.found.iter().filter(|f| **f).count();
    info_rows.push(("Disk".into(), label.clone()));
    info_rows.push(("Sectors read".into(), format!("{readable} of {}", sectors.on_disk)));
    let five_inch = woz.disk_type == 1;
    // A copy with the unread sectors as zeros, to tell the file system by and to give as an image if there's none.
    let plain = sectors.bytes.clone();
    let source: Arc<dyn Source> = Arc::new(sectors);
    let what = |inner: &str| format!("WOZ {} image ({label}): {inner}", woz.version);
    let inner: Result<Box<dyn Volume>, String> = if five_inch {
        match super::apple2::Apple2::open(source.clone(), "do") {
            Ok(apple) => Ok(Box::new(apple)),
            Err(problem) => super::cpm::open_guessed(source.clone(), None, "").map(|cpm| Box::new(cpm) as Box<dyn Volume>).map_err(|_| problem),
        }
    } else if super::mac::kind_of(&plain[..plain.len().min(4096)]).is_some() {
        super::mac::open_disk(source.clone(), &format!("{}K Macintosh disk", plain.len() / 1024))
    } else {
        super::apple2::Apple2::open(source.clone(), "po").map(|apple| Box::new(apple) as Box<dyn Volume>)
    };
    let info = vec![InfoSection { title: "WOZ image".into(), rows: info_rows }];
    let disk = match inner {
        Ok(inner) => WozDisk { what: what(&inner.describe()), inner: Some(inner), image: None, notes, info },
        Err(problem) => {
            let (name, kind) = if five_inch { ("sectors.do", "DOS-order") } else { ("blocks.po", "ProDOS-order") };
            notes.insert(0, format!("No file system could be read: {problem} The sectors that could be read are given as a {kind} disk image, to look at in hex; those that couldn't are zeros in it."));
            WozDisk { what: what("no file system found"), inner: None, image: Some((name.into(), plain)), notes, info }
        }
    };
    Ok(Box::new(disk))
}

impl Volume for WozDisk {
    fn describe(&self) -> String {
        self.what.clone()
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        match (&self.inner, &self.image) {
            (Some(inner), _) => inner.list(dir),
            (None, Some((name, bytes))) if dir.is_none() => {
                let mut entry = Entry::file(name.clone(), bytes.len() as u64, 0);
                entry.kind = "Disk image".into();
                entry.detail = "The sectors that could be read".into();
                Ok(vec![entry])
            }
            _ => Ok(Vec::new()),
        }
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        match (&self.inner, &self.image) {
            (Some(inner), _) => inner.read(entry),
            (None, Some((_, bytes))) => Ok(bytes.clone()),
            _ => Err("That file isn't on the disk.".into()),
        }
    }

    fn notes(&self) -> Vec<String> {
        self.notes.iter().cloned().chain(self.inner.iter().flat_map(|inner| inner.notes())).collect()
    }

    fn text_preset(&self, entry: &Entry) -> Option<&'static str> {
        self.inner.as_ref().and_then(|inner| inner.text_preset(entry))
    }

    fn info(&self) -> Vec<InfoSection> {
        self.info.iter().cloned().chain(self.inner.iter().flat_map(|inner| inner.info())).collect()
    }

    fn reading_bytes(&self, entry: &Entry, bytes: Vec<u8>) -> Vec<u8> {
        match &self.inner {
            Some(inner) => inner.reading_bytes(entry, bytes),
            None => bytes,
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Bits for bytes written to a disk: each byte's bits, top first, and two zero bits after each sync byte.
    fn write(bits: &mut Vec<u8>, bytes: &[u8], sync: bool) {
        for &byte in bytes {
            bits.extend((0..8).rev().map(|i| byte >> i & 1));
            if sync {
                bits.extend([0, 0]);
            }
        }
    }

    fn encode_62(data: &[u8]) -> Vec<u8> {
        let mut values = vec![0u8; 342];
        for (i, &byte) in data.iter().enumerate() {
            values[86 + i] = byte >> 2;
            let two = (byte & 1) << 1 | (byte >> 1 & 1);
            values[i % 86] |= two << (2 * (i / 86));
        }
        let mut out = Vec::new();
        let mut previous = 0;
        for v in values {
            out.push(SIX_AND_TWO[usize::from(v ^ previous)]);
            previous = v;
        }
        out.push(SIX_AND_TWO[usize::from(previous)]);
        out
    }

    /// A 16-sector track as DOS 3.3 writes it, from a DOS-order image's track, with the sectors `damaged` names
    /// given a wrong checksum.
    fn track_bits(image: &[u8], track: usize, damaged: &[usize]) -> Vec<u8> {
        let mut bits = Vec::new();
        write(&mut bits, &[0xFF; 40], true);
        for physical in 0..16 {
            let logical = DOS_TO_PHYSICAL.iter().position(|&p| p == physical).unwrap();
            let four = |v: u8| [v >> 1 | 0xAA, v | 0xAA];
            let (volume, t, s) = (254u8, track as u8, physical as u8);
            let mut address = vec![0xD5, 0xAA, 0x96];
            for v in [volume, t, s, volume ^ t ^ s] {
                address.extend(four(v));
            }
            address.extend([0xDE, 0xAA, 0xEB]);
            write(&mut bits, &address, false);
            write(&mut bits, &[0xFF; 6], true);
            let at = (track * 16 + logical) * 256;
            let mut data = vec![0xD5, 0xAA, 0xAD];
            data.extend(encode_62(&image[at..at + 256]));
            if damaged.contains(&physical) {
                data[10] = if data[10] == 0x96 { 0x97 } else { 0x96 };
            }
            data.extend([0xDE, 0xAA, 0xEB]);
            write(&mut bits, &data, false);
            write(&mut bits, &[0xFF; 20], true);
        }
        bits
    }

    /// A WOZ 2 image of a 140K DOS-order disk, its sectors written as DOS 3.3 writes them.
    pub fn woz_of(image: &[u8], damaged: &[(usize, usize)]) -> Vec<u8> {
        let mut file = b"WOZ2\xFF\x0A\x0D\x0A\0\0\0\0".to_vec();
        let mut info = vec![2u8, 1, 0, 0, 0];
        info.extend(format!("{:<32}", "Limoniter test").bytes());
        info.extend([1, 1, 32]);
        info.resize(60, 0);
        file.extend(b"INFO");
        file.extend(60u32.to_le_bytes());
        file.extend(info);
        let mut tmap = vec![0xFFu8; 160];
        for track in 0..35 {
            tmap[track * 4] = track as u8;
            if track > 0 {
                tmap[track * 4 - 1] = track as u8;
            }
            tmap[track * 4 + 1] = track as u8;
        }
        file.extend(b"TMAP");
        file.extend(160u32.to_le_bytes());
        file.extend(tmap);
        let tracks: Vec<Vec<u8>> = (0..35).map(|t| track_bits(image, t, &damaged.iter().filter(|(dt, _)| *dt == t).map(|(_, s)| *s).collect::<Vec<_>>())).collect();
        let mut table = Vec::new();
        let mut blocks = Vec::new();
        let mut next_block = 3usize;
        for bits in &tracks {
            let mut bytes: Vec<u8> = bits.chunks(8).map(|c| c.iter().enumerate().fold(0u8, |a, (i, b)| a | b << (7 - i))).collect();
            bytes.resize(bytes.len().div_ceil(512) * 512, 0);
            let count = bytes.len() / 512;
            table.extend((next_block as u16).to_le_bytes());
            table.extend((count as u16).to_le_bytes());
            table.extend((bits.len() as u32).to_le_bytes());
            next_block += count;
            blocks.extend(bytes);
        }
        table.resize(1280, 0);
        file.extend(b"TRKS");
        file.extend(((1280 + blocks.len()) as u32).to_le_bytes());
        file.extend(table);
        assert_eq!(file.len(), 1536);
        file.extend(blocks);
        file
    }

    #[test]
    fn the_tables_are_whole() {
        assert_eq!(inverse(&SIX_AND_TWO).iter().filter(|&&v| v != 0xFF).count(), 64);
        assert_eq!(inverse(&FIVE_AND_THREE).iter().filter(|&&v| v != 0xFF).count(), 32);
        let data: Vec<u8> = (0..=255).collect();
        assert_eq!(decode_62(&encode_62(&data), &inverse(&SIX_AND_TWO)), Some(data));
    }

    #[test]
    fn a_dos_33_disk_is_read_back_from_its_bits() {
        let image = super::super::apple2::tests::dos33_disk();
        let woz = woz_of(&image, &[]);
        assert!(looks_like(&woz));
        let decoded = decode(&woz).unwrap();
        assert!(decoded.sectors.found.iter().all(|f| *f));
        assert!(decoded.sectors.bytes == image, "every sector comes back where a DOS-order image has it");
        let volume = super::super::open(Arc::new(woz), "disk.woz").unwrap();
        assert!(volume.describe().contains("DOS 3.3"), "{}", volume.describe());
        let files = volume.list(None).unwrap();
        assert_eq!(volume.read(&files[0]).unwrap(), b"\xC8\xC9\x8D");
        assert!(volume.notes().iter().all(|n| !n.contains("couldn't be read")));
    }

    #[test]
    fn unreadable_sectors_are_said_and_never_made_up() {
        let image = super::super::apple2::tests::dos33_disk();
        // Track 19's data sector (logical 1, physical 13), and one nothing uses.
        let woz = woz_of(&image, &[(19, 13), (30, 4)]);
        let volume = super::super::open(Arc::new(woz), "disk.woz").unwrap();
        let notes = volume.notes().join(" ");
        assert!(notes.contains("track 19 sector 13") && notes.contains("track 30 sector 4"), "{notes}");
        let files = volume.list(None).unwrap();
        assert_eq!(volume.read(&files[0]).unwrap(), b"\xC8\xC9\x8D", "a file on good sectors still reads");
        let problem = volume.read(&files[1]).unwrap_err();
        assert!(problem.contains("Track 19 sector 13"), "{problem}");
    }

    #[test]
    fn a_disk_with_no_file_system_gives_its_sectors() {
        let mut image = vec![0u8; 35 * 16 * 256];
        image[..4].copy_from_slice(b"BOOT");
        let woz = woz_of(&image, &[]);
        let volume = super::super::open(Arc::new(woz), "game.woz").unwrap();
        assert!(volume.describe().contains("no file system"), "{}", volume.describe());
        let files = volume.list(None).unwrap();
        assert_eq!(files[0].name, "sectors.do");
        assert!(volume.read(&files[0]).unwrap() == image);
    }

    #[test]
    fn damaged_woz_files_give_errors_never_panics() {
        let image = super::super::apple2::tests::dos33_disk();
        let woz = woz_of(&image, &[]);
        for cut in [0, 8, 12, 20, 100, 300, 1536, 4000, woz.len() / 2] {
            let _ = super::super::open(Arc::new(woz[..cut].to_vec()), "x.woz");
        }
        let mut state = 7u32;
        for _ in 0..30 {
            let mut bytes = woz.clone();
            for _ in 0..40 {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let at = 12 + state as usize % 1600;
                bytes[at] = (state >> 16) as u8;
            }
            if let Ok(volume) = super::super::open(Arc::new(bytes), "x.woz") {
                for entry in volume.list(None).unwrap_or_default() {
                    let _ = volume.read(&entry);
                }
            }
        }
    }

    /// Real WOZ images from the WOZ-A-Day collection, in ..\testmedia\apple2-woz: what each holds, as a2kit reads
    /// the same disks. Skipped when the folder isn't there.
    #[test]
    fn real_woz_images_open_as_their_file_systems() {
        let root = std::path::Path::new("../testmedia/apple2-woz");
        if !root.is_dir() {
            return;
        }
        let open = |name: &str| {
            let bytes = std::fs::read(root.join(name)).unwrap();
            super::super::open(Arc::new(bytes), name).unwrap_or_else(|e| panic!("{name}: {e}"))
        };
        let spider = open("Apple_Cider_Spider__Apple Cider Spider.woz");
        assert!(spider.describe().starts_with("WOZ 1 image (5.25-inch, 16-sector, 35 tracks): Apple II DOS 3.3"), "{}", spider.describe());
        for entry in spider.list(None).unwrap() {
            spider.read(&entry).unwrap();
        }
        let prodos = open("Anchorman__Anchorman.woz");
        assert!(prodos.describe().contains("ProDOS"), "{}", prodos.describe());
        let dos32 = open("APMAIL_v12__APMAIL v1.2.woz");
        assert!(dos32.describe().contains("13-sector (DOS 3.2), 35 tracks): Apple II DOS 3.2"), "{}", dos32.describe());
        assert!(dos32.notes().iter().any(|n| n.starts_with("Sectors never written")), "{:?}", dos32.notes());
        let gs = open("Ancient_Glory_IIgs__Ancient Glory IIgs disk 2.woz");
        assert!(gs.describe().starts_with("WOZ 2 image (3.5-inch, 800K): Apple II ProDOS, /AG2"), "{}", gs.describe());
        for entry in gs.list(None).unwrap() {
            gs.read(&entry).unwrap();
        }
        let ankh = open("Ankh__Ankh.woz");
        assert!(ankh.describe().contains("no file system found"), "{}", ankh.describe());
        assert_eq!(ankh.read(&ankh.list(None).unwrap()[0]).unwrap().len(), 35 * 4096);
        let protected = open("Animal_Homes_and_Stories__Animal Homes and Stories.woz");
        assert!(protected.notes().iter().any(|n| n.starts_with("Sectors that couldn't be read")), "{:?}", protected.notes());
    }

    /// The WOZ images in the folder LIMONITER_WOZ_DIR names are each read, and their sectors written as DOS-order
    /// (.do) or ProDOS-order (.po) images to LIMONITER_WOZ_OUT with a list of those missing, to compare with another
    /// tool's; a line on each goes to standard error. Run by hand: `cargo test woz_sweep -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn woz_sweep() {
        let Some(dir) = std::env::var_os("LIMONITER_WOZ_DIR") else { return };
        let out = std::env::var_os("LIMONITER_WOZ_OUT").map(std::path::PathBuf::from);
        let mut paths: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "woz")).collect();
        paths.sort();
        for path in paths {
            let bytes = std::fs::read(&path).unwrap();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            match decode(&bytes) {
                Ok(decoded) => {
                    let read = decoded.sectors.found.iter().filter(|f| **f).count();
                    if let Some(out) = &out {
                        let ending = if decoded.sectors.unit == 256 { "do" } else { "po" };
                        std::fs::write(out.join(format!("{name}.{ending}")), &decoded.sectors.bytes).unwrap();
                        let missing: Vec<String> = decoded.sectors.found.iter().enumerate().filter(|(_, f)| !**f).map(|(i, _)| i.to_string()).collect();
                        std::fs::write(out.join(format!("{name}.missing")), missing.join("\n")).unwrap();
                    }
                    match super::super::open(Arc::new(bytes), &name) {
                        Ok(volume) => {
                            let files = volume.list(None).map(|f| f.len()).unwrap_or(0);
                            eprintln!("{name}\t{}\t{read}/{}\t{files} files", volume.describe(), decoded.sectors.on_disk);
                        }
                        Err(problem) => eprintln!("{name}\tOPEN FAILED {problem}"),
                    }
                }
                Err(problem) => eprintln!("{name}\tREFUSED {problem}"),
            }
        }
    }
}
