//! SAM Coupé disks, as SAMDOS, MasterDOS and BDOS write them, and the +D and DISCiPLE disks of the Spectrum that
//! they grew from: `.mgt` and `.dsk` images (each track's two sides in turn) and SAD images (Aley Keprt's, a
//! 22-byte header giving the shape, then side 0's tracks and side 1's).
//!
//! A disk is 80 tracks of 10 sectors of 512 bytes on each side; the DOS numbers side 1's tracks from 128. The first
//! four tracks of side 0 are the directory: 80 entries of 256 bytes, two to a sector (MasterDOS can have more
//! tracks of them, the count less four in the first entry's last byte). An entry is the file's type (bits 0 to 4;
//! bit 7 hidden, bit 6 protected), a ten-letter name, the number of sectors (high byte first), the first sector's
//! track and sector, a bitmap of every sector the file has, then what each type needs: a Spectrum file's tape
//! header at 211, a SAM file's start, length and where it runs (each a page and an offset) at 236, the date it was
//! saved at 245, and for MasterDOS the folder it's in at 254. A file's sectors are 510 bytes of it and then the
//! track and sector of the next. Most files start with a 9-byte header, the type and where the file goes.
//!
//! This follows Simon Owen's mgtdisklib and SAMdisk (github.com/simonowen) and the SAM Technical Manual. MasterDOS
//! files saved compressed (its SAVE modes 2 and 3) are given as they're stored.

use super::{Entry, Source, Volume, tidy_name};
use std::sync::Arc;

const SECTOR: usize = 512;
const SAD_SIGNATURE: &[u8] = b"Aley's disk backup";

/// How an image lays its sectors out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Shape {
    /// Bytes before the first sector.
    start: u64,
    /// Whether side 0's tracks all come first (SAD), rather than each track's two sides together (MGT).
    by_side: bool,
    tracks: u64,
    sectors: u64,
}

impl Shape {
    /// Where a sector is, from the DOS's track (side 1 from 128) and sector (from 1).
    fn offset(self, track: u8, sector: u8) -> Option<u64> {
        let (cylinder, side) = (u64::from(track & 0x7F), u64::from(track >> 7));
        if cylinder >= self.tracks || sector == 0 || u64::from(sector) > self.sectors {
            return None;
        }
        let index = if self.by_side { side * self.tracks + cylinder } else { cylinder * 2 + side } * self.sectors + u64::from(sector) - 1;
        Some(self.start + index * SECTOR as u64)
    }
}

/// The shape of a SAM image, from its size or SAD header.
fn shape_of(head: &[u8], len: u64) -> Option<Shape> {
    if head.starts_with(SAD_SIGNATURE) && head.len() >= 22 {
        let (sides, tracks, sectors, size) = (head[18], head[19], head[20], usize::from(head[21]) * 64);
        if sides != 2 || !(40..=83).contains(&tracks) || !(9..=10).contains(&sectors) || size != SECTOR {
            return None;
        }
        return Some(Shape { start: 22, by_side: true, tracks: u64::from(tracks), sectors: u64::from(sectors) });
    }
    (len == 819_200).then_some(Shape { start: 0, by_side: false, tracks: 80, sectors: 10 })
}

/// The file types, from the low five bits of an entry's first byte.
fn type_name(kind: u8) -> Option<&'static str> {
    Some(match kind {
        1 => "ZX BASIC",
        2 => "ZX number array",
        3 => "ZX character array",
        4 => "ZX CODE",
        5 => "ZX 48K snapshot",
        6 => "ZX Microdrive file",
        7 => "ZX SCREEN$",
        8 => "Special",
        9 => "ZX 128K snapshot",
        10 => "Opentype",
        11 => "ZX execute",
        12 => "UniDOS folder",
        13 => "UniDOS create",
        16 => "SAM BASIC",
        17 => "SAM number array",
        18 => "SAM string array",
        19 => "SAM CODE",
        20 => "SAM SCREEN$",
        21 => "Folder",
        22 => "Driver application",
        23 => "Driver boot",
        24 => "EDOS nomen",
        25 => "EDOS system",
        26 => "EDOS overlay",
        28 => "HDOS DOS",
        29 => "HDOS folder",
        30 => "HDOS disk",
        31 => "HDOS temporary",
        _ => return None,
    })
}

