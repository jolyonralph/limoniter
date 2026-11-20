//! Tools for a file's bytes: checksums of many kinds, the checksums in a ROM's header put right, IPS, BPS and UPS
//! patches made and applied, and the file's entropy and byte counts. Nothing here draws anything.

use sha2::{Digest, Sha256};

// ---------- Checksums ----------

/// A CRC, as the catalogue of parametrised CRCs gives each: its width, polynomial, starting value, whether bytes go
/// in and the result comes out reflected, and what the result is XORed with.
#[derive(Clone, Copy, Debug)]
struct Crc {
    name: &'static str,
    width: u32,
    poly: u32,
    init: u32,
    reflect: bool,
    xor_out: u32,
}

const CRCS: [Crc; 10] = [
    Crc { name: "CRC-16/CCITT-FALSE", width: 16, poly: 0x1021, init: 0xFFFF, reflect: false, xor_out: 0 },
    Crc { name: "CRC-16/XMODEM", width: 16, poly: 0x1021, init: 0, reflect: false, xor_out: 0 },
    Crc { name: "CRC-16/KERMIT", width: 16, poly: 0x1021, init: 0, reflect: true, xor_out: 0 },
    Crc { name: "CRC-16/ARC (IBM)", width: 16, poly: 0x8005, init: 0, reflect: true, xor_out: 0 },
    Crc { name: "CRC-16/MODBUS", width: 16, poly: 0x8005, init: 0xFFFF, reflect: true, xor_out: 0 },
    Crc { name: "CRC-16/X-25", width: 16, poly: 0x1021, init: 0xFFFF, reflect: true, xor_out: 0xFFFF },
    Crc { name: "CRC-32 (zip, PNG)", width: 32, poly: 0x04C1_1DB7, init: 0xFFFF_FFFF, reflect: true, xor_out: 0xFFFF_FFFF },
    Crc { name: "CRC-32/BZIP2", width: 32, poly: 0x04C1_1DB7, init: 0xFFFF_FFFF, reflect: false, xor_out: 0xFFFF_FFFF },
    Crc { name: "CRC-32/MPEG-2", width: 32, poly: 0x04C1_1DB7, init: 0xFFFF_FFFF, reflect: false, xor_out: 0 },
    Crc { name: "CRC-32C (Castagnoli)", width: 32, poly: 0x1EDC_6F41, init: 0xFFFF_FFFF, reflect: true, xor_out: 0xFFFF_FFFF },
];

impl Crc {
    fn of(&self, bytes: &[u8]) -> u32 {
        let top = 1u64 << (self.width - 1);
        let mask = (1u64 << self.width) - 1;
        if self.reflect {
            let poly = (self.poly.reverse_bits() >> (32 - self.width)) as u64;
            let mut crc = self.init as u64;
            for byte in bytes {
                crc ^= u64::from(*byte);
                for _ in 0..8 {
                    crc = if crc & 1 != 0 { (crc >> 1) ^ poly } else { crc >> 1 };
                }
            }
            (crc ^ self.xor_out as u64) as u32
        } else {
            let mut crc = self.init as u64;
            for byte in bytes {
                crc ^= u64::from(*byte) << (self.width - 8);
                for _ in 0..8 {
                    crc = if crc & top != 0 { ((crc << 1) ^ self.poly as u64) & mask } else { (crc << 1) & mask };
                }
            }
            ((crc ^ self.xor_out as u64) & mask) as u32
        }
    }
}

