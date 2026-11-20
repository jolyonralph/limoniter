//! Crunched programs: AmigaDOS executables whose first hunk is a cruncher's decruncher. Each is known by its
//! decruncher's code (the instructions that do the work, with the numbers that differ from file to file, such as
//! lengths, read from the places the code takes them from), its crunched data found where the decruncher looks for
//! it and decoded, and the program built again: its hunks with their sizes, kinds and memory, and its relocations.
//! The layouts were worked out from the decrunchers themselves, and each checked against what xfdmaster (and, for
//! Imploder, its own Deplode) makes of real crunched files, and against programs crunched here with the real tools.

use super::bits::{Bad, R, be16, be32};
use super::formats::{self, ImploderEnd, ImploderTables, Stone};
use super::{Cruncher, Found};
use crate::asm::hunk::{HUNK_BSS, HUNK_CODE, HUNK_DATA, HUNK_END, HUNK_HEADER, HUNK_RELOC32, Kind};

/// A hunk as it's loaded from a file: its kind, its size and memory bits as the header gives them (the size in
/// longs, the memory in the top two bits), and its bytes (for BSS, none).
struct RawHunk {
    kind: Kind,
    header: u32,
    bytes: Vec<u8>,
}

/// The hunks of an executable, as the header and the hunks themselves give them, without their relocations.
fn raw_hunks(file: &[u8]) -> R<Vec<RawHunk>> {
    let mut reader = Longs { bytes: file, at: 4 };
    let mut next = || reader.next();
    if be32(file, 0)? != HUNK_HEADER {
        return Err(Bad);
    }
    loop {
        let n = next()?;
        if n == 0 {
            break;
        }
        for _ in 0..n {
            next()?;
        }
    }
    let (count, first, last) = (next()?, next()?, next()?);
    if count == 0 || count > 0xFFFF || last < first || last - first + 1 != count {
        return Err(Bad);
    }
    let mut headers = Vec::new();
    for _ in 0..count {
        let size = next()?;
        if size >> 30 == 3 {
            next()?;
        }
        headers.push(size);
    }
    let mut hunks = Vec::new();
    while hunks.len() < headers.len() {
        let id = next()? & 0x3FFF_FFFF;
        match id {
            HUNK_CODE | HUNK_DATA => {
                let length = next()? as usize;
                let mut bytes = Vec::with_capacity(length * 4);
                for _ in 0..length {
                    bytes.extend(next()?.to_be_bytes());
                }
                hunks.push(RawHunk { kind: if id == HUNK_CODE { Kind::Code } else { Kind::Data }, header: headers[hunks.len()], bytes });
            }
            HUNK_BSS => {
                next()?;
                hunks.push(RawHunk { kind: Kind::Bss, header: headers[hunks.len()], bytes: Vec::new() });
            }
            HUNK_RELOC32 => loop {
                let n = next()?;
                if n == 0 {
                    break;
                }
                for _ in 0..=n {
                    next()?;
                }
            },
            HUNK_END => {}
            // Symbols, debug information and names are passed over: decrunchers don't use them.
            0x3F0 => loop {
                let n = next()? & 0x00FF_FFFF;
                if n == 0 {
                    break;
                }
                for _ in 0..=n {
                    next()?;
                }
            },
            0x3F1 | 0x3E8 => {
                let n = next()?;
                for _ in 0..n {
                    next()?;
                }
            }
            _ => return Err(Bad),
        }
    }
    Ok(hunks)
}

/// Longs read one after another.
struct Longs<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Longs<'_> {
    fn next(&mut self) -> R<u32> {
        let value = be32(self.bytes, self.at)?;
        self.at += 4;
        Ok(value)
    }
}

/// A hunk of the program being built again.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Hunk {
    kind: Kind,
    /// The size in longs and the memory bits, as the header gives them.
    header: u32,
    /// The bytes it's loaded with (a whole number of longs; fewer than its size if it ends in zeros).
    bytes: Vec<u8>,
    /// Its relocations: for each hunk addressed, the places in this one holding addresses in it.
    relocations: Vec<(u32, Vec<u32>)>,
    /// The memory bits on its type long, kept where the cruncher kept them.
    flags: u32,
    /// Whether it had a table of relocations even with none in it, kept where the cruncher kept it.
    reloc_block: bool,
    /// HUNK_ENDs more than the one it needs, kept where the cruncher kept them.
    extra_ends: usize,
}

/// An AmigaDOS executable with these hunks: the header, then each hunk's contents, relocations and end.
fn executable(hunks: &[Hunk]) -> Vec<u8> {
    let mut file = Vec::new();
    let long = |file: &mut Vec<u8>, value: u32| file.extend(value.to_be_bytes());
    for value in [HUNK_HEADER, 0, hunks.len() as u32, 0, hunks.len() as u32 - 1] {
        long(&mut file, value);
    }
    for hunk in hunks {
        long(&mut file, hunk.header);
    }
    for hunk in hunks {
        match hunk.kind {
            Kind::Bss => {
                long(&mut file, HUNK_BSS | hunk.flags);
                long(&mut file, hunk.header & 0x3FFF_FFFF);
            }
            kind => {
                long(&mut file, (if kind == Kind::Code { HUNK_CODE } else { HUNK_DATA }) | hunk.flags);
                long(&mut file, hunk.bytes.len().div_ceil(4) as u32);
                file.extend(&hunk.bytes);
                file.resize(file.len().next_multiple_of(4), 0);
                if hunk.reloc_block || hunk.relocations.iter().any(|(_, places)| !places.is_empty()) {
                    long(&mut file, HUNK_RELOC32);
                    for (target, places) in hunk.relocations.iter().filter(|(_, p)| !p.is_empty()) {
                        long(&mut file, places.len() as u32);
                        long(&mut file, *target);
                        for place in places {
                            long(&mut file, *place);
                        }
                    }
                    long(&mut file, 0);
                }
            }
        }
        for _ in 0..=hunk.extra_ends {
            long(&mut file, HUNK_END);
        }
    }
    file
}

