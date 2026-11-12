//! CP/M's file system, on any disk that holds it: Amstrad CPC, +3 and PCW disks (DSK images), and the many other
//! machines' disks kept as ImageDisk, Teledisk or plain sector images.
//!
//! CP/M keeps no description of its disk on the disk: the machine's BIOS knew it. So a disk's layout comes from
//! the table of formats in `cpm_formats.rs` (cpmtools' collection), and which one a disk is in is worked out by
//! reading its directory each way that fits and keeping the one that makes the most sense of it.
//!
//! The layout: the first tracks are the system's own; then blocks, the first of them the directory. Each
//! 32-byte directory entry is a user number (0xE5 for an unused one), an 8.3 name with attributes in the
//! characters' top bits (read-only, system, archived), an extent number and record count saying where in the file
//! it is and how much of it there is, and the blocks it uses (bytes if the disk has 256 blocks or fewer, words if
//! more). A file larger than an entry can hold takes several. A sector of a track is found through its skew table,
//! the order the machine wrote them in.
//!
//! CP/M 3 adds a disk label (an entry with user 32), date stamps (every fourth entry, user 33, holding the
//! dates of the three before it) and passwords (entries with users 16 to 31). Days count from 1 January 1978.

use super::{Entry, Source, Volume, tidy_name};
use std::collections::HashSet;
use std::sync::Arc;

/// How a format orders a track's sectors.
#[derive(Clone, Copy, Debug)]
pub enum Skew {
    /// Each sector this many places on from the one before, as cpmtools makes a table from a skew factor.
    Every(u8),
    /// The physical place of each sector, in CP/M's order.
    Table(&'static [u8]),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Cpm22,
    Cpm3,
    /// P2DOS and ZSDOS: CP/M 2.2's layout, with users up to 31.
    P2dos,
}

/// A format, as cpmtools' diskdefs describe it.
#[derive(Clone, Copy, Debug)]
pub struct DiskDef {
    pub name: &'static str,
    pub described: &'static str,
    pub sector_size: u64,
    /// Tracks counted over both sides, as CP/M counts them.
    pub tracks: u64,
    pub sectors_per_track: u64,
    pub block_size: u64,
    pub directory_entries: u64,
    pub skew: Skew,
    pub boot_tracks: u64,
    pub os: Os,
}

/// A disk's CP/M layout, from the table or from what the disk itself says.
#[derive(Clone, Debug)]
pub struct Layout {
    /// cpmtools' name for the format, when it's from the table.
    pub name: String,
    pub described: String,
    pub sector_size: u64,
    pub sectors_per_track: u64,
    pub tracks: u64,
    pub block_size: u64,
    pub directory_entries: u64,
    pub boot_tracks: u64,
    /// The place on the track of each of CP/M's sectors in turn.
    pub skew: Vec<u64>,
    pub os: Os,
}

impl Layout {
    pub fn from_def(def: &DiskDef) -> Self {
        let skew = match def.skew {
            Skew::Table(table) => table.iter().map(|&s| u64::from(s)).collect(),
            Skew::Every(step) => skew_table(def.sectors_per_track, u64::from(step)),
        };
        Self {
            name: def.name.to_string(),
            described: def.described.to_string(),
            sector_size: def.sector_size,
            sectors_per_track: def.sectors_per_track,
            tracks: def.tracks,
            block_size: def.block_size,
            directory_entries: def.directory_entries,
            boot_tracks: def.boot_tracks,
            skew,
            os: def.os,
        }
    }

    fn blocks(&self) -> u64 {
        self.tracks.saturating_sub(self.boot_tracks) * self.sectors_per_track * self.sector_size / self.block_size
    }

    /// Block numbers are words when there are more than 256 blocks.
    fn wide(&self) -> bool {
        self.blocks() > 256
    }

    /// How many 16K logical extents one directory entry covers, less one.
    fn extent_mask(&self) -> u64 {
        let pointers = if self.wide() { 8 } else { 16 };
        (pointers * self.block_size / 16384).saturating_sub(1)
    }

    fn directory_blocks(&self) -> u64 {
        (self.directory_entries * 32).div_ceil(self.block_size)
    }

