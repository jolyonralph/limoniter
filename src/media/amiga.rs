//! Amiga media: AmigaDOS's file system, on floppies (.adf, and .adz gzipped) and hard disk images (.hdf, a single
//! partition or a whole disk with a Rigid Disk Block and its partitions).
//!
//! The layout is the one the ADF-FAQ ("The .ADF (Amiga Disk File) format FAQ", Laurent Clévy) sets out, with the long
//! name variant (DOS\6 and DOS\7, from AmigaOS 3.2) as amitools reads it: every block is big-endian longs; the root
//! block, in the middle of the volume, has a hash table of the entries in the top folder, each the head of a chain;
//! a folder's header block has the same; a file's header lists its data blocks from the end of its table backwards,
//! with extension blocks for more. The Old File System's data blocks start with a 24-byte header; the Fast File
//! System's are all data.

use super::{Entry, MOST_FILE_BYTES, Slice, Source, Volume, tidy_name};
use std::collections::HashSet;
use std::sync::Arc;

const T_HEADER: u32 = 2;
const T_LIST: u32 = 16;
const T_DATA: u32 = 8;
const ST_ROOT: u32 = 1;
const ST_USERDIR: u32 = 2;
const ST_SOFTLINK: u32 = 3;
const ST_LINKDIR: u32 = 4;
const ST_FILE: u32 = (-3i32) as u32;
const ST_LINKFILE: u32 = (-4i32) as u32;
/// More than any real volume would have, so a damaged disk with a loop in it stops.
const MOST_BLOCKS_FOLLOWED: usize = 1 << 22;

/// A date as AmigaDOS keeps it: days since 1 January 1978, minutes, and ticks of a fiftieth of a second.
fn amiga_date(days: u32, minutes: u32, ticks: u32) -> String {
    // Days since 1970, then the civil date (Howard Hinnant's days_from_civil, inverted).
    let z = i64::from(days) + 2922 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year}-{month:02}-{day:02} {:02}:{:02}:{:02}", minutes / 60 % 24, minutes % 60, ticks / 50 % 60)
}

/// The protection bits as `list` shows them: h, s, p and a when set; r, w, e and d when allowed (their bits clear).
fn protection(bits: u32) -> String {
    let set = |bit: u32, c: char| if bits & (1 << bit) != 0 { c } else { '-' };
    let allowed = |bit: u32, c: char| if bits & (1 << bit) == 0 { c } else { '-' };
    [set(7, 'h'), set(6, 's'), set(5, 'p'), set(4, 'a'), allowed(3, 'r'), allowed(2, 'w'), allowed(1, 'e'), allowed(0, 'd')].iter().collect()
}

/// A BCPL string: a length byte, then the characters, in ISO 8859-1 as the Amiga has them.
fn bstr(bytes: &[u8], most: usize) -> String {
    let length = usize::from(bytes.first().copied().unwrap_or(0)).min(most).min(bytes.len().saturating_sub(1));
    bytes[1..1 + length].iter().map(|b| char::from(*b)).collect()
}

/// An AmigaDOS volume.
pub struct AmigaFs {
    source: Arc<dyn Source>,
    block_size: usize,
    blocks: u64,
    root: u32,
    dos_type: u8,
    name: String,
    what: String,
}