/// The zip and PNG CRC-32, as BPS and UPS patches check with.
pub fn crc32(bytes: &[u8]) -> u32 {
    CRCS[6].of(bytes)
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in bytes.chunks(5552) {
        for byte in chunk {
            a += u32::from(*byte);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

fn fletcher16(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (0u32, 0u32);
    for byte in bytes {
        a = (a + u32::from(*byte)) % 255;
        b = (b + a) % 255;
    }
    (b << 8) | a
}

/// Fletcher-32 over 16-bit words, little-endian, an odd last byte padded with 0.
fn fletcher32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (0u64, 0u64);
    for pair in bytes.chunks(2) {
        let word = u64::from(pair[0]) | (u64::from(*pair.get(1).unwrap_or(&0)) << 8);
        a = (a + word) % 65535;
        b = (b + a) % 65535;
    }
    ((b << 16) | a) as u32
}

/// Every checksum of some bytes: its name, and its value as it's usually written.
pub fn checksums(bytes: &[u8]) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = CRCS.iter().map(|crc| (crc.name, if crc.width == 16 { format!("{:04X}", crc.of(bytes)) } else { format!("{:08X}", crc.of(bytes)) })).collect();
    out.push(("Adler-32", format!("{:08X}", adler32(bytes))));
    out.push(("Fletcher-16", format!("{:04X}", fletcher16(bytes))));
    out.push(("Fletcher-32", format!("{:08X}", fletcher32(bytes))));
    let sum: u64 = bytes.iter().map(|b| u64::from(*b)).sum();
    out.push(("Sum of the bytes, 8 bits", format!("{:02X}", sum & 0xFF)));
    out.push(("Sum of the bytes, 16 bits", format!("{:04X}", sum & 0xFFFF)));
    out.push(("Sum of the bytes, 32 bits", format!("{:08X}", sum & 0xFFFF_FFFF)));
    out.push(("SHA-256", Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()));
    out
}

// ---------- ROM headers ----------

/// A ROM whose header holds a checksum of the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rom {
    MegaDrive,
    GameBoy,
    Snes,
    Kickstart,
}

impl Rom {
    pub fn name(self) -> &'static str {
        match self {
            Rom::MegaDrive => "Mega Drive / Genesis",
            Rom::GameBoy => "Game Boy",
            Rom::Snes => "SNES",
            Rom::Kickstart => "Amiga Kickstart",
        }
    }
}

/// What kind of ROM some bytes are, by their header.
pub fn rom_kind(bytes: &[u8]) -> Option<Rom> {
    if bytes.len() >= 0x200 && (bytes[0x100..0x104] == *b"SEGA" || bytes[0x101..0x105] == *b"SEGA") {
        return Some(Rom::MegaDrive);
    }
    // The Nintendo logo's first bytes, which the Game Boy's boot ROM checks.
    if bytes.len() >= 0x150 && bytes[0x104..0x108] == [0xCE, 0xED, 0x66, 0x66] {
        return Some(Rom::GameBoy);
    }
    if snes_header(bytes).is_some() {
        return Some(Rom::Snes);
    }
    if (bytes.len() == 256 * 1024 || bytes.len() == 512 * 1024) && (bytes[..4] == [0x11, 0x11, 0x4E, 0xF9] || bytes[..4] == [0x11, 0x14, 0x4E, 0xF9]) {
        return Some(Rom::Kickstart);
    }
    None
}

/// Where a SNES ROM's header is (LoROM at $7FC0, HiROM at $FFC0), found by its checksum and complement adding up.
fn snes_header(bytes: &[u8]) -> Option<usize> {
    [0x7FC0usize, 0xFFC0].into_iter().find(|at| {
        bytes.len() >= at + 0x40 && {
            let word = |o: usize| u16::from_le_bytes([bytes[at + o], bytes[at + o + 1]]);
            word(0x1C) ^ word(0x1E) == 0xFFFF
        }
    })
}

/// The checksum the header has, and the one it should have, as written.
pub fn rom_checksum(bytes: &[u8], rom: Rom) -> Option<(String, String)> {
    match rom {
        Rom::MegaDrive => {
            // A sum of the big-endian words after the header.
            let has = u16::from_be_bytes([*bytes.get(0x18E)?, *bytes.get(0x18F)?]);
            let mut sum = 0u16;
            for pair in bytes.get(0x200..)?.chunks(2) {
                sum = sum.wrapping_add(u16::from_be_bytes([pair[0], *pair.get(1).unwrap_or(&0)]));
            }
            Some((format!("{has:04X}"), format!("{sum:04X}")))
        }
        Rom::GameBoy => {
            // The header's own check, from $134 to $14C, and the whole ROM's but those two bytes.
            let mut header = 0u8;
            for byte in bytes.get(0x134..=0x14C)? {
                header = header.wrapping_sub(*byte).wrapping_sub(1);
            }
            let mut global = 0u16;
            for (at, byte) in bytes.iter().enumerate() {
                if at != 0x14E && at != 0x14F {
                    global = global.wrapping_add(u16::from(*byte));
                }
            }
            let has = format!("{:02X} {:02X}{:02X}", bytes[0x14D], bytes[0x14E], bytes[0x14F]);
            Some((has, format!("{header:02X} {global:04X}")))
        }
        Rom::Snes => {
            let at = snes_header(bytes)?;
            let has = u16::from_le_bytes([bytes[at + 0x1E], bytes[at + 0x1F]]);
            Some((format!("{has:04X}"), format!("{:04X}", snes_sum(bytes, at))))
        }
        Rom::Kickstart => {
            let has = u32::from_be_bytes(bytes.get(bytes.len() - 24..bytes.len() - 20)?.try_into().ok()?);
            Some((format!("{has:08X}"), format!("{:08X}", kickstart_fixed(bytes))))
        }
    }
}

