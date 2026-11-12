//! Acorn's disks and tapes: DFS, the BBC Micro's and Electron's disk system (single and double-sided `.ssd` and
//! `.dsd` images, and Watford DFS's 62-file catalogue); ADFS in the 8-bit machines' old-map form (the S, M and L
//! floppies, `.ads`, `.adm`, `.adl`); RISC OS's ADFS (D, with old-map space and new directories; E and F, with the
//! new map's fragments), whose floppies are `.adf` like the Amiga's; and UEF tape images.
//!
//! The layouts are those of Acorn's DFS and ADFS manuals and the RISC OS Programmer's Reference Manual (the
//! FileCore chapter), with the new map read the way Linux's `fs/adfs` reads it. A UEF holds the bytes the
//! cassette filing system wrote, block by block, each with its header and CRCs, as Thomas Harte's UEF
//! specification lays them out.

use super::{Entry, MOST_FILE_BYTES, Source, Volume, tidy_name};
use crate::basic_more;
use std::sync::Arc;

/// The text preset for files from an 8-bit Acorn's disk or tape: the BBC Micro's character set, lines ending in CR.
pub const BBC_TEXT_PRESET: &str = "bbc-micro";

/// Files no larger than this are read as they're listed, so a BBC BASIC program can be given its ending.
const MOST_SNIFFED: u64 = 64 * 1024;

fn le(bytes: &[u8]) -> u64 {
    bytes.iter().rev().fold(0, |n, b| (n << 8) | u64::from(*b))
}

/// Seven-bit text as an 8-bit Acorn names things, ending at a control character, its padding taken off.
fn acorn_text(bytes: &[u8], mask: bool) -> String {
    let mut out = String::new();
    for &b in bytes {
        let b = if mask { b & 0x7F } else { b };
        if b < 0x20 || b == 0x7F {
            break;
        }
        // Latin-1, as RISC OS names are; 8-bit names are ASCII, so this is the same for them.
        out.push(char::from(b));
    }
    out.trim_end().to_string()
}

/// The kind and ending a file's bytes give it: a BBC BASIC program is listed as one, under `.bbc`.
fn sniff(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    basic_more::looks_like_bbc(bytes).then_some(("BBC BASIC program", "bbc"))
}

/// A load or execution address as *INFO shows a 32-bit one.
fn hex32(address: u32) -> String {
    format!("{address:08X}")
}

/// The civil date of a RISC OS time stamp: centiseconds since the start of 1900.
fn riscos_date(centiseconds: u64) -> String {
    let seconds = centiseconds / 100;
    let days = (seconds / 86_400) as i64 - 25_567;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let time = seconds % 86_400;
    format!("{year}-{month:02}-{day:02} {:02}:{:02}:{:02}", time / 3600, time / 60 % 60, time % 60)
}

/// RISC OS file types: their names, as the Filer shows them, and an ending where Limoniter has a use for one.
fn file_type(kind: u32) -> (String, Option<&'static str>) {
    let (name, ending) = match kind {
        0xFFF => ("Text", Some("txt")),
        0xFFE => ("Command", None),
        0xFFD => ("Data", None),
        0xFFC => ("Utility", None),
        0xFFB => ("BASIC", Some("bbc")),
        0xFFA => ("Module", None),
        0xFF9 => ("Sprite", Some("ff9")),
        0xFF8 => ("Absolute", None),
        0xFF7 => ("BBC font", None),
        0xFF6 => ("Font", None),
        0xFF5 => ("PostScript", Some("ps")),
        0xFF4 => ("Printout", None),
        0xFF2 => ("Config", None),
        0xFF0 => ("TIFF", Some("tif")),
        0xFED => ("Palette", None),
        0xFEC => ("Template", None),
        0xFEB => ("Obey", Some("txt")),
        0xFE6 => ("UNIX executable", None),
        0xFE4 => ("DOS", None),
        0xFDC => ("Soft link", None),
        0xFD7 => ("TaskObey", Some("txt")),
        0xFCA => ("Squash", None),
        0xFAF => ("HTML", Some("html")),
        0xF89 => ("GZip", None),
        0xDFE => ("CSV", Some("csv")),
        0xDDC => ("Archive (Spark)", None),
        0xC85 => ("JPEG", Some("jpg")),
        0xB60 => ("PNG", Some("png")),
        0xAFF => ("DrawFile", None),
        0xA91 => ("Zip", Some("zip")),
        0x69C => ("BMP", Some("bmp")),
        0x695 => ("GIF", Some("gif")),
        0x3FB => ("ArcFS", None),
        _ => return (format!("Type &{kind:03X}"), None),
    };
    (name.to_string(), ending)
}

/// What an Acorn file's load and execution addresses say: a RISC OS file type and date, or the two addresses.
fn addresses(load: u32, exec: u32, typed: bool) -> (Option<u32>, String) {
    if typed && load >> 20 == 0xFFF {
        let stamp = (u64::from(load & 0xFF) << 32) | u64::from(exec);
        return (Some((load >> 8) & 0xFFF), riscos_date(stamp));
    }
    (None, format!("Load {} \u{B7} Exec {}", hex32(load), hex32(exec)))
}

// ---------- DFS ----------

struct DfsFile {
    name: String,
    load: u32,
    exec: u32,
    length: u32,
    start: u32,
    locked: bool,
}

struct DfsSide {
    title: String,
    files: Vec<DfsFile>,
    sectors: u32,
    boot: u8,
    watford: bool,
}

pub struct Dfs {
    source: Arc<dyn Source>,
    double: bool,
    /// The sides with a catalogue, as (side, catalogue).
    sides: Vec<(u8, DfsSide)>,
    notes: Vec<String>,
}

const BOOT_OPTIONS: [&str; 4] = ["none", "LOAD", "RUN", "EXEC"];

/// Whether the start of an image reads as a DFS catalogue: names of printable characters, file counts that are
/// whole entries, and files lying after the catalogue and within the disk. `strict` also wants a disk size of 40 or
/// 80 tracks and at least one file, for an ending such as `.img` that says nothing of what's inside.
pub fn looks_like_dfs(head: &[u8], strict: bool) -> bool {
    head.len() >= 512 && read_catalogue(&|sector: u32| head.get(sector as usize * 256..sector as usize * 256 + 256).map(<[u8]>::to_vec).ok_or_else(String::new)).is_ok_and(|side| !strict || ((side.sectors == 400 || side.sectors == 800) && !side.files.is_empty()))
}

/// One side's catalogue, from its first two sectors, and Watford DFS's second pair if it has one.
fn read_catalogue(sector: &dyn Fn(u32) -> Result<Vec<u8>, String>) -> Result<DfsSide, String> {
    let (s0, s1) = (sector(0)?, sector(1)?);
    let sectors = (u32::from(s1[6] & 3) << 8) | u32::from(s1[7]);
    // Even an empty disk counts its sectors; none at all is an unformatted side or something else.
    if sectors < 2 {
        return Err("This isn't a DFS catalogue: it says the disk has no sectors.".into());
    }
    let mut side = DfsSide { title: acorn_text(&[&s0[..8], &s1[..4]].concat(), true), files: Vec::new(), sectors, boot: (s1[6] >> 4) & 3, watford: false };
    let mut catalogues = vec![(s0, s1)];
    // Watford DFS marks its second catalogue, in sectors 2 and 3, with eight bytes of &AA.
    if let (Ok(s2), Ok(s3)) = (sector(2), sector(3))
        && s2[..8].iter().all(|b| *b == 0xAA)
    {
        side.watford = true;
        catalogues.push((s2, s3));
    }
    for (names, info) in &catalogues {
        let count = usize::from(info[5]);
        if !count.is_multiple_of(8) || count > 31 * 8 {
            return Err("This isn't a DFS catalogue: its file count isn't a whole number of entries.".into());
        }
        for at in (8..8 + count).step_by(8) {
            let (name, about) = (&names[at..at + 8], &info[at..at + 8]);
            let shown = acorn_text(&name[..7], true);
            let folder = name[7] & 0x7F;
            if name[..7].iter().any(|b| !(0x20..0x7F).contains(&(b & 0x7F))) || !(0x21..0x7F).contains(&folder) {
                return Err("This isn't a DFS catalogue: a file name has characters a name can't.".into());
            }
            let high = about[6];
            let wide = |bits: u8, low: u64| -> u32 {
                let top = u32::from(bits & 3);
                // Both high bits set means an address in the I/O processor, which *INFO shows as FFxxxx.
                if top == 3 { 0xFFFF_0000 | low as u32 } else { (top << 16) | low as u32 }
            };
            let file = DfsFile {
                name: format!("{}.{shown}", char::from(folder)),
                load: wide(high >> 2, le(&about[0..2])),
                exec: wide(high >> 6, le(&about[2..4])),
                length: (u32::from((high >> 4) & 3) << 16) | le(&about[4..6]) as u32,
                start: (u32::from(high & 3) << 8) | u32::from(about[7]),
                locked: name[7] & 0x80 != 0,
            };
            let first = if side.watford { 4 } else { 2 };
            if file.start < first || file.start + file.length.div_ceil(256) > sectors.max(2) {
                return Err(format!("This isn't a DFS catalogue: {} lies outside the disk.", file.name));
            }
            side.files.push(file);
        }
    }
    side.files.sort_by_key(|file| (!file.name.starts_with('$'), file.name.to_lowercase()));
    Ok(side)
}