impl AmigaFs {
    /// Opens a volume: `what` says where it is (a floppy, a partition), for the heading.
    pub fn open(source: Arc<dyn Source>, block_size: usize, what: &str) -> Result<Self, String> {
        let boot = source.read_vec(0, 12.min(source.len() as usize))?;
        if boot.len() < 4 {
            return Err("This Amiga image is too short to hold a disk.".into());
        }
        match &boot[..3] {
            b"DOS" if boot[3] <= 7 => {}
            b"NDO" => return Err("This Amiga disk isn't in AmigaDOS's format (\"NDOS\"): it's probably a game or demo that reads the disk its own way. It can still be shown in hex.".into()),
            b"KIC" => return Err("This is a Kickstart disk, holding the A1000's ROM, not files. It can still be shown in hex.".into()),
            b"PFS" | b"PDS" | b"SFS" | b"FFS" => return Err(format!("This volume uses {} rather than AmigaDOS's own file system, which Limoniter doesn't read yet.", String::from_utf8_lossy(&boot[..3]))),
            _ if boot.starts_with(b"DOS") => return Err(format!("This AmigaDOS volume is of a kind (DOS\\{}) Limoniter doesn't know.", boot[3])),
            _ => return Err("This Amiga image has no AmigaDOS boot block: it's probably a game or demo that reads the disk its own way. It can still be shown in hex.".into()),
        }
        let blocks = source.len() / block_size as u64;
        let mut volume = Self { source, block_size, blocks, root: 0, dos_type: boot[3], name: String::new(), what: what.to_string() };
        // The root is in the middle; the boot block says where too, which a disk formatted oddly may need.
        let given = u32::from_be_bytes([boot[8], boot[9], boot[10], boot[11]]);
        for candidate in [(blocks / 2) as u32, given] {
            if let Ok(block) = volume.block(candidate)
                && volume.long(&block, 0) == T_HEADER
                && volume.tail(&block, 1) == ST_ROOT
            {
                volume.root = candidate;
                volume.name = bstr(&block[block_size - 80..], 30);
                return Ok(volume);
            }
        }
        Err("This disk has an AmigaDOS boot block but no root block where AmigaDOS keeps one, so it has no files to list: it's probably a game or demo that loads itself its own way. It can still be shown in hex.".into())
    }

    fn block(&self, number: u32) -> Result<Vec<u8>, String> {
        if u64::from(number) >= self.blocks || number == 0 {
            return Err(format!("block {number} is outside the volume"));
        }
        self.source.read_vec(u64::from(number) * self.block_size as u64, self.block_size)
    }