/// A SNES ROM's sum, the checksum and complement counted as $FF $FF $00 $00 so the answer doesn't depend on them.
fn snes_sum(bytes: &[u8], at: usize) -> u16 {
    let mut sum = 0u16;
    for (index, byte) in bytes.iter().enumerate() {
        let byte = match index.checked_sub(at + 0x1C) {
            Some(0 | 1) => 0xFF,
            Some(2 | 3) => 0,
            _ => *byte,
        };
        sum = sum.wrapping_add(u16::from(byte));
    }
    sum
}

/// The checksum a Kickstart needs so that its long words, added with the carry going round, come to $FFFFFFFF.
fn kickstart_fixed(bytes: &[u8]) -> u32 {
    let place = bytes.len() - 24;
    let mut sum = 0u32;
    for (at, chunk) in bytes.chunks(4).enumerate() {
        let word = if at * 4 == place { 0 } else { u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) };
        let (added, carried) = sum.overflowing_add(word);
        sum = added + u32::from(carried);
    }
    !sum
}

/// The ROM with its header's checksum put right.
pub fn fix_rom_checksum(bytes: &mut [u8], rom: Rom) -> bool {
    let Some((_, right)) = rom_checksum(bytes, rom) else { return false };
    let hex = |s: &str| u32::from_str_radix(s, 16).unwrap_or(0);
    match rom {
        Rom::MegaDrive => bytes[0x18E..0x190].copy_from_slice(&(hex(&right) as u16).to_be_bytes()),
        Rom::GameBoy => {
            let (header, _) = right.split_once(' ').unwrap_or((&right, ""));
            bytes[0x14D] = hex(header) as u8;
            // The global sum takes in the header check just written.
            let (_, global) = rom_checksum(bytes, rom).unwrap_or_default();
            let global = global.split_once(' ').map_or(0, |(_, g)| hex(g)) as u16;
            bytes[0x14E..0x150].copy_from_slice(&global.to_be_bytes());
        }
        Rom::Snes => {
            let Some(at) = snes_header(bytes) else { return false };
            let sum = hex(&right) as u16;
            bytes[at + 0x1C..at + 0x1E].copy_from_slice(&(!sum).to_le_bytes());
            bytes[at + 0x1E..at + 0x20].copy_from_slice(&sum.to_le_bytes());
        }
        Rom::Kickstart => {
            let place = bytes.len() - 24;
            bytes[place..place + 4].copy_from_slice(&hex(&right).to_be_bytes());
        }
    }
    true
}

// ---------- Patches ----------

/// A ROM patch's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchKind {
    Ips,
    Bps,
    Ups,
}

impl PatchKind {
    pub fn ending(self) -> &'static str {
        match self {
            PatchKind::Ips => "ips",
            PatchKind::Bps => "bps",
            PatchKind::Ups => "ups",
        }
    }

    pub fn of(patch: &[u8]) -> Option<PatchKind> {
        if patch.starts_with(b"PATCH") {
            Some(PatchKind::Ips)
        } else if patch.starts_with(b"BPS1") {
            Some(PatchKind::Bps)
        } else if patch.starts_with(b"UPS1") {
            Some(PatchKind::Ups)
        } else {
            None
        }
    }
}

/// The most an IPS patch can reach: its offsets are three bytes, and "EOF" ends it.
const IPS_MOST: usize = 0xFF_FFFF;

