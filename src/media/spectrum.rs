//! ZX Spectrum (and Amstrad CPC) media:
//! - DSK disk images, standard ("MV - CPC") and extended, holding a +3DOS or AMSDOS disk: CP/M 2.2's file system,
//!   with its geometry from the +3's disk specification in the boot sector, or from the CPC's sector numbers.
//! - TR-DOS disks (.trd) and their SCL archives, as the Beta Disk interface lays them out.
//! - Microdrive cartridges (.mdr), as Interface 1 records them.
//! - Tapes (.tap, .tzx) as lists of their files, and snapshots (.z80, .sna) as the BASIC, screen and memory in them.
//!
//! A BASIC program from a tape, a TR-DOS disk, a cartridge or a snapshot is given back as a +3DOS file, so the
//! BASIC listing reads it and a +3 or Spectrum Next can load it; the file says so in its kind.

use super::{Entry, Source, Volume, cpm, tidy_name};
use crate::basic;
use crate::charset;
use std::collections::HashMap;
use std::sync::Arc;

/// A name in the Spectrum's set, with its padding taken off.
fn zx_name(bytes: &[u8]) -> String {
    let set = charset::known("zx-spectrum");
    let text: String = bytes.iter().map(|b| set.entry(0, *b & 0x7F).and_then(|e| e.unicode).filter(|c| !c.is_control()).unwrap_or('?')).collect();
    text.trim_end().to_string()
}

/// A BASIC program and its variables as a +3DOS file, with its autostart line, so it lists and loads as one.
fn as_plus3(program_and_variables: &[u8], program_length: usize, run_from: Option<u16>) -> Vec<u8> {
    basic::plus3dos_basic(program_and_variables, program_length, run_from.filter(|line| *line < 10000))
}

// ---------- DSK images and CP/M ----------

/// Where each sector of a DSK image is, by cylinder, head and sector number.
struct Dsk {
    source: Arc<dyn Source>,
    sectors: HashMap<(u8, u8, u8), (u64, usize)>,
    cylinders: u8,
    heads: u8,
    first_ids: Vec<u8>,
    extended: bool,
}

impl Dsk {
    fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let header = source.read_vec(0, 256)?;
        let extended = header.starts_with(b"EXTENDED CPC DSK");
        if !extended && !header.starts_with(b"MV - CPC") {
            return Err("This isn't a CPC or +3 DSK image.".into());
        }
        let (cylinders, heads) = (header[0x30], header[0x31].max(1));
        let standard_size = u64::from(u16::from_le_bytes([header[0x32], header[0x33]]));
        let mut sectors = HashMap::new();
        let mut first_ids = Vec::new();
        let mut at = 256u64;
        for index in 0..usize::from(cylinders) * usize::from(heads) {
            let size = if extended { u64::from(header[0x34 + index]) * 256 } else { standard_size };
            if size == 0 {
                continue;
            }
            let track = source.read_vec(at, 256)?;
            if !track.starts_with(b"Track-Info") {
                return Err(format!("Track {index} of the DSK image has no track header."));
            }
            let count = usize::from(track[0x15]).min(29);
            let mut data = at + 256;
            for s in 0..count {
                let info = &track[0x18 + s * 8..0x20 + s * 8];
                let length = if extended { usize::from(u16::from_le_bytes([info[6], info[7]])) } else { 128usize << info[3].min(6) };
                sectors.insert((track[0x10], track[0x11], info[2]), (data, length));
                if s == 0 && index == 0 {
                    first_ids.push(info[2]);
                }
                data += length as u64;
            }
            at += size;
        }
        Ok(Self { source, sectors, cylinders, heads, first_ids, extended })
    }

    fn sector(&self, cylinder: u8, head: u8, id: u8) -> Result<Vec<u8>, String> {
        let (at, length) = self.sectors.get(&(cylinder, head, id)).ok_or_else(|| format!("track {cylinder} side {head} sector {id} isn't in the image"))?;
        self.source.read_vec(*at, *length)
    }
}

/// A DSK image's sectors as CP/M asks for them: tracks counted across both sides in turn, as the +3 and the CPC
/// count them, sectors numbered on from the format's first.
struct DskSectors {
    disk: Dsk,
    first_id: u8,
    sides: u64,
}