    /// The bytes from the start of the disk the directory reaches, to know whether an image's start holds it.
    fn directory_end(&self) -> u64 {
        self.boot_tracks * self.sectors_per_track * self.sector_size + self.directory_blocks() * self.block_size
    }
}

/// cpmtools' table from a skew factor: each sector `step` on from the last, or the next free place after it.
fn skew_table(per_track: u64, step: u64) -> Vec<u64> {
    if step <= 1 {
        return (0..per_track).collect();
    }
    let mut used = vec![false; per_track as usize];
    let mut table = Vec::with_capacity(per_track as usize);
    let mut at = 0;
    for _ in 0..per_track {
        while used[at as usize] {
            at = (at + 1) % per_track;
        }
        table.push(at);
        used[at as usize] = true;
        at = (at + step) % per_track;
    }
    table
}

/// Where a disk's sectors are.
pub trait Sectors: Send + Sync {
    /// The sector at this place on a CP/M track (tracks counted over both sides), counting from 0 along the track.
    fn sector(&self, track: u64, place: u64) -> Result<Vec<u8>, String>;
}

/// A plain image: the tracks one after another, a cylinder's sides in turn, which is how ImageDisk and Teledisk
/// images are laid out here and how cpmtools reads a plain image.
pub struct Flat {
    source: Arc<dyn Source>,
    sector_size: u64,
    per_track: u64,
}

impl Sectors for Flat {
    fn sector(&self, track: u64, place: u64) -> Result<Vec<u8>, String> {
        let at = (track * self.per_track + place) * self.sector_size;
        if at + self.sector_size > self.source.len() {
            return Err(format!("track {track} isn't in the image"));
        }
        self.source.read_vec(at, self.sector_size as usize)
    }
}

/// When a CP/M 3 file was made (or last read, as the disk's label says), and last changed.
#[derive(Clone, Copy, Default)]
struct Stamps {
    first: u32,
    updated: u32,
}

struct File {
    user: u8,
    name: String,
    attributes: String,
    blocks: Vec<u64>,
    records: u64,
    stamps: Stamps,
    password: bool,
}

/// What reading a directory one way made of it, to judge which way is right.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
struct Judgement {
    files: u32,
    /// Entries whose blocks are as many as their records need.
    consistent: u32,
    bad: u32,
    shared_blocks: u32,
}

impl Judgement {
    fn score(&self) -> i64 {
        i64::from(self.consistent) * 3 + i64::from(self.files) - i64::from(self.bad) * 12 - i64::from(self.shared_blocks) * 6
    }

    fn believable(&self) -> bool {
        self.files > 0 && self.bad * 8 <= self.files && self.consistent * 2 >= self.files
    }
}

fn name_ok(bytes: &[u8]) -> bool {
    let chars: Vec<u8> = bytes.iter().map(|b| b & 0x7F).collect();
    chars[0] > b' ' && chars.iter().all(|c| (b' '..0x7F).contains(c)) && !chars.iter().any(|c| b"<>.,;:=?*[]".contains(c))
}

/// Reads a directory's entries as this layout would, and how much sense they make.
fn judge(raw: &[u8], layout: &Layout) -> Judgement {
    let mut judgement = Judgement::default();
    let (blocks, first) = (layout.blocks(), layout.directory_blocks());
    let mask = layout.extent_mask();
    let mut claimed = HashSet::new();
    for entry in raw.as_chunks::<32>().0.iter() {
        match entry[0] {
            0xE5 => {}
            0..=31 => {
                if !name_ok(&entry[1..12]) || entry[12] > 31 || entry[15] > 0x80 {
                    judgement.bad += 1;
                    continue;
                }
                let pointers: Vec<u64> =
                    if layout.wide() { entry[16..32].as_chunks::<2>().0.iter().map(|p| u64::from(u16::from_le_bytes(*p))).collect() } else { entry[16..32].iter().map(|&b| u64::from(b)).collect() };
                let used: Vec<u64> = pointers.into_iter().filter(|&b| b != 0).collect();
                if used.iter().any(|&b| b >= blocks || b < first) {
                    judgement.bad += 1;
                    continue;
                }
                judgement.files += 1;
                if entry[0] < 16 {
                    for &b in &used {
                        if !claimed.insert(b) {
                            judgement.shared_blocks += 1;
                        }
                    }
                }
                let records = (u64::from(entry[12]) & mask) * 128 + u64::from(entry[15]);
                if used.len() as u64 == (records * 128).div_ceil(layout.block_size) {
                    judgement.consistent += 1;
                }
            }
            // A CP/M 3 label and date stamps.
            0x20 | 0x21 => {}
            _ => judgement.bad += 1,
        }
    }
    judgement
}

/// A CP/M 3 date: days from 1 January 1978 (day 1), then the hour and minute in BCD.
fn stamp_text(stamp: u32) -> Option<String> {
    let days = stamp & 0xFFFF;
    if days == 0 {
        return None;
    }
    let bcd = |b: u32| (b >> 4) * 10 + (b & 15);
    let (hour, minute) = (bcd(stamp >> 16 & 0xFF), bcd(stamp >> 24 & 0xFF));
    // Days since 1970, then the civil date (Howard Hinnant's days_from_civil, inverted).
    let z = i64::from(days) - 1 + 2922 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    Some(format!("{year}-{month:02}-{day:02} {hour:02}:{minute:02}"))
}

pub struct Cpm {
    sectors: Box<dyn Sectors>,
    layout: Layout,
    kind: String,
    files: Vec<File>,
    label: Option<String>,
    /// Whether the label says the first date stamp is when a file was last read rather than made.
    access_stamps: bool,
    notes: Vec<String>,
    /// For a +3 or CPC disk: their files' own headers give their exact lengths.
    amstrad: bool,
}

impl Cpm {
    pub fn open(sectors: Box<dyn Sectors>, layout: Layout, kind: String, amstrad: bool) -> Result<Self, String> {
        let mut cpm = Self { sectors, layout, kind, files: Vec::new(), label: None, access_stamps: false, notes: Vec::new(), amstrad };
        let raw = cpm.directory()?;
        cpm.read_directory(&raw);
        Ok(cpm)
    }