/// An IPS patch from one file to another: each run of changed bytes as a record (a long run of one byte as an RLE
/// record), and the new length at the end if the file got shorter.
pub fn make_ips(old: &[u8], new: &[u8]) -> Result<Vec<u8>, String> {
    if new.len() > IPS_MOST {
        return Err(crate::i18n::tr("IPS patches only reach the first 16 MB of a file; BPS or UPS can do larger ones.").to_string());
    }
    let mut out = b"PATCH".to_vec();
    let mut at = 0;
    while at < new.len() {
        if old.get(at) == Some(&new[at]) {
            at += 1;
            continue;
        }
        let start = at;
        // A run of changed bytes, kept short of 64K and of the offset that spells EOF.
        while at < new.len() && old.get(at) != Some(&new[at]) && at - start < 0xFFFF {
            at += 1;
        }
        let (mut start, end) = (start, at);
        if start == 0x45_4F46 {
            start -= 1;
        }
        let run = &new[start..end];
        out.extend_from_slice(&(start as u32).to_be_bytes()[1..]);
        if run.len() > 8 && run.iter().all(|b| *b == run[0]) {
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(&(run.len() as u16).to_be_bytes());
            out.push(run[0]);
        } else {
            out.extend_from_slice(&(run.len() as u16).to_be_bytes());
            out.extend_from_slice(run);
        }
    }
    out.extend_from_slice(b"EOF");
    if new.len() < old.len() {
        out.extend_from_slice(&(new.len() as u32).to_be_bytes()[1..]);
    }
    Ok(out)
}

fn apply_ips(old: &[u8], patch: &[u8]) -> Result<Vec<u8>, String> {
    let broken = || crate::i18n::tr("This IPS patch is cut short or damaged.").to_string();
    let mut out = old.to_vec();
    let mut at = 5;
    loop {
        let head = patch.get(at..at + 3).ok_or_else(broken)?;
        if head == b"EOF" {
            at += 3;
            // A length after EOF cuts the file to it.
            if let Some(length) = patch.get(at..at + 3) {
                out.truncate(u32::from_be_bytes([0, length[0], length[1], length[2]]) as usize);
            }
            return Ok(out);
        }
        let offset = u32::from_be_bytes([0, head[0], head[1], head[2]]) as usize;
        let size = u16::from_be_bytes(patch.get(at + 3..at + 5).ok_or_else(broken)?.try_into().map_err(|_| broken())?) as usize;
        at += 5;
        let bytes: Vec<u8> = if size == 0 {
            let run = u16::from_be_bytes(patch.get(at..at + 2).ok_or_else(broken)?.try_into().map_err(|_| broken())?) as usize;
            let value = *patch.get(at + 2).ok_or_else(broken)?;
            at += 3;
            vec![value; run]
        } else {
            let data = patch.get(at..at + size).ok_or_else(broken)?.to_vec();
            at += size;
            data
        };
        if out.len() < offset + bytes.len() {
            out.resize(offset + bytes.len(), 0);
        }
        out[offset..offset + bytes.len()].copy_from_slice(&bytes);
    }
}

/// beat's and UPS's numbers: seven bits a byte, the top bit ending it, each byte after the first worth one more.
fn write_number(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let low = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            out.push(0x80 | low);
            return;
        }
        out.push(low);
        value -= 1;
    }
}

fn read_number(patch: &[u8], at: &mut usize) -> Option<u64> {
    let (mut value, mut shift) = (0u64, 1u64);
    loop {
        let byte = *patch.get(*at)?;
        *at += 1;
        value = value.checked_add(u64::from(byte & 0x7F).checked_mul(shift)?)?;
        if byte & 0x80 != 0 {
            return Some(value);
        }
        shift = shift.checked_shl(7)?;
        value = value.checked_add(shift)?;
    }
}

fn add_checks(out: &mut Vec<u8>, old: &[u8], new: &[u8]) {
    out.extend_from_slice(&crc32(old).to_le_bytes());
    out.extend_from_slice(&crc32(new).to_le_bytes());
    let own = crc32(out);
    out.extend_from_slice(&own.to_le_bytes());
}

/// A UPS patch: the XOR of the two files, a run at a time, with both files' CRCs. It works either way round.
pub fn make_ups(old: &[u8], new: &[u8]) -> Vec<u8> {
    let mut out = b"UPS1".to_vec();
    write_number(&mut out, old.len() as u64);
    write_number(&mut out, new.len() as u64);
    let length = old.len().max(new.len());
    let byte = |file: &[u8], at: usize| file.get(at).copied().unwrap_or(0);
    let (mut at, mut last) = (0, 0);
    while at < length {
        if byte(old, at) == byte(new, at) {
            at += 1;
            continue;
        }
        write_number(&mut out, (at - last) as u64);
        while at < length && byte(old, at) != byte(new, at) {
            out.push(byte(old, at) ^ byte(new, at));
            at += 1;
        }
        out.push(0);
        at += 1;
        last = at;
    }
    add_checks(&mut out, old, new);
    out
}