impl cpm::Sectors for DskSectors {
    fn sector(&self, track: u64, place: u64) -> Result<Vec<u8>, String> {
        let (cylinder, head) = if self.sides == 2 { (track / 2, track % 2) } else { (track, 0) };
        self.disk.sector(cylinder as u8, head as u8, self.first_id.wrapping_add(place as u8))
    }
}

/// A CPC, +3 or PCW disk in a DSK image, read by the CP/M reader.
pub struct CpmDisk;

impl CpmDisk {
    pub fn open(source: Arc<dyn Source>) -> Result<cpm::Cpm, String> {
        let disk = Dsk::open(source)?;
        let first = disk.first_ids.first().copied().unwrap_or(1);
        let tracks = u64::from(disk.cylinders);
        let sides = u64::from(disk.heads);
        let extended = if disk.extended { " (extended)" } else { "" };
        // (first sector, sectors a track, sector size, reserved tracks, block size, directory blocks, tracks a side, sides)
        // The CPC's formats are known by their sector numbers; the +3's disk says what it is in its boot sector.
        let (spec, kind) = match first {
            0xC1 => ((0xC1, 9, 512, 0, 1024, 2, tracks, sides), "Amstrad CPC disk (DSK), data format"),
            0x41 => ((0x41, 9, 512, 2, 1024, 2, tracks, sides), "Amstrad CPC disk (DSK), system format"),
            _ => {
                let boot = disk.sector(0, 0, first)?;
                let b = if boot[0] <= 3 && boot[3] != 0 { [boot[0], boot[1], boot[2], boot[3], boot[4], boot[5], boot[6], boot[7]] } else { [0, 0, 40, 9, 2, 1, 3, 2] };
                (
                    (first, u64::from(b[3]), 128u64 << b[4].min(3), u64::from(b[5]), 128u64 << b[6].min(4), u64::from(b[7]).max(1), u64::from(b[2]), if b[1] & 0x03 == 0 { 1 } else { 2 }),
                    if b[0] == 3 { "Spectrum +3 or Amstrad PCW disk (DSK), +3DOS" } else { "Spectrum +3 disk (DSK), +3DOS" },
                )
            }
        };
        let (first_id, per_track, sector_size, reserved, block_size, directory_blocks, tracks_per_side, sides) = spec;
        let layout = cpm::Layout {
            name: String::new(),
            described: kind.to_string(),
            sector_size,
            sectors_per_track: per_track,
            tracks: tracks_per_side * sides,
            block_size,
            directory_entries: directory_blocks * block_size / 32,
            boot_tracks: reserved,
            skew: (0..per_track).collect(),
            os: cpm::Os::Cpm3,
        };
        cpm::Cpm::open(Box::new(DskSectors { disk, first_id, sides }), layout, format!("{kind}{extended}"), true)
    }
}

// ---------- TR-DOS ----------

/// A TR-DOS disk (.trd: sectors in order, 16 of 256 bytes to a track, sides in turn) or an SCL archive.
pub struct TrDos {
    source: Arc<dyn Source>,
    label: String,
    scl: bool,
    files: Vec<(String, u8, u16, u16, u64, u64)>,
}

impl TrDos {
    pub fn open(source: Arc<dyn Source>, scl: bool) -> Result<Self, String> {
        let mut files = Vec::new();
        let mut label = String::new();
        if scl {
            let head = source.read_vec(0, 9)?;
            if &head[..8] != b"SINCLAIR" {
                return Err("This SCL file doesn't start with SINCLAIR.".into());
            }
            let count = usize::from(head[8]);
            let table = source.read_vec(9, count * 14)?;
            let mut at = 9 + count as u64 * 14;
            for entry in table.as_chunks::<14>().0.iter() {
                let sectors = u64::from(entry[13]);
                files.push((zx_name(&entry[..8]), entry[8], u16::from_le_bytes([entry[9], entry[10]]), u16::from_le_bytes([entry[11], entry[12]]), at, sectors * 256));
                at += sectors * 256;
            }
        } else {
            let catalogue = source.read_vec(0, 2304)?;
            if catalogue[0x8E7] != 0x10 {
                return Err("This doesn't have TR-DOS's disk information sector.".into());
            }
            label = zx_name(&catalogue[0x8F5..0x8FD]);
            for entry in catalogue[..2048].as_chunks::<16>().0.iter() {
                match entry[0] {
                    0 => break,
                    1 => continue,
                    _ => {}
                }
                let at = (u64::from(entry[15]) * 16 + u64::from(entry[14])) * 256;
                files.push((zx_name(&entry[..8]), entry[8], u16::from_le_bytes([entry[9], entry[10]]), u16::from_le_bytes([entry[11], entry[12]]), at, u64::from(entry[13]) * 256));
            }
        }
        Ok(Self { source, label, scl, files })
    }
}