    pub fn note(&mut self, note: String) {
        self.notes.push(note);
    }

    /// A block's bytes, through the skew table.
    fn block(&self, block: u64) -> Result<Vec<u8>, String> {
        let l = &self.layout;
        let per_block = l.block_size / l.sector_size;
        let mut out = Vec::with_capacity(l.block_size as usize);
        for i in 0..per_block {
            let logical = block * per_block + i + l.boot_tracks * l.sectors_per_track;
            let (track, sector) = (logical / l.sectors_per_track, logical % l.sectors_per_track);
            out.extend(self.sectors.sector(track, l.skew.get(sector as usize).copied().unwrap_or(sector))?);
        }
        Ok(out)
    }

    fn directory(&self) -> Result<Vec<u8>, String> {
        let mut raw = Vec::new();
        for b in 0..self.layout.directory_blocks() {
            raw.extend(self.block(b)?);
        }
        raw.truncate(self.layout.directory_entries as usize * 32);
        Ok(raw)
    }

    fn read_directory(&mut self, raw: &[u8]) {
        let l = &self.layout;
        let (blocks, wide) = (l.blocks(), l.wide());
        let entries = raw.as_chunks::<32>().0;
        let mut extents = Vec::new();
        let mut passwords = HashSet::new();
        for (index, entry) in entries.iter().enumerate() {
            let text = |range: std::ops::Range<usize>| entry[range].iter().map(|b| char::from(b & 0x7F)).collect::<String>().trim_end().to_string();
            match entry[0] {
                0x20 => {
                    let label = format!("{}{}", text(1..9), if text(9..12).is_empty() { String::new() } else { format!(".{}", text(9..12)) });
                    self.label = Some(label.trim().to_string()).filter(|l| !l.is_empty());
                    self.access_stamps = entry[12] & 0x40 != 0;
                }
                user @ 0..=31 => {
                    let (name, extension) = (text(1..9), text(9..12));
                    let full = if extension.is_empty() { name } else { format!("{name}.{extension}") };
                    // CP/M 3 keeps a file's password in an entry 16 users on; P2DOS has 32 users.
                    if user >= 16 && self.layout.os == Os::Cpm3 {
                        passwords.insert((user - 16, full));
                        continue;
                    }
                    let attributes: String = [(9, 'R'), (10, 'S'), (11, 'A')].iter().filter(|(i, _)| entry[*i] & 0x80 != 0).map(|(_, c)| *c).collect();
                    let number = u64::from(entry[14] & 0x3F) * 32 + u64::from(entry[12] & 0x1F);
                    let pointers: Vec<u64> = if wide { entry[16..32].as_chunks::<2>().0.iter().map(|p| u64::from(u16::from_le_bytes(*p))).collect() } else { entry[16..32].iter().map(|&b| u64::from(b)).collect() };
                    // The date stamps for the three entries before every fourth are in that fourth.
                    let stamps = entries.get(index | 3).filter(|s| s[0] == 0x21 && index % 4 != 3).map_or(Stamps::default(), |s| {
                        let at = 1 + (index % 4) * 10;
                        Stamps { first: u32::from_le_bytes([s[at], s[at + 1], s[at + 2], s[at + 3]]), updated: u32::from_le_bytes([s[at + 4], s[at + 5], s[at + 6], s[at + 7]]) }
                    });
                    extents.push((user, full, attributes, number, pointers.into_iter().filter(|b| *b != 0 && *b < blocks).collect::<Vec<_>>(), u64::from(entry[15]), stamps));
                }
                _ => {}
            }
        }
        extents.sort_by(|a, b| (a.0, &a.1, a.3).cmp(&(b.0, &b.1, b.3)));
        for (user, name, attributes, number, blocks, records, stamps) in extents {
            let password = passwords.contains(&(user, name.clone()));
            match self.files.last_mut().filter(|f| f.user == user && f.name == name) {
                Some(file) => {
                    file.blocks.extend(blocks);
                    file.records = number * 128 + records;
                    if stamps.updated != 0 || stamps.first != 0 {
                        file.stamps = stamps;
                    }
                }
                None => self.files.push(File { user, name, attributes, blocks, records: number * 128 + records, stamps, password }),
            }
        }
    }