fn apply_ups(old: &[u8], patch: &[u8]) -> Result<Vec<u8>, String> {
    let broken = || crate::i18n::tr("This UPS patch is cut short or damaged.").to_string();
    let mut at = 4;
    let from = read_number(patch, &mut at).ok_or_else(broken)? as usize;
    let to = read_number(patch, &mut at).ok_or_else(broken)? as usize;
    // Either way round: the file given can be either of the two.
    let (from, to) = if old.len() == from { (from, to) } else { (to, from) };
    let mut out = old.to_vec();
    out.resize(to.max(from), 0);
    let end = patch.len().checked_sub(12).ok_or_else(broken)?;
    let mut place = 0;
    while at < end {
        place += read_number(patch, &mut at).ok_or_else(broken)? as usize;
        while at < end {
            let xor = patch[at];
            at += 1;
            if xor == 0 {
                break;
            }
            if place < out.len() {
                out[place] ^= xor;
            }
            place += 1;
        }
        place += 1;
    }
    out.truncate(to);
    Ok(out)
}

/// A BPS patch: here kept simple, the changed bytes read from the patch and the rest copied from the source where
/// it lies, which every BPS tool applies.
pub fn make_bps(old: &[u8], new: &[u8]) -> Vec<u8> {
    let mut out = b"BPS1".to_vec();
    write_number(&mut out, old.len() as u64);
    write_number(&mut out, new.len() as u64);
    write_number(&mut out, 0);
    let same = |at: usize| old.get(at) == Some(&new[at]);
    let mut at = 0;
    while at < new.len() {
        let start = at;
        let copying = same(at);
        while at < new.len() && same(at) == copying {
            at += 1;
        }
        let length = (at - start) as u64;
        // An action is two bits under the length less one: 0 copies from the source where it lies, 1 reads from
        // the patch.
        write_number(&mut out, ((length - 1) << 2) | if copying { 0 } else { 1 });
        if !copying {
            out.extend_from_slice(&new[start..at]);
        }
    }
    add_checks(&mut out, old, new);
    out
}

fn apply_bps(old: &[u8], patch: &[u8]) -> Result<Vec<u8>, String> {
    let broken = || crate::i18n::tr("This BPS patch is cut short or damaged.").to_string();
    let mut at = 4;
    let _from = read_number(patch, &mut at).ok_or_else(broken)?;
    let to = read_number(patch, &mut at).ok_or_else(broken)? as usize;
    let notes = read_number(patch, &mut at).ok_or_else(broken)? as usize;
    at += notes;
    let end = patch.len().checked_sub(12).ok_or_else(broken)?;
    let mut out: Vec<u8> = Vec::with_capacity(to);
    let (mut source_at, mut target_at) = (0i64, 0i64);
    let signed = |patch: &[u8], at: &mut usize| -> Option<i64> {
        let n = read_number(patch, at)?;
        let magnitude = (n >> 1) as i64;
        Some(if n & 1 != 0 { -magnitude } else { magnitude })
    };
    while at < end {
        let action = read_number(patch, &mut at).ok_or_else(broken)?;
        let length = ((action >> 2) + 1) as usize;
        match action & 3 {
            0 => {
                let from = out.len();
                out.extend((from..from + length).map(|i| old.get(i).copied().unwrap_or(0)));
            }
            1 => {
                out.extend_from_slice(patch.get(at..at + length).ok_or_else(broken)?);
                at += length;
            }
            2 => {
                source_at += signed(patch, &mut at).ok_or_else(broken)?;
                for _ in 0..length {
                    out.push(*old.get(source_at as usize).ok_or_else(broken)?);
                    source_at += 1;
                }
            }
            _ => {
                target_at += signed(patch, &mut at).ok_or_else(broken)?;
                for _ in 0..length {
                    let byte = *out.get(target_at as usize).ok_or_else(broken)?;
                    out.push(byte);
                    target_at += 1;
                }
            }
        }
    }
    Ok(out)
}

/// A patch made from two files, of the kind asked for.
pub fn make_patch(kind: PatchKind, old: &[u8], new: &[u8]) -> Result<Vec<u8>, String> {
    match kind {
        PatchKind::Ips => make_ips(old, new),
        PatchKind::Bps => Ok(make_bps(old, new)),
        PatchKind::Ups => Ok(make_ups(old, new)),
    }
}