impl Dfs {
    pub fn open(source: Arc<dyn Source>, double: bool) -> Result<Self, String> {
        let mut dfs = Self { source, double, sides: Vec::new(), notes: Vec::new() };
        let mut first_problem = None;
        for side in 0..if double { 2 } else { 1 } {
            match read_catalogue(&|sector| dfs.sector(side, sector)) {
                Ok(catalogue) => dfs.sides.push((side, catalogue)),
                Err(problem) => {
                    let blank = dfs.sector(side, 0).and_then(|s0| Ok([s0, dfs.sector(side, 1)?].concat())).is_ok_and(|both| both.iter().all(|b| *b == both[0]));
                    dfs.notes.push(if blank { format!("Side {} is blank.", side + 1) } else { format!("Side {} has no DFS catalogue that can be read.", side + 1) });
                    first_problem.get_or_insert(problem);
                }
            }
        }
        if dfs.sides.is_empty() {
            return Err(first_problem.unwrap_or_else(|| "This disk has no DFS catalogue.".into()));
        }
        Ok(dfs)
    }

    /// A sector's place in the image: one side after another track by track in a `.dsd`, ten sectors to a track.
    fn offset(&self, side: u8, sector: u32) -> u64 {
        let sector = u64::from(sector);
        if self.double { ((sector / 10 * 2 + u64::from(side)) * 10 + sector % 10) * 256 } else { sector * 256 }
    }

    fn sector(&self, side: u8, sector: u32) -> Result<Vec<u8>, String> {
        self.source.read_vec(self.offset(side, sector), 256).map_err(|_| format!("Sector {sector} is past the end of the image."))
    }

    fn side(&self, side: u8) -> Result<&DfsSide, String> {
        self.sides.iter().find(|(s, _)| *s == side).map(|(_, c)| c).ok_or_else(|| "That side has no catalogue.".to_string())
    }

    fn file_bytes(&self, side: u8, file: &DfsFile) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(file.length as usize);
        let mut sector = file.start;
        while out.len() < file.length as usize {
            let want = (file.length as usize - out.len()).min(256);
            let at = self.offset(side, sector);
            if at + want as u64 > self.source.len() {
                return Err(format!("{} runs past the end of the image, which has been cut short.", file.name));
            }
            out.extend(self.source.read_vec(at, want)?);
            sector += 1;
        }
        Ok(out)
    }

    fn entries(&self, side: u8) -> Result<Vec<Entry>, String> {
        let catalogue = self.side(side)?;
        Ok(catalogue
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                let mut entry = Entry::file(tidy_name(&file.name), u64::from(file.length), index as u64);
                entry.extra = u64::from(side);
                entry.kind = "File".into();
                entry.detail = format!("Load {:06X} \u{B7} Exec {:06X}{}", file.load & 0xFF_FFFF, file.exec & 0xFF_FFFF, if file.locked { " \u{B7} locked" } else { "" });
                if u64::from(file.length) <= MOST_SNIFFED
                    && let Ok(bytes) = self.file_bytes(side, file)
                    && let Some((kind, ending)) = sniff(&bytes)
                {
                    entry.kind = kind.into();
                    entry.open_name = format!("{}.{ending}", entry.name);
                }
                entry
            })
            .collect())
    }
}

impl Volume for Dfs {
    fn describe(&self) -> String {
        let (_, first) = &self.sides[0];
        let kind = if first.watford { "Watford DFS" } else { "Acorn DFS" };
        let shape = if self.double { "double-sided (DSD)" } else { "single-sided (SSD)" };
        let title = if first.title.is_empty() { "no title".to_string() } else { first.title.clone() };
        format!("{kind} disk, {shape}, {} tracks \u{B7} {title} \u{B7} boot {}", first.sectors / 10, BOOT_OPTIONS[usize::from(first.boot)])
    }

    /// A single-sided disk's files are at the top; a double-sided one's two sides are folders, as drives 0 and 2.
    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        match dir {
            Some(side) => self.entries(side.location as u8),
            None if !self.double => self.entries(0),
            None => Ok(self
                .sides
                .iter()
                .map(|(side, catalogue)| {
                    let mut entry = Entry::dir(format!("Side {}", side + 1), u64::from(*side), 0);
                    entry.kind = format!("Drive {}", side * 2);
                    entry.detail = format!("{} \u{B7} {} file(s){}", if catalogue.title.is_empty() { "no title" } else { &catalogue.title }, catalogue.files.len(), if catalogue.watford { " \u{B7} Watford DFS" } else { "" });
                    entry
                })
                .collect()),
        }
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let side = entry.extra as u8;
        let file = self.side(side)?.files.get(entry.location as usize).ok_or("That file isn't on the disk.")?;
        self.file_bytes(side, file)
    }

    fn notes(&self) -> Vec<String> {
        self.notes.clone()
    }

    fn text_preset(&self, _entry: &Entry) -> Option<&'static str> {
        Some(BBC_TEXT_PRESET)
    }
}

// ---------- ADFS ----------

/// The new map's disc record: the shape of the disc and of its map.
#[derive(Clone, Debug)]
struct DiscRecord {
    log2secsize: u32,
    idlen: u32,
    log2bpmb: u32,
    nzones: u32,
    zone_spare: u32,
    root: u32,
    disc_size: u64,
    name: String,
    log2sharesize: u32,
    big_directories: bool,
}

impl DiscRecord {
    /// A disc record, if these 60 bytes are a believable one.
    fn read(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 60 {
            return None;
        }
        let record = Self {
            log2secsize: u32::from(bytes[0]),
            idlen: u32::from(bytes[4]),
            log2bpmb: u32::from(bytes[5]),
            nzones: u32::from(bytes[9]) | (u32::from(bytes[42]) << 8),
            zone_spare: le(&bytes[10..12]) as u32,
            root: le(&bytes[12..16]) as u32,
            disc_size: le(&bytes[16..20]) | (le(&bytes[36..40]) << 32),
            name: acorn_text(&bytes[22..32], false),
            log2sharesize: u32::from(bytes[40] & 0x0F),
            big_directories: bytes[41] & 1 != 0,
        };
        let sane = (8..=12).contains(&record.log2secsize)
            && bytes[1] > 0
            && bytes[2] > 0
            && (record.log2secsize + 3..=21).contains(&record.idlen)
            && record.log2bpmb <= 12
            && record.nzones > 0
            && record.zone_spare < (8 << record.log2secsize)
            && record.disc_size > 0
            && record.root >> 8 > 0;
        sane.then_some(record)
    }