impl Volume for TrDos {
    fn describe(&self) -> String {
        if self.scl { "TR-DOS archive (SCL)".to_string() } else { format!("TR-DOS disk (TRD) \u{B7} {}", if self.label.is_empty() { "no label" } else { &self.label }) }
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        if dir.is_some() {
            return Ok(Vec::new());
        }
        Ok(self
            .files
            .iter()
            .enumerate()
            .map(|(index, (name, kind, first, second, _, sectors))| {
                let kind_char = char::from(*kind);
                let mut entry = Entry::file(tidy_name(&format!("{name}.{kind_char}")), *sectors, index as u64);
                (entry.kind, entry.detail) = match *kind {
                    b'B' => ("BASIC (as a +3DOS file)".into(), format!("program {second} bytes, with variables {first}")),
                    b'C' => ("Code".into(), format!("{second} bytes at {first}")),
                    b'D' => ("Data".into(), format!("{second} bytes")),
                    _ => (format!("Type {kind_char}"), format!("{second} bytes")),
                };
                entry.open_name = match *kind {
                    b'B' => format!("{}.bas", tidy_name(name)),
                    b'C' if *second == 6912 => format!("{}.scr", tidy_name(name)),
                    _ => entry.name.clone(),
                };
                entry
            })
            .collect())
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let (_, kind, first, second, at, sectors) = self.files.get(entry.location as usize).ok_or("That file isn't on the disk.")?;
        let data = self.source.read_vec(*at, (*sectors).min(self.source.len().saturating_sub(*at)) as usize)?;
        match kind {
            // A BASIC file is its program and variables, then 0x80 0xAA and the line it runs from.
            b'B' => {
                let total = usize::from(*first).min(data.len());
                let run_from = data.get(total..total + 4).filter(|b| b[0] == 0x80 && b[1] == 0xAA).map(|b| u16::from_le_bytes([b[2], b[3]]));
                Ok(as_plus3(&data[..total], usize::from(*second), run_from))
            }
            _ => Ok(data[..usize::from(*second).min(data.len())].to_vec()),
        }
    }
}

// ---------- Microdrive ----------

/// A Microdrive cartridge: 254 sectors of 543 bytes, each a 15-byte header, a 15-byte record descriptor and 512
/// bytes of data with a checksum. A file is its records in order, the first starting with a 9-byte header.
pub struct Microdrive {
    name: String,
    files: Vec<(String, Vec<u8>)>,
}

impl Microdrive {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let data = source.read_vec(0, source.len().min(254 * 543) as usize)?;
        let mut records: HashMap<String, Vec<(u8, bool, Vec<u8>)>> = HashMap::new();
        let mut order = Vec::new();
        let mut name = String::new();
        for sector in data.as_chunks::<543>().0.iter() {
            if sector[0] & 1 == 0 {
                continue;
            }
            if name.is_empty() {
                name = zx_name(&sector[4..14]);
            }
            let flag = sector[15];
            let length = usize::from(u16::from_le_bytes([sector[17], sector[18]])).min(512);
            // An unused sector has a record with no length and no name.
            if flag & 1 != 0 || (length == 0 && flag & 2 == 0) {
                continue;
            }
            let file = zx_name(&sector[19..29]);
            if !records.contains_key(&file) {
                order.push(file.clone());
            }
            records.entry(file).or_default().push((sector[16], flag & 2 != 0, sector[30..30 + length].to_vec()));
        }
        let files = order
            .into_iter()
            .map(|file| {
                let mut parts = records.remove(&file).unwrap_or_default();
                parts.sort_by_key(|p| p.0);
                (file, parts.into_iter().flat_map(|p| p.2).collect::<Vec<u8>>())
            })
            .collect();
        Ok(Self { name, files })
    }
}