    fn long(&self, block: &[u8], index: usize) -> u32 {
        block.get(index * 4..index * 4 + 4).map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A long counted back from the end of the block, as the FAQ numbers the fields after the table.
    fn tail(&self, block: &[u8], back: usize) -> u32 {
        self.long(block, self.block_size / 4 - back)
    }

    fn table_size(&self) -> usize {
        self.block_size / 4 - 56
    }

    fn long_names(&self) -> bool {
        self.dos_type >= 6
    }

    /// The entries in a folder: every hash chain from its table.
    fn entries(&self, folder: u32) -> Result<Vec<Entry>, String> {
        let header = self.block(folder)?;
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for slot in 0..self.table_size() {
            let mut next = self.long(&header, 6 + slot);
            while next != 0 {
                if !seen.insert(next) || seen.len() > MOST_BLOCKS_FOLLOWED {
                    return Err("A folder's hash chains loop, so the disk is damaged.".into());
                }
                let block = self.block(next)?;
                if self.long(&block, 0) != T_HEADER {
                    return Err(format!("Block {next} should be a file or folder's header, and isn't."));
                }
                out.push(self.entry(next, &block)?);
                next = self.tail(&block, 4);
            }
        }
        // Folders first, then files, by name as the Workbench sorts them.
        out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(out)
    }

    fn entry(&self, number: u32, block: &[u8]) -> Result<Entry, String> {
        let size = self.block_size;
        let (name, comment, date_at) = if self.long_names() {
            // Name and comment as two BCPL strings together; a comment that doesn't fit is in a block of its own.
            let both = &block[size - 184..size - 72];
            let name = bstr(both, 107);
            let rest = &both[(1 + usize::from(both[0])).min(both.len())..];
            (name, bstr(rest, 79), 15)
        } else {
            (bstr(&block[size - 80..], 30), bstr(&block[size - 184..], 79), 23)
        };
        let date = amiga_date(self.tail(block, date_at), self.tail(block, date_at - 1), self.tail(block, date_at - 2));
        let kind = self.tail(block, 1);
        let mut entry = match kind {
            ST_USERDIR | ST_LINKDIR => Entry::dir(tidy_name(&name), u64::from(number), 0),
            _ => Entry::file(tidy_name(&name), u64::from(self.tail(block, 47)), u64::from(number)),
        };
        entry.kind = match kind {
            ST_USERDIR => "Folder".into(),
            ST_LINKDIR => "Folder (hard link)".into(),
            ST_LINKFILE => "Hard link".into(),
            ST_SOFTLINK => "Soft link".into(),
            ST_FILE if name.to_lowercase().ends_with(".info") => "Workbench icon".into(),
            ST_FILE => "File".into(),
            other => format!("Entry type {}", other as i32),
        };
        // A hard link's size is its target's.
        if kind == ST_LINKFILE
            && let Ok(target) = self.block(self.tail(block, 11))
        {
            entry.size = u64::from(self.tail(&target, 47));
        }
        if kind == ST_SOFTLINK {
            entry.size = block[24..size - 200].iter().position(|b| *b == 0).unwrap_or(0) as u64;
        }
        entry.detail = format!("{date}  {}{}", protection(self.tail(block, 48)), if comment.is_empty() { String::new() } else { format!("  \u{201C}{comment}\u{201D}") });
        Ok(entry)
    }

    /// Where a folder's entries are: its own header, or a hard link's target.
    fn folder_block(&self, entry: &Entry) -> Result<u32, String> {
        let number = entry.location as u32;
        let block = self.block(number)?;
        Ok(if self.tail(&block, 1) == ST_LINKDIR { self.tail(&block, 11) } else { number })
    }

    fn read_file(&self, number: u32) -> Result<Vec<u8>, String> {
        let mut header = self.block(number)?;
        match self.tail(&header, 1) {
            ST_LINKFILE => header = self.block(self.tail(&header, 11))?,
            ST_SOFTLINK => {
                let text = &header[24..self.block_size - 200];
                return Ok(text[..text.iter().position(|b| *b == 0).unwrap_or(0)].to_vec());
            }
            _ => {}
        }
        let size = u64::from(self.tail(&header, 47));
        if size > MOST_FILE_BYTES {
            return Err("This file is larger than the 1 GB that can be opened.".into());
        }
        let ofs = self.dos_type & 1 == 0;
        let mut out = Vec::with_capacity(size as usize);
        let mut list = header;
        let mut seen = HashSet::new();
        // Each list (the header, then its extension blocks) gives its data blocks from the end of its table back.
        'lists: loop {
            let count = (self.long(&list, 2) as usize).min(self.table_size());
            for i in 0..count {
                if out.len() as u64 >= size {
                    break 'lists;
                }
                let data = self.block(self.long(&list, 6 + self.table_size() - 1 - i))?;
                if ofs {
                    if self.long(&data, 0) != T_DATA {
                        return Err("One of the file's data blocks isn't a data block, so the disk is damaged.".into());
                    }
                    let length = (self.long(&data, 3) as usize).min(self.block_size - 24);
                    out.extend_from_slice(&data[24..24 + length]);
                } else {
                    out.extend_from_slice(&data);
                }
            }
            let next = self.tail(&list, 2);
            if next == 0 || out.len() as u64 >= size {
                break;
            }
            if !seen.insert(next) || seen.len() > MOST_BLOCKS_FOLLOWED {
                return Err("The file's list of blocks loops, so the disk is damaged.".into());
            }
            list = self.block(next)?;
            if self.long(&list, 0) != T_LIST {
                return Err("The file's list of blocks continues into a block that isn't one.".into());
            }
        }
        if (out.len() as u64) < size {
            return Err(format!("The file should be {size} bytes, and its blocks hold only {}.", out.len()));
        }
        out.truncate(size as usize);
        Ok(out)
    }

    fn kind(&self) -> String {
        let mut words = vec![if self.dos_type & 1 == 0 { "OFS" } else { "FFS" }];
        match self.dos_type {
            2 | 3 => words.push("international"),
            4 | 5 => words.push("directory cache"),
            6 | 7 => words.push("long names"),
            _ => {}
        }
        words.join(", ")
    }
}

impl Volume for AmigaFs {
    fn describe(&self) -> String {
        format!("{} \u{B7} {} \u{B7} {}", self.what, self.kind(), if self.name.is_empty() { "no name" } else { &self.name })
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        let folder = match dir {
            Some(entry) => self.folder_block(entry)?,
            None => self.root,
        };
        self.entries(folder)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        self.read_file(entry.location as u32)
    }