/// Whether `code` holds `pattern` at `at`: bytes in hex, `??` for any byte.
fn has(code: &[u8], at: usize, pattern: &str) -> bool {
    let wanted: Vec<Option<u8>> = pattern.split_whitespace().flat_map(|group| group.as_bytes().chunks(2).map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("??"), 16).ok()).collect::<Vec<_>>()).collect();
    code.get(at..at + wanted.len()).is_some_and(|got| got.iter().zip(&wanted).all(|(g, w)| w.is_none_or(|w| w == *g)))
}

/// Where `pattern` is first found in `code`, from `from` on.
fn find(code: &[u8], from: usize, pattern: &str) -> Option<usize> {
    (from..code.len()).find(|&at| has(code, at, pattern))
}

/// Where a PC-relative word at `at` points: the word is added to the address it's at.
fn relative(code: &[u8], at: usize) -> Option<usize> {
    let offset = be16(code, at).ok()? as u16 as i16;
    at.checked_add_signed(offset as isize)
}

/// The kinds of crunched program known, each a decruncher (and version) that lays its program out its own way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layout {
    /// PowerPacker 2.3: its data hunk holds a stream that decrunches to the whole original file. The decruncher
    /// says how long the stream is, its efficiency's widths of distance, and whether its literal bit is the usual way
    /// round (PowerPacker's own program has it the other way, to keep it from being decrunched as data).
    PowerPacker23,
    /// PowerPacker 3.0: its data hunk holds a stream that decrunches to the whole original file.
    PowerPacker30,
    /// PowerPacker 4.0: its data hunk holds a stream that decrunches to the original file with its relocations
    /// packed smaller.
    PowerPacker40,
    /// StoneCracker 4.10, with its decruncher built in or in stc.library: the decruncher's hunk ends with S404 data
    /// that decrunches to the program in StoneCracker's own shorter form of hunks.
    StoneCracker410 { library: bool },
    /// Imploder, normal mode: the program's hunks are left for LoadSeg to make (as BSS), after a stub that calls the
    /// decruncher, whose hunk, the crunched data and a buffer follow them.
    ImploderNormal,
    /// Imploder's library mode: explode.library decrunches it, as it's loaded.
    ImploderLibrary,
    /// CrunchMania's normal decruncher: the program's hunks are left empty, after a stub that calls the decruncher,
    /// which, with the crunched data, is the last hunk.
    CrunchManiaNormal,
    /// CrunchMania's simple decruncher: the decruncher and the crunched data are the first hunk, the program's
    /// hunks (empty) the rest.
    CrunchManiaSimple,
    /// ByteKiller (1.3, 2.0, and the many crunchers built on its decruncher): a program crunched to be put at a fixed
    /// address and started there, its data after the decruncher in a single hunk.
    ByteKiller,
    /// CrunchMania's library decruncher: the program's first hunk starts with a root that calls CrM.library, with
    /// what it needs to know after it; the crunched data is the last hunk.
    CrunchManiaLibrary,
}

/// A crunched program recognised: what crunched it, and where its parts are.
struct Recognised {
    kind: Layout,
    name: String,
}

fn recognised(file: &[u8]) -> Option<(Recognised, Vec<RawHunk>)> {
    let hunks = raw_hunks(file).ok()?;
    let first = hunks.first().filter(|h| h.kind == Kind::Code)?;
    let code = &first.bytes;
    let kind = if hunks.len() == 2 && has(code, 0, "48E7FFFE 41FAFFF6 2050 D1C8 D1C8 4A98 2248 D3FC 00000200") {
        Layout::PowerPacker23
    } else if hunks.len() == 2 && has(code, 0, "487A???? 48E7FFFE 49FAFFF2 2054 D1C8 D1C8 5848 2648 504B 2C780004 2F08 D1FC") {
        // 4.0 packs its relocations into words and bytes, aligning the tables as it reads them.
        if find(code, 0, "524A 200A 08800000").is_some() { Layout::PowerPacker40 } else { Layout::PowerPacker30 }
    } else if hunks.len() == 2 && hunks[1].kind == Kind::Code && has(code, 0, "487A???? 48E7FFFE 49FAFFF2 2654 D7CB D7CB 4EAB") {
        // The built-in decruncher is called at its start (JSR 4(A3)), the library's at its own place.
        Layout::StoneCracker410 { library: !has(code, 0x12, "4EAB0004") }
    } else if hunks.len() >= 5 && (has(code, 0, "48E7FFFF 49FA???? 3C3C???? 61?? 4E91") || has(code, 0, "48E7FFFF 49FA???? 3C3C???? 4E94 4E91")) {
        Layout::ImploderNormal
    } else if hunks.len() >= 3 && has(code, 0, "48E7FFFF 70") && has(code, 0x18, "4EAEFFE2") && code[..code.len().min(0x60)].windows(16).any(|w| w == b"explode.library\0") {
        Layout::ImploderLibrary
    } else if hunks.len() >= 3 && has(code, 0, "48E7FFFF 3C3C???? 49FA???? 4E94 4E91 4E94 2449 5246 4E94") {
        Layout::CrunchManiaNormal
    } else if hunks.len() >= 2 && has(code, 0, "48E7FFFF 45FA???? 221A 241A 224A 287A") {
        Layout::CrunchManiaSimple
    } else if hunks.len() >= 2 && has(code, 0, "48E7FFFF 43FA0014 2C780004 4EAEFE68 2C40 200E 67EE 4EAEFFE2") && code.get(0x1A..0x26) == Some(b"CrM.library\0") {
        Layout::CrunchManiaLibrary
    } else if byte_killer(code).is_some() {
        Layout::ByteKiller
    } else {
        return None;
    };
    let name = match kind {
        Layout::PowerPacker23 => "PowerPacker 2.3".to_string(),
        Layout::PowerPacker30 => "PowerPacker 3.0".to_string(),
        Layout::PowerPacker40 => "PowerPacker 4.0".to_string(),
        Layout::ByteKiller if has(code, 0, "41FA") => "ByteKiller 1.3".to_string(),
        Layout::ByteKiller if has(code, 0, "48E7FFFE 4DF900DFF180 41FA") => "ByteKiller 2.0".to_string(),
        Layout::ByteKiller => crate::i18n::tr("ByteKiller (or a cruncher built on it)").to_string(),
        Layout::StoneCracker410 { library: false } => "StoneCracker 4.10".to_string(),
        Layout::StoneCracker410 { library: true } => crate::i18n::tr("StoneCracker 4.10 (its decruncher in stc.library)").to_string(),
        Layout::ImploderNormal => "Imploder".to_string(),
        Layout::ImploderLibrary => crate::i18n::tr("Imploder (its decruncher in explode.library)").to_string(),
        Layout::CrunchManiaNormal => "CrunchMania".to_string(),
        Layout::CrunchManiaSimple => crate::i18n::tr("CrunchMania (its simple decruncher)").to_string(),
        Layout::CrunchManiaLibrary => crate::i18n::tr("CrunchMania (its decruncher in CrM.library)").to_string(),
    };
    Some((Recognised { kind, name }, hunks))
}