impl Volume for Microdrive {
    fn describe(&self) -> String {
        format!("ZX Microdrive cartridge (MDR) \u{B7} {}", if self.name.is_empty() { "no name" } else { &self.name })
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        if dir.is_some() {
            return Ok(Vec::new());
        }
        Ok(self
            .files
            .iter()
            .enumerate()
            .map(|(index, (name, data))| {
                let mut entry = Entry::file(tidy_name(name), data.len().saturating_sub(9) as u64, index as u64);
                let kind = data.first().copied().unwrap_or(255);
                entry.kind = ["BASIC (as a +3DOS file)", "Number array", "Character array", "Code"].get(usize::from(kind)).map_or("Print file".to_string(), |k| k.to_string());
                if kind == 0 {
                    entry.open_name = format!("{}.bas", tidy_name(name));
                }
                entry
            })
            .collect())
    }

    /// The first record's 9-byte header: type, length, start, program length and the line it runs from.
    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let (_, data) = self.files.get(entry.location as usize).ok_or("That file isn't on the cartridge.")?;
        if data.len() < 9 {
            return Ok(data.clone());
        }
        let word = |at: usize| usize::from(u16::from_le_bytes([data[at], data[at + 1]]));
        let body = &data[9..(9 + word(1)).min(data.len())];
        match data[0] {
            0 => Ok(as_plus3(body, word(5), Some(word(7) as u16))),
            _ => Ok(body.to_vec()),
        }
    }
}

// ---------- Tapes ----------

/// A tape's files: each header block with the data block after it, and data with no header on its own.
pub struct Tape {
    tzx: bool,
    files: Vec<(String, String, String, Vec<u8>)>,
    problem: Option<String>,
}

impl Tape {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let data = source.read_vec(0, source.len().min(64 * 1024 * 1024) as usize)?;
        let tzx = data.starts_with(b"ZXTape!\x1A");
        let (blocks, problem) = basic::tape_blocks(&data);
        let mut files = Vec::new();
        let mut pending: Option<&[u8]> = None;
        for (index, block) in blocks.iter().enumerate() {
            if block.len() < 2 {
                continue;
            }
            let (flag, body) = (block[0], &block[1..block.len() - 1]);
            if flag == 0 && body.len() == 17 {
                pending = Some(body);
                continue;
            }
            match pending.take() {
                Some(header) => {
                    let name = zx_name(&header[1..11]);
                    let word = |at: usize| u16::from_le_bytes([header[at], header[at + 1]]);
                    let (length, first, second) = (usize::from(word(11)), word(13), word(15));
                    let data = &body[..length.min(body.len())];
                    files.push(match header[0] {
                        0 => (format!("{}.bas", tidy_name(&name)), "BASIC (as a +3DOS file)".into(), if first < 10000 { format!("runs from line {first}") } else { String::new() }, as_plus3(data, usize::from(second), Some(first))),
                        3 if length == 6912 => (format!("{}.scr", tidy_name(&name)), "Screen".into(), format!("6912 bytes at {first}"), data.to_vec()),
                        3 => (format!("{}.code", tidy_name(&name)), "Code".into(), format!("{length} bytes at {first}"), data.to_vec()),
                        k => (format!("{}.dat", tidy_name(&name)), if k == 1 { "Number array" } else { "Character array" }.into(), format!("{length} bytes"), data.to_vec()),
                    });
                }
                None => files.push((format!("Block {}.bin", index + 1), format!("Headerless block, flag {flag:02X}"), format!("{} bytes", body.len()), body.to_vec())),
            }
        }
        Ok(Self { tzx, files, problem })
    }
}

impl Volume for Tape {
    fn describe(&self) -> String {
        format!("ZX Spectrum tape ({}) \u{B7} {} file(s)", if self.tzx { "TZX" } else { "TAP" }, self.files.len())
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        if dir.is_some() {
            return Ok(Vec::new());
        }
        Ok(self
            .files
            .iter()
            .enumerate()
            .map(|(index, (name, kind, detail, data))| {
                let mut entry = Entry::file(name.clone(), data.len() as u64, index as u64);
                entry.kind = kind.clone();
                entry.detail = detail.clone();
                entry
            })
            .collect())
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        self.files.get(entry.location as usize).map(|f| f.3.clone()).ok_or_else(|| "That file isn't on the tape.".into())
    }