    fn text_preset(&self, _entry: &Entry) -> Option<&'static str> {
        Some(super::AMIGA_TEXT_PRESET)
    }
}

/// An Amiga floppy image: 1760 blocks for a double density disk, 3520 for high density.
pub fn open_adf(source: Arc<dyn Source>) -> Result<AmigaFs, String> {
    let what = match source.len() {
        901_120 => "Amiga floppy (ADF, 880K)",
        1_802_240 => "Amiga floppy (ADF, 1.76 MB)",
        _ => "Amiga disk image (ADF)",
    };
    AmigaFs::open(source, 512, what)
}

/// A gzipped ADF (.adz), unpacked into memory.
pub fn gunzip(source: &dyn Source) -> Result<Vec<u8>, String> {
    let data = source.read_vec(0, source.len().min(256 * 1024 * 1024) as usize)?;
    if data.len() < 18 || data[..3] != [0x1F, 0x8B, 8] {
        return Err("This isn't a gzip file.".into());
    }
    let flags = data[3];
    let mut at = 10;
    if flags & 4 != 0 {
        at += 2 + usize::from(u16::from_le_bytes([data[10], data[11]]));
    }
    for bit in [8u8, 16] {
        if flags & bit != 0 {
            at += data.get(at..).and_then(|rest| rest.iter().position(|b| *b == 0)).ok_or("The gzip header is cut short.")? + 1;
        }
    }
    if flags & 2 != 0 {
        at += 2;
    }
    let size = u32::from_le_bytes(data[data.len() - 4..].try_into().unwrap_or([0; 4])) as usize;
    miniz_oxide::inflate::decompress_to_vec_with_limit(data.get(at..).unwrap_or(&[]), size.clamp(1, 256 * 1024 * 1024)).map_err(|e| format!("The gzip file couldn't be unpacked: {e:?}"))
}

/// A DMS (DiskMasher) archive of a floppy, unpacked by the `xdms` crate (a port of xDMS) into the disk image it
/// holds. Unpacking stops at 8 MB, more than any Amiga floppy, so a damaged archive can't run away with memory.
pub fn undms(source: &dyn Source) -> Result<Vec<u8>, String> {
    struct Capped(Vec<u8>);
    impl std::io::Write for Capped {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            if self.0.len() + data.len() > 8 * 1024 * 1024 {
                return Err(std::io::Error::other("it unpacks to more than any floppy holds"));
            }
            self.0.extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let data = source.read_vec(0, source.len().min(16 * 1024 * 1024) as usize)?;
    let mut archive = xdms::DmsArchive::read(std::io::Cursor::new(data)).map_err(|e| format!("This DMS archive can't be read: {e}"))?;
    let mut out = Capped(Vec::new());
    archive.unpack_to(&mut out).map_err(|e| format!("This DMS archive couldn't be unpacked: {e}"))?;
    Ok(out.0)
}

/// A hard disk image: a whole disk with a Rigid Disk Block in its first 16 blocks, or a single partition.
pub enum HardDisk {
    Partition(AmigaFs),
    Disk { partitions: Vec<(String, Result<AmigaFs, String>)> },
}

impl HardDisk {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let head = source.read_vec(0, source.len().min(16 * 512) as usize)?;
        let Some(rdb) = (0..head.len() / 512).find(|b| head[b * 512..].starts_with(b"RDSK")) else {
            return Ok(Self::Partition(AmigaFs::open(source, 512, "Amiga hard disk partition (HDF)")?));
        };
        let at = rdb * 512;
        let long = |b: &[u8], i: usize| b.get(i * 4..i * 4 + 4).map_or(0, |x| u32::from_be_bytes([x[0], x[1], x[2], x[3]]));
        let rdb_block = &head[at..at + 512];
        let block_bytes = u64::from(long(rdb_block, 4)).clamp(256, 32_768);
        let mut partitions = Vec::new();
        let mut next = long(rdb_block, 7);
        let mut seen = HashSet::new();
        while next != u32::MAX && next != 0 && seen.insert(next) && partitions.len() < 64 {
            let part = source.read_vec(u64::from(next) * block_bytes, 256)?;
            if !part.starts_with(b"PART") {
                break;
            }
            let name = bstr(&part[36..], 31);
            // The partition's DOS environment: its geometry, and so where it starts and how big its blocks are.
            let env = |i: usize| u64::from(long(&part, 32 + i));
            let (size_longs, surfaces, sectors_per_block, blocks_per_track, low, high) = (env(1), env(3), env(4).max(1), env(5), env(9), env(10));
            let sector = size_longs * 4;
            let start = low * surfaces * blocks_per_track * sector;
            let length = (high + 1).saturating_sub(low) * surfaces * blocks_per_track * sector;
            let opened = if sector == 0 || start + length > source.len() {
                Err("This partition's geometry runs past the end of the image.".to_string())
            } else {
                let slice: Arc<dyn Source> = Arc::new(Slice { inner: source.clone(), start, len: length });
                AmigaFs::open(slice, (sector * sectors_per_block) as usize, &format!("Amiga hard disk (HDF) \u{203A} {name}"))
            };
            partitions.push((name, opened));
            next = long(&part, 4);
        }
        if partitions.is_empty() {
            return Err("This Amiga hard disk image has a Rigid Disk Block but no partitions in it.".into());
        }
        Ok(Self::Disk { partitions })
    }
}