    fn zone_bits(&self) -> u64 {
        (8u64 << self.log2secsize) - u64::from(self.zone_spare)
    }
}

/// A disc address's directory: the old 8-bit one (`Hugo`, 47 entries), RISC OS's new one (`Nick`, 77 entries).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DirKind {
    Old,
    New,
}

impl DirKind {
    fn size(self) -> usize {
        match self {
            DirKind::Old => 0x500,
            DirKind::New => 0x800,
        }
    }

    /// Where the entries stop and the tail (name, parent, title, sequence number and mark) starts.
    fn tail(self) -> usize {
        match self {
            DirKind::Old => 0x4CB,
            DirKind::New => 0x7D7,
        }
    }
}

/// Bits of the disc record's 60 bytes in zone 0, which the map doesn't use for the disc.
const RECORD_BITS: u64 = 60 * 8;

/// The new map: each zone's fragments, as (fragment id, first map bit's block, length in map bits).
struct Map {
    record: DiscRecord,
    zones: Vec<Vec<(u32, u64, u64)>>,
}

impl Map {
    fn read(source: &dyn Source, record: DiscRecord) -> Result<Self, String> {
        let secsize = 1u64 << record.log2secsize;
        let zone_bits = record.zone_bits();
        let nzones = u64::from(record.nzones);
        // The map sits in the middle of a disc with more than one zone, and at its start otherwise.
        let map_start = ((nzones / 2) * zone_bits - if nzones > 1 { RECORD_BITS } else { 0 }) << record.log2bpmb;
        let total_blocks = record.disc_size >> record.log2bpmb;
        let idlen = record.idlen as u64;
        let mut zones = Vec::new();
        for zone in 0..nzones {
            let bytes = source.read_vec(map_start + zone * secsize, secsize as usize).map_err(|_| format!("Zone {zone} of the map is past the end of the image."))?;
            // Zone 0 holds the disc record after its header, which the map's bits step over.
            let (start_bit, start_block) = if zone == 0 { (32 + RECORD_BITS, 0) } else { (32, zone * zone_bits - RECORD_BITS) };
            let end_bit = (32 + zone_bits).min(start_bit + total_blocks.saturating_sub(start_block)).min(secsize * 8);
            let bit = |n: u64| bytes[(n / 8) as usize] >> (n % 8) & 1;
            let field = |at: u64, width: u64| (0..width).fold(0u32, |v, i| v | (u32::from(bit(at + i)) << i));
            // The free list: bits 8 to 23 give the first free fragment's distance from bit 8, and each free fragment
            // its distance to the next. A free fragment's link isn't an id.
            let first_free = field(8, 15);
            let mut free_at = if first_free != 0 { 8 + u64::from(first_free) } else { 0 };
            let mut fragments = Vec::new();
            let mut at = start_bit;
            while at < end_bit {
                let id = field(at, idlen);
                let mut end = at + idlen;
                while end < end_bit && bit(end) == 0 {
                    end += 1;
                }
                if end >= end_bit {
                    break;
                }
                if at == free_at {
                    free_at += u64::from(id & 0x7FFF);
                } else {
                    fragments.push((id, start_block + (at - start_bit), end + 1 - at));
                }
                at = end + 1;
            }
            zones.push(fragments);
        }
        Ok(Self { record, zones })
    }

    /// The pieces of the disc an object is in, in order, as (byte offset, length).
    fn extents(&self, id: u32) -> Vec<(u64, u64)> {
        let ids_per_zone = (self.record.zone_bits() / (u64::from(self.record.idlen) + 1)).max(1);
        let count = self.zones.len();
        // The map and root directory (fragment 2) start in the middle zone; others in the zone their id names.
        let first = if id == 2 { count / 2 } else { (u64::from(id) / ids_per_zone) as usize % count };
        (0..count)
            .flat_map(|step| self.zones[(first + step) % count].iter())
            .filter(|(fragment, ..)| *fragment == id)
            .map(|(_, block, bits)| (block << self.record.log2bpmb, bits << self.record.log2bpmb))
            .collect()
    }
}

enum Space {
    /// Disc addresses in 256-byte sectors, each object in one piece.
    Old,
    New(Map),
}

pub struct Adfs {
    source: Arc<dyn Source>,
    space: Space,
    dirs: DirKind,
    root: u32,
    what: String,
    title: String,
}

/// Whether an image is an ADFS disc: an old-map root directory where the S, M, L and D formats keep it, or a new
/// map's disc record where E and F keep it.
pub fn looks_like_adfs(head: &[u8]) -> bool {
    let at = |offset: usize, bytes: &[u8]| head.get(offset..offset + bytes.len()) == Some(bytes);
    (at(0x201, b"Hugo") && at(0x6FB, b"Hugo")) || (at(0x401, b"Nick") && at(0xBFB, b"Nick")) || head.get(4..64).and_then(DiscRecord::read).is_some() || head.get(0xDC0..0xDC0 + 60).and_then(DiscRecord::read).is_some()
}