    fn notes(&self) -> Vec<String> {
        self.problem.iter().cloned().collect()
    }
}

// ---------- Snapshots ----------

/// A snapshot's memory: the 48K from 0x4000, or a 128K machine's eight banks.
pub struct Snapshot {
    kind: String,
    /// 0x4000 to 0xFFFF as the machine had it paged.
    memory: Vec<u8>,
    banks: Vec<(u8, Vec<u8>)>,
}

/// Z80's compression: ED ED, a count and a byte stand for that many of the byte.
fn z80_unpack(data: &[u8], limit: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(limit);
    let mut i = 0;
    while i < data.len() && out.len() < limit {
        if data[i] == 0xED && data.get(i + 1) == Some(&0xED) && i + 3 < data.len() {
            out.extend(std::iter::repeat_n(data[i + 3], usize::from(data[i + 2])));
            i += 4;
        } else {
            out.push(data[i]);
            i += 1;
        }
    }
    out.resize(limit, 0);
    out
}

impl Snapshot {
    pub fn open(source: Arc<dyn Source>, sna: bool) -> Result<Self, String> {
        let data = source.read_vec(0, source.len().min(512 * 1024) as usize)?;
        if sna {
            if data.len() < 27 + 49152 {
                return Err("This SNA snapshot is too short.".into());
            }
            let memory = data[27..27 + 49152].to_vec();
            let mut banks = Vec::new();
            let mut kind = "ZX Spectrum snapshot (SNA, 48K)".to_string();
            if data.len() >= 27 + 49152 + 4 + 5 * 16384 {
                // A 128K snapshot: banks 5 and 2 and the paged one, then the paging, then the rest in order.
                let paged = data[27 + 49152 + 2] & 7;
                banks.push((5, memory[..16384].to_vec()));
                banks.push((2, memory[16384..32768].to_vec()));
                banks.push((paged, memory[32768..].to_vec()));
                let mut at = 27 + 49152 + 4;
                for bank in (0..8u8).filter(|b| *b != 2 && *b != 5 && *b != paged) {
                    if let Some(page) = data.get(at..at + 16384) {
                        banks.push((bank, page.to_vec()));
                    }
                    at += 16384;
                }
                banks.sort_by_key(|b| b.0);
                kind = "ZX Spectrum snapshot (SNA, 128K)".into();
            }
            return Ok(Self { kind, memory, banks });
        }
        if data.len() < 30 {
            return Err("This Z80 snapshot is too short.".into());
        }
        let pc = u16::from_le_bytes([data[6], data[7]]);
        if pc != 0 {
            // Version 1: 48K only, compressed when bit 5 of byte 12 says so.
            let body = &data[30..];
            let memory = if data[12] & 0x20 != 0 { z80_unpack(body, 49152) } else { body[..body.len().min(49152)].to_vec() };
            return Ok(Self { kind: "ZX Spectrum snapshot (Z80 version 1, 48K)".into(), memory, banks: Vec::new() });
        }
        let extra = usize::from(u16::from_le_bytes([data[30], data[31]]));
        let version = if extra == 23 { 2 } else { 3 };
        let mode = data[34];
        let is128 = if version == 2 { mode >= 3 } else { mode >= 4 };
        let mut pages: HashMap<u8, Vec<u8>> = HashMap::new();
        let mut at = 32 + extra;
        while at + 3 <= data.len() {
            let length = usize::from(u16::from_le_bytes([data[at], data[at + 1]]));
            let page = data[at + 2];
            at += 3;
            let (raw, used) = if length == 0xFFFF { (data.get(at..at + 16384).unwrap_or(&[]), 16384) } else { (data.get(at..at + length).unwrap_or(&[]), length) };
            pages.insert(page, if length == 0xFFFF { raw.to_vec() } else { z80_unpack(raw, 16384) });
            at += used;
        }
        let page = |p: u8| pages.get(&p).cloned().unwrap_or_else(|| vec![0; 16384]);
        let (memory, banks) = if is128 {
            // Pages 3 to 10 are banks 0 to 7; bank 5 is at 0x4000 and bank 2 at 0x8000.
            let paged = data[35] & 7;
            let mut memory = page(8);
            memory.extend(page(5));
            memory.extend(page(paged + 3));
            ((memory), (0..8u8).map(|b| (b, page(b + 3))).collect())
        } else {
            let mut memory = page(8);
            memory.extend(page(4));
            memory.extend(page(5));
            (memory, Vec::new())
        };
        Ok(Self { kind: format!("ZX Spectrum snapshot (Z80 version {version}, {})", if is128 { "128K" } else { "48K" }), memory, banks })
    }