/// Whether a directory entry is in use and makes sense: a known type, a name, and a first sector on the disk.
fn sensible(entry: &[u8], shape: Shape) -> Option<bool> {
    let kind = entry[0] & 0x1F;
    if kind == 0 {
        return Some(false);
    }
    let name_ok = entry[1..11].iter().all(|&b| b & 0x7F >= 0x20 || b == 0);
    let sectors = u16::from_be_bytes([entry[11], entry[12]]);
    let starts = kind == 21 || shape.offset(entry[13], entry[14]).is_some();
    (type_name(kind).is_some() && name_ok && sectors <= 1600 && starts).then_some(true)
}

/// Whether an image is a SAM Coupé or +D disk: its first directory sector has at least one file in it, and nothing
/// that can't be a file.
pub fn looks_like(head: &[u8], len: u64) -> bool {
    let Some(shape) = shape_of(head, len) else {
        return false;
    };
    let Some(first) = shape.offset(0, 1).and_then(|at| head.get(at as usize..at as usize + SECTOR)) else {
        return false;
    };
    let entries: Vec<Option<bool>> = first.chunks(256).map(|entry| sensible(entry, shape)).collect();
    entries.iter().all(Option::is_some) && entries.contains(&Some(true))
}

struct File {
    entry: Vec<u8>,
    name: String,
    /// The folder it's in (a MasterDOS folder's tag), or 0.
    folder: u8,
}

pub struct Sam {
    source: Arc<dyn Source>,
    shape: Shape,
    files: Vec<File>,
    what: String,
    notes: Vec<String>,
}

/// A page and an offset, as SAM entries keep lengths: whole 16K pages and what's left.
fn length_of(triple: &[u8]) -> u64 {
    u64::from(triple[0] & 0x1F) * 16384 + u64::from(u16::from_le_bytes([triple[1], triple[2] & 0x7F]))
}

/// A start address as SAM entries keep it: a page, then an offset in the 32K to 48K window it's seen through.
fn address_of(triple: &[u8]) -> u64 {
    u64::from(triple[0] & 0x1F) * 16384 + u64::from(u16::from_le_bytes([triple[1], triple[2] & 0x7F])) + 0x4000
}