impl Adfs {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let head = source.read_vec(0, source.len().min(0x1000) as usize)?;
        let at = |offset: usize, bytes: &[u8]| head.get(offset..offset + bytes.len()) == Some(bytes);
        let len = source.len();
        if at(0x201, b"Hugo") {
            let sectors = le(&head[0xFC..0xFF]);
            let what = match sectors {
                640 => "ADFS S (160K)",
                1280 => "ADFS M (320K)",
                2560 => "ADFS L (640K)",
                _ => "ADFS (old map)",
            };
            return Self::finish(source, Space::Old, DirKind::Old, 2, what.into());
        }
        if at(0x401, b"Nick") {
            return Self::finish(source, Space::Old, DirKind::New, 4, "ADFS D (800K)".into());
        }
        // F format and hard discs keep the record in the boot block; E format only in the map's first zone.
        for place in [0xDC0, 4] {
            let Some(record) = head.get(place..place + 60).and_then(DiscRecord::read) else {
                continue;
            };
            if record.big_directories {
                return Err("This disc uses RISC OS 4's big directories (E+ or F+), which Limoniter can't read yet.".into());
            }
            let size = record.disc_size;
            let what = match (size, record.log2secsize) {
                (819_200, 10) => "ADFS E (800K)".to_string(),
                (1_638_400, 10) => "ADFS F (1.6MB)".to_string(),
                _ => format!("ADFS new map, {} sectors of {} bytes", size >> record.log2secsize, 1u32 << record.log2secsize),
            };
            if size > len.max(1) * 2 {
                return Err(format!("This disc's record says it holds {size} bytes, and the image is {len}."));
            }
            let root = record.root;
            let map = Map::read(&*source, record)?;
            return Self::finish(source, Space::New(map), DirKind::New, root, what);
        }
        Err("This isn't an ADFS disc: there's no root directory or disc record where ADFS keeps them.".into())
    }

    fn finish(source: Arc<dyn Source>, space: Space, dirs: DirKind, root: u32, what: String) -> Result<Self, String> {
        let mut adfs = Self { source, space, dirs, root, what, title: String::new() };
        let root_dir = adfs.object(root, dirs.size() as u64)?;
        let (_, title) = adfs.parse_dir(&root_dir)?;
        adfs.title = match &adfs.space {
            Space::New(map) if !map.record.name.is_empty() => map.record.name.clone(),
            _ => title,
        };
        Ok(adfs)
    }

    /// An object's first `length` bytes, from its disc address.
    fn object(&self, address: u32, length: u64) -> Result<Vec<u8>, String> {
        if length > MOST_FILE_BYTES {
            return Err("This file is larger than the 1 GB that can be opened.".into());
        }
        match &self.space {
            Space::Old => {
                let at = u64::from(address) * 256;
                if at + length > self.source.len() {
                    return Err("The file runs past the end of the image.".into());
                }
                self.source.read_vec(at, length as usize)
            }
            Space::New(map) => {
                let record = &map.record;
                // A small object can share a fragment with others: the low byte, less one, says how many sharing
                // units into it this one starts.
                let mut skip = match address & 0xFF {
                    0 => 0,
                    n => u64::from(n - 1) << (record.log2secsize + record.log2sharesize),
                };
                let mut out = Vec::with_capacity(length as usize);
                let extents = map.extents(address >> 8);
                if extents.is_empty() {
                    return Err(format!("Fragment {} isn't in the map, so the disc is damaged.", address >> 8));
                }
                for (start, size) in extents {
                    if out.len() as u64 >= length {
                        break;
                    }
                    if skip >= size {
                        skip -= size;
                        continue;
                    }
                    let take = (size - skip).min(length - out.len() as u64);
                    if start + skip + take > self.source.len() {
                        return Err("The file runs past the end of the image.".into());
                    }
                    out.extend(self.source.read_vec(start + skip, take as usize)?);
                    skip = 0;
                }
                if (out.len() as u64) < length {
                    return Err("The map gives this file less space than its length, so the disc is damaged.".into());
                }
                Ok(out)
            }
        }
    }

    /// A directory's entries and title, checked as ADFS checks one: its two marks and sequence numbers must agree.
    fn parse_dir(&self, bytes: &[u8]) -> Result<(Vec<Entry>, String), String> {
        let kind = self.dirs;
        let size = kind.size();
        let mark: &[u8] = if kind == DirKind::Old { b"Hugo" } else { b"Nick" };
        let (tail_mark, tail_sequence) = (&bytes[size - 5..size - 1], bytes[size - 6]);
        if &bytes[1..5] != mark || tail_mark != mark || bytes[0] != tail_sequence {
            return Err("This directory is broken: its start and end don't match, as ADFS would say.".into());
        }
        let title = match kind {
            DirKind::Old => acorn_text(&bytes[0x4D9..0x4EC], true),
            DirKind::New => acorn_text(&bytes[0x7DD..0x7F0], false),
        };
        let typed = kind == DirKind::New;
        let mut out = Vec::new();
        for at in (5..kind.tail()).step_by(26) {
            let raw = &bytes[at..at + 26];
            if raw[0] == 0 {
                break;
            }
            let (load, exec, length, address) = (le(&raw[10..14]) as u32, le(&raw[14..18]) as u32, le(&raw[18..22]), le(&raw[22..25]) as u32);
            // Old directories keep the attributes in the names' top bits; new ones in a byte of their own.
            let (name, attributes) = match kind {
                DirKind::Old => (acorn_text(&raw[..10], true), (0..7).fold(0u8, |a, i| a | ((raw[i] >> 7) << i))),
                DirKind::New => (acorn_text(&raw[..10], false), raw[25]),
            };
            let shown = tidy_name(&name.replace('/', "."));
            let is_dir = attributes & 8 != 0;
            let mut entry = if is_dir { Entry::dir(shown, u64::from(address), 0) } else { Entry::file(shown, length, u64::from(address)) };
            let access: String = [(0, 'R'), (1, 'W'), (2, 'L'), (4, 'E'), (5, 'r'), (6, 'w')].iter().filter(|(bit, _)| attributes >> bit & 1 != 0).map(|(_, c)| *c).collect();
            let (riscos_type, about) = addresses(load, exec, typed);
            if is_dir {
                entry.detail = access;
            } else {
                entry.kind = "File".into();
                entry.detail = if access.is_empty() { about } else { format!("{about} \u{B7} {access}") };
                match riscos_type {
                    Some(kind) => {
                        let (name, ending) = file_type(kind);
                        entry.kind = name;
                        if let Some(ending) = ending {
                            entry.open_name = format!("{}.{ending}", entry.name);
                        }
                    }
                    None => {
                        if length <= MOST_SNIFFED
                            && let Ok(bytes) = self.object(address, length)
                            && let Some((kind, ending)) = sniff(&bytes)
                        {
                            entry.kind = kind.into();
                            entry.open_name = format!("{}.{ending}", entry.name);
                        }
                    }
                }
            }
            out.push(entry);
        }
        out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok((out, title))
    }
}

impl Volume for Adfs {
    fn describe(&self) -> String {
        format!("Acorn {} \u{B7} {}", self.what, if self.title.is_empty() { "no title" } else { &self.title })
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        let address = dir.map_or(self.root, |d| d.location as u32);
        let bytes = self.object(address, self.dirs.size() as u64)?;
        Ok(self.parse_dir(&bytes)?.0)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        self.object(entry.location as u32, entry.size)
    }

    /// An 8-bit disc's text is the BBC Micro's. RISC OS's is Latin-1 with line feeds, which reading the bytes finds.
    fn text_preset(&self, _entry: &Entry) -> Option<&'static str> {
        (self.dirs == DirKind::Old).then_some(BBC_TEXT_PRESET)
    }
}

/// An `.adl` image in ADFS's order. The image keeps the disc's tracks as the drive meets them, each of side 0's
/// sixteen sectors then side 1's; ADFS numbers all of side 0's 1,280 sectors before side 1's.
pub struct AdlOrder {
    pub inner: Arc<dyn Source>,
}

impl AdlOrder {
    fn physical(sector: u64) -> u64 {
        let (side, track, within) = (sector / 1280, sector % 1280 / 16, sector % 16);
        (track * 2 + side) * 16 + within
    }
}

impl Source for AdlOrder {
    fn len(&self) -> u64 {
        self.inner.len()
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        let mut done = 0;
        while done < buf.len() {
            let at = offset + done as u64;
            let take = (256 - (at % 256) as usize).min(buf.len() - done);
            self.inner.read_at(Self::physical(at / 256) * 256 + at % 256, &mut buf[done..done + take])?;
            done += take;
        }
        Ok(())
    }
}

// ---------- UEF tapes ----------

struct TapeFile {
    name: String,
    load: u32,
    exec: u32,
    data: Vec<u8>,
    blocks: u32,
    locked: bool,
    problem: Option<String>,
}

pub struct Uef {
    files: Vec<TapeFile>,
    version: String,
    notes: Vec<String>,
}

/// The cassette filing system's CRC: CCITT's polynomial, from zero, high bit first.
fn tape_crc(bytes: &[u8]) -> u16 {
    bytes.iter().fold(0u16, |crc, b| (0..8).fold(crc ^ (u16::from(*b) << 8), |c, _| if c & 0x8000 != 0 { (c << 1) ^ 0x1021 } else { c << 1 }))
}

/// One block as the cassette filing system writes it, after its `*` sync byte: the name and a zero, then the load
/// and execution addresses, block number, data length, flag and next file's address (17 bytes), and the CRC of all
/// that, high byte first; then the data and its CRC.
struct Block {
    name: String,
    load: u32,
    exec: u32,
    number: u32,
    flag: u8,
    data: Vec<u8>,
    data_ok: bool,
}

/// A block starting at `at`, just after a `*`, and where the next one may start; None if the header's CRC is wrong.
fn read_block(stream: &[u8], at: usize) -> Option<(Block, usize)> {
    let name_end = at + stream.get(at..at + 11)?.iter().position(|b| *b == 0)?;
    let header = stream.get(at..name_end + 1 + 19)?;
    let fields = &header[name_end + 1 - at..];
    let crc = u16::from_be_bytes([fields[17], fields[18]]);
    if tape_crc(&header[..header.len() - 2]) != crc {
        return None;
    }
    let length = le(&fields[10..12]) as usize;
    let data_at = name_end + 1 + 19;
    let (data, data_ok, next) = if length == 0 {
        (Vec::new(), true, data_at)
    } else {
        let data = stream.get(data_at..data_at + length).map(<[u8]>::to_vec).unwrap_or_else(|| stream[data_at.min(stream.len())..].to_vec());
        let ok = stream.get(data_at + length..data_at + length + 2).is_some_and(|c| u16::from_be_bytes([c[0], c[1]]) == tape_crc(&data)) && data.len() == length;
        (data, ok, data_at + length + 2)
    };
    let block = Block { name: acorn_text(&stream[at..name_end], true), load: le(&fields[0..4]) as u32, exec: le(&fields[4..8]) as u32, number: le(&fields[8..10]) as u32, flag: fields[12], data, data_ok };
    Some((block, next))
}

