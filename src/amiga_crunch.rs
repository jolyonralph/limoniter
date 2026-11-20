//! Amiga crunched files, decrunched: PowerPacker, Imploder, StoneCracker, CrunchMania and ByteKiller. Demo coders
//! and cracking groups crunched nearly everything, data and programs alike, so a crunched file is shown for what it
//! holds. Crunched data is known by its header (PP20, IMP!, CrM!, S404 and the rest) and decoded; a crunched program,
//! an AmigaDOS executable whose first hunk is its cruncher's decruncher, is known by that decruncher's code, its
//! crunched data found in the file and decoded, and the original program built again with its hunks, their memory
//! and their relocations, so that it runs as it did before it was crunched.
//!
//! The decoders are ported from Teemu Suutari's ancient library (github.com/temisu/ancient, BSD 2-clause licence;
//! its notice is in data/ancient-license.txt), and each was checked byte for byte against files crunched by the
//! real tools. Nothing here crunches.

mod bits;
mod formats;
mod programs;
#[cfg(test)]
mod tests;

pub use formats::Stone;

/// The most a crunched file may decrunch to here: more than any Amiga of the time had.
pub const MOST: usize = 64 << 20;

/// The crunchers recognised.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cruncher {
    PowerPacker,
    Imploder,
    CrunchMania,
    StoneCracker,
    ByteKiller,
}

/// What a crunched file was found to be, before it's decrunched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub cruncher: Cruncher,
    /// The cruncher with its version or mode, as far as the file tells: "PowerPacker 4.0", "CrunchMania (LZH mode)".
    pub name: String,
    /// Whether it's a crunched program (an executable that decrunches itself), rather than crunched data.
    pub program: bool,
    /// How long it says it is decrunched, where it says: for a program, its crunched contents.
    pub size: Option<usize>,
    /// For a program crunched to be put at a fixed address (as ByteKiller's were), where it's put and where it's
    /// started: it decrunches to that memory, not to an AmigaDOS executable.
    pub load: Option<(u32, u32)>,
}

/// A crunched file, decrunched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decrunched {
    pub found: Found,
    /// The bytes as they were before crunching: the data, or for a program, the AmigaDOS executable built again.
    pub bytes: Vec<u8>,
}

impl Decrunched {
    /// "This program was crunched with PowerPacker 4.0", or of data, "This file was crunched with Imploder".
    pub fn said(&self) -> String {
        said(&self.found)
    }
}

/// "This program was crunched with …" or "This file was crunched with …".
pub fn said(found: &Found) -> String {
    let name = &found.name;
    if found.program { crate::i18n::tf!("This program was crunched with {name}") } else { crate::i18n::tf!("This file was crunched with {name}") }
}

/// Which crunched data the bytes start with, if any, from the header alone (and the checks it carries).
fn data_found(bytes: &[u8]) -> Option<Found> {
    let id = bytes.get(..4)?;
    let long = |at: usize| bits::be32(bytes, at).ok();
    let found = |cruncher: Cruncher, name: String, size: Option<u32>| Some(Found { cruncher, name, program: false, size: size.map(|s| s as usize), load: None });
    match id {
        b"PP20" if bytes.len() >= 16 && formats::powerpacker_modes(long(4)?).is_some() => found(Cruncher::PowerPacker, "PowerPacker".into(), Some(long(bytes.len() - 4)? >> 8)),
        b"PX20" if bytes.len() >= 18 && formats::powerpacker_modes(long(6)?).is_some() => found(Cruncher::PowerPacker, crate::i18n::tr("PowerPacker, encrypted").into(), Some(long(bytes.len() - 4)? >> 8)),
        b"IMP!" if bytes.len() >= 0x32 => {
            let end = long(8)? as usize;
            (end >= 12 && end & 1 == 0 && end.checked_add(0x32)? <= bytes.len() && formats::imploder_checksum(bytes, end)).then_some(())?;
            found(Cruncher::Imploder, "Imploder".into(), long(4))
        }
        b"CrM!" | b"CrM2" | b"Crm!" | b"Crm2" if bytes.len() >= 20 => {
            let packed = long(10)? as usize;
            (packed.checked_add(14)? <= bytes.len()).then_some(())?;
            found(Cruncher::CrunchMania, crunchmania_name(id).into(), long(6))
        }
        b"S300" | b"S310" | b"S400" | b"S401" | b"S403" | b"S404" => {
            let version = match &id[1..] {
                b"300" => Stone::V300,
                b"310" => Stone::V310,
                b"400" => Stone::V400,
                b"401" => Stone::V401,
                b"403" => Stone::V403,
                _ => Stone::V404,
            };
            let header = formats::stone_header(bytes, version).ok()?;
            found(Cruncher::StoneCracker, version.name().into(), Some(header.size as u32))
        }
        // StoneCracker 2.92 and 2.99 had no mark, but the widths of distance they start with tell them.
        [8, 9, 10, 8..=14] if id[3] != 9 && formats::stone_header(bytes, Stone::V292).is_ok() && stone_whole(bytes, Stone::V292) => found(Cruncher::StoneCracker, Stone::V292.name().into(), long(4)),
        _ => bytekiller_data(bytes).map(|(name, size)| Found { cruncher: Cruncher::ByteKiller, name: name.into(), program: false, size: Some(size), load: None }),
    }
}