    fn read_file(&self, file: &File) -> Result<Vec<u8>, String> {
        let size = (file.records * 128) as usize;
        let mut out = Vec::with_capacity(size);
        for block in &file.blocks {
            if out.len() >= size {
                break;
            }
            out.extend(self.block(*block)?);
        }
        out.truncate(size);
        if self.amstrad {
            exact_amstrad_length(&mut out);
        }
        Ok(out)
    }
}

/// A +3DOS header and an AMSDOS header each say exactly how long the file is.
fn exact_amstrad_length(out: &mut Vec<u8>) {
    if out.starts_with(b"PLUS3DOS") && out.len() >= 128 {
        let length = u32::from_le_bytes([out[11], out[12], out[13], out[14]]) as usize;
        if (128..=out.len()).contains(&length) {
            out.truncate(length);
        }
    } else if out.len() >= 128 && amsdos_checksum_ok(out) {
        let length = usize::from(out[64]) | usize::from(out[65]) << 8 | usize::from(out[66]) << 16;
        if length + 128 <= out.len() {
            out.truncate(length + 128);
        }
    }
}

/// AMSDOS's 128-byte header ends its first 67 bytes with their sum.
fn amsdos_checksum_ok(header: &[u8]) -> bool {
    let sum: u32 = header[..67].iter().map(|b| u32::from(*b)).sum();
    sum as u16 == u16::from_le_bytes([header[67], header[68]]) && header[..67].iter().any(|b| *b != 0)
}

impl Volume for Cpm {
    fn describe(&self) -> String {
        match &self.label {
            Some(label) => format!("{}, labelled {label}", self.kind),
            None => self.kind.clone(),
        }
    }

    /// User 0's files at the top; other user areas, which CP/M keeps apart, as folders.
    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        let user = dir.map_or(0, |d| d.location as u8);
        let mut out = Vec::new();
        if dir.is_none() {
            let mut users: Vec<u8> = self.files.iter().map(|f| f.user).filter(|u| *u != 0).collect();
            users.dedup();
            out.extend(users.into_iter().map(|u| Entry::dir(format!("User {u}"), u64::from(u), 0)));
        }
        for (index, file) in self.files.iter().enumerate().filter(|(_, f)| f.user == user) {
            let mut entry = Entry::file(tidy_name(&file.name), file.records * 128, index as u64);
            let mut kind = if file.attributes.is_empty() { "File".to_string() } else { format!("File, {}", file.attributes) };
            if file.password {
                kind.push_str(", password");
            }
            entry.kind = kind;
            let first = if self.access_stamps { "read" } else { "made" };
            let dates: Vec<String> = [(first, file.stamps.first), ("changed", file.stamps.updated)].iter().filter_map(|(what, stamp)| stamp_text(*stamp).map(|t| format!("{what} {t}"))).collect();
            entry.detail = dates.join(", ");
            // A +3 BASIC program with no ending is given one, so the BASIC listing finds it.
            if self.amstrad
                && let Ok(head) = file.blocks.first().map_or(Ok(Vec::new()), |b| self.block(*b))
                && head.starts_with(b"PLUS3DOS")
                && head.len() > 15
            {
                entry.kind = format!("+3DOS {}", ["BASIC", "number array", "character array", "code"].get(usize::from(head[15])).unwrap_or(&"file"));
                entry.detail = format!("{} bytes", u32::from_le_bytes([head[11], head[12], head[13], head[14]]).saturating_sub(128));
                if head[15] == 0 && !file.name.contains('.') {
                    entry.open_name = format!("{}.bas", entry.name);
                }
            }
            out.push(entry);
        }
        Ok(out)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let file = self.files.get(entry.location as usize).ok_or("That file isn't on the disk.")?;
        self.read_file(file)
    }