impl Volume for HardDisk {
    fn describe(&self) -> String {
        match self {
            Self::Partition(fs) => fs.describe(),
            Self::Disk { partitions } => format!("Amiga hard disk (HDF) \u{B7} {} partition(s)", partitions.len()),
        }
    }

    /// A whole disk's partitions are its top folders; their entries carry which partition they're in, in `extra`.
    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        let partitions = match self {
            Self::Partition(fs) => return fs.list(dir),
            Self::Disk { partitions } => partitions,
        };
        let Some(dir) = dir else {
            return Ok(partitions
                .iter()
                .enumerate()
                .map(|(index, (name, fs))| {
                    let mut entry = Entry::dir(tidy_name(name), index as u64, u64::MAX);
                    entry.kind = "Partition".into();
                    entry.detail = match fs {
                        Ok(fs) => fs.describe().rsplit(" \u{B7} ").take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" \u{B7} "),
                        Err(problem) => problem.clone(),
                    };
                    entry
                })
                .collect());
        };
        let (index, inner) = if dir.extra == u64::MAX { (dir.location as usize, None) } else { (dir.extra as usize, Some(dir)) };
        let fs = partitions.get(index).ok_or("That partition isn't on the disk.")?.1.as_ref().map_err(Clone::clone)?;
        let mut entries = fs.list(inner)?;
        for entry in &mut entries {
            entry.extra = index as u64;
        }
        Ok(entries)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        match self {
            Self::Partition(fs) => fs.read(entry),
            Self::Disk { partitions } => partitions.get(entry.extra as usize).ok_or("That partition isn't on the disk.")?.1.as_ref().map_err(Clone::clone)?.read(entry),
        }
    }

    fn notes(&self) -> Vec<String> {
        match self {
            Self::Partition(_) => Vec::new(),
            Self::Disk { partitions } => partitions.iter().filter_map(|(name, fs)| fs.as_ref().err().map(|e| format!("{name}: {e}"))).collect(),
        }
    }

    fn text_preset(&self, _entry: &Entry) -> Option<&'static str> {
        Some(super::AMIGA_TEXT_PRESET)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{find, fixture, names};
    use super::super::open;
    use super::*;

    /// Every DMS in ../testmedia unpacked, to compare with what xDMS unpacks.
    #[test]
    fn real_dms_archives_unpack_for_comparing() {
        let root = std::path::Path::new("../testmedia");
        let Ok(folders) = std::fs::read_dir(root) else {
            return;
        };
        let out = root.join("limoniter-unpacked");
        for folder in folders.flatten().filter(|f| f.path().is_dir()) {
            for file in std::fs::read_dir(folder.path()).unwrap().flatten() {
                let path = file.path();
                if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("dms")) {
                    let source: Arc<dyn Source> = Arc::new(std::fs::read(&path).unwrap());
                    if let Ok(disk) = undms(&*source) {
                        std::fs::create_dir_all(&out).unwrap();
                        std::fs::write(out.join(path.with_extension("adf").file_name().unwrap()), disk).unwrap();
                    }
                }
            }
        }
    }

    #[test]
    fn dates_and_protection_read_as_amigados_shows_them() {
        assert_eq!(amiga_date(0, 0, 0), "1978-01-01 00:00:00");
        // Fred Fish disk 801's volume date, as xdftool shows it: 24.01.1993 16:12:21.
        assert_eq!(amiga_date(5502, 972, 21 * 50 + 18), "1993-01-24 16:12:21");
        assert_eq!(protection(0), "----rwed");
        assert_eq!(protection(0x20 | 0x01), "--p-rwe-");
    }

    #[test]
    fn ofs_and_ffs_floppies_list_their_folders_and_read_their_files() {
        for (image, kind) in [("ofs.adf", "OFS"), ("ffs.adf", "FFS"), ("ffs-intl.adf", "FFS, international"), ("ofs-dircache.adf", "OFS, directory cache"), ("ffs-longnames.adf", "FFS, long names")] {
            let volume = open(Arc::new(fixture(image)), image).unwrap_or_else(|e| panic!("{image}: {e}"));
            assert_eq!(volume.describe(), format!("Amiga floppy (ADF, 880K) \u{B7} {kind} \u{B7} Test"), "{image}");
            let top = volume.list(None).unwrap();
            assert_eq!(names(&top), ["docs", "big.bin", "hello.txt"], "{image}");
            assert_eq!(volume.read(find(&top, "hello.txt")).unwrap(), b"Hello from the Amiga\n", "{image}");
            // A file larger than one header's 72 blocks goes on into an extension block.
            let big = volume.read(find(&top, "big.bin")).unwrap();
            assert_eq!(big.len(), 60_000, "{image}");
            assert!(big.iter().enumerate().all(|(i, b)| *b == (i % 251) as u8), "{image}");
            let docs = volume.list(Some(find(&top, "docs"))).unwrap();
            assert_eq!(names(&docs), ["readme"], "{image}");
            assert_eq!(volume.read(&docs[0]).unwrap(), b"in a folder\n", "{image}");
        }
        let long = open(Arc::new(fixture("ffs-longnames.adf")), "x.adf").unwrap();
        let docs = long.list(Some(find(&long.list(None).unwrap(), "docs"))).unwrap();
        assert_eq!(long.read(&docs[0]).unwrap(), b"in a folder\n");
    }

    #[test]
    fn a_hard_disk_with_partitions_and_a_gzipped_floppy_open() {
        let disk = open(Arc::new(fixture("rdb.hdf")), "rdb.hdf").unwrap();
        assert_eq!(disk.describe(), "Amiga hard disk (HDF) \u{B7} 2 partition(s)");
        let parts = disk.list(None).unwrap();
        assert_eq!(names(&parts), ["DH0", "DH1"]);
        let second = disk.list(Some(&parts[1])).unwrap();
        assert_eq!(names(&second), ["docs", "big.bin", "hello.txt"]);
        assert_eq!(disk.read(find(&second, "hello.txt")).unwrap(), b"Hello from the Amiga\n");
        let docs = disk.list(Some(find(&second, "docs"))).unwrap();
        assert_eq!(disk.read(&docs[0]).unwrap(), b"in a folder\n", "entries remember their partition");
        let adz = open(Arc::new(fixture("ffs.adz")), "ffs.adz").unwrap();
        assert!(adz.describe().contains("FFS"));
        // A disk that isn't AmigaDOS's says so, rather than listing nothing.
        let mut ndos = fixture("ffs.adf");
        ndos[..4].copy_from_slice(b"NDOS");
        assert!(open(Arc::new(ndos), "game.adf").err().unwrap().contains("game or demo"));
    }
}