/// Whether a StoneCracker stream without a mark of its own fills the file exactly and decodes: the only way to be
/// sure of it.
fn stone_whole(bytes: &[u8], version: Stone) -> bool {
    formats::stone_header(bytes, version).is_ok_and(|h| bytes.len() - h.end < 4 && formats::stonecracker(bytes, &h).is_ok())
}

fn crunchmania_name(id: &[u8]) -> &'static str {
    match id {
        b"CrM!" => "CrunchMania",
        b"CrM2" => "CrunchMania (LZH mode)",
        b"Crm!" => "CrunchMania (sampled)",
        _ => "CrunchMania (LZH mode, sampled)",
    }
}

/// ByteKiller data, which has no mark: its crunched length, its decrunched length and the XOR of its longs, then the
/// longs, filling the file exactly (ByteKiller Pro's with "data" after). Known only when all of that holds and it
/// decodes using every bit.
fn bytekiller_data(bytes: &[u8]) -> Option<(&'static str, usize)> {
    let packed = bits::be32(bytes, 0).ok()? as usize;
    let size = bits::be32(bytes, 4).ok()? as usize;
    if packed < 4 || !packed.is_multiple_of(4) || size == 0 || size > MOST {
        return None;
    }
    let name = if bytes.len() == packed + 12 {
        "ByteKiller"
    } else if bytes.len() == packed + 16 && bytes.ends_with(b"data") {
        "ByteKiller Pro"
    } else {
        return None;
    };
    formats::bytekiller(bytes, 12, 12 + packed, size, bits::be32(bytes, 8).ok()?).ok()?;
    Some((name, size))
}

/// What a file was crunched with, if it's crunched data or a crunched program Limoniter knows, without
/// decrunching it (though for data without a mark of its own, knowing means decoding it).
pub fn recognise(bytes: &[u8]) -> Option<Found> {
    if bits::be32(bytes, 0).ok()? == crate::asm::hunk::HUNK_HEADER {
        return programs::recognise(bytes);
    }
    data_found(bytes)
}

/// Decrunches a crunched file: its data, or its program built again. `None` if it isn't crunched with anything
/// known; an error, said for the person, if it is but can't be decrunched.
pub fn decrunch(bytes: &[u8]) -> Option<Result<Decrunched, String>> {
    let found = recognise(bytes)?;
    let result = if found.program { programs::decrunch(bytes, &found) } else { decrunch_data(bytes, &found) };
    Some(result.map(|bytes| Decrunched { found: found.clone(), bytes }))
}