    fn notes(&self) -> Vec<String> {
        let mut notes = self.notes.clone();
        notes.push(if self.amstrad {
            "Sizes are CP/M's, in 128-byte records; a +3DOS or AMSDOS file opened has its exact length, header included.".into()
        } else {
            "Sizes are CP/M's, in 128-byte records; a text file ends at its first Ctrl+Z when opened.".into()
        });
        if self.files.iter().any(|f| f.password) {
            notes.push("Files marked \"password\" are protected on CP/M 3, but their contents aren't hidden, so they read as any other.".into());
        }
        notes
    }

    fn reading_bytes(&self, _entry: &Entry, bytes: Vec<u8>) -> Vec<u8> {
        cut_at_ctrl_z(bytes)
    }
}

/// CP/M only knows a file's length in 128-byte records, so a text file ends at its first Ctrl+Z, and whatever
/// follows in the last record isn't part of it. A file that isn't text up to there is left whole.
pub fn cut_at_ctrl_z(mut bytes: Vec<u8>) -> Vec<u8> {
    if let Some(at) = bytes.iter().position(|&b| b == 0x1A)
        && at > 0
        && !crate::codec::looks_binary(&bytes[..at])
    {
        bytes.truncate(at);
    }
    bytes
}

/// What a sector image's tracks look like, when the image recorded them.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub sector_size: u64,
    pub per_track: u64,
    pub heads: u64,
}

/// Which formats could be on this image, best first, and how they read its directory.
fn candidates(source: &Arc<dyn Source>, len: u64, shape: Option<Shape>) -> Vec<(Layout, Judgement, bool)> {
    let mut found = Vec::new();
    for def in &super::cpm_formats::FORMATS {
        let track = def.sectors_per_track * def.sector_size;
        let whole = def.tracks * track;
        let fits = match shape {
            Some(s) => def.sector_size == s.sector_size && (def.sectors_per_track == s.per_track || def.sectors_per_track == s.per_track * s.heads),
            // A plain image is a whole number of the format's tracks, and no more than the disk's.
            None => len.is_multiple_of(track) && len <= whole,
        };
        let layout = Layout::from_def(def);
        if !fits || layout.directory_end() > source.len() {
            continue;
        }
        let flat = Flat { source: source.clone(), sector_size: def.sector_size, per_track: def.sectors_per_track };
        let cpm = Cpm { sectors: Box::new(flat), layout, kind: String::new(), files: Vec::new(), label: None, access_stamps: false, notes: Vec::new(), amstrad: false };
        let Ok(raw) = cpm.directory() else {
            continue;
        };
        let judgement = judge(&raw, &cpm.layout);
        if judgement.believable() {
            found.push((cpm.layout, judgement, len == whole));
        }
    }
    // Best first: the most sense made of the directory, then an image that's the whole disk, then the table's order.
    found.sort_by_key(|(_, judgement, exact)| std::cmp::Reverse((judgement.score(), *exact)));
    if found.is_empty()
        && let Some(shape) = shape
    {
        found = worked_out(source, len, shape);
    }
    found
}