fn cruncher_of(kind: Layout) -> Cruncher {
    match kind {
        Layout::PowerPacker23 | Layout::PowerPacker30 | Layout::PowerPacker40 => Cruncher::PowerPacker,
        Layout::ByteKiller => Cruncher::ByteKiller,
        Layout::StoneCracker410 { .. } => Cruncher::StoneCracker,
        Layout::ImploderNormal | Layout::ImploderLibrary => Cruncher::Imploder,
        Layout::CrunchManiaNormal | Layout::CrunchManiaSimple | Layout::CrunchManiaLibrary => Cruncher::CrunchMania,
    }
}

pub fn recognise(file: &[u8]) -> Option<Found> {
    let (found, hunks) = recognised(file)?;
    let load = if found.kind == Layout::ByteKiller { byte_killer(&hunks[0].bytes).map(|b| (b.load, b.start)) } else { None };
    Some(Found { cruncher: cruncher_of(found.kind), name: found.name, program: true, size: None, load })
}

pub fn decrunch(file: &[u8], found: &Found) -> Result<Vec<u8>, String> {
    let name = &found.name;
    let damaged = || crate::i18n::tf!("This program was crunched with {name}, but it doesn't decrunch: it may be damaged, or crunched in a way Limoniter doesn't know.");
    let (what, hunks) = recognised(file).ok_or_else(damaged)?;
    let built = match what.kind {
        Layout::PowerPacker23 => powerpacker_23(&hunks),
        Layout::PowerPacker30 | Layout::PowerPacker40 => powerpacker_30_40(&hunks, what.kind == Layout::PowerPacker40),
        Layout::ByteKiller => byte_killer_program(&hunks),
        Layout::StoneCracker410 { .. } => stonecracker_410(&hunks),
        Layout::ImploderNormal => imploder_normal(&hunks),
        Layout::ImploderLibrary => imploder_library(&hunks),
        Layout::CrunchManiaNormal | Layout::CrunchManiaSimple | Layout::CrunchManiaLibrary => crunchmania(what.kind, &hunks),
    };
    built.map_err(|_| damaged())
}

/// Checks a rebuilt program loads as an executable, as LoadSeg would load it.
fn checked(file: Vec<u8>) -> R<Vec<u8>> {
    crate::asm::hunk::read_executable(&file).map_err(|_| Bad)?;
    Ok(file)
}

/// PowerPacker 2.3's stream: the decruncher's LEA of the stream's length, and of the widths of distance.
fn powerpacker_23(hunks: &[RawHunk]) -> R<Vec<u8>> {
    let code = &hunks[0].bytes;
    let length_at = find(code, 0, "45FA???? 2012 D1C0 6100").and_then(|at| relative(code, at + 2)).ok_or(Bad)?;
    let modes_at = find(code, 0, "2449 4BFA").and_then(|at| relative(code, at + 4)).ok_or(Bad)?;
    let inverted = powerpacker_inverted(code, "6170 4A01 ??20")?;
    let data = &hunks[1].bytes;
    let length = be32(code, length_at)? as usize;
    let modes = formats::powerpacker_modes(be32(code, modes_at)?).ok_or(Bad)?;
    if length < 8 || length > data.len() {
        return Err(Bad);
    }
    checked(formats::powerpacker_with(data, 0, length - 4, modes, 0, inverted)?)
}

/// Whether the decruncher takes a 1 for a literal run, where PowerPacker's own takes a 0: the branch after it reads
/// the first bit (BNE there for the usual way round, BEQ for the other).
fn powerpacker_inverted(code: &[u8], pattern: &str) -> R<bool> {
    let at = find(code, 0, pattern).ok_or(Bad)?;
    match code[at + 4] {
        0x66 => Ok(false),
        0x67 => Ok(true),
        _ => Err(Bad),
    }
}