    fn peek16(&self, address: usize) -> usize {
        let at = address - 0x4000;
        self.memory.get(at..at + 2).map_or(0, |b| usize::from(u16::from_le_bytes([b[0], b[1]])))
    }

    /// The BASIC program in memory, from the system variables PROG and VARS, with the line it was running.
    fn program(&self) -> Option<Vec<u8>> {
        let (prog, vars, e_line) = (self.peek16(23635), self.peek16(23627), self.peek16(23641));
        if prog < 0x5CB6 || vars < prog || e_line < vars || e_line > 0xFFFF || vars - prog > 41_000 {
            return None;
        }
        // The variables run from VARS up to the 0x80 that ends them, just before E_LINE.
        let with_variables = &self.memory[prog - 0x4000..e_line - 0x4000];
        Some(as_plus3(with_variables, vars - prog, None))
    }

    fn files(&self) -> Vec<(String, String, Vec<u8>)> {
        let mut files = Vec::new();
        if let Some(program) = self.program() {
            files.push(("PROGRAM.bas".into(), "BASIC in memory (as a +3DOS file)".into(), program));
        }
        files.push(("SCREEN.scr".into(), "Screen".into(), self.memory[..6912].to_vec()));
        if self.banks.is_empty() {
            files.push(("MEMORY.bin".into(), "Memory from 16384".into(), self.memory.clone()));
        } else {
            files.extend(self.banks.iter().map(|(bank, data)| (format!("BANK {bank}.bin"), "Memory bank".into(), data.clone())));
        }
        files
    }
}