/// For a disk in no format in the table, but whose tracks' shape is known: the usual choices of system tracks,
/// block size, directory size and skew, each tried. Only a directory that makes complete sense is believed, as
/// nothing else says the layout is right.
fn worked_out(source: &Arc<dyn Source>, len: u64, shape: Shape) -> Vec<(Layout, Judgement, bool)> {
    let mut found = Vec::new();
    for per_track in [shape.per_track, shape.per_track * shape.heads] {
        let track = per_track * shape.sector_size;
        let tracks = len / track;
        for boot in [2, 1, 3, 0, 4] {
            for block_size in [1024, 2048, 4096] {
                for entries in [64, 128, 32, 256, 512] {
                    for step in [0, 6, 2, 3, 4, 5, 7] {
                        let layout = Layout {
                            name: String::new(),
                            described: format!(
                                "unlisted format worked out as {}-byte sectors, {per_track} a track, {boot} system track{}, {block_size}-byte blocks, {entries} directory entries, skew {}",
                                shape.sector_size,
                                if boot == 1 { "" } else { "s" },
                                step.max(1)
                            ),
                            sector_size: shape.sector_size,
                            sectors_per_track: per_track,
                            tracks,
                            block_size,
                            directory_entries: entries,
                            boot_tracks: boot,
                            skew: skew_table(per_track, step),
                            os: Os::Cpm22,
                        };
                        if block_size < shape.sector_size || layout.directory_end() > source.len() || layout.blocks() < 16 {
                            continue;
                        }
                        let flat = Flat { source: source.clone(), sector_size: shape.sector_size, per_track };
                        let cpm = Cpm { sectors: Box::new(flat), layout, kind: String::new(), files: Vec::new(), label: None, access_stamps: false, notes: Vec::new(), amstrad: false };
                        let Ok(raw) = cpm.directory() else {
                            continue;
                        };
                        let judgement = judge(&raw, &cpm.layout);
                        if judgement.files >= 2 && judgement.bad == 0 && judgement.shared_blocks == 0 && judgement.consistent == judgement.files {
                            found.push((cpm.layout, judgement, false));
                        }
                    }
                }
            }
        }
        if shape.heads == 1 {
            break;
        }
    }
    // Most files read, then the smallest directory that holds them (a larger one would take in file data).
    found.sort_by_key(|(layout, judgement, _)| (std::cmp::Reverse(judgement.score()), layout.directory_entries));
    found.truncate(1);
    found
}

/// Whether the start of a plain image (`len` bytes in all) reads as a CP/M directory in some format.
pub fn looks_like(head: &[u8], len: u64) -> bool {
    let source: Arc<dyn Source> = Arc::new(head.to_vec());
    !candidates(&source, len, None).is_empty()
}