/// PowerPacker 3.0's and 4.0's stream: ADDA.L of its length; the widths of distance after the decruncher. 3.0's
/// decrunches to the original file as it was, 4.0's to it with its relocations packed smaller.
fn powerpacker_30_40(hunks: &[RawHunk], packed_relocations: bool) -> R<Vec<u8>> {
    let code = &hunks[0].bytes;
    let length = be32(code, 0x20)? as usize;
    let modes_at = find(code, 0, "4BFA???? 7C03 7801 7E07 7A01").and_then(|at| relative(code, at + 2)).ok_or(Bad)?;
    let modes = formats::powerpacker_modes(be32(code, modes_at)?).ok_or(Bad)?;
    let data = &hunks[1].bytes;
    if length < 8 || length > data.len() {
        return Err(Bad);
    }
    // The decruncher takes a 1 for a match (BCS), so a 0 for a literal run, as data files do.
    let at = find(code, 0, "4BFA???? 7C03").ok_or(Bad)?;
    if !has(code, at, "4BFA???? 7C03 7801 7E07 7A01 244B 2220 4A01 6706 6128 9284 E2AD E089 D7C1 611E 653E") {
        return Err(Bad);
    }
    let stream = formats::powerpacker_with(data, 0, length - 4, modes, 0, false)?;
    if packed_relocations { checked(executable(&powerpacker_40_hunks(&stream)?)) } else { checked(stream) }
}

/// The hunks of a decrunched PowerPacker 4.0 program: a hunk file whose relocations are each a count, the hunk
/// addressed, the first place, and then each next place as the distance from the last in words (a byte, or a zero
/// and three bytes), the tables aligned to words and the hunks to longs.
fn powerpacker_40_hunks(s: &[u8]) -> R<Vec<Hunk>> {
    if be32(s, 0)? != HUNK_HEADER {
        return Err(Bad);
    }
    let count = be32(s, 8)? as usize;
    if count == 0 || count > 0xFFFF {
        return Err(Bad);
    }
    let mut at = 20;
    let mut headers = Vec::new();
    for _ in 0..count {
        headers.push(be32(s, at)?);
        at += 4;
    }
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut open = false;
    loop {
        at = at.next_multiple_of(4);
        let raw = be32(s, at)?;
        let (id, flags) = (raw & 0xFFFF, raw & 0xC000_0000);
        at += 4;
        match id {
            HUNK_RELOC32 => {
                let hunk = hunks.last_mut().ok_or(Bad)?;
                hunk.reloc_block = true;
                loop {
                    at = at.next_multiple_of(2);
                    let n = be16(s, at)?;
                    at += 2;
                    if n == 0 {
                        break;
                    }
                    let target = be16(s, at)?;
                    let mut place = be32(s, at + 2)?;
                    at += 6;
                    let mut places = vec![place];
                    for _ in 1..n {
                        let mut step = u32::from(*s.get(at).ok_or(Bad)?);
                        at += 1;
                        if step == 0 {
                            step = u32::from(*s.get(at).ok_or(Bad)?) << 16 | be16(s, at + 1)?;
                            at += 3;
                        }
                        place = place.checked_add(step * 2).ok_or(Bad)?;
                        places.push(place);
                    }
                    hunk.relocations.push((target, places));
                }
            }
            HUNK_END => {
                open = false;
                if hunks.len() == count {
                    // Any more HUNK_ENDs the original file had, PowerPacker kept: they go back in too.
                    while be32(s, at).is_ok_and(|id| id & 0xFFFF == HUNK_END) {
                        hunks.last_mut().ok_or(Bad)?.extra_ends += 1;
                        at += 4;
                    }
                    break;
                }
            }
            HUNK_CODE | HUNK_DATA | HUNK_BSS => {
                if open || hunks.len() == count {
                    return Err(Bad);
                }
                open = true;
                let longs = be32(s, at)? as usize;
                at += 4;
                let header = headers[hunks.len()];
                let (kind, bytes) = match id {
                    HUNK_BSS => (Kind::Bss, Vec::new()),
                    _ => {
                        let bytes = s.get(at..at + longs * 4).ok_or(Bad)?.to_vec();
                        at += longs * 4;
                        (if id == HUNK_CODE { Kind::Code } else { Kind::Data }, bytes)
                    }
                };
                hunks.push(Hunk { kind, header, bytes, relocations: Vec::new(), flags, reloc_block: false, extra_ends: 0 });
            }
            _ => return Err(Bad),
        }
    }
    Ok(hunks)
}

/// StoneCracker 4.10: the S404 data at the end of the decruncher's hunk, and the program in its shorter form.
fn stonecracker_410(hunks: &[RawHunk]) -> R<Vec<u8>> {
    let code = &hunks[1].bytes;
    // The built-in decruncher points at its data first thing (whose mark is sometimes rubbed out); otherwise the
    // data is the last S404 header in the hunk whose crunched length reaches the hunk's end.
    let start = if has(code, 0, "45FA???? 43EA0004 D5D9 2A4A D5D9 D3D1") {
        relative(code, 2).ok_or(Bad)?
    } else {
        (0..code.len().saturating_sub(16))
            .rev()
            .find(|&at| code[at..].starts_with(b"S404") && formats::stone_header(&code[at..], Stone::V404).is_ok_and(|h| at + h.end <= code.len() && code.len() - (at + h.end) < 4))
            .ok_or(Bad)?
    };
    let data = &code[start..];
    let header = formats::stone_header(data, Stone::V404)?;
    let stream = formats::stonecracker(data, &header)?;
    checked(executable(&stonecracker_hunks(&stream)?))
}