impl Sam {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let head = source.read_vec(0, source.len().min(SECTOR as u64 * 2) as usize)?;
        let shape = shape_of(&head, source.len()).ok_or("This isn't the size or shape of a SAM Coupé disk.")?;
        let sector = |track: u8, number: u8| -> Option<Vec<u8>> { shape.offset(track, number).and_then(|at| source.read_vec(at, SECTOR).ok()) };
        let first = sector(0, 1).ok_or("The disk's first sector can't be read.")?;
        // MasterDOS keeps a label at 210 and the number of extra directory tracks at 255; BDOS writes its name at 232.
        let (dos, tracks) = if &first[232..236] == b"BDOS" {
            ("BDOS", 4)
        } else if first[210] != 0 && first[210] != 0xFF {
            ("MasterDOS", 4 + usize::from(first[255]).min(35))
        } else {
            ("SAMDOS", 4)
        };
        let mut files = Vec::new();
        let mut notes = Vec::new();
        let mut nonsense = 0;
        'directory: for track in 0..tracks as u8 {
            for number in 1..=shape.sectors as u8 {
                // With more than four tracks of directory, track 4's first sector is the boot sector.
                if track == 4 && number == 1 {
                    continue;
                }
                let Some(data) = sector(track, number) else {
                    notes.push(format!("The directory's track {track}, sector {number} isn't on the disk."));
                    break 'directory;
                };
                for entry in data.chunks(256) {
                    match sensible(entry, shape) {
                        // The directory ends at the first entry never used; an erased one has its name still.
                        Some(false) if entry[1] == 0 => break 'directory,
                        Some(false) => continue,
                        Some(true) => {}
                        None => {
                            nonsense += 1;
                            continue;
                        }
                    }
                    let name: String = entry[1..11].iter().map(|&b| char::from(b & 0x7F)).collect::<String>().trim_end_matches([' ', '\0']).to_string();
                    let kind = entry[0] & 0x1F;
                    let folder = if kind >= 16 && kind != 21 && !matches!(entry[254], 0 | 0xFF) { entry[254] } else { 0 };
                    files.push(File { entry: entry.to_vec(), name, folder });
                }
            }
        }
        if nonsense > 0 {
            notes.push(format!("{nonsense} directory entries that can't be files (an unknown type, or a first sector off the disk) are left out."));
        }
        let sam = files.iter().any(|f| f.entry[0] & 0x1F >= 16);
        let label: String = if dos == "MasterDOS" && first[210] != b'*' { first[210..220].iter().map(|&b| char::from(b & 0x7F)).collect::<String>().trim_end().to_string() } else { String::new() };
        let label = if label.chars().all(|c| c.is_ascii_graphic() || c == ' ') && !label.is_empty() { format!(" \"{label}\"") } else { String::new() };
        let format = if shape.by_side { "SAD" } else { "MGT" };
        let what = if sam { format!("SAM Coupé {dos} disk{label} ({format})") } else { format!("+D / DISCiPLE disk ({format})") };
        Ok(Self { source, shape, files, what, notes })
    }

    /// A file's bytes as the DOS reads them: along its chain of sectors, each one's last two bytes saying which is
    /// next, for as many sectors as its entry says. A chain that loops or leaves the disk is an error.
    fn chain(&self, file: &File) -> Result<Vec<u8>, String> {
        let entry = &file.entry;
        let count = usize::from(u16::from_be_bytes([entry[11], entry[12]]));
        let (mut track, mut number) = (entry[13], entry[14]);
        let mut out = Vec::with_capacity(count * 510);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..count {
            if !seen.insert((track, number)) {
                return Err(format!("{}'s chain of sectors comes back on itself.", file.name));
            }
            let at = self.shape.offset(track, number).ok_or_else(|| format!("{}'s sectors lead to track {track}, sector {number}, which isn't on the disk.", file.name))?;
            let data = self.source.read_vec(at, SECTOR)?;
            out.extend_from_slice(&data[..510]);
            (track, number) = (data[510], data[511]);
            if track == 0 && number == 0 {
                break;
            }
        }
        Ok(out)
    }

    /// A file's bytes from its sector map instead: a bit for each sector from track 4 on (side 1's tracks after
    /// side 0's), lowest bit first, taken in order. Specials are read this way, whole sectors with no links; for
    /// other files it's what's left when the chain is broken.
    fn mapped(&self, file: &File, chunk: usize) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        for index in (0..1560).filter(|i| file.entry[15 + i / 8] & (1 << (i % 8)) != 0) {
            let track_index = 4 + index / 10;
            let track = (track_index % 80) as u8 | if track_index >= 80 { 0x80 } else { 0 };
            let at = self.shape.offset(track, (index % 10) as u8 + 1).ok_or_else(|| format!("{}'s sector map has sectors that aren't on the disk.", file.name))?;
            out.extend_from_slice(&self.source.read_vec(at, SECTOR)?[..chunk]);
        }
        Ok(out)
    }

    fn entry_for(&self, index: usize, file: &File) -> Entry {
        let entry = &file.entry;
        let kind = entry[0] & 0x1F;
        let word = |at: usize| u16::from_le_bytes([entry[at], entry[at + 1]]);
        let sectors = u64::from(u16::from_be_bytes([entry[11], entry[12]]));
        if kind == 21 {
            return Entry { kind: "Folder".into(), ..Entry::dir(tidy_name(&file.name), index as u64, u64::from(entry[250])) };
        }
        let name = tidy_name(&file.name);
        let mut out = Entry::file(name.clone(), sectors * 510, index as u64);
        out.kind = type_name(kind).unwrap_or("File").to_string();
        if entry[0] & 0x80 != 0 {
            out.kind.push_str(", hidden");
        }
        if entry[0] & 0x40 != 0 {
            out.kind.push_str(", protected");
        }
        let zx_length = u64::from(entry[210]) * 0x10000 + u64::from(word(212));
        let (ending, size, detail) = match kind {
            1 => ("bas", zx_length, if entry[219] == 0xFF { String::new() } else { format!("runs from line {}", word(218)) }),
            4 => ("code", zx_length, format!("{zx_length} bytes at {}", word(214))),
            7 => ("scr", zx_length, String::new()),
            2 | 3 => ("dat", zx_length, String::new()),
            16 => ("bas", 9 + length_of(&entry[239..242]), if entry[242] == 0xFF { String::new() } else { format!("runs from line {}", u16::from_le_bytes([entry[243], entry[244]])) }),
            19 => {
                let run = if entry[242] == 0xFF { String::new() } else { format!(", runs at {}", length_of(&entry[242..245])) };
                ("bin", length_of(&entry[239..242]), format!("loads at {}{run}", address_of(&entry[236..239])))
            }
            20 => ("bin", length_of(&entry[239..242]), format!("mode {}", 1 + (entry[221] & 3))),
            22 | 23 => ("bin", length_of(&entry[239..242]), String::new()),
            // Snapshots: only UniDOS says how long; otherwise 48K, or 128K and the paging byte.
            5 => ("bin", if zx_length == 0 { 0xC000 } else { zx_length }, String::new()),
            9 => ("bin", if zx_length == 0 { 0x20001 } else { zx_length }, String::new()),
            17 | 18 => ("dat", length_of(&entry[239..242]), String::new()),
            // Specials are whole sectors; an Opentype file's length is kept as a Spectrum file's is.
            8 => ("bin", sectors * SECTOR as u64, String::new()),
            10 | 13 => ("bin", zx_length, String::new()),
            _ => ("bin", sectors * 510, String::new()),
        };
        out.open_name = format!("{name}.{ending}");
        // A SAM BASIC program whose first sector doesn't start with its header has been overwritten: it's given as
        // plain bytes, so it isn't listed as some other BASIC.
        if kind == 16 {
            let header = self.shape.offset(entry[13], entry[14]).and_then(|at| self.source.read_vec(at, 9).ok());
            if header.is_none_or(|header| header[0] != 16 || header[1..3] != entry[240..242] || header[7] != entry[239]) {
                out.open_name = format!("{name}.bin");
                out.kind.push_str(", overwritten");
            }
        }
        out.size = size;
        // SAM files keep the date they were saved: day, month, year from 1900, hour, minute.
        let date = &entry[245..250];
        if kind >= 16 && (1..=31).contains(&date[0]) && (1..=12).contains(&(date[1] & 0x7F)) && date[2] != 0xFF && date[1] & 0x80 == 0 {
            let year = if date[2] < 80 { 2000 + u32::from(date[2]) } else { 1900 + u32::from(date[2]) };
            let when = format!("{:02}/{:02}/{year} {:02}:{:02}", date[0], date[1], date[3], date[4]);
            out.detail = if detail.is_empty() { when } else { format!("{detail}, {when}") };
        } else {
            out.detail = detail;
        }
        out
    }
}