/// A patch applied, and what's known of whether it came out right: BPS and UPS carry the CRCs of the file they're
/// for and the file they make, and their own; IPS carries nothing to check.
pub struct Applied {
    pub bytes: Vec<u8>,
    pub kind: PatchKind,
    /// Notes on the checks, each a problem if not all's well.
    pub problems: Vec<String>,
}

pub fn apply_patch(old: &[u8], patch: &[u8]) -> Result<Applied, String> {
    let kind = PatchKind::of(patch).ok_or_else(|| crate::i18n::tr("That isn't an IPS, BPS or UPS patch.").to_string())?;
    let bytes = match kind {
        PatchKind::Ips => apply_ips(old, patch)?,
        PatchKind::Bps => apply_bps(old, patch)?,
        PatchKind::Ups => apply_ups(old, patch)?,
    };
    let mut problems = Vec::new();
    if kind != PatchKind::Ips && patch.len() >= 12 {
        let word = |at: usize| u32::from_le_bytes(patch[at..at + 4].try_into().unwrap_or_default());
        let end = patch.len();
        let (for_source, for_target, own) = (word(end - 12), word(end - 8), word(end - 4));
        if crc32(&patch[..end - 4]) != own {
            problems.push(crate::i18n::tr("The patch itself is damaged: its own check doesn't match.").to_string());
        }
        // A UPS patch can go either way, so either file's CRC will do for the one given.
        let given = crc32(old);
        let made = crc32(&bytes);
        let right_file = given == for_source || (kind == PatchKind::Ups && given == for_target);
        if !right_file {
            problems.push(crate::i18n::tr("The file isn't the one this patch was made for (its CRC-32 is different).").to_string());
        }
        let right_result = made == for_target || (kind == PatchKind::Ups && made == for_source);
        if !right_result {
            problems.push(crate::i18n::tr("The patched file isn't what the patch says it makes (its CRC-32 is different).").to_string());
        }
    }
    Ok(Applied { bytes, kind, problems })
}

// ---------- Entropy ----------

/// How many of each byte value there are.
pub fn histogram(bytes: &[u8]) -> [u64; 256] {
    let mut counts = [0u64; 256];
    for byte in bytes {
        counts[*byte as usize] += 1;
    }
    counts
}

/// Shannon entropy in bits a byte (0 for one value repeated, 8 for bytes that look random: packed or encrypted).
pub fn entropy(bytes: &[u8]) -> f64 {
    if bytes.is_empty() {
        return 0.0;
    }
    let total = bytes.len() as f64;
    histogram(bytes).iter().filter(|c| **c > 0).map(|c| {
        let p = *c as f64 / total;
        -p * p.log2()
    }).sum()
}