/// The hunks of a program in StoneCracker's shorter form: the number of hunks less one, their sizes, then each
/// hunk's contents (a long: %01 and the length in longs for code, %11 for data; a word $8000 for BSS) and its
/// relocations (a zero word, then for each hunk addressed: a count, the hunk, and a long whose top byte says how
/// many bytes each next place's distance takes and whose rest is the first place), and $FFFF at the end.
fn stonecracker_hunks(s: &[u8]) -> R<Vec<Hunk>> {
    let count = be32(s, 0)? as usize + 1;
    if count > 0xFFFF {
        return Err(Bad);
    }
    let mut at = 4;
    let mut headers = Vec::new();
    for _ in 0..count {
        headers.push(be32(s, at)?);
        at += 4;
    }
    let mut hunks: Vec<Hunk> = Vec::new();
    loop {
        let word = be16(s, at)?;
        if word == 0xFFFF {
            break;
        }
        at += 2;
        if word != 0 {
            if hunks.len() == count {
                return Err(Bad);
            }
            let header = headers[hunks.len()];
            if word & 0x4000 == 0 {
                if word != 0x8000 {
                    return Err(Bad);
                }
                hunks.push(Hunk { kind: Kind::Bss, header, bytes: Vec::new(), relocations: Vec::new(), flags: 0, reloc_block: false, extra_ends: 0 });
            } else {
                let length = ((word << 16 | be16(s, at)?) & 0x3FFF_FFFF) as usize * 4;
                at += 2;
                let bytes = s.get(at..at + length).ok_or(Bad)?.to_vec();
                at += length;
                let kind = if word & 0x8000 == 0 { Kind::Code } else { Kind::Data };
                hunks.push(Hunk { kind, header, bytes, relocations: Vec::new(), flags: 0, reloc_block: false, extra_ends: 0 });
            }
            continue;
        }
        let hunk = hunks.last_mut().ok_or(Bad)?;
        loop {
            at = at.next_multiple_of(2);
            let n = be16(s, at)?;
            at += 2;
            if n == 0 {
                break;
            }
            let target = be16(s, at)?;
            let first = be32(s, at + 2)?;
            at += 6;
            // The top byte picks how much of the instructions that add each distance are run: 2 for three bytes,
            // 4 or 6 for two, 8 or 10 for one.
            let width = match first >> 24 {
                _ if n == 1 => 0,
                2 => 3,
                4 | 6 => 2,
                8 | 10 => 1,
                _ => return Err(Bad),
            };
            let mut place = first & 0x00FF_FFFF;
            let mut places = vec![place];
            for _ in 1..n {
                let mut step = 0u32;
                for _ in 0..width {
                    step = step << 8 | u32::from(*s.get(at).ok_or(Bad)?);
                    at += 1;
                }
                place = place.checked_add(step).ok_or(Bad)?;
                places.push(place);
            }
            hunk.relocations.push((target, places));
        }
    }
    if hunks.len() != count {
        return Err(Bad);
    }
    Ok(hunks)
}

/// Imploder's normal mode (3.x and 4.0). The decruncher's hunk says how long the crunched data is and how many of
/// the program's hunks there are, and holds the stream's starting state and its mode's tables; the stream it
/// decrunches is a series of longs saying, hunk by hunk, how many longs to fill it with (and those longs), where
/// its relocations go, and when to go on to the next hunk.
fn imploder_normal(hunks: &[RawHunk]) -> R<Vec<u8>> {
    let code = &hunks.get(hunks.len() - 3).ok_or(Bad)?.bytes;
    if !has(code, 0, "7A00 5246 4E94 D3FC ???????? 2049 5246 4E94 2C49 244E 383C ???? 48C4 3C05 4E94 2649 61?? 6A?? 5245 0C45 ????") {
        return Err(Bad);
    }
    let length = be32(code, 8)? as usize;
    let targets = be16(code, 0x2A)? as usize;
    let state = find(code, 0, "D5C4 48E7F618 264A 3A04 49FA").and_then(|at| relative(code, at + 12)).ok_or(Bad)?;
    let literal_table = find(code, 0, "060A0A12 02030304 0405 07").ok_or(Bad)?;
    let data = &hunks[hunks.len() - 2].bytes;
    if targets + 4 != hunks.len() || length > data.len() {
        return Err(Bad);
    }
    // The tables of distance follow the literal tables: the bases of the second and third ranges, then each
    // range's width (8 and more marked with $80, the decruncher taking a byte and then the rest).
    let mut distances = [[0u32; 4]; 2];
    for i in 0..8 {
        distances[i >> 2][i & 3] = be16(code, literal_table + 0x52 + i * 2)?;
    }
    let mut widths = [[0u32; 4]; 3];
    for i in 0..12 {
        widths[i >> 2][i & 3] = imploder_width(*code.get(literal_table + 0x62 + i).ok_or(Bad)?);
    }
    let longest_literals = u32::from(code[literal_table + 11]);
    let literals = be16(code, state)?;
    let bits = u32::from(*code.get(state + 2).ok_or(Bad)?);
    if code.get(state + 3) != Some(&0) {
        return Err(Bad);
    }
    let matches = be32(code, state + 4)?;
    let tables = ImploderTables { literals, bits: imploder_bits(bits), distances, widths, longest_literals };
    let stream = formats::imploder_stream(backwards(data, length)?, &tables, ImploderEnd::Matches(matches));
    let stream = stream?;
    let headers: Vec<u32> = hunks[1..=targets].iter().map(|h| h.header).collect();
    checked(executable(&imploder_hunks(&stream, &headers)?))
}

/// A width in an Imploder program's table, as the stream decoder takes it: 8 and more are marked with $80, the
/// decruncher taking a byte and then the rest.
fn imploder_width(w: u8) -> u32 {
    u32::from(w)
}

/// How many bits a width in an Imploder program's table stands for.
fn imploder_bit_count(w: u32) -> u32 {
    if w & 0x80 != 0 { 8 + (w & 0x0F) } else { w }
}

/// The bits held in a byte whose lowest set bit marks where they end.
fn imploder_bits(byte: u32) -> (u32, u32) {
    if byte == 0 { (0, 0) } else { (byte >> (byte.trailing_zeros() + 1), 7 - byte.trailing_zeros()) }
}