impl Volume for Sam {
    fn describe(&self) -> String {
        self.what.clone()
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        // MasterDOS folders: a folder's entry has its tag at 250, and each file in it has that tag at 254. A file whose
        // tag no folder has is shown at the top.
        let folders: Vec<u8> = self.files.iter().filter(|f| f.entry[0] & 0x1F == 21).map(|f| f.entry[250]).collect();
        let tag = dir.map_or(0, |dir| dir.extra as u8);
        let mut entries: Vec<Entry> = self
            .files
            .iter()
            .enumerate()
            .filter(|(_, file)| {
                let folder = if folders.contains(&file.folder) { file.folder } else { 0 };
                if file.entry[0] & 0x1F == 21 { tag == 0 } else { folder == tag }
            })
            .map(|(index, file)| self.entry_for(index, file))
            .collect();
        entries.sort_by_key(|entry| !entry.is_dir);
        Ok(entries)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let file = self.files.get(entry.location as usize).ok_or("That file isn't on the disk.")?;
        let kind = file.entry[0] & 0x1F;
        let wanted = entry.size as usize;
        let data = if kind == 8 {
            self.mapped(file, SECTOR)?
        } else {
            // The chain is what the DOS follows; if it's broken or ends early, the sector map may still say where
            // the rest of the file is, as mgtdisklib finds.
            let needed = wanted + if matches!(kind, 1 | 16) { 0 } else { 9 };
            let chain = self.chain(file);
            match chain {
                Ok(data) if data.len() >= needed => data,
                Ok(data) => match self.mapped(file, 510) {
                    Ok(map) if map.starts_with(&data) || map.len() > data.len() => map,
                    _ => data,
                },
                Err(problem) => self.mapped(file, 510).ok().filter(|data| data.len() >= wanted).ok_or(problem)?,
            }
        };
        // Files with a 9-byte header: Spectrum ones are given without it, a SAM BASIC program with it, so it can be
        // told for what it is; other SAM files without.
        let has_header = matches!(kind, 1 | 2 | 3 | 4 | 7 | 16..=20 | 22 | 23);
        let body = if has_header { data.get(9..).unwrap_or_default() } else { &data[..] };
        let short = |have: usize| format!("{} should be {wanted} bytes, but its sectors hold only {have}.", file.name);
        match kind {
            1 => {
                // As a +3DOS file, so it lists as the Spectrum's BASIC does; the entry says where the program ends and
                // the variables begin, and the line it runs from.
                let program = body.get(..wanted).ok_or_else(|| short(body.len()))?;
                let length = usize::from(u16::from_le_bytes([file.entry[216], file.entry[217]])).min(program.len());
                let run = (file.entry[219] != 0xFF).then(|| u16::from_le_bytes([file.entry[218], file.entry[219]]));
                Ok(crate::basic::plus3dos_basic(program, length, run))
            }
            16 => data.get(..wanted).map(<[u8]>::to_vec).ok_or_else(|| short(data.len())),
            _ => Ok(body[..wanted.min(body.len())].to_vec()),
        }
    }