fn decrunch_data(bytes: &[u8], found: &Found) -> Result<Vec<u8>, String> {
    let name = &found.name;
    let damaged = || crate::i18n::tf!("This file was crunched with {name}, but it doesn't decrunch: it may be damaged or cut short.");
    let long = |at: usize| bits::be32(bytes, at).map_err(|_| damaged());
    let decoded = match &bytes[..4] {
        b"PP20" => formats::powerpacker(bytes, 8, bytes.len() - 4, formats::powerpacker_modes(long(4)?).ok_or_else(damaged)?, 0),
        b"PX20" => return Err(crate::i18n::tr("This file was crunched with PowerPacker and encrypted with a password, which Limoniter doesn't decrypt.").into()),
        b"IMP!" => formats::imploder(bytes, long(8)? as usize, long(4)? as usize),
        b"CrM!" | b"CrM2" | b"Crm!" | b"Crm2" => formats::crunchmania(bytes, long(10)? as usize, long(6)? as usize, bytes[3] == b'2', bytes[2] == b'm'),
        _ if found.cruncher == Cruncher::StoneCracker => {
            let version = [Stone::V300, Stone::V310, Stone::V400, Stone::V401, Stone::V403, Stone::V404, Stone::V292].into_iter().find(|v| v.name() == name.as_str()).ok_or_else(damaged)?;
            formats::stone_header(bytes, version).and_then(|h| formats::stonecracker(bytes, &h))
        }
        _ => {
            let packed = long(0)? as usize;
            formats::bytekiller(bytes, 12, 12 + packed, long(4)? as usize, long(8)?)
        }
    };
    decoded.map_err(|_| damaged())
}

/// The bytes a file holds: decrunched if it's crunched data that decrunches cleanly, else as they are. A crunched
/// program is left as it is, since it runs as it is.
pub fn contents(bytes: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    match decrunch(bytes) {
        Some(Ok(d)) if !d.found.program => std::borrow::Cow::Owned(d.bytes),
        _ => std::borrow::Cow::Borrowed(bytes),
    }
}

/// Whether a file starting with these bytes, and this long, might be crunched, so it's worth reading whole to see:
/// an AmigaDOS executable, a cruncher's mark, or the lengths ByteKiller's data starts with.
pub fn worth_reading(head: &[u8], len: u64) -> bool {
    let Some(id) = head.get(..4) else { return false };
    let long = u64::from(u32::from_be_bytes([id[0], id[1], id[2], id[3]]));
    long == u64::from(crate::asm::hunk::HUNK_HEADER)
        || matches!(id, b"PP20" | b"PX20" | b"IMP!" | b"CrM!" | b"CrM2" | b"Crm!" | b"Crm2" | b"S300" | b"S310" | b"S400" | b"S401" | b"S403" | b"S404" | [8, 9, 10, 8..=14])
        || (long > 0 && long.is_multiple_of(4) && (long + 12 == len || long + 16 == len))
}

/// What File properties says of a crunched file: what crunched it, and what it decrunches to.
pub fn sections(bytes: &[u8]) -> Vec<crate::media::InfoSection> {
    let Some(found) = recognise(bytes) else { return Vec::new() };
    let mut rows = vec![(crate::i18n::tr("Crunched with").to_string(), found.name.clone())];
    let kind = if found.program { crate::i18n::tr("A program that decrunches itself as it starts") } else { crate::i18n::tr("Data") };
    rows.push((crate::i18n::tr("What it is").to_string(), kind.to_string()));
    match decrunch(bytes) {
        Some(Ok(d)) => {
            rows.push((crate::i18n::tr("Decrunched size").to_string(), crate::i18n::tf!("{} bytes", d.bytes.len())));
            if found.program
                && let Ok(hunks) = crate::asm::hunk::read_executable(&d.bytes)
            {
                let parts: Vec<String> = hunks
                    .iter()
                    .map(|h| {
                        let kind = match h.kind {
                            crate::asm::hunk::Kind::Code => crate::i18n::tr("code"),
                            crate::asm::hunk::Kind::Data => crate::i18n::tr("data"),
                            crate::asm::hunk::Kind::Bss => crate::i18n::tr("BSS"),
                        };
                        let chip = if h.memory == crate::asm::hunk::Memory::Chip { crate::i18n::tr(" (chip)") } else { "" };
                        crate::i18n::tf!("{kind}{chip} {} bytes", h.bytes.len())
                    })
                    .collect();
                rows.push((crate::i18n::tr("Its hunks").to_string(), parts.join(", ")));
                let relocations: usize = hunks.iter().map(|h| h.relocations.len()).sum();
                rows.push((crate::i18n::tr("Relocations").to_string(), relocations.to_string()));
            }
            rows.push((String::new(), crate::i18n::tr("Disassemble shows it decrunched; Tools, Save the decrunched program\u{2026} writes it out.").to_string()));
        }
        Some(Err(why)) => rows.push((String::new(), why)),
        None => {}
    }
    vec![crate::media::InfoSection { title: crate::i18n::tr("Crunched").to_string(), rows }]
}