impl Uef {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let mut bytes = source.read_vec(0, source.len().min(64 * 1024 * 1024) as usize)?;
        if bytes.starts_with(&[0x1F, 0x8B]) {
            bytes = super::amiga::gunzip(&bytes)?;
        }
        if !bytes.starts_with(b"UEF File!\0") || bytes.len() < 12 {
            return Err("This isn't a UEF tape image: it doesn't start with \"UEF File!\".".into());
        }
        let version = format!("{}.{}", bytes[11], bytes[10]);
        // The bytes the tape holds, from the chunks that hold them as bytes, run together: a block can be split
        // across chunks, and the carrier tones and gaps between chunks don't matter to the bytes.
        let mut stream = Vec::new();
        let mut unread = 0;
        let mut at = 12;
        while at + 6 <= bytes.len() {
            let id = le(&bytes[at..at + 2]);
            let length = le(&bytes[at + 2..at + 6]) as usize;
            let body = &bytes[at + 6..(at + 6 + length).min(bytes.len())];
            match id {
                0x0100 => stream.extend_from_slice(body),
                // Bytes with a stated format; eight data bits are ordinary bytes whatever the parity and stop bits.
                0x0104 if body.len() >= 3 && body[0] == 8 => stream.extend_from_slice(&body[3..]),
                0x0102 | 0x0104 => unread += 1,
                _ => {}
            }
            at += 6 + length;
        }
        let mut files: Vec<TapeFile> = Vec::new();
        let mut open = false;
        let mut at = 0;
        while at < stream.len() {
            if stream[at] != b'*' {
                at += 1;
                continue;
            }
            let Some((block, next)) = read_block(&stream, at + 1) else {
                at += 1;
                continue;
            };
            at = next;
            let continues = open && files.last().is_some_and(|f| f.name == block.name && f.blocks == block.number);
            if !continues {
                files.push(TapeFile { name: block.name.clone(), load: block.load, exec: block.exec, data: Vec::new(), blocks: block.number, locked: false, problem: None });
                if block.number != 0 {
                    files.last_mut().unwrap().problem = Some(format!("The tape starts this file at block {}: the blocks before it are missing.", block.number));
                }
            }
            let file = files.last_mut().unwrap();
            if !block.data_ok && file.problem.is_none() {
                file.problem = Some(format!("Block {}'s data doesn't match its CRC, so it was read wrongly from the tape.", block.number));
            }
            file.data.extend(block.data);
            file.blocks = block.number + 1;
            file.locked |= block.flag & 1 != 0;
            open = block.flag & 0x80 == 0;
        }
        if open && let Some(file) = files.last_mut() {
            file.problem.get_or_insert_with(|| "The tape ends before this file's last block.".into());
        }
        let mut notes = Vec::new();
        if unread > 0 {
            notes.push(format!("{unread} part(s) of the tape are recorded as bits or in an unusual format, and aren't read here."));
        }
        Ok(Self { files, version, notes })
    }
}

impl Volume for Uef {
    fn describe(&self) -> String {
        format!("Acorn tape (UEF {}) \u{B7} {} file(s)", self.version, self.files.len())
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        if dir.is_some() {
            return Ok(Vec::new());
        }
        Ok(self
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                let name = if file.name.is_empty() { "(no name)".to_string() } else { tidy_name(&file.name) };
                let mut entry = Entry::file(name, file.data.len() as u64, index as u64);
                entry.kind = "File".into();
                entry.detail = format!("Load {} \u{B7} Exec {} \u{B7} {} block(s){}", hex32(file.load), hex32(file.exec), file.blocks, if file.locked { " \u{B7} locked" } else { "" });
                if let Some(problem) = &file.problem {
                    entry.kind = "Damaged".into();
                    entry.detail = format!("{} \u{B7} {problem}", entry.detail);
                } else if let Some((kind, ending)) = sniff(&file.data) {
                    entry.kind = kind.into();
                    entry.open_name = format!("{}.{ending}", entry.name);
                }
                entry
            })
            .collect())
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let file = self.files.get(entry.location as usize).ok_or("That file isn't on the tape.")?;
        match &file.problem {
            Some(problem) => Err(problem.clone()),
            None => Ok(file.data.clone()),
        }
    }

    fn notes(&self) -> Vec<String> {
        self.notes.clone()
    }

    fn text_preset(&self, _entry: &Entry) -> Option<&'static str> {
        Some(BBC_TEXT_PRESET)
    }
}

#[cfg(test)]
pub mod tests {
    use super::super::{is_image, open};
    use super::*;
    use std::path::Path;

    /// A tokenised BBC BASIC program: `10 PRINT"HI"`, `20 GOTO 10`.
    pub fn bbc_program() -> Vec<u8> {
        let mut out = vec![0x0D, 0, 10, 9, 0xF1, b'"', b'H', b'I', b'"'];
        out.extend([0x0D, 0, 20, 9, 0xE5, 0x8D, 0x54, 0x4A, 0x40]);
        out.extend([0x0D, 0xFF]);
        out
    }