/// The bytes of `data` before `end`, from the last.
fn backwards(data: &[u8], end: usize) -> R<impl FnMut() -> R<u8> + '_> {
    if end > data.len() {
        return Err(Bad);
    }
    let mut at = end;
    Ok(move || -> R<u8> {
        if at == 0 {
            return Err(Bad);
        }
        at -= 1;
        Ok(data[at])
    })
}

/// The widths of distance for each of explode.library's modes (0 to 8), as its own table gives them: the first
/// range's four, the second's and the third's, 8 and more marked with $80.
const EXPLODE_WIDTHS: [[u8; 12]; 9] = [
    [0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x05, 0x06, 0x06, 0x06, 0x06],
    [0x05, 0x06, 0x07, 0x07, 0x06, 0x06, 0x06, 0x06, 0x07, 0x07, 0x06, 0x06],
    [0x05, 0x06, 0x07, 0x07, 0x07, 0x07, 0x07, 0x07, 0x80, 0x80, 0x80, 0x80],
    [0x05, 0x06, 0x07, 0x80, 0x07, 0x07, 0x80, 0x80, 0x80, 0x81, 0x81, 0x81],
    [0x06, 0x07, 0x07, 0x80, 0x07, 0x80, 0x81, 0x81, 0x80, 0x81, 0x82, 0x82],
    [0x06, 0x07, 0x07, 0x80, 0x07, 0x81, 0x81, 0x82, 0x80, 0x82, 0x83, 0x83],
    [0x06, 0x07, 0x07, 0x80, 0x07, 0x81, 0x81, 0x82, 0x80, 0x82, 0x84, 0x84],
    [0x06, 0x07, 0x07, 0x80, 0x07, 0x81, 0x82, 0x82, 0x80, 0x83, 0x84, 0x85],
    [0x06, 0x07, 0x07, 0x80, 0x07, 0x81, 0x82, 0x83, 0x80, 0x83, 0x85, 0x86],
];

/// Imploder's library mode: the program's first hunk starts with a little root that opens explode.library and
/// calls it, followed by the stream's starting state (its mode and the width of its longest literal count, the bits
/// left over, the first literal run and the number of matches); the crunched data is the last hunk but one, and
/// the library works out its tables of distance from its mode.
fn imploder_library(hunks: &[RawHunk]) -> R<Vec<u8>> {
    let root = &hunks[0].bytes;
    let targets = hunks.len() - 2;
    let mode = usize::from(*root.get(0x1C).ok_or(Bad)?);
    let row = EXPLODE_WIDTHS.get(mode & 0x0F).ok_or(Bad)?;
    let mut matches = be32(root, 0x20)?;
    // The top bit of the count asks for every base to be one less.
    let less = u32::from(matches & 0x8000_0000 != 0);
    matches &= 0x7FFF_FFFF;
    let mut widths = [[0u32; 4]; 3];
    for (i, w) in row.iter().enumerate() {
        widths[i >> 2][i & 3] = imploder_width(*w);
    }
    let mut distances = [[0u32; 4]; 2];
    for s in 0..4 {
        distances[0][s] = (1 << imploder_bit_count(widths[0][s])) - less;
        distances[1][s] = distances[0][s] + (1 << imploder_bit_count(widths[1][s])) - less;
    }
    let tables = ImploderTables { literals: be16(root, 0x1E)?, bits: imploder_bits(u32::from(root[0x1D])), distances, widths, longest_literals: u32::from(root[0x1C] >> 4) };
    // The data hunk is read from the end of the room LoadSeg gives it; a flag in the root says when the last long
    // holds only the stream's last few bytes.
    let data_hunk = &hunks[hunks.len() - 2];
    let mut data = data_hunk.bytes.clone();
    let mut length = (data_hunk.header & 0x3FFF_FFFF) as usize * 4;
    data.resize(length.max(data.len()), 0);
    let odd = root[5] as usize;
    if odd != 0 {
        length = (length.checked_sub(4).ok_or(Bad)?) | odd;
    }
    let stream = formats::imploder_stream(backwards(&data, length)?, &tables, ImploderEnd::Matches(matches));
    let stream = stream?;
    let headers: Vec<u32> = hunks[..targets].iter().map(|h| h.header).collect();
    checked(executable(&imploder_hunks(&stream, &headers)?))
}

/// The program from Imploder's stream of longs, as its own Deplode builds it again: each hunk given data is code,
/// or data if the long that ends it says so (Imploder 3 didn't keep which were which, only what's loaded), and the
/// rest BSS.
fn imploder_hunks(s: &[u8], headers: &[u32]) -> R<Vec<Hunk>> {
    let mut reader = Longs { bytes: s, at: 0 };
    let mut next = || reader.next();
    let mut hunks = Vec::new();
    for &header in headers {
        let mut hunk = Hunk { kind: Kind::Bss, header, bytes: Vec::new(), relocations: Vec::new(), flags: 0, reloc_block: false, extra_ends: 0 };
        loop {
            let value = next()?;
            if value & 0x8000_0000 != 0 {
                // The end of the hunk: Imploder 4.0 marks a data hunk with the next bit too.
                if value & 0x4000_0000 != 0 && hunk.kind == Kind::Code {
                    hunk.kind = Kind::Data;
                }
                break;
            }
            if value & 0x4000_0000 != 0 {
                loop {
                    let n = next()?;
                    if n == 0 {
                        break;
                    }
                    let target = next()?;
                    let mut place = 0u32;
                    let mut places = Vec::new();
                    for _ in 0..n {
                        place = place.wrapping_add(next()?);
                        places.push(place);
                    }
                    hunk.relocations.push((target, places));
                }
            } else if value != 0 {
                // Each block of longs is copied to the start of the hunk, as the decruncher copies it.
                let mut bytes = Vec::with_capacity(value as usize * 4);
                for _ in 0..value {
                    bytes.extend(next()?.to_be_bytes());
                }
                if hunk.bytes.len() > bytes.len() {
                    bytes.extend_from_slice(&hunk.bytes[bytes.len()..]);
                }
                hunk.bytes = bytes;
                hunk.kind = Kind::Code;
            }
        }
        hunks.push(hunk);
    }
    Ok(hunks)
}