/// The entropy of each block along a file, for a graph of where the packed or encrypted parts are. The block size
/// is chosen so there are about `points` of them.
pub fn entropy_along(bytes: &[u8], points: usize) -> (usize, Vec<f64>) {
    let block = (bytes.len() / points.max(1)).max(256);
    (block, bytes.chunks(block).map(entropy).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_give_the_catalogues_check_values() {
        // Every CRC catalogue gives each CRC's value for the nine digits.
        let sums = checksums(b"123456789");
        let value = |name: &str| sums.iter().find(|(n, _)| *n == name).unwrap().1.clone();
        assert_eq!(value("CRC-16/CCITT-FALSE"), "29B1");
        assert_eq!(value("CRC-16/XMODEM"), "31C3");
        assert_eq!(value("CRC-16/KERMIT"), "2189");
        assert_eq!(value("CRC-16/ARC (IBM)"), "BB3D");
        assert_eq!(value("CRC-16/MODBUS"), "4B37");
        assert_eq!(value("CRC-16/X-25"), "906E");
        assert_eq!(value("CRC-32 (zip, PNG)"), "CBF43926");
        assert_eq!(value("CRC-32/BZIP2"), "FC891918");
        assert_eq!(value("CRC-32/MPEG-2"), "0376E6E7");
        assert_eq!(value("CRC-32C (Castagnoli)"), "E3069283");
        assert_eq!(value("Adler-32"), "091E01DE");
        assert_eq!(value("Fletcher-16"), "1EDE");
        assert_eq!(value("SHA-256"), "15e2b0d3c33891ebb0f1ef609ec419420c20e320ce94c65fbc8c3312448eb225");
        assert_eq!(fletcher32(b"abcde"), 0xF04F_C729);
    }

    #[test]
    fn rom_headers_are_put_right() {
        // A Mega Drive ROM: SEGA at $100, and words after $200.
        let mut md = vec![0u8; 0x400];
        md[0x100..0x104].copy_from_slice(b"SEGA");
        md[0x200] = 0x12;
        md[0x203] = 0x34;
        assert_eq!(rom_kind(&md), Some(Rom::MegaDrive));
        assert_eq!(rom_checksum(&md, Rom::MegaDrive), Some(("0000".into(), "1234".into())));
        assert!(fix_rom_checksum(&mut md, Rom::MegaDrive));
        assert_eq!(&md[0x18E..0x190], &[0x12, 0x34]);
        // A Game Boy ROM: both checks, the global one taking in the header's.
        let mut gb = vec![0u8; 0x8000];
        gb[0x104..0x108].copy_from_slice(&[0xCE, 0xED, 0x66, 0x66]);
        gb[0x134..0x138].copy_from_slice(b"TEST");
        assert_eq!(rom_kind(&gb), Some(Rom::GameBoy));
        assert!(fix_rom_checksum(&mut gb, Rom::GameBoy));
        let (has, right) = rom_checksum(&gb, Rom::GameBoy).unwrap();
        assert_eq!(has, right);
        // A Kickstart: its long words come to $FFFFFFFF once fixed.
        let mut kick = vec![0u8; 256 * 1024];
        kick[..4].copy_from_slice(&[0x11, 0x11, 0x4E, 0xF9]);
        kick[100] = 0x55;
        assert_eq!(rom_kind(&kick), Some(Rom::Kickstart));
        assert!(fix_rom_checksum(&mut kick, Rom::Kickstart));
        let mut sum = 0u32;
        for chunk in kick.chunks(4) {
            let (added, carried) = sum.overflowing_add(u32::from_be_bytes(chunk.try_into().unwrap()));
            sum = added + u32::from(carried);
        }
        assert_eq!(sum, 0xFFFF_FFFF);
        // A SNES ROM, LoROM.
        let mut snes = vec![0u8; 0x8000];
        snes[0x7FDC..0x7FE0].copy_from_slice(&[0xFF, 0xFF, 0x00, 0x00]);
        snes[10] = 7;
        assert_eq!(rom_kind(&snes), Some(Rom::Snes));
        assert!(fix_rom_checksum(&mut snes, Rom::Snes));
        let (has, right) = rom_checksum(&snes, Rom::Snes).unwrap();
        assert_eq!(has, right);
    }

    #[test]
    fn patches_go_from_one_file_to_the_other_and_are_checked() {
        let old: Vec<u8> = (0..5000u32).map(|i| (i * 7 % 251) as u8).collect();
        let mut new = old.clone();
        new[10] = 0xAA;
        new[11] = 0xBB;
        new[3000..3100].fill(0x42);
        new.extend_from_slice(b"more at the end");
        for kind in [PatchKind::Ips, PatchKind::Bps, PatchKind::Ups] {
            let patch = make_patch(kind, &old, &new).unwrap();
            let applied = apply_patch(&old, &patch).unwrap();
            assert_eq!(applied.kind, kind);
            assert!(applied.bytes == new, "{kind:?}");
            assert!(applied.problems.is_empty(), "{kind:?} {:?}", applied.problems);
            // The wrong file is caught by BPS and UPS.
            if kind != PatchKind::Ips {
                let mut wrong = old.clone();
                wrong[0] ^= 1;
                assert!(!apply_patch(&wrong, &patch).unwrap().problems.is_empty(), "{kind:?}");
            }
        }
        // UPS goes back the other way too, and IPS can make a file shorter.
        let ups = make_ups(&old, &new);
        assert!(apply_patch(&new, &ups).unwrap().bytes == old);
        let shorter = make_ips(&new, &old).unwrap();
        assert!(apply_patch(&new, &shorter).unwrap().bytes == old);
    }

    #[test]
    fn entropy_tells_plain_from_packed() {
        assert_eq!(entropy(&[0; 1000]), 0.0);
        let all: Vec<u8> = (0..=255).collect();
        assert!((entropy(&all) - 8.0).abs() < 1e-9);
        let (block, along) = entropy_along(&[0u8; 4096], 8);
        assert_eq!((block, along.len()), (512, 8));
    }
}