/// Opens a disk in whichever format makes the most sense of its directory.
pub fn open_guessed(source: Arc<dyn Source>, shape: Option<Shape>, what: &str) -> Result<Cpm, String> {
    let mut found = candidates(&source, source.len(), shape);
    if found.is_empty() {
        return Err("No CP/M directory was found on this disk in any format Limoniter knows.".into());
    }
    let (layout, best, _) = found.remove(0);
    // Others that read it just as well (the same layout under other machines' names, often) are mentioned.
    let alike: Vec<String> = found.iter().filter(|(_, j, _)| j.score() == best.score()).map(|(l, _, _)| l.described.clone()).filter(|d| *d != layout.described).take(4).collect();
    let flat = Flat { source, sector_size: layout.sector_size, per_track: layout.sectors_per_track };
    let kind = format!("{what}CP/M disk, {} format", layout.described);
    let named = !layout.name.is_empty();
    let mut cpm = Cpm::open(Box::new(flat), layout, kind, false)?;
    let mut note = if !named {
        "This disk's format isn't one Limoniter knows by name, so its layout was worked out from its directory, which it reads completely. Check that its files look right.".to_string()
    } else {
        "CP/M disks don't say what format they're in; this one was worked out from its directory.".to_string()
    };
    if !alike.is_empty() {
        note.push_str(&format!(" These read it the same way: {}.", alike.join(", ")));
    }
    cpm.note(note);
    Ok(cpm)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// An 8-inch IBM 3740 disk with two files: one of 3 records, one taking two entries (17K, over a 16K extent).
    pub fn ibm_3740_disk() -> Vec<u8> {
        let def = super::super::cpm_formats::FORMATS.iter().find(|d| d.name == "ibm-3740").unwrap();
        let layout = Layout::from_def(def);
        let mut disk = vec![0xE5u8; (def.tracks * def.sectors_per_track * def.sector_size) as usize];
        // Writes a block through the skew table, as the machine would have.
        let put_block = |disk: &mut Vec<u8>, block: u64, data: &[u8]| {
            for i in 0..8 {
                let logical = block * 8 + i + 2 * 26;
                let (track, sector) = (logical / 26, logical % 26);
                let at = ((track * 26 + layout.skew[sector as usize]) * 128) as usize;
                disk[at..at + 128].copy_from_slice(&data[(i * 128) as usize..(i * 128 + 128) as usize]);
            }
        };
        let mut directory = vec![0xE5u8; 2048];
        let entry = |user: u8, name: &[u8; 11], ex: u8, rc: u8, blocks: &[u8]| {
            let mut e = vec![user];
            e.extend(name);
            e.extend([ex, 0, 0, rc]);
            let mut pointers = blocks.to_vec();
            pointers.resize(16, 0);
            e.extend(pointers);
            e
        };
        directory[..32].copy_from_slice(&entry(0, b"HELLO   TXT", 0, 3, &[2]));
        directory[32..64].copy_from_slice(&entry(0, b"BIG     DAT", 0, 0x80, &[3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18]));
        directory[64..96].copy_from_slice(&entry(0, b"BIG     DAT", 1, 8, &[19]));
        directory[96..128].copy_from_slice(&entry(3, b"OTHER   COM", 0, 1, &[20]));
        put_block(&mut disk, 0, &directory[..1024]);
        put_block(&mut disk, 1, &directory[1024..]);
        let mut text = b"Hello from CP/M\r\n\x1A".to_vec();
        text.resize(1024, 0x1A);
        put_block(&mut disk, 2, &text);
        for b in 3..=19u64 {
            put_block(&mut disk, b, &[b as u8; 1024]);
        }
        put_block(&mut disk, 20, &[0xC9; 1024]);
        disk
    }

    #[test]
    fn every_format_has_its_own_description() {
        let mut seen = HashSet::new();
        for def in &super::super::cpm_formats::FORMATS {
            assert!(seen.insert(def.described), "{} is described as another is", def.name);
        }
    }

    #[test]
    fn skew_tables_are_made_as_cpmtools_makes_them() {
        assert_eq!(skew_table(26, 6)[..8], [0, 6, 12, 18, 24, 4, 10, 16]);
        assert_eq!(skew_table(9, 1), (0..9).collect::<Vec<_>>());
    }

    #[test]
    fn an_8_inch_disk_is_recognised_and_read_through_its_skew() {
        let disk = ibm_3740_disk();
        assert!(looks_like(&disk[..0x8800], disk.len() as u64));
        let cpm = open_guessed(Arc::new(disk), None, "").unwrap();
        assert!(cpm.describe().contains("8-inch IBM 3740"), "{}", cpm.describe());
        let top = cpm.list(None).unwrap();
        let names: Vec<&str> = top.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["User 3", "BIG.DAT", "HELLO.TXT"]);
        let hello = cpm.read(&top[2]).unwrap();
        assert_eq!(hello.len(), 384, "three records");
        assert_eq!(cpm.reading_bytes(&top[2], hello), b"Hello from CP/M\r\n", "a text file ends at its Ctrl+Z");
        let big = cpm.read(&top[1]).unwrap();
        assert_eq!(big.len(), 17 * 1024);
        assert_eq!((big[0], big[16 * 1024]), (3, 19), "the second extent's block follows the first's");
        let other = cpm.list(Some(&top[0])).unwrap();
        assert_eq!(cpm.read(&other[0]).unwrap(), vec![0xC9; 128]);
        assert_eq!(cut_at_ctrl_z(vec![0xC9, 0, 0x1A, 5]), vec![0xC9, 0, 0x1A, 5], "a program isn't cut");
    }

    #[test]
    fn cpm3_stamps_labels_and_passwords() {
        assert_eq!(stamp_text(1 | 0x12 << 16 | 0x34 << 24).as_deref(), Some("1978-01-01 12:34"));
        assert_eq!(stamp_text(2922 + 1).as_deref(), Some("1986-01-01 00:00"));
        assert_eq!(stamp_text(0), None);
    }

    /// CP/M disks under ..\testmedia\cpm (or the folder LIMONITER_CPM names), in archives or not. When LIMONITER_CPM_OUT
    /// names a folder, each disk read as CP/M is written there as a plain sector image with the cpmtools format
    /// it was taken for and every file's length and SHA-256, for comparing with cpmtools. Skipped without the folder.
    #[test]
    fn real_cpm_disks() {
        use sha2::Digest;
        let root = std::env::var_os("LIMONITER_CPM").map_or_else(|| std::path::PathBuf::from("../testmedia/cpm"), std::path::PathBuf::from);
        if !root.is_dir() {
            return;
        }
        let out = std::env::var_os("LIMONITER_CPM_OUT").map(std::path::PathBuf::from);
        let mut report = String::new();
        let (mut read, mut not_cpm) = (0, Vec::new());
        let mut found: Vec<(String, String, Vec<u8>)> = Vec::new();
        let mut folders = vec![root.clone()];
        while let Some(folder) = folders.pop() {
            for item in std::fs::read_dir(&folder).unwrap().flatten() {
                let path = item.path();
                if path.is_dir() {
                    folders.push(path);
                } else {
                    let name = path.file_name().unwrap().to_string_lossy().into_owned();
                    found.push((path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"), name, std::fs::read(&path).unwrap()));
                }
            }
        }
        // Archives are opened and their disks added.
        let mut i = 0;
        while i < found.len() {
            let (place, name, bytes) = found[i].clone();
            if name.to_lowercase().ends_with(".zip")
                && let Ok(zip) = super::super::open(Arc::new(bytes), &name)
            {
                for entry in zip.list(None).unwrap_or_default().into_iter().filter(|e| !e.is_dir) {
                    if let Ok(inner) = zip.read(&entry) {
                        found.push((format!("{place}!{}", entry.name), entry.open_name.clone(), inner));
                    }
                }
            }
            i += 1;
        }
        for (place, name, bytes) in found {
            let lower = name.to_lowercase();
            if !(lower.ends_with(".imd") || lower.ends_with(".td0") || lower.ends_with(".img") || lower.ends_with(".dsk")) {
                continue;
            }
            let volume = match super::super::open(Arc::new(bytes.clone()), &name) {
                Ok(volume) => volume,
                Err(problem) => {
                    not_cpm.push(format!("{place}: {problem}"));
                    continue;
                }
            };
            let described = volume.describe();
            let format = match super::super::cpm_formats::FORMATS.iter().find(|d| described.contains(&format!("CP/M disk, {} format", d.described))) {
                Some(def) => def.name.to_string(),
                None => match described.split_once("unlisted format worked out as ") {
                    Some((_, layout)) => format!("worked: {}", layout.trim_end_matches(" format")),
                    None => {
                        not_cpm.push(format!("{place}: {described}"));
                        continue;
                    }
                },
            };
            read += 1;
            let entries = volume.list(None).unwrap();
            let raw = match entries.iter().find(|e| e.name == "Whole disk (sectors).raw") {
                Some(whole) => volume.read(whole).unwrap(),
                None => bytes,
            };
            let mut files = Vec::new();
            let mut stack: Vec<(String, Entry)> = entries.into_iter().filter(|e| e.name != "Whole disk (sectors).raw").map(|e| (String::from("0"), e)).collect();
            while let Some((user, entry)) = stack.pop() {
                if entry.is_dir {
                    let number = entry.location.to_string();
                    stack.extend(volume.list(Some(&entry)).unwrap().into_iter().map(|e| (number.clone(), e)));
                    continue;
                }
                match volume.read(&entry) {
                    Ok(data) => files.push(format!("{user}:{}\t{}\t{:x}", entry.name, data.len(), sha2::Sha256::digest(&data))),
                    Err(problem) => files.push(format!("{user}:{}\tERROR {problem}", entry.name)),
                }
            }
            files.sort();
            report.push_str(&format!("# {read:03}\t{format}\t{place}\n{}\n", files.join("\n")));
            if let Some(out) = &out {
                std::fs::create_dir_all(out).unwrap();
                std::fs::write(out.join(format!("{read:03}.img")), raw).unwrap();
            }
        }
        if let Some(out) = &out {
            std::fs::write(out.join("report.txt"), &report).unwrap();
            std::fs::write(out.join("not-cpm.txt"), not_cpm.join("\n")).unwrap();
        }
        eprintln!("{read} CP/M disks read; {} not read as CP/M", not_cpm.len());
    }

    #[test]
    fn noise_isnt_taken_for_a_directory() {
        let noise: Vec<u8> = (0..256 * 1024u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
        assert!(!looks_like(&noise[..0x8800], noise.len() as u64));
        assert!(!looks_like(&vec![0u8; 0x8800], 256 * 1024));
    }
}