/// CrunchMania's programs. Wherever the decruncher is, it has the crunched data's two lengths (decrunched and
/// crunched) and the stream after them, as a data file has, and a table of how many longs of data each of the
/// program's hunks is given; the hunks themselves are in the file, empty, with their kinds and sizes. The stream
/// decrunches to each hunk's longs, one hunk after another, and then the relocations: for each hunk, its number,
/// then for each hunk addressed a count, that hunk's number and each place's distance from the last in words (a
/// word, or a zero word and a long), a count of 0 ending the hunk and a negative number the lot.
fn crunchmania(layout: Layout, hunks: &[RawHunk]) -> R<Vec<u8>> {
    let first = &hunks[0].bytes;
    let (code, params, table, lzh, targets): (&[u8], usize, usize, bool, std::ops::Range<usize>) = match layout {
        Layout::CrunchManiaNormal => {
            let code = &hunks.last().ok_or(Bad)?.bytes;
            let params = if has(code, 0, "45FA") { relative(code, 2).ok_or(Bad)? } else { return Err(Bad) };
            let table = find(code, 0, "7CFF 45FA").and_then(|at| relative(code, at + 4)).ok_or(Bad)?;
            (code, params, table, crunchmania_lzh(code)?, 1..hunks.len() - 1)
        }
        Layout::CrunchManiaSimple => {
            let params = relative(first, 6).ok_or(Bad)?;
            let table = find(first, 0, "2F49003C 45FA").and_then(|at| relative(first, at + 6)).ok_or(Bad)?;
            if crunchmania_lzh(first)? {
                return Err(Bad);
            }
            (first, params, table, false, 1..hunks.len())
        }
        _ => {
            // The root's table follows it; the stream is the last hunk.
            if be16(first, 0x30)? as usize != hunks.len() - 1 {
                return Err(Bad);
            }
            let lzh = match first.get(0x32) {
                Some(1) => true,
                Some(0) => false,
                _ => return Err(Bad),
            };
            let data = &hunks.last().ok_or(Bad)?.bytes;
            let (size, packed) = (be32(first, 0x28)? as usize, be32(first, 0x2C)? as usize);
            let stream = crunchmania_stream(data, 0, size, packed, lzh)?;
            let counts = (0..hunks.len() - 1).map(|i| be32(first, 0x34 + i * 4)).collect::<R<Vec<u32>>>()?;
            return checked(executable(&crunchmania_hunks(&stream, &counts, &hunks[..hunks.len() - 1])?));
        }
    };
    let (size, packed) = (be32(code, params)? as usize, be32(code, params + 4)? as usize);
    let stream = crunchmania_stream(code, params + 8, size, packed, lzh)?;
    let counts = targets.clone().map(|i| be32(code, table + (i - targets.start) * 4)).collect::<R<Vec<u32>>>()?;
    checked(executable(&crunchmania_hunks(&stream, &counts, &hunks[targets])?))
}

/// Whether a CrunchMania decruncher is the LZ-Huffman one (which builds its tables in the room after the data), or
/// the one for its normal algorithm (which counts its lengths up to 22).
fn crunchmania_lzh(code: &[u8]) -> R<bool> {
    if find(code, 0, "41EE049E").is_some() {
        Ok(true)
    } else if find(code, 0, "0C440016").is_some() {
        Ok(false)
    } else {
        Err(Bad)
    }
}

/// A CrunchMania stream `packed` bytes long at `at`, decoded as a data file's is.
fn crunchmania_stream(bytes: &[u8], at: usize, size: usize, packed: usize, lzh: bool) -> R<Vec<u8>> {
    let mut file = Vec::with_capacity(packed + 14);
    file.extend(if lzh { b"CrM2" } else { b"CrM!" });
    file.extend([0, 0]);
    file.extend((size as u32).to_be_bytes());
    file.extend((packed as u32).to_be_bytes());
    file.extend(bytes.get(at..at.checked_add(packed).ok_or(Bad)?).ok_or(Bad)?);
    formats::crunchmania(&file, packed, size, lzh, false)
}

/// The program from CrunchMania's decrunched stream, the hunks' kinds and sizes taken from the file.
fn crunchmania_hunks(s: &[u8], counts: &[u32], targets: &[RawHunk]) -> R<Vec<Hunk>> {
    let mut at = 0;
    let mut hunks = Vec::new();
    for (count, target) in counts.iter().zip(targets) {
        let length = *count as usize * 4;
        let bytes = s.get(at..at + length).ok_or(Bad)?.to_vec();
        at += length;
        // The root of a library-crunched program takes the place of its first hunk, which can only have been code.
        let kind = if target.kind == Kind::Bss && !bytes.is_empty() { Kind::Data } else { target.kind };
        // The memory goes in the header, where LoadSeg reads it, and not on the type long too.
        hunks.push(Hunk { kind, header: target.header, bytes, relocations: Vec::new(), flags: 0, reloc_block: false, extra_ends: 0 });
    }
    loop {
        let hunk = be16(s, at)?;
        at += 2;
        if hunk & 0x8000 != 0 {
            break;
        }
        let hunk = hunks.get_mut(hunk as usize).ok_or(Bad)?;
        loop {
            let n = be16(s, at)?;
            at += 2;
            if n == 0 {
                break;
            }
            let target = be16(s, at)?;
            at += 2;
            let mut place = 0u32;
            let mut places = Vec::new();
            for _ in 0..n {
                let mut step = be16(s, at)?;
                at += 2;
                if step == 0 {
                    step = be32(s, at)?;
                    at += 4;
                }
                place = place.wrapping_add(step.wrapping_mul(2));
                places.push(place);
            }
            hunk.relocations.push((target, places));
        }
    }
    Ok(hunks)
}