    /// A file for a DFS side: directory, name, load and execution addresses, bytes, and whether it's locked.
    pub type DfsTestFile<'a> = (char, &'a str, u32, u32, Vec<u8>, bool);

    /// One DFS side's catalogue and files. Files past 31 go in Watford DFS's second catalogue.
    pub fn dfs_side(title: &str, files: &[DfsTestFile]) -> Vec<u8> {
        let mut disk = vec![0u8; 800 * 256];
        let watford = files.len() > 31;
        let mut sector = if watford { 4 } else { 2 };
        let title = format!("{title:\0<12}");
        disk[..8].copy_from_slice(&title.as_bytes()[..8]);
        disk[256..260].copy_from_slice(&title.as_bytes()[8..12]);
        disk[256 + 6] = 0x30 | 3; // EXEC boot, 800 sectors
        disk[256 + 7] = 0x20;
        if watford {
            disk[512..520].fill(0xAA);
        }
        for (index, (folder, name, load, exec, data, locked)) in files.iter().enumerate() {
            let (names, info, slot) = if index < 31 { (0, 256, index) } else { (512, 768, index - 31) };
            let at = 8 + slot * 8;
            let padded = format!("{name:<7}");
            disk[names + at..names + at + 7].copy_from_slice(padded.as_bytes());
            disk[names + at + 7] = *folder as u8 | if *locked { 0x80 } else { 0 };
            let length = data.len() as u32;
            disk[info + at..info + at + 2].copy_from_slice(&(*load as u16).to_le_bytes());
            disk[info + at + 2..info + at + 4].copy_from_slice(&(*exec as u16).to_le_bytes());
            disk[info + at + 4..info + at + 6].copy_from_slice(&(length as u16).to_le_bytes());
            disk[info + at + 6] = (((exec >> 16) & 3) << 6 | ((length >> 16) & 3) << 4 | ((load >> 16) & 3) << 2 | (sector >> 8) & 3) as u8;
            disk[info + at + 7] = sector as u8;
            disk[sector as usize * 256..sector as usize * 256 + data.len()].copy_from_slice(data);
            sector += length.div_ceil(256).max(1);
            disk[info + 5] = (slot as u8 + 1) * 8;
        }
        disk
    }

    /// Two sides as a `.dsd` holds them: track by track, side 0's ten sectors then side 1's.
    pub fn dsd(first: &[u8], second: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for track in 0..80 {
            for side in [first, second] {
                out.extend_from_slice(&side[track * 2560..track * 2560 + 2560]);
            }
        }
        out
    }

    /// A directory: old (`Hugo`) or new (`Nick`), its entries as (name, load, exec, length, address, attributes).
    fn adfs_dir(kind: DirKind, title: &str, parent: u32, entries: &[(&str, u32, u32, u32, u32, u8)]) -> Vec<u8> {
        let size = kind.size();
        let mark: &[u8] = if kind == DirKind::Old { b"Hugo" } else { b"Nick" };
        let mut dir = vec![0u8; size];
        dir[0] = 1;
        dir[1..5].copy_from_slice(mark);
        for (index, (name, load, exec, length, address, attributes)) in entries.iter().enumerate() {
            let at = 5 + index * 26;
            let mut raw = format!("{name}\r").into_bytes();
            raw.resize(10, 0);
            if kind == DirKind::Old {
                for (bit, byte) in raw.iter_mut().enumerate().take(7) {
                    *byte |= ((attributes >> bit) & 1) << 7;
                }
            } else {
                dir[at + 25] = *attributes;
            }
            dir[at..at + 10].copy_from_slice(&raw);
            dir[at + 10..at + 14].copy_from_slice(&load.to_le_bytes());
            dir[at + 14..at + 18].copy_from_slice(&exec.to_le_bytes());
            dir[at + 18..at + 22].copy_from_slice(&length.to_le_bytes());
            dir[at + 22..at + 25].copy_from_slice(&address.to_le_bytes()[..3]);
        }
        let (parent_at, title_at) = if kind == DirKind::Old { (0x4D6, 0x4D9) } else { (0x7DA, 0x7DD) };
        dir[parent_at..parent_at + 3].copy_from_slice(&parent.to_le_bytes()[..3]);
        dir[title_at..title_at + title.len()].copy_from_slice(title.as_bytes());
        dir[size - 6] = 1;
        dir[size - 5..size - 1].copy_from_slice(mark);
        dir
    }

    /// An 8-bit ADFS L disc (640K) in ADFS's own sector order: a text file, a BASIC program, and a folder whose
    /// file is on side 1.
    pub fn adfs_l() -> Vec<u8> {
        let mut disc = vec![0u8; 2560 * 256];
        disc[0xFC..0xFF].copy_from_slice(&2560u32.to_le_bytes()[..3]);
        let program = bbc_program();
        let root = adfs_dir(DirKind::Old, "Test disc", 2, &[("HELLO", 0xFFFF_1900, 0xFFFF_8023, 13, 7, 0b011), ("PROG", 0xFFFF_1900, 0xFFFF_8023, program.len() as u32, 8, 0b111), ("SUB", 0, 0, 0x500, 9, 0b1011)]);
        disc[0x200..0x700].copy_from_slice(&root);
        disc[7 * 256..7 * 256 + 13].copy_from_slice(b"Hello, Acorn\r");
        disc[8 * 256..8 * 256 + program.len()].copy_from_slice(&program);
        let sub = adfs_dir(DirKind::Old, "Sub", 2, &[("INNER", 0, 0, 300, 1300, 0b011)]);
        disc[9 * 256..14 * 256].copy_from_slice(&sub);
        for (i, b) in disc[1300 * 256..1300 * 256 + 300].iter_mut().enumerate() {
            *b = (i % 7) as u8;
        }
        disc
    }

    /// The same disc as an `.adl` image keeps it: each track's side 0 sectors, then side 1's.
    pub fn as_adl(logical: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; logical.len()];
        for sector in 0..logical.len() as u64 / 256 {
            let to = AdlOrder::physical(sector) as usize * 256;
            out[to..to + 256].copy_from_slice(&logical[sector as usize * 256..sector as usize * 256 + 256]);
        }
        out
    }

    /// An E format disc (800K, new map, one zone): the root, a text file, a folder holding two small files that share
    /// a fragment, and a file in two fragments either side of another file's. The rest is on the free list.
    pub fn adfs_e() -> Vec<u8> {
        let mut disc = vec![0u8; 819_200];
        let record: [u8; 60] = {
            let mut r = [0u8; 60];
            r[..12].copy_from_slice(&[10, 5, 2, 2, 15, 7, 1, 0, 0, 1, 0x20, 0x05]);
            r[12..16].copy_from_slice(&0x203u32.to_le_bytes());
            r[16..20].copy_from_slice(&819_200u32.to_le_bytes());
            r[22..31].copy_from_slice(b"TestDisc\0");
            r
        };
        disc[4..64].copy_from_slice(&record);
        // The map's bits, from bit 512 of the zone, a bit for each 128 bytes of the disc.
        let mut bit = 512u64;
        let mut fragment = |disc: &mut Vec<u8>, id: u32, bits: u64| -> u64 {
            let start = bit;
            for i in 0..15 {
                if id >> i & 1 != 0 {
                    disc[((start + i) / 8) as usize] |= 1 << ((start + i) % 8);
                }
            }
            let end = start + bits - 1;
            disc[(end / 8) as usize] |= 1 << (end % 8);
            bit += bits;
            (start - 512) * 128
        };
        fragment(&mut disc, 2, 32); // the map and root, 4K
        let hello = fragment(&mut disc, 3, 16);
        let docs = fragment(&mut disc, 4, 16);
        let shared = fragment(&mut disc, 5, 16);
        let split_a = fragment(&mut disc, 6, 16);
        let other = fragment(&mut disc, 7, 16);
        let split_b = fragment(&mut disc, 6, 16);
        // The rest is free: the header's link (bits 8 to 22) says how far from bit 8 it starts; its own link is 0.
        let free_start = bit;
        let link = (free_start - 8) as u32;
        disc[1] = link as u8;
        disc[2] = (link >> 8) as u8 & 0x7F;
        let end = 32 + 6880 - 1;
        disc[(end / 8) as usize] |= 1 << (end % 8);
        let root = adfs_dir(DirKind::New, "TestDisc", 0x203, &[("Hello/txt", 0xFFFF_FF34, 0x5678_9ABC, 14, 0x301, 0b0011), ("Docs", 0, 0, 0x800, 0x401, 0b1011), ("Big", 0xFFFF_FD00, 0, 3000, 0x601, 0b0011), ("Other", 0xFFFF_FD00, 0, 5, 0x701, 0b0011)]);
        disc[0x800..0x1000].copy_from_slice(&root);
        disc[hello as usize..hello as usize + 14].copy_from_slice(b"Hello, RISC\nOS");
        let program = bbc_program();
        let sub = adfs_dir(DirKind::New, "Docs", 0x203, &[("A", 0xFFFF_FF00, 0, 4, 0x501, 0b0011), ("Prog", 0xFFFF_FB00, 0, program.len() as u32, 0x502, 0b0011)]);
        disc[docs as usize..docs as usize + 0x800].copy_from_slice(&sub);
        disc[shared as usize..shared as usize + 4].copy_from_slice(b"AAAA");
        disc[shared as usize + 1024..shared as usize + 1024 + program.len()].copy_from_slice(&program);
        for (i, b) in disc[split_a as usize..split_a as usize + 2048].iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        for (i, b) in disc[split_b as usize..split_b as usize + 952].iter_mut().enumerate() {
            *b = ((i + 2048) % 251) as u8;
        }
        disc[other as usize..other as usize + 5].copy_from_slice(b"other");
        disc
    }

    /// A block as the cassette filing system writes it, with its sync byte and CRCs; `spoil` breaks the data's CRC.
    pub fn tape_block(name: &str, load: u32, exec: u32, number: u16, data: &[u8], last: bool, spoil: bool) -> Vec<u8> {
        let mut header = name.as_bytes().to_vec();
        header.push(0);
        header.extend(load.to_le_bytes());
        header.extend(exec.to_le_bytes());
        header.extend(number.to_le_bytes());
        header.extend((data.len() as u16).to_le_bytes());
        header.push(if last { 0x80 } else { 0 });
        header.extend([0; 4]);
        let mut out = vec![b'*'];
        out.extend(&header);
        out.extend(tape_crc(&header).to_be_bytes());
        if !data.is_empty() {
            out.extend(data);
            out.extend((tape_crc(data) ^ if spoil { 1 } else { 0 }).to_be_bytes());
        }
        out
    }

    /// A UEF holding these blocks, each in a chunk &0100 of its own with a carrier tone before it.
    pub fn uef(blocks: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"UEF File!\0\x0A\x00".to_vec();
        for block in blocks {
            out.extend([0x10, 0x01, 2, 0, 0, 0, 0xDC, 0x05]);
            out.extend([0x00, 0x01]);
            out.extend((block.len() as u32).to_le_bytes());
            out.extend(block);
        }
        out
    }

    /// A tape with a BASIC program in two blocks, a file whose second block was read wrongly, and one cut short.
    pub fn tape() -> Vec<u8> {
        let program = bbc_program();
        let long: Vec<u8> = (0..300u32).map(|i| (i % 200) as u8).collect();
        uef(&[
            tape_block("PROG", 0xFFFF_1900, 0xFFFF_8023, 0, &program, true, false),
            tape_block("DATA", 0x3000, 0x3000, 0, &long[..256], false, false),
            tape_block("DATA", 0x3000, 0x3000, 1, &long[256..], true, true),
            tape_block("CUT", 0x3000, 0x3000, 0, &long[..256], false, false),
        ])
    }

    fn names(entries: &[Entry]) -> Vec<String> {
        entries.iter().map(|e| e.name.clone()).collect()
    }

    fn find<'a>(entries: &'a [Entry], name: &str) -> &'a Entry {
        entries.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("{name} in {:?}", names(entries)))
    }

    #[test]
    fn dfs_disks_list_their_catalogues_and_read_their_files() {
        let program = bbc_program();
        let side = dfs_side("GAMES", &[('T', "NOTES", 0, 0, b"Some notes\r".to_vec(), false), ('$', "!BOOT", 0, 0, b"CHAIN\"MENU\"\r".to_vec(), true), ('$', "MENU", 0x3_1900, 0x3_8023, program.clone(), false)]);
        let volume = open(Arc::new(side.clone()), "games.ssd").unwrap();
        assert_eq!(volume.describe(), "Acorn DFS disk, single-sided (SSD), 80 tracks \u{B7} GAMES \u{B7} boot EXEC");
        let files = volume.list(None).unwrap();
        assert_eq!(names(&files), ["$.!BOOT", "$.MENU", "T.NOTES"], "the $ directory first, then the others");
        let menu = find(&files, "$.MENU");
        assert_eq!((menu.kind.as_str(), menu.open_name.as_str()), ("BBC BASIC program", "$.MENU.bbc"));
        assert_eq!(menu.detail, "Load FF1900 \u{B7} Exec FF8023", "an I/O processor address is shown as *INFO shows it");
        assert_eq!(volume.read(menu).unwrap(), program);
        assert!(find(&files, "$.!BOOT").detail.ends_with("locked"));
        assert_eq!(volume.read(find(&files, "T.NOTES")).unwrap(), b"Some notes\r");
        assert_eq!(volume.text_preset(menu), Some(BBC_TEXT_PRESET));
        // Two sides, interleaved track by track, are drives 0 and 2.
        let other = dfs_side("SIDE2", &[('$', "DATA", 0x1900, 0x1900, vec![7u8; 3000], false)]);
        let double = open(Arc::new(dsd(&side, &other)), "both.dsd").unwrap();
        let sides = double.list(None).unwrap();
        assert_eq!(names(&sides), ["Side 1", "Side 2"]);
        assert_eq!(sides[1].kind, "Drive 2");
        let data = double.list(Some(&sides[1])).unwrap();
        assert_eq!(double.read(&data[0]).unwrap(), vec![7u8; 3000], "a file across tracks reads in order");
        assert_eq!(double.read(find(&double.list(Some(&sides[0])).unwrap(), "$.MENU")).unwrap(), program);
        // A blank second side is said to be blank, not a catalogue to complain about.
        let blank = open(Arc::new(dsd(&side, &vec![0u8; side.len()])), "one.dsd").unwrap();
        assert_eq!(names(&blank.list(None).unwrap()), ["Side 1"]);
        assert_eq!(blank.notes(), ["Side 2 is blank."]);
    }

    #[test]
    fn watford_dfs_keeps_files_past_31_in_its_second_catalogue() {
        let files: Vec<_> = (0..40).map(|i| ('$', format!("F{i:02}"), 0, 0, vec![i as u8; 10], false)).collect();
        let files: Vec<_> = files.iter().map(|(d, n, l, e, b, k)| (*d, n.as_str(), *l, *e, b.clone(), *k)).collect();
        let volume = open(Arc::new(dfs_side("WATFORD", &files)), "w.ssd").unwrap();
        assert!(volume.describe().starts_with("Watford DFS disk"));
        let listed = volume.list(None).unwrap();
        assert_eq!(listed.len(), 40);
        assert_eq!(volume.read(find(&listed, "$.F39")).unwrap(), vec![39u8; 10]);
    }

    #[test]
    fn a_dfs_disk_kept_as_img_is_only_believed_when_its_catalogue_is_whole() {
        let side = dfs_side("FORTH", &[('$', "WORDS", 0, 0, b"hello\r".to_vec(), false)]);
        assert!(is_image("forth.img", &side[..0x8800], side.len() as u64));
        assert!(open(Arc::new(side), "forth.img").unwrap().describe().starts_with("Acorn DFS"));
        assert!(!is_image("blank.img", &[0u8; 0x8800], 204_800), "an empty catalogue says nothing");
        let mut odd = vec![0u8; 0x8800];
        odd[256 + 5] = 3;
        assert!(!looks_like_dfs(&odd, false), "a file count that isn't whole entries");
    }

    #[test]
    fn eight_bit_adfs_reads_old_directories_and_adl_images_in_adfs_order() {
        let logical = adfs_l();
        for (name, bytes) in [("disc.adf", logical.clone()), ("disc.adl", as_adl(&logical))] {
            let volume = open(Arc::new(bytes), name).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(volume.describe(), "Acorn ADFS L (640K) \u{B7} Test disc", "{name}");
            let top = volume.list(None).unwrap();
            assert_eq!(names(&top), ["SUB", "HELLO", "PROG"], "{name}");
            assert_eq!(volume.read(find(&top, "HELLO")).unwrap(), b"Hello, Acorn\r");
            let prog = find(&top, "PROG");
            assert_eq!((prog.open_name.as_str(), prog.detail.as_str()), ("PROG.bbc", "Load FFFF1900 \u{B7} Exec FFFF8023 \u{B7} RWL"), "{name}");
            let inner = volume.list(Some(find(&top, "SUB"))).unwrap();
            let bytes = volume.read(&inner[0]).unwrap();
            assert!(bytes.len() == 300 && bytes.iter().enumerate().all(|(i, b)| *b == (i % 7) as u8), "{name}: a file on side 1 reads whole");
            assert_eq!(volume.text_preset(&inner[0]), Some(BBC_TEXT_PRESET));
        }
    }

    #[test]
    fn riscos_new_map_discs_follow_their_fragments() {
        let volume = open(Arc::new(adfs_e()), "risc.adf").unwrap();
        assert_eq!(volume.describe(), "Acorn ADFS E (800K) \u{B7} TestDisc");
        let top = volume.list(None).unwrap();
        assert_eq!(names(&top), ["Docs", "Big", "Hello.txt", "Other"], "a RISC OS name's / is its ending's dot");
        let hello = find(&top, "Hello.txt");
        assert_eq!((hello.kind.as_str(), hello.open_name.as_str()), ("Text", "Hello.txt.txt"));
        assert_eq!(hello.detail, "1971-03-27 06:00:39 \u{B7} RW", "the date is from the load and execution addresses, in centiseconds from 1900");
        assert_eq!(volume.read(hello).unwrap(), b"Hello, RISC\nOS");
        assert_eq!(volume.text_preset(hello), None, "RISC OS text is Latin-1, which the bytes show");
        // A file in two fragments, with another file's between them.
        let big = volume.read(find(&top, "Big")).unwrap();
        assert!(big.len() == 3000 && big.iter().enumerate().all(|(i, b)| *b == (i % 251) as u8));
        assert_eq!(volume.read(find(&top, "Other")).unwrap(), b"other");
        // Two small files in one fragment, the second a sharing unit in.
        let docs = volume.list(Some(find(&top, "Docs"))).unwrap();
        assert_eq!(volume.read(find(&docs, "A")).unwrap(), b"AAAA");
        let prog = find(&docs, "Prog");
        assert_eq!((prog.kind.as_str(), prog.open_name.as_str()), ("BASIC", "Prog.bbc"));
        assert_eq!(volume.read(prog).unwrap(), bbc_program());
        // The same disc with a DOS boot block is the Amiga's, and an Amiga disc stays the Amiga's.
        assert!(open(Arc::new(super::super::tests::fixture("ofs.adf")), "ofs.adf").unwrap().describe().starts_with("Amiga"));
    }

    #[test]
    fn uef_tapes_join_blocks_into_files_and_say_which_were_read_wrongly() {
        let volume = open(Arc::new(tape()), "tape.uef").unwrap();
        assert_eq!(volume.describe(), "Acorn tape (UEF 0.10) \u{B7} 3 file(s)");
        let files = volume.list(None).unwrap();
        assert_eq!(names(&files), ["PROG", "DATA", "CUT"]);
        assert_eq!(files[0].open_name, "PROG.bbc");
        assert_eq!(volume.read(&files[0]).unwrap(), bbc_program());
        assert_eq!(files[1].kind, "Damaged");
        assert!(volume.read(&files[1]).unwrap_err().contains("Block 1"));
        assert!(volume.read(&files[2]).unwrap_err().contains("ends before"));
        // Gzipped, as UEF files often are.
        let raw = tape();
        let packed = miniz_oxide::deflate::compress_to_vec(&raw, 6);
        let mut gz = vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 0];
        gz.extend(packed);
        gz.extend([0; 4]);
        gz.extend((raw.len() as u32).to_le_bytes());
        assert_eq!(names(&open(Arc::new(gz), "tape.uef").unwrap().list(None).unwrap()), ["PROG", "DATA", "CUT"]);
    }

    #[test]
    fn damaged_acorn_images_give_errors_never_panics() {
        let program = bbc_program();
        let images = [
            ("d.ssd", dfs_side("X", &[('$', "A", 0, 0, program.clone(), false), ('$', "B", 0, 0, vec![1; 3000], false)])),
            ("d.adl", as_adl(&adfs_l())),
            ("d.adf", adfs_e()),
            ("d.uef", tape()),
        ];
        for (name, image) in images {
            for seed in 0..60u32 {
                let mut bytes = image.clone();
                let mut state = seed.wrapping_mul(2_654_435_761);
                let span = bytes.len().min(16 * 1024);
                for _ in 0..48 {
                    state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    bytes[state as usize % span] = (state >> 16) as u8;
                }
                if let Ok(volume) = open(Arc::new(bytes), name) {
                    let _ = volume.describe();
                    for entry in volume.list(None).unwrap_or_default().iter().take(40) {
                        let _ = if entry.is_dir { volume.list(Some(entry)).map(|_| Vec::new()) } else { volume.read(entry) };
                    }
                }
            }
        }
    }

    /// Whether a file's bytes have what its RISC OS file type says they start with, so a file put together from the
    /// wrong fragments is caught. None for a type with nothing to check.
    fn signature_holds(kind: &str, data: &[u8]) -> Option<bool> {
        let word = |at: usize| data.get(at..at + 4).map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]) as usize);
        Some(match kind {
            "BASIC" => basic_more::looks_like_bbc(data),
            "Squash" => data.starts_with(b"SQSH"),
            "DrawFile" => data.starts_with(b"Draw"),
            "Archive (Spark)" => data.first() == Some(&0x1A),
            "ArcFS" => data.starts_with(b"Archive\0"),
            "JPEG" => data.starts_with(&[0xFF, 0xD8]),
            "Text" | "Obey" | "TaskObey" => !data.contains(&0),
            // A sprite file: the count, then the first sprite's offset (16, counting the area size word the file
            // leaves out), then the free space's, which is within the file.
            "Sprite" => word(4) == Some(16) && word(8).is_some_and(|free| free <= data.len() + 4),
            // A module's title is a string within it, after its header.
            "Module" => word(16).is_some_and(|title| title >= 0x14 && title < data.len() && data[title..].iter().take_while(|b| **b != 0).all(|b| (0x20..0x7F).contains(b))),
            _ => return None,
        })
    }

    fn walk(volume: &dyn Volume, dir: Option<&Entry>, depth: usize, checked: &mut usize, wrong: &mut Vec<String>) {
        if depth > 10 {
            return;
        }
        for entry in volume.list(dir).unwrap() {
            if entry.is_dir {
                walk(volume, Some(&entry), depth + 1, checked, wrong);
            } else if let Some(holds) = signature_holds(&entry.kind, &volume.read(&entry).unwrap()) {
                *checked += 1;
                if !holds {
                    wrong.push(format!("{} ({})", entry.name, entry.kind));
                }
            }
        }
    }

    fn list_programs(volume: &dyn Volume, dir: Option<&Entry>, place: &str, out: &mut String, depth: usize) {
        if depth > 8 {
            return;
        }
        for entry in volume.list(dir).unwrap_or_default() {
            let path = format!("{place}/{}", entry.name);
            if entry.is_dir {
                list_programs(volume, Some(&entry), &path, out, depth + 1);
                continue;
            }
            let Ok(bytes) = volume.read(&entry) else {
                continue;
            };
            if entry.open_name.ends_with(".bbc") {
                out.push_str(&format!("# {path}\n"));
                for line in crate::basic::list(&bytes, "bbc", crate::charset::utf8()).lines {
                    out.push_str(&line);
                    out.push('\n');
                }
            } else if super::super::is_nested_image(&entry.open_name, &bytes)
                && let Ok(inner) = open(Arc::new(bytes), &entry.open_name)
            {
                list_programs(&*inner, None, &format!("{path}!"), out, depth + 1);
            }
        }
    }

    /// Every BBC BASIC program on the real BBC disks and tapes in ..\testmedia, listed as the preview lists it, to
    /// compare with another detokeniser's listings.
    #[test]
    fn real_bbc_programs_are_listed() {
        let root = Path::new("../testmedia");
        let Ok(folders) = std::fs::read_dir(root) else {
            return;
        };
        let mut images: Vec<_> = folders.flatten().map(|f| f.path()).filter(|p| p.is_dir() && !p.file_name().unwrap().to_string_lossy().starts_with("limoniter-")).flat_map(|folder| std::fs::read_dir(folder).unwrap().flatten().map(|f| f.path())).filter(|p| p.is_file()).collect();
        images.sort();
        let mut out = String::new();
        for path in images {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let Ok(volume) = open(Arc::new(std::fs::read(&path).unwrap()), &name) else {
                continue;
            };
            list_programs(&*volume, None, &name, &mut out, 0);
        }
        std::fs::write(root.join("limoniter-bbc-listings.txt"), out).unwrap();
    }

    /// The real RISC OS discs in ..\testmedia, when it's there: every typed file's bytes start as its type says.
    #[test]
    fn real_riscos_discs_files_match_their_types() {
        let root = Path::new("../testmedia");
        let Ok(folders) = std::fs::read_dir(root) else {
            return;
        };
        let mut checked = 0;
        let mut wrong = Vec::new();
        for folder in folders.flatten().map(|f| f.path()).filter(|p| p.is_dir()) {
            for file in std::fs::read_dir(&folder).unwrap().flatten().map(|f| f.path()).filter(|p| p.is_file()) {
                let name = file.file_name().unwrap().to_string_lossy().into_owned();
                if !name.to_lowercase().ends_with(".adf") {
                    continue;
                }
                if let Ok(volume) = open(Arc::new(std::fs::read(&file).unwrap()), &name)
                    && volume.describe().starts_with("Acorn ADFS")
                {
                    walk(&*volume, None, 0, &mut checked, &mut wrong);
                }
            }
        }
        assert!(wrong.is_empty(), "{checked} checked: {wrong:#?}");
        eprintln!("{checked} typed files on RISC OS discs start as their types say");
    }
}