impl Volume for Snapshot {
    fn describe(&self) -> String {
        self.kind.clone()
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        if dir.is_some() {
            return Ok(Vec::new());
        }
        Ok(self
            .files()
            .into_iter()
            .enumerate()
            .map(|(index, (name, kind, data))| {
                let mut entry = Entry::file(name, data.len() as u64, index as u64);
                entry.kind = kind;
                entry
            })
            .collect())
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        self.files().into_iter().nth(entry.location as usize).map(|f| f.2).ok_or_else(|| "That isn't in the snapshot.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Volume, open};
    use super::*;

    fn volume_of(bytes: Vec<u8>, name: &str) -> Box<dyn Volume> {
        open(Arc::new(bytes), name).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    fn program() -> Vec<u8> {
        basic::tokenise_spectrum(&["10 PRINT \"HELLO\"", "20 GO TO 10"], basic::ZxDialect::Spectrum).unwrap()
    }

    #[test]
    fn a_tape_lists_its_files_and_gives_back_programs_as_plus3dos_files() {
        let mut tape = basic::spectrum_tape(&program(), "hello", Some(10));
        // A code block with a header, and one with none.
        let mut header = vec![0u8, 3];
        header.extend(b"pic       ");
        header.extend(6912u16.to_le_bytes());
        header.extend(16384u16.to_le_bytes());
        header.extend(32768u16.to_le_bytes());
        let block = |flag: u8, bytes: &[u8]| {
            let mut out = ((bytes.len() + 2) as u16).to_le_bytes().to_vec();
            out.push(flag);
            out.extend_from_slice(bytes);
            out.push(bytes.iter().fold(flag, |a, b| a ^ b));
            out
        };
        tape.extend(block(0, &header[1..]));
        tape.extend(block(0xFF, &[7u8; 6912]));
        tape.extend(block(0xFF, &[1, 2, 3]));
        let volume = volume_of(tape, "game.tap");
        let files = volume.list(None).unwrap();
        assert_eq!(files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["hello.bas", "pic.scr", "Block 5.bin"]);
        let listing = basic::list(&volume.read(&files[0]).unwrap(), "bas", charset::utf8());
        assert_eq!(listing.lines, ["  10 PRINT \"HELLO\"", "  20 GO TO 10"]);
        assert!(listing.info.iter().any(|(k, v)| k == "Runs from line" && v == "10"));
        assert_eq!(volume.read(&files[1]).unwrap().len(), 6912);
    }

    #[test]
    fn a_trdos_disk_and_an_scl_archive_give_back_their_files() {
        let program = program();
        // A TRD: the catalogue, the disk information sector, then the program on track 1.
        let mut disk = vec![0u8; 2 * 16 * 256 * 3];
        let mut basic_file = program.clone();
        basic_file.extend([0x80, 0xAA, 10, 0]);
        let entry: Vec<u8> = [&b"HELLO   B"[..], &(program.len() as u16).to_le_bytes(), &(program.len() as u16).to_le_bytes(), &[1, 0, 1]].concat();
        disk[..16].copy_from_slice(&entry);
        disk[0x8E3] = 0x16;
        disk[0x8E4] = 1;
        disk[0x8E7] = 0x10;
        disk[0x8F5..0x8FD].copy_from_slice(b"MYDISK  ");
        disk[16 * 256..16 * 256 + basic_file.len()].copy_from_slice(&basic_file);
        let trd = volume_of(disk, "disk.trd");
        assert_eq!(trd.describe(), "TR-DOS disk (TRD) \u{B7} MYDISK");
        let files = trd.list(None).unwrap();
        assert_eq!((files[0].name.as_str(), files[0].open_name.as_str()), ("HELLO.B", "HELLO.bas"));
        let listing = basic::list(&trd.read(&files[0]).unwrap(), "bas", charset::utf8());
        assert_eq!(listing.lines[0], "  10 PRINT \"HELLO\"");
        assert!(listing.info.iter().any(|(k, v)| k == "Runs from line" && v == "10"), "the 0x80 0xAA autostart");
        // The same file in an SCL archive.
        let mut scl = b"SINCLAIR\x01".to_vec();
        scl.extend(&entry[..14]);
        scl.extend(&basic_file);
        scl.resize(9 + 14 + 256, 0);
        let archive = volume_of(scl, "disk.scl");
        assert_eq!(basic::list(&archive.read(&archive.list(None).unwrap()[0]).unwrap(), "bas", charset::utf8()).lines[1], "  20 GO TO 10");
    }

    #[test]
    fn a_microdrive_cartridge_gives_back_its_files() {
        let program = program();
        let mut cartridge = vec![0u8; 254 * 543 + 1];
        let mut record = vec![0u8; 543];
        record[0] = 1;
        record[1] = 254;
        record[4..14].copy_from_slice(b"CARTRIDGE ");
        record[15] = 2;
        let mut body = vec![0u8];
        body.extend((program.len() as u16).to_le_bytes());
        body.extend(23755u16.to_le_bytes());
        body.extend((program.len() as u16).to_le_bytes());
        body.extend(20u16.to_le_bytes());
        body.extend(&program);
        record[17..19].copy_from_slice(&(body.len() as u16).to_le_bytes());
        record[19..29].copy_from_slice(b"run       ");
        record[30..30 + body.len()].copy_from_slice(&body);
        cartridge[..543].copy_from_slice(&record);
        let drive = volume_of(cartridge, "tape.mdr");
        assert_eq!(drive.describe(), "ZX Microdrive cartridge (MDR) \u{B7} CARTRIDGE");
        let files = drive.list(None).unwrap();
        assert_eq!(files[0].open_name, "run.bas");
        let listing = basic::list(&drive.read(&files[0]).unwrap(), "bas", charset::utf8());
        assert_eq!(listing.lines, ["  10 PRINT \"HELLO\"", "  20 GO TO 10"]);
    }

    #[test]
    fn a_snapshot_gives_back_its_program_screen_and_memory() {
        let program = program();
        let mut memory = vec![0u8; 49152];
        let prog = 23755usize;
        memory[prog - 0x4000..prog - 0x4000 + program.len()].copy_from_slice(&program);
        let vars = prog + program.len();
        memory[23635 - 0x4000..23637 - 0x4000].copy_from_slice(&(prog as u16).to_le_bytes());
        memory[23627 - 0x4000..23629 - 0x4000].copy_from_slice(&(vars as u16).to_le_bytes());
        memory[23641 - 0x4000..23643 - 0x4000].copy_from_slice(&((vars + 1) as u16).to_le_bytes());
        memory[vars - 0x4000] = 0x80;
        let mut sna = vec![0u8; 27];
        sna.extend(&memory);
        let snapshot = volume_of(sna, "game.sna");
        let files = snapshot.list(None).unwrap();
        assert_eq!(files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["PROGRAM.bas", "SCREEN.scr", "MEMORY.bin"]);
        assert_eq!(basic::list(&snapshot.read(&files[0]).unwrap(), "bas", charset::utf8()).lines, ["  10 PRINT \"HELLO\"", "  20 GO TO 10"]);
        // The same memory as a version 1 Z80 snapshot, compressed.
        let mut z80 = vec![0u8; 30];
        z80[6] = 0x00;
        z80[7] = 0x80;
        z80[12] = 0x20;
        let mut packed = Vec::new();
        let mut i = 0;
        while i < memory.len() {
            let run = memory[i..].iter().take(255).take_while(|b| **b == memory[i]).count();
            if run >= 5 || memory[i] == 0xED {
                packed.extend([0xED, 0xED, run as u8, memory[i]]);
                i += run;
            } else {
                packed.push(memory[i]);
                i += 1;
            }
        }
        packed.extend([0x00, 0xED, 0xED, 0x00]);
        z80.extend(packed);
        let snapshot = volume_of(z80, "game.z80");
        assert!(snapshot.describe().contains("version 1"));
        let program_file = snapshot.read(&snapshot.list(None).unwrap()[0]).unwrap();
        assert_eq!(basic::list(&program_file, "bas", charset::utf8()).lines[0], "  10 PRINT \"HELLO\"");
    }

    /// Every BASIC program on the real Spectrum media in ../testmedia, listed, for comparing with Fuse's listbasic.
    #[test]
    fn real_spectrum_media_list_their_programs() {
        let root = std::path::Path::new("../testmedia");
        if !root.is_dir() {
            return;
        }
        let mut out = String::new();
        let mut folders = vec![root.to_path_buf()];
        let mut images = Vec::new();
        while let Some(folder) = folders.pop() {
            for item in std::fs::read_dir(&folder).unwrap().flatten() {
                let path = item.path();
                let ending = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
                if path.is_dir() {
                    folders.push(path);
                } else if ["tap", "tzx", "z80", "sna", "trd", "scl", "mdr"].contains(&ending.as_str()) {
                    images.push(path);
                }
            }
        }
        images.sort();
        for path in images {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let volume = open(Arc::new(std::fs::read(&path).unwrap()), &name).unwrap();
            for entry in volume.list(None).unwrap().iter().filter(|e| e.open_name.ends_with(".bas")) {
                let bytes = volume.read(entry).unwrap();
                let listing = basic::list(&bytes, "bas", charset::utf8());
                // Every REM line tokenises back to the bytes it came from.
                if listing.dialect == basic::ZxDialect::Spectrum.name() {
                    let program = &bytes[128..128 + usize::from(u16::from_le_bytes([bytes[20], bytes[21]])).min(bytes.len() - 128)];
                    let mut at = 0;
                    for line in &listing.lines {
                        let length = usize::from(u16::from_le_bytes([program[at + 2], program[at + 3]]));
                        let original = &program[at..at + 4 + length];
                        at += 4 + length;
                        let rem = original.iter().position(|b| *b == 0xEA).unwrap_or(original.len());
                        if original[rem..].contains(&b':') && !original.contains(&0x0E) && !original.ends_with(b" \r") {
                            assert_eq!(basic::tokenise_spectrum(&[line.trim_start()], basic::ZxDialect::Spectrum).unwrap(), original, "{name}: {line}");
                        }
                    }
                }
                out.push_str(&format!("# {name} / {}{nl}", entry.name, nl = "\n"));
                for line in &listing.lines {
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
        std::fs::write(root.join("limoniter-listings.txt"), out).unwrap();
    }
}