/// Where a ByteKiller decruncher finds its data, where it puts what it decrunches and where it starts it.
struct ByteKillerStub {
    data: usize,
    load: u32,
    start: u32,
}

/// ByteKiller's decruncher, wherever it is in the hunk (others put their own code before it): LEA of its data, LEA
/// of where it decrunches to, the three longs of the data's header taken, and the stream read back from its end
/// with every long XORed into a check; then a JMP to where the program starts.
fn byte_killer(code: &[u8]) -> Option<ByteKillerStub> {
    let at = find(code, 0, "41FA???? 43F9???????? 2018 2218 2A18 2449 D1C0 D5C1 2020 B185")?;
    let data = relative(code, at + 2)?;
    let load = be32(code, at + 6).ok()?;
    let jump = find(code, at, "4EF9")?;
    let start = be32(code, jump + 2).ok()?;
    Some(ByteKillerStub { data, load, start })
}

/// The memory a ByteKiller program decrunches to: its data, in ByteKiller's own format.
fn byte_killer_program(hunks: &[RawHunk]) -> R<Vec<u8>> {
    let code = &hunks[0].bytes;
    let stub = byte_killer(code).ok_or(Bad)?;
    let packed = be32(code, stub.data)? as usize;
    let start = stub.data + 12;
    formats::bytekiller(code, start, start.checked_add(packed).ok_or(Bad)?, be32(code, stub.data + 4)? as usize, be32(code, stub.data + 8)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn longs(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    #[test]
    fn powerpacker_4_relocations_are_unpacked() {
        // One code hunk of four longs, with places 0, 4, 4 + 2 * 300 and 4 + 2 * 300 + 2 * 70000 holding addresses.
        let mut s = longs(&[HUNK_HEADER, 0, 1, 0, 0, 4, HUNK_CODE, 4, 1, 2, 3, 4, HUNK_RELOC32]);
        // A count of four, the hunk, the first place, then distances in words: a byte, then two as a zero and three.
        s.extend([0, 4, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0x01, 0x2C, 0, 0x01, 0x11, 0x70]);
        // Aligned to a word, a count of 0 ends the table.
        s.extend([0, 0, 0]);
        s.resize(s.len().next_multiple_of(4), 0);
        s.extend(longs(&[HUNK_END]));
        let hunks = powerpacker_40_hunks(&s).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].bytes, longs(&[1, 2, 3, 4]));
        assert_eq!(hunks[0].relocations, vec![(0, vec![0, 4, 604, 140604])]);
        let file = executable(&hunks);
        let read = crate::asm::hunk::read_executable(&file).unwrap();
        assert_eq!(read[0].relocations, vec![(0, 0), (4, 0), (604, 0), (140604, 0)]);
    }

    #[test]
    fn stonecracker_and_imploder_hunk_streams_are_read() {
        // StoneCracker's shorter form: two hunks, code (one long, a relocation to the BSS) and BSS.
        let mut s = longs(&[1, 1, 0x4000_0002]);
        s.extend([0x40, 0, 0, 1, 0xDE, 0xAD, 0xBE, 0xEF, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0x80, 0, 0xFF, 0xFF]);
        let hunks = stonecracker_hunks(&s).unwrap();
        assert_eq!(hunks.iter().map(|h| (h.kind, h.header)).collect::<Vec<_>>(), [(Kind::Code, 1), (Kind::Bss, 0x4000_0002)]);
        assert_eq!(hunks[0].relocations, vec![(1, vec![0])]);
        // Imploder's longs: a hunk of two longs with a relocation, ended as data; then a BSS hunk.
        let s = longs(&[2, 7, 8, 0x4000_0000, 1, 1, 4, 0, 0xC000_0000, 0x8000_0000]);
        let hunks = imploder_hunks(&s, &[2, 5]).unwrap();
        assert_eq!(hunks.iter().map(|h| h.kind).collect::<Vec<_>>(), [Kind::Data, Kind::Bss]);
        assert_eq!(hunks[0].relocations, vec![(1, vec![4])]);
    }

    #[test]
    fn a_bytekiller_program_decrunches_to_the_memory_it_fills() {
        let memory = b"The memory a ByteKiller program fills, at its own address, and starts.".repeat(3);
        let data = super::super::tests::bytekiller_literals(&memory);
        // ByteKiller 1.3's decruncher, as far as Limoniter looks at it: its data after it, put at $40000.
        let mut code = vec![0x41, 0xFA, 0, 0, 0x43, 0xF9, 0, 4, 0, 0, 0x20, 0x18, 0x22, 0x18, 0x2A, 0x18, 0x24, 0x49, 0xD1, 0xC0, 0xD5, 0xC1, 0x20, 0x20, 0xB1, 0x85, 0x4E, 0xF9, 0, 4, 0, 0];
        let at = code.len() as u16 - 2;
        code[2..4].copy_from_slice(&at.to_be_bytes());
        code.extend(&data);
        code.resize(code.len().next_multiple_of(4), 0);
        let hunk = Hunk { kind: Kind::Code, header: code.len() as u32 / 4, bytes: code, relocations: Vec::new(), flags: 0, reloc_block: false, extra_ends: 0 };
        let file = executable(&[hunk]);
        let found = super::super::recognise(&file).unwrap();
        assert_eq!((found.name.as_str(), found.load), ("ByteKiller 1.3", Some((0x40000, 0x40000))));
        assert_eq!(super::super::decrunch(&file).unwrap().unwrap().bytes, memory);
    }
}