    fn notes(&self) -> Vec<String> {
        let mut notes = self.notes.clone();
        if self.files.iter().any(|f| f.entry[0] & 0x1F >= 16 && f.entry[0] & 0x1F <= 20 && matches!((f.entry[220] >> 2) & 3, 1 | 3)) {
            notes.push("Some files were saved compressed by MasterDOS, and are given as they're stored.".into());
        }
        notes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An MGT image: side 0's track 0 holds the directory; files from track 4.
    fn disk() -> Vec<u8> {
        let mut image = vec![0u8; 819_200];
        let shape = Shape { start: 0, by_side: false, tracks: 80, sectors: 10 };
        let at = |track: u8, sector: u8| shape.offset(track, sector).unwrap() as usize;
        // A SAM BASIC program, 10 PRINT "HI", in two sectors: its header, then the program and its &FF.
        let program = [0x00, 0x0A, 0x06, 0x00, 0xBB, b'"', b'H', b'I', b'"', 0x0D, 0xFF];
        let mut file = vec![16, program.len() as u8, 0, 0x00, 0x80, 0, 0, 0, 1];
        file.extend(program);
        let entry = at(0, 1);
        image[entry] = 16;
        image[entry + 1..entry + 11].copy_from_slice(b"hello     ");
        image[entry + 11..entry + 15].copy_from_slice(&[0, 1, 4, 1]);
        image[entry + 239..entry + 242].copy_from_slice(&[0, program.len() as u8, 0]);
        image[entry + 242..entry + 245].copy_from_slice(&[0, 10, 0]);
        image[entry + 245..entry + 250].copy_from_slice(&[13, 11, 126, 9, 30]);
        let data = at(4, 1);
        image[data..data + file.len()].copy_from_slice(&file);
        // A SAM CODE file of 600 bytes at 32768, over two sectors on side 1.
        let code = entry + 256;
        image[code] = 19 | 0x80;
        image[code + 1..code + 11].copy_from_slice(b"code      ");
        image[code + 11..code + 15].copy_from_slice(&[0, 2, 0x84, 5]);
        image[code + 236..code + 239].copy_from_slice(&[1, 0x00, 0x80]);
        image[code + 239..code + 242].copy_from_slice(&[0, 0x58, 0x02]);
        image[code + 242..code + 245].copy_from_slice(&[0xFF, 0xFF, 0xFF]);
        let first = at(0x84, 5);
        image[first..first + 9].copy_from_slice(&[19, 0x58, 0x02, 0x00, 0x80, 0, 0, 0, 1]);
        image[first + 9..first + 510].fill(0xAA);
        image[first + 510..first + 512].copy_from_slice(&[0x84, 6]);
        let second = at(0x84, 6);
        image[second..second + 99].fill(0xBB);
        image
    }

    #[test]
    fn a_sam_disk_lists_its_files_and_reads_them_along_their_chains() {
        let image = disk();
        assert!(looks_like(&image[..0x8800], image.len() as u64));
        let sam = super::super::open(Arc::new(image), "disk.mgt").unwrap();
        assert!(sam.describe().contains("SAM Coupé SAMDOS disk"), "{}", sam.describe());
        let files = sam.list(None).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!((files[0].open_name.as_str(), files[0].kind.as_str()), ("hello.bas", "SAM BASIC"));
        assert_eq!(files[0].detail, "runs from line 10, 13/11/2026 09:30");
        let program = sam.read(&files[0]).unwrap();
        assert_eq!(program.len(), 9 + 11);
        assert_eq!(crate::basic::list(&program, "bas", crate::charset::utf8()).lines, ["10 PRINT \"HI\""]);
        assert_eq!((files[1].kind.as_str(), files[1].detail.as_str(), files[1].size), ("SAM CODE, hidden", "loads at 32768", 600));
        let code = sam.read(&files[1]).unwrap();
        assert_eq!(code.len(), 600);
        assert!(code[..501].iter().all(|&b| b == 0xAA) && code[501..].iter().all(|&b| b == 0xBB), "the second sector follows the link");
    }

    #[test]
    fn a_sad_image_is_read_side_by_side_and_damage_is_said() {
        let mgt = disk();
        let mut sad = SAD_SIGNATURE.to_vec();
        sad.extend([2, 80, 10, 8]);
        // Re-laid: side 0's tracks, then side 1's.
        let mgt_shape = Shape { start: 0, by_side: false, tracks: 80, sectors: 10 };
        let sad_shape = Shape { start: 22, by_side: true, tracks: 80, sectors: 10 };
        sad.resize(22 + 819_200, 0);
        for track in (0..80u8).chain(128..208) {
            for sector in 1..=10 {
                let (from, to) = (mgt_shape.offset(track, sector).unwrap() as usize, sad_shape.offset(track, sector).unwrap() as usize);
                let bytes = mgt[from..from + SECTOR].to_vec();
                sad[to..to + SECTOR].copy_from_slice(&bytes);
            }
        }
        let image = super::super::open(Arc::new(sad), "disk.sad").unwrap();
        assert!(image.describe().contains("SAD"));
        let files = image.list(None).unwrap();
        assert_eq!(image.read(&files[1]).unwrap().len(), 600);
        // A chain that leads off the disk.
        let mut bad = disk();
        let first = mgt_shape.offset(0x84, 5).unwrap() as usize;
        bad[first + 510] = 99;
        let image = super::super::open(Arc::new(bad), "bad.mgt").unwrap();
        let files = image.list(None).unwrap();
        assert!(image.read(&files[1]).unwrap_err().contains("isn't on the disk"));
        assert!(!looks_like(&[0u8; 0x8800], 819_200), "an empty directory isn't believed");
    }
}
