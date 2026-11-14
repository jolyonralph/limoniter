//! Classic Macintosh disks: MFS, the original 400K floppies' flat file system; HFS, on 800K and 1.4MB floppies,
//! hard disks and CD-ROMs; Apple partition maps; and Disk Copy 4.2 images, which wrap a floppy with a header and
//! the sectors' tags. The layouts are Inside Macintosh's (Volume II for MFS, "Files" for HFS and its B-trees, and
//! "Devices" for the partition map), and Apple's Disk Copy 4.2 format note for the images.
//!
//! A Mac file has two forks. The data fork is the file as other machines see it; the resource fork holds an
//! application's code, icons and the like. Each is listed, the resource fork as `name.rsrc` beside the file, and the
//! file's type and creator are shown as the Finder keeps them.

use super::{Entry, MOST_FILE_BYTES, Slice, Source, Volume, tidy_name};
use crate::charset;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// The text preset for a Mac file's data: Mac Roman, lines ending in CR.
pub const MAC_TEXT_PRESET: &str = "classic-mac";

/// Entries with this in `extra` are a file's resource fork; without, its data fork.
pub(super) const RESOURCE_FORK: u64 = 1;

pub(super) fn be(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0, |n, b| (n << 8) | u64::from(*b))
}

/// Mac Roman text, as Mac names and labels are written.
pub(super) fn mac_roman(bytes: &[u8]) -> String {
    let set = charset::known("mac-roman");
    bytes.iter().map(|b| set.entry(0, *b).and_then(|e| e.unicode).filter(|c| !c.is_control()).unwrap_or('?')).collect()
}

/// A Pascal string: a length byte, then up to `most` characters.
fn pascal(bytes: &[u8], most: usize) -> String {
    let length = usize::from(*bytes.first().unwrap_or(&0)).min(most).min(bytes.len().saturating_sub(1));
    mac_roman(&bytes[1..1 + length])
}

/// A Mac date: seconds since the start of 1904, in the Mac's local time. Nothing for zero, which means none.
pub(super) fn mac_date(seconds: u64) -> String {
    if seconds == 0 {
        return String::new();
    }
    let days = (seconds / 86_400) as i64 - 24_107;
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
    format!("{year}-{month:02}-{day:02} {:02}:{:02}", time / 3600, time / 60 % 60)
}

/// A four-character code, as type and creator are shown.
pub(super) fn code(bytes: &[u8]) -> String {
    mac_roman(bytes)
}

/// What the Finder would call a file of this type, and an ending where Limoniter has a use for one.
pub(super) fn file_kind(kind: &[u8]) -> (&'static str, Option<&'static str>) {
    match kind {
        b"APPL" => ("Application", None),
        b"TEXT" => ("Text", None),
        b"ttro" => ("Read-only text", None),
        b"PICT" => ("Picture (PICT)", None),
        b"PNTG" => ("MacPaint picture", Some("pntg")),
        b"STAK" => ("HyperCard stack", None),
        b"WORD" => ("MacWrite document", None),
        b"ZSYS" => ("System file", None),
        b"FNDR" => ("Finder", None),
        b"INIT" => ("System extension", None),
        b"cdev" => ("Control panel", None),
        b"DFIL" | b"dfil" => ("Desk accessory", None),
        b"FFIL" => ("Font file", None),
        b"PRES" | b"PRER" => ("Printer driver", None),
        b"dImg" => ("Disk Copy image", Some("image")),
        b"JPEG" => ("JPEG picture", Some("jpg")),
        b"GIFf" => ("GIF picture", Some("gif")),
        b"PNGf" => ("PNG picture", Some("png")),
        b"BMPf" | b"BMP " => ("BMP picture", Some("bmp")),
        b"ZIP " => ("Zip archive", Some("zip")),
        b"SIT!" | b"SITD" | b"SIT5" => ("StuffIt archive", Some("sit")),
        b"PACT" => ("Compact Pro archive", Some("cpt")),
        _ => ("Document", None),
    }
}

// ---------- Disk Copy 4.2 ----------

/// Whether bytes are a Disk Copy 4.2 image: a name of up to 63 characters, data and tag sizes that add up to the
/// file's length with the 84-byte header, and the format's mark, &0100, at byte 82.
pub fn looks_like_disk_copy(head: &[u8], len: u64) -> bool {
    head.len() >= 84 && (1..=63).contains(&head[0]) && head[82..84] == [1, 0] && {
        let (data, tags) = (be(&head[64..68]), be(&head[68..72]));
        data > 0 && data.is_multiple_of(512) && 84 + data + tags == len
    }
}

/// Disk Copy's checksum: each big-endian 16-bit word added in, then the total rotated right by one.
fn disk_copy_sum(source: &dyn Source, start: u64, length: u64) -> Result<u32, String> {
    let mut sum = 0u32;
    let mut at = 0;
    while at < length {
        let chunk = source.read_vec(start + at, (length - at).min(1 << 20) as usize)?;
        for pair in chunk.chunks(2) {
            sum = sum.wrapping_add(u32::from(pair[0]) << 8 | u32::from(*pair.get(1).unwrap_or(&0))).rotate_right(1);
        }
        at += chunk.len() as u64;
    }
    Ok(sum)
}

/// The disk inside a Disk Copy 4.2 image, its name, and a note if its checksum doesn't match.
pub struct Unwrapped {
    pub disk: Arc<dyn Source>,
    pub name: String,
    pub note: Option<String>,
}

pub fn unwrap_disk_copy(source: Arc<dyn Source>) -> Result<Unwrapped, String> {
    let head = source.read_vec(0, 84)?;
    let data = be(&head[64..68]);
    let name = pascal(&head, 63);
    let note = (disk_copy_sum(&*source, 84, data)? != be(&head[72..76]) as u32).then(|| "The image's checksum doesn't match its data, so some of it may be damaged.".to_string());
    if head[81] == 0x24 {
        return Err("This Disk Copy image holds an Apple II disk (ProDOS), which Limoniter can't read yet.".into());
    }
    let disk: Arc<dyn Source> = Arc::new(Slice { inner: source, start: 84, len: data });
    Ok(Unwrapped { disk, name, note })
}

// ---------- MFS ----------

pub(super) struct MacFile {
    pub(super) name: String,
    pub(super) kind: [u8; 4],
    pub(super) creator: [u8; 4],
    pub(super) flags: u16,
    pub(super) data: Fork,
    pub(super) resource: Fork,
    pub(super) created: u64,
    pub(super) modified: u64,
    pub(super) parent: u32,
    pub(super) id: u32,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Fork {
    pub(super) length: u64,
    /// MFS: the first allocation block. HFS: the first three extents, as (first block, count).
    start: u16,
    extents: [(u16, u16); 3],
}

impl Fork {
    /// A fork known only by its length, as an archive's entry gives one.
    pub(super) fn of_length(length: u64) -> Self {
        Self { length, ..Default::default() }
    }
}

/// Which a file system is at an image's start, by the signature of its volume information at byte 1024.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Mfs,
    Hfs,
    HfsPlus,
    Partitioned,
    /// A GUID partition table with an HFS+ volume in it.
    Gpt,
}

pub fn kind_of(head: &[u8]) -> Option<Kind> {
    let at = |offset: usize, bytes: &[u8]| head.get(offset..offset + bytes.len()) == Some(bytes);
    if at(0, b"ER") && (at(512, b"PM") || at(2048, b"PM") || at(be(head.get(2..4).unwrap_or(&[0, 0])) as usize, b"PM")) {
        return Some(Kind::Partitioned);
    }
    if super::hfsplus::looks_like_gpt(head) {
        return Some(Kind::Gpt);
    }
    match head.get(1024..1026)? {
        [0xD2, 0xD7] => Some(Kind::Mfs),
        b"BD" => Some(Kind::Hfs),
        b"H+" | b"HX" => Some(Kind::HfsPlus),
        _ => None,
    }
}

pub struct Mfs {
    source: Arc<dyn Source>,
    name: String,
    what: String,
    files: Vec<MacFile>,
    map: Vec<u16>,
    block_size: u64,
    blocks_at: u64,
    notes: Vec<String>,
}

impl Mfs {
    pub fn open(source: Arc<dyn Source>, what: &str) -> Result<Self, String> {
        let mdb = source.read_vec(1024, 1024).map_err(|_| "The image is too short for an MFS disk.".to_string())?;
        if mdb[..2] != [0xD2, 0xD7] {
            return Err("This isn't an MFS disk.".into());
        }
        let (directory, directory_blocks, blocks) = (be(&mdb[14..16]), be(&mdb[16..18]), be(&mdb[18..20]) as usize);
        let (block_size, blocks_at) = (be(&mdb[20..24]), be(&mdb[28..30]) * 512);
        if block_size == 0 || !block_size.is_multiple_of(512) || blocks > 4094 {
            return Err("This MFS disk's volume information doesn't make sense, so it can't be read.".into());
        }
        // The allocation block map follows the volume information: twelve bits for each block from block 2, each
        // the next block of its file, 1 for a file's last, 0 for free.
        let map = (0..blocks)
            .map(|i| {
                let at = 64 + i * 3 / 2;
                let (a, b) = (u16::from(*mdb.get(at).unwrap_or(&0)), u16::from(*mdb.get(at + 1).unwrap_or(&0)));
                if i % 2 == 0 { (a << 4) | (b >> 4) } else { ((a & 0xF) << 8) | b }
            })
            .collect();
        let mut volume = Self { source, name: pascal(&mdb[36..], 27), what: what.to_string(), files: Vec::new(), map, block_size, blocks_at, notes: Vec::new() };
        // The directory: entries that don't cross a 512-byte block, each padded to an even length; a block's
        // entries end at one whose flags byte is zero.
        let bytes = volume.source.read_vec(directory * 512, (directory_blocks * 512) as usize).map_err(|_| "The directory is past the end of the image.".to_string())?;
        for block in bytes.chunks(512) {
            let mut at = 0;
            while at + 51 <= block.len() && block[at] & 0x80 != 0 {
                let entry = &block[at..];
                let name_length = usize::from(entry[50]);
                if at + 51 + name_length > block.len() {
                    break;
                }
                let fork = |start: usize| Fork { start: be(&entry[start..start + 2]) as u16, length: be(&entry[start + 2..start + 6]), extents: Default::default() };
                volume.files.push(MacFile {
                    name: mac_roman(&entry[51..51 + name_length]),
                    kind: entry[2..6].try_into().unwrap(),
                    creator: entry[6..10].try_into().unwrap(),
                    flags: be(&entry[10..12]) as u16,
                    data: fork(22),
                    resource: fork(32),
                    created: be(&entry[42..46]),
                    modified: be(&entry[46..50]),
                    parent: be(&entry[16..18]) as u32,
                    id: be(&entry[18..22]) as u32,
                });
                at += (51 + name_length + 1) & !1;
            }
        }
        if volume.files.iter().any(|f| f.parent != 0) {
            volume.notes.push("MFS keeps the Finder's folders only in its Desktop file, so every file is listed together here.".into());
        }
        volume.files.sort_by_key(|f| f.name.to_lowercase());
        Ok(volume)
    }

    fn fork_bytes(&self, fork: &Fork) -> Result<Vec<u8>, String> {
        if fork.length > MOST_FILE_BYTES {
            return Err("This file is larger than the 1 GB that can be opened.".into());
        }
        let mut out = Vec::with_capacity(fork.length as usize);
        let mut block = fork.start;
        let mut seen = HashSet::new();
        while (out.len() as u64) < fork.length {
            if block < 2 || !seen.insert(block) {
                return Err("The file's chain of blocks breaks off or loops, so the disk is damaged.".into());
            }
            out.extend(self.source.read_vec(self.blocks_at + u64::from(block - 2) * self.block_size, self.block_size as usize).map_err(|_| "The file runs past the end of the image.".to_string())?);
            block = *self.map.get(usize::from(block - 2)).ok_or("The file's chain of blocks leaves the disk.")?;
            if block == 1 {
                break;
            }
        }
        if (out.len() as u64) < fork.length {
            return Err("The file's chain of blocks ends before the file does, so the disk is damaged.".into());
        }
        out.truncate(fork.length as usize);
        Ok(out)
    }
}

/// A file's entries: its data fork, and its resource fork beside it if it has one.
pub(super) fn file_entries(index: usize, file: &MacFile) -> Vec<Entry> {
    let name = tidy_name(&file.name);
    let (kind, ending) = file_kind(&file.kind);
    let mut data = Entry::file(name.clone(), file.data.length, index as u64);
    data.kind = kind.into();
    let mut about = vec![format!("{} \u{B7} {}", code(&file.kind), code(&file.creator))];
    let date = mac_date(if file.modified != 0 { file.modified } else { file.created });
    if !date.is_empty() {
        about.push(date);
    }
    if file.flags & 0x4000 != 0 {
        about.push("invisible".into());
    }
    data.detail = about.join(" \u{B7} ");
    if let Some(ending) = ending
        && !name.to_lowercase().ends_with(&format!(".{ending}"))
    {
        data.open_name = format!("{name}.{ending}");
    }
    let mut out = vec![data];
    if file.resource.length > 0 {
        let mut resource = Entry::file(format!("{name}.rsrc"), file.resource.length, index as u64);
        resource.extra = RESOURCE_FORK;
        resource.kind = "Resource fork".into();
        resource.detail = format!("of {name}");
        out.push(resource);
    }
    out
}

impl Volume for Mfs {
    fn describe(&self) -> String {
        format!("{} \u{B7} MFS \u{B7} {}", self.what, self.name)
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        if dir.is_some() {
            return Ok(Vec::new());
        }
        Ok(self.files.iter().enumerate().flat_map(|(index, file)| file_entries(index, file)).collect())
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let file = self.files.get(entry.location as usize).ok_or("That file isn't on the disk.")?;
        self.fork_bytes(if entry.extra == RESOURCE_FORK { &file.resource } else { &file.data })
    }

    fn notes(&self) -> Vec<String> {
        self.notes.clone()
    }

    fn text_preset(&self, entry: &Entry) -> Option<&'static str> {
        (entry.extra != RESOURCE_FORK).then_some(MAC_TEXT_PRESET)
    }
}

// ---------- HFS ----------

struct Folder {
    name: String,
    id: u32,
    parent: u32,
    count: u64,
    modified: u64,
}

pub struct Hfs {
    source: Arc<dyn Source>,
    name: String,
    what: String,
    block_size: u64,
    blocks_at: u64,
    folders: Vec<Folder>,
    files: Vec<MacFile>,
    /// Extents past a fork's first three, by (file id, resource fork).
    overflow: HashMap<(u32, bool), Vec<Overflow>>,
    created: u64,
}

/// Three extents, each (first allocation block, count), as an extent record holds them.
type Extents = [(u16, u16); 3];

/// More extents for a fork, from the extents tree: the fork's block they start at, and the extents.
type Overflow = (u16, Extents);

/// A B-tree record: its key (without the length byte) and its data.
type Record = (Vec<u8>, Vec<u8>);

fn extents(bytes: &[u8]) -> Extents {
    std::array::from_fn(|i| (be(&bytes[i * 4..i * 4 + 2]) as u16, be(&bytes[i * 4 + 2..i * 4 + 4]) as u16))
}

/// Every record in a B-tree's leaves, as (key, data), following the leaves' chain from the header's first.
fn leaf_records(tree: &[u8]) -> Result<Vec<Record>, String> {
    let header = tree.get(..512).ok_or("A B-tree is too short for its header node.")?;
    if header[8] != 1 {
        return Err("A B-tree's first node isn't its header, so the disk is damaged.".into());
    }
    let node_size = be(&header[32..34]) as usize;
    if node_size < 512 || !node_size.is_power_of_two() {
        return Err("A B-tree's node size doesn't make sense, so the disk is damaged.".into());
    }
    let mut node = be(&header[24..28]) as usize;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    while node != 0 {
        if !seen.insert(node) {
            return Err("A B-tree's leaves loop back on themselves, so the disk is damaged.".into());
        }
        let bytes = tree.get(node * node_size..(node + 1) * node_size).ok_or("A B-tree's leaf is past its end, so the disk is damaged.")?;
        if bytes[8] != 0xFF {
            return Err("A B-tree's chain of leaves leads to a node that isn't one, so the disk is damaged.".into());
        }
        let count = be(&bytes[10..12]) as usize;
        if 14 + 2 * (count + 1) > node_size {
            return Err("A B-tree node says it holds more records than fit in it, so the disk is damaged.".into());
        }
        for i in 0..count {
            let start = be(&bytes[node_size - 2 * (i + 1)..node_size - 2 * i]) as usize;
            let end = be(&bytes[node_size - 2 * (i + 2)..node_size - 2 * (i + 1)]) as usize;
            if start < 14 || end > node_size - 2 * (count + 1) || start >= end {
                return Err("A B-tree record lies outside its node, so the disk is damaged.".into());
            }
            let key_length = usize::from(bytes[start]);
            let data_at = (start + 1 + key_length + 1) & !1;
            if data_at > end {
                return Err("A B-tree record's key is longer than the record, so the disk is damaged.".into());
            }
            out.push((bytes[start + 1..start + 1 + key_length].to_vec(), bytes[data_at..end].to_vec()));
        }
        node = be(&bytes[0..4]) as usize;
    }
    Ok(out)
}

impl Hfs {
    pub fn open(source: Arc<dyn Source>, what: &str) -> Result<Self, String> {
        let mdb = source.read_vec(1024, 512).map_err(|_| "The image is too short for an HFS disk.".to_string())?;
        if &mdb[..2] != b"BD" {
            return Err("This isn't an HFS disk.".into());
        }
        if &mdb[124..126] == b"H+" {
            return Err("This is an HFS+ (Mac OS Extended) disk inside an HFS wrapper, to be opened as HFS+.".into());
        }
        let block_size = be(&mdb[20..24]);
        if block_size == 0 || !block_size.is_multiple_of(512) {
            return Err("This HFS disk's allocation block size doesn't make sense, so it can't be read.".into());
        }
        let mut volume = Self {
            source,
            name: pascal(&mdb[36..], 27),
            what: what.to_string(),
            block_size,
            blocks_at: be(&mdb[28..30]) * 512,
            folders: Vec::new(),
            files: Vec::new(),
            overflow: HashMap::new(),
            created: be(&mdb[2..6]),
        };
        // The extents tree first, as the catalog's own extents past its first three are in it.
        let extents_tree = volume.fork_bytes(3, false, &Fork { length: be(&mdb[130..134]), start: 0, extents: extents(&mdb[134..146]) })?;
        for (key, data) in leaf_records(&extents_tree)? {
            // The key: the fork (0 data, &FF resource), the file's number, and the fork's block these extents start at.
            if key.len() >= 7 && data.len() >= 12 {
                let entry = volume.overflow.entry((be(&key[1..5]) as u32, key[0] == 0xFF)).or_default();
                entry.push((be(&key[5..7]) as u16, extents(&data)));
            }
        }
        for list in volume.overflow.values_mut() {
            list.sort_by_key(|(start, _)| *start);
        }
        let catalog = volume.fork_bytes(4, false, &Fork { length: be(&mdb[146..150]), start: 0, extents: extents(&mdb[150..162]) })?;
        for (key, data) in leaf_records(&catalog)? {
            if key.len() < 6 || data.is_empty() {
                continue;
            }
            let parent = be(&key[1..5]) as u32;
            let name = pascal(&key[5..], 31);
            match data[0] {
                1 if data.len() >= 70 => volume.folders.push(Folder { name, parent, count: be(&data[4..6]), id: be(&data[6..10]) as u32, modified: be(&data[14..18]) }),
                2 if data.len() >= 102 => volume.files.push(MacFile {
                    name,
                    parent,
                    kind: data[4..8].try_into().unwrap(),
                    creator: data[8..12].try_into().unwrap(),
                    flags: be(&data[12..14]) as u16,
                    id: be(&data[20..24]) as u32,
                    data: Fork { length: be(&data[26..30]), start: 0, extents: extents(&data[74..86]) },
                    resource: Fork { length: be(&data[36..40]), start: 0, extents: extents(&data[86..98]) },
                    created: be(&data[44..48]),
                    modified: be(&data[48..52]),
                }),
                _ => {}
            }
        }
        if !volume.folders.iter().any(|f| f.id == 2) {
            return Err("This HFS disk's catalog has no root folder, so the disk is damaged.".into());
        }
        Ok(volume)
    }

    /// A fork's bytes: its first three extents, then those in the extents tree, block by block until its length.
    fn fork_bytes(&self, id: u32, resource: bool, fork: &Fork) -> Result<Vec<u8>, String> {
        if fork.length > MOST_FILE_BYTES {
            return Err("This file is larger than the 1 GB that can be opened.".into());
        }
        let mut runs: Vec<(u16, u16)> = fork.extents.to_vec();
        let mut covered: u64 = runs.iter().map(|(_, count)| u64::from(*count)).sum();
        for (start, more) in self.overflow.get(&(id, resource)).map(Vec::as_slice).unwrap_or_default() {
            if u64::from(*start) == covered {
                runs.extend(more);
                covered += more.iter().map(|(_, count)| u64::from(*count)).sum::<u64>();
            }
        }
        let mut out = Vec::with_capacity(fork.length as usize);
        for (first, count) in runs {
            if out.len() as u64 >= fork.length {
                break;
            }
            let at = self.blocks_at + u64::from(first) * self.block_size;
            let want = (u64::from(count) * self.block_size).min(fork.length - out.len() as u64);
            if at + want > self.source.len() {
                return Err("The file runs past the end of the image.".into());
            }
            out.extend(self.source.read_vec(at, want as usize)?);
        }
        if (out.len() as u64) < fork.length {
            return Err("The disk's extents give this file less space than its length, so the disk is damaged.".into());
        }
        Ok(out)
    }
}

impl Volume for Hfs {
    fn describe(&self) -> String {
        let made = mac_date(self.created);
        format!("{} \u{B7} HFS \u{B7} {}{}", self.what, self.name, if made.is_empty() { String::new() } else { format!(" \u{B7} made {}", &made[..10]) })
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        let parent = dir.map_or(2, |d| d.location as u32);
        let mut folders: Vec<Entry> = self
            .folders
            .iter()
            .filter(|f| f.parent == parent)
            .map(|f| {
                let mut entry = Entry::dir(tidy_name(&f.name), u64::from(f.id), 0);
                entry.detail = format!("{} item(s){}", f.count, if f.modified == 0 { String::new() } else { format!(" \u{B7} {}", mac_date(f.modified)) });
                entry
            })
            .collect();
        folders.sort_by_key(|e| e.name.to_lowercase());
        let mut files: Vec<(String, usize)> = self.files.iter().enumerate().filter(|(_, f)| f.parent == parent).map(|(i, f)| (f.name.to_lowercase(), i)).collect();
        files.sort();
        folders.extend(files.into_iter().flat_map(|(_, i)| file_entries(i, &self.files[i])));
        Ok(folders)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let file = self.files.get(entry.location as usize).ok_or("That file isn't on the disk.")?;
        let resource = entry.extra == RESOURCE_FORK;
        self.fork_bytes(file.id, resource, if resource { &file.resource } else { &file.data })
    }

    fn text_preset(&self, entry: &Entry) -> Option<&'static str> {
        (entry.extra != RESOURCE_FORK).then_some(MAC_TEXT_PRESET)
    }
}

// ---------- Partition maps ----------

/// A partition of a disk: its name and kind as the map gives them, and its file system if one could be read.
pub(super) struct Partition {
    pub(super) name: String,
    pub(super) kind: String,
    pub(super) volume: Option<Box<dyn Volume>>,
    pub(super) problem: Option<String>,
    /// The partition's length in bytes.
    pub(super) blocks: u64,
    /// The partition's bytes, where they can be opened as they are when there's no file system to read.
    pub(super) raw: Option<Arc<dyn Source>>,
}

/// A disk with an Apple partition map or a GUID partition table: each partition, its file system read where it's
/// HFS, HFS+ or MFS. A disk with one such partition, as a CD-ROM has, shows its files at once.
pub struct Partitioned {
    what: String,
    scheme: &'static str,
    parts: Vec<Partition>,
}

/// Opens a disk's file system: a partition map, MFS, HFS or HFS+, by the signatures at its start.
pub fn open_disk(source: Arc<dyn Source>, what: &str) -> Result<Box<dyn Volume>, String> {
    let head = source.read_vec(0, source.len().min(0x8800) as usize)?;
    match kind_of(&head) {
        Some(Kind::Mfs) => Ok(Box::new(Mfs::open(source, what)?)),
        // An HFS volume whose master directory block names an embedded HFS+ volume is only its wrapper.
        Some(Kind::Hfs) if head.get(1024 + 124..1024 + 126) == Some(b"H+") => super::hfsplus::open_wrapped(source, what),
        Some(Kind::Hfs) => Ok(Box::new(Hfs::open(source, what)?)),
        Some(Kind::HfsPlus) => super::hfsplus::open(source, what),
        Some(Kind::Gpt) => super::hfsplus::open_gpt(source, what),
        Some(Kind::Partitioned) => Ok(Partitioned::open(source, what)?.into_volume()),
        None => Err("This disk has no Macintosh file system that Limoniter can find: no MFS, HFS, HFS+ or partition map.".into()),
    }
}

/// Opens a partition's file system, which, unlike a whole disk's, can't be another partition map: a damaged map with
/// a partition pointing back at itself would otherwise be opened again and again.
pub(super) fn open_partition(source: Arc<dyn Source>, what: &str) -> Result<Box<dyn Volume>, String> {
    let head = source.read_vec(0, source.len().min(0x8800) as usize)?;
    if matches!(kind_of(&head), Some(Kind::Partitioned | Kind::Gpt)) {
        return Err("This partition holds another partition map, so the disk is damaged.".into());
    }
    open_disk(source, what)
}

impl Partitioned {
    pub(super) fn new(what: &str, scheme: &'static str, parts: Vec<Partition>) -> Self {
        Self { what: what.to_string(), scheme, parts }
    }

    /// The map as a volume, or its one readable partition's file system when there's only one.
    pub(super) fn into_volume(self) -> Box<dyn Volume> {
        let mut readable: Vec<usize> = self.parts.iter().enumerate().filter(|(_, p)| p.volume.is_some()).map(|(i, _)| i).collect();
        if readable.len() == 1 {
            let only = readable.remove(0);
            return self.parts.into_iter().nth(only).and_then(|p| p.volume).expect("a readable partition has its volume");
        }
        Box::new(self)
    }

    fn open(source: Arc<dyn Source>, what: &str) -> Result<Self, String> {
        let ddm = source.read_vec(0, 512)?;
        // The map's entries are a block apart, in the driver descriptor's block size, as are their places.
        let block = match be(&ddm[2..4]) {
            size if size >= 512 && size.is_multiple_of(512) => size,
            _ => 512,
        };
        let first = source.read_vec(block, 512)?;
        if &first[..2] != b"PM" {
            return Err("The partition map's first entry isn't where the disk says, so it can't be read.".into());
        }
        let count = be(&first[4..8]).min(256);
        let mut parts = Vec::new();
        for i in 0..count {
            let entry = source.read_vec(block * (1 + i), 512)?;
            if &entry[..2] != b"PM" {
                break;
            }
            let (start, blocks) = (be(&entry[8..12]) * block, be(&entry[12..16]) * block);
            let text = |bytes: &[u8]| mac_roman(&bytes[..bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len())]);
            let (name, kind) = (text(&entry[16..48]), text(&entry[48..80]));
            let mut part = Partition { name, kind, volume: None, problem: None, blocks, raw: None };
            if matches!(part.kind.as_str(), "Apple_HFS" | "Apple_HFSX" | "Apple_MFS") {
                let inner: Arc<dyn Source> = Arc::new(Slice { inner: source.clone(), start, len: blocks.min(source.len().saturating_sub(start)) });
                match open_partition(inner, &format!("{what}, partition {}", i + 1)) {
                    Ok(volume) => part.volume = Some(volume),
                    Err(problem) => part.problem = Some(problem),
                }
            }
            parts.push(part);
        }
        Ok(Self::new(what, "Apple partition map", parts))
    }
}

impl Volume for Partitioned {
    fn describe(&self) -> String {
        format!("{} \u{B7} {} \u{B7} {} partition(s)", self.what, self.scheme, self.parts.len())
    }

    /// The partitions are the top folders; their entries carry which partition they're in, plus one, above the
    /// file system's own number in `extra`.
    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        let Some(dir) = dir else {
            return Ok(self
                .parts
                .iter()
                .enumerate()
                .map(|(i, part)| {
                    let label = if part.name.is_empty() { format!("Partition {}", i + 1) } else { tidy_name(&part.name) };
                    let mut entry = match (&part.volume, &part.raw) {
                        (Some(_), _) => Entry::dir(label, 0, (i as u64 + 1) << 8),
                        (None, Some(_)) => Entry { extra: (i as u64 + 1) << 8, ..Entry::file(label, part.blocks, 0) },
                        (None, None) => Entry::file(label, part.blocks, 0),
                    };
                    entry.kind = part.kind.clone();
                    entry.detail = part.problem.clone().unwrap_or_else(|| part.volume.as_ref().map_or_else(|| "not a file system Limoniter reads".into(), |v| v.describe()));
                    entry
                })
                .collect());
        };
        let index = (dir.extra >> 8) as usize;
        let volume = self.parts.get(index.wrapping_sub(1)).and_then(|p| p.volume.as_ref()).ok_or("That isn't a partition Limoniter can read.")?;
        let inner = (dir.location != 0 || dir.extra & 0xFF != 0).then(|| Entry { extra: dir.extra & 0xFF, ..dir.clone() });
        let mut entries = volume.list(inner.as_ref())?;
        for entry in &mut entries {
            entry.extra |= (index as u64) << 8;
        }
        Ok(entries)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let index = (entry.extra >> 8) as usize;
        let part = self.parts.get(index.wrapping_sub(1)).ok_or("That partition isn't one Limoniter reads.")?;
        match (&part.volume, &part.raw) {
            (Some(volume), _) => volume.read(&Entry { extra: entry.extra & 0xFF, ..entry.clone() }),
            // A partition with no file system to read is opened as its bytes.
            (None, Some(raw)) if raw.len() > MOST_FILE_BYTES => Err("This partition is larger than the 1 GB that can be opened.".into()),
            (None, Some(raw)) => raw.read_vec(0, raw.len() as usize),
            (None, None) => Err("That partition isn't one Limoniter reads.".into()),
        }
    }

    fn text_preset(&self, entry: &Entry) -> Option<&'static str> {
        let volume = self.parts.get(((entry.extra >> 8) as usize).wrapping_sub(1)).and_then(|p| p.volume.as_ref())?;
        volume.text_preset(&Entry { extra: entry.extra & 0xFF, ..entry.clone() })
    }

    fn notes(&self) -> Vec<String> {
        self.parts.iter().filter_map(|p| p.volume.as_ref()).flat_map(|v| v.notes()).collect()
    }
}

/// A Disk Copy 4.2 image's disk, opened, with the image's name and any checksum trouble said.
pub struct DiskCopy {
    inner: Box<dyn Volume>,
    note: Option<String>,
}

pub fn open_disk_copy(source: Arc<dyn Source>) -> Result<Box<dyn Volume>, String> {
    let Unwrapped { disk, name, note } = unwrap_disk_copy(source)?;
    let what = format!("Disk Copy 4.2 image \u{201C}{name}\u{201D}, {}K", disk.len() / 1024);
    let inner = open_disk(disk, &what)?;
    Ok(Box::new(DiskCopy { inner, note }))
}

impl Volume for DiskCopy {
    fn describe(&self) -> String {
        self.inner.describe()
    }
    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        self.inner.list(dir)
    }
    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        self.inner.read(entry)
    }
    fn notes(&self) -> Vec<String> {
        self.note.iter().cloned().chain(self.inner.notes()).collect()
    }
    fn text_preset(&self, entry: &Entry) -> Option<&'static str> {
        self.inner.text_preset(entry)
    }
}

#[cfg(test)]
pub mod tests {
    use super::super::{is_image, is_nested_image, open};
    use super::*;
    use std::path::Path;

    /// Whether bytes are a whole resource fork as Inside Macintosh lays one out: a header giving the data and map's
    /// places and lengths within the fork, a copy of it at the map's start, a type list inside the map, and every
    /// resource's data within the data area. A fork put together from the wrong blocks fails this.
    pub fn resource_fork_holds(fork: &[u8]) -> bool {
        let word = |at: usize| fork.get(at..at + 4).map(be);
        let half = |at: usize| fork.get(at..at + 2).map(be);
        let (Some(data_at), Some(map_at), Some(data_len), Some(map_len)) = (word(0), word(4), word(8), word(12)) else {
            return false;
        };
        let (data_at, map_at, data_len, map_len) = (data_at as usize, map_at as usize, data_len as usize, map_len as usize);
        // The map starts with room for a copy of the header, which is often left as zeros.
        let copy = fork.get(map_at..map_at + 16);
        if data_at + data_len > fork.len() || map_at + map_len > fork.len() || map_len < 30 || (copy != fork.get(..16) && copy.is_some_and(|c| c.iter().any(|b| *b != 0))) {
            return false;
        }
        let (Some(types_at), Some(names_at)) = (half(map_at + 24), half(map_at + 26)) else {
            return false;
        };
        let types = map_at + types_at as usize;
        let Some(count) = half(types).map(|n| (n + 1) & 0xFFFF) else {
            return false;
        };
        if types_at as usize > map_len || names_at as usize > map_len {
            return false;
        }
        (0..count as usize).all(|t| {
            let entry = types + 2 + t * 8;
            let (Some(resources), Some(list)) = (half(entry + 4), half(entry + 6)) else {
                return false;
            };
            (0..=resources as usize).all(|r| {
                let at = types + list as usize + r * 12;
                let Some(offset) = fork.get(at + 5..at + 8).map(be) else {
                    return false;
                };
                let start = data_at + offset as usize;
                word(start).is_some_and(|length| start + 4 + length as usize <= data_at + data_len)
            })
        })
    }

    fn names(entries: &[Entry]) -> Vec<String> {
        entries.iter().map(|e| e.name.clone()).collect()
    }

    fn find<'a>(entries: &'a [Entry], name: &str) -> &'a Entry {
        entries.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("{name} in {:?}", names(entries)))
    }

    /// A resource fork holding one 'STR ' resource, as the Resource Manager writes one.
    pub fn small_resource_fork() -> Vec<u8> {
        let data = [&3u32.to_be_bytes()[..], b"\x02Hi"].concat();
        let types = [&0u16.to_be_bytes()[..], b"STR ", &0u16.to_be_bytes(), &10u16.to_be_bytes()].concat();
        let refs = [&128i16.to_be_bytes()[..], &(-1i16).to_be_bytes(), &[0, 0, 0, 0], &[0; 4]].concat();
        let map = [&[0u8; 24][..], &28u16.to_be_bytes(), &(28 + types.len() as u16 + refs.len() as u16).to_be_bytes(), &types, &refs].concat();
        let mut fork = [256u32, 256 + data.len() as u32, data.len() as u32, map.len() as u32].iter().flat_map(|n| n.to_be_bytes()).collect::<Vec<u8>>();
        fork.resize(256, 0);
        fork.extend(&data);
        fork.extend(&map);
        fork
    }

    /// A 400K MFS disk: ReadMe's data in two blocks out of order, with a resource fork, and a game in a Finder folder.
    pub fn mfs_disk() -> Vec<u8> {
        let mut disk = vec![0u8; 409_600];
        let mdb = 1024;
        disk[mdb..mdb + 2].copy_from_slice(&[0xD2, 0xD7]);
        disk[mdb + 12..mdb + 14].copy_from_slice(&2u16.to_be_bytes());
        disk[mdb + 14..mdb + 16].copy_from_slice(&4u16.to_be_bytes());
        disk[mdb + 16..mdb + 18].copy_from_slice(&2u16.to_be_bytes());
        disk[mdb + 18..mdb + 20].copy_from_slice(&100u16.to_be_bytes());
        disk[mdb + 20..mdb + 24].copy_from_slice(&1024u32.to_be_bytes());
        disk[mdb + 28..mdb + 30].copy_from_slice(&6u16.to_be_bytes());
        disk[mdb + 36..mdb + 44].copy_from_slice(b"\x07MFSTest");
        let mut link = |block: usize, next: u16| {
            let (i, at) = (block - 2, mdb + 64 + (block - 2) * 3 / 2);
            if i % 2 == 0 {
                disk[at] = (next >> 4) as u8;
                disk[at + 1] = (disk[at + 1] & 0x0F) | ((next & 0xF) << 4) as u8;
            } else {
                disk[at] = (disk[at] & 0xF0) | (next >> 8) as u8;
                disk[at + 1] = next as u8;
            }
        };
        link(2, 5);
        link(5, 1);
        link(3, 1);
        link(4, 1);
        let block = |n: usize| 6 * 512 + (n - 2) * 1024;
        let text: Vec<u8> = (0..1500).map(|i| b"Hello from MFS\r"[i % 15]).collect();
        disk[block(2)..block(2) + 1024].copy_from_slice(&text[..1024]);
        disk[block(5)..block(5) + 476].copy_from_slice(&text[1024..]);
        let fork = small_resource_fork();
        disk[block(3)..block(3) + fork.len()].copy_from_slice(&fork);
        disk[block(4)..block(4) + 10].copy_from_slice(b"GAME CODE!");
        let mut at = 4 * 512;
        for (name, kind, folder, data, resource) in [(&b"ReadMe"[..], b"TEXTttxt", 0u16, (2u16, 1500u32), (3u16, fork.len() as u32)), (b"Game", b"APPLGAME", 0xFFFE, (4, 10), (0, 0))] {
            disk[at] = 0x80;
            disk[at + 2..at + 10].copy_from_slice(kind);
            disk[at + 16..at + 18].copy_from_slice(&folder.to_be_bytes());
            disk[at + 22..at + 24].copy_from_slice(&data.0.to_be_bytes());
            disk[at + 24..at + 28].copy_from_slice(&data.1.to_be_bytes());
            disk[at + 32..at + 34].copy_from_slice(&resource.0.to_be_bytes());
            disk[at + 34..at + 38].copy_from_slice(&resource.1.to_be_bytes());
            disk[at + 46..at + 50].copy_from_slice(&2_800_000_000u32.to_be_bytes());
            disk[at + 50] = name.len() as u8;
            disk[at + 51..at + 51 + name.len()].copy_from_slice(name);
            at += (51 + name.len() + 1) & !1;
        }
        disk
    }

    /// A disk in a Disk Copy 4.2 image, its checksum as Disk Copy works it out unless `spoil`.
    pub fn disk_copy(name: &str, disk: &[u8], spoil: bool) -> Vec<u8> {
        let mut out = vec![0u8; 84];
        out[0] = name.len() as u8;
        out[1..1 + name.len()].copy_from_slice(name.as_bytes());
        out[64..68].copy_from_slice(&(disk.len() as u32).to_be_bytes());
        let sum = disk_copy_sum(&disk.to_vec(), 0, disk.len() as u64).unwrap() ^ u32::from(spoil);
        out[72..76].copy_from_slice(&sum.to_be_bytes());
        out[80] = 1;
        out[81] = 0x22;
        out[82..84].copy_from_slice(&[1, 0]);
        out.extend_from_slice(disk);
        out
    }

    /// A disk with an Apple partition map: the map, a driver, and an HFS partition for each volume given.
    pub fn partitioned(volumes: &[&[u8]]) -> Vec<u8> {
        let mut parts: Vec<(String, String, u32, u32)> = vec![("Apple".into(), "Apple_partition_map".into(), 1, 63), ("Macintosh".into(), "Apple_Driver43".into(), 32, 16)];
        let mut next = 64u32;
        for (i, volume) in volumes.iter().enumerate() {
            let blocks = (volume.len() / 512) as u32;
            parts.push((format!("Disk {}", i + 1), "Apple_HFS".into(), next, blocks));
            next += blocks;
        }
        let mut disk = vec![0u8; next as usize * 512];
        disk[..2].copy_from_slice(b"ER");
        disk[2..4].copy_from_slice(&512u16.to_be_bytes());
        disk[4..8].copy_from_slice(&next.to_be_bytes());
        for (i, (name, kind, start, blocks)) in parts.iter().enumerate() {
            let at = 512 * (i + 1);
            disk[at..at + 2].copy_from_slice(b"PM");
            disk[at + 4..at + 8].copy_from_slice(&(parts.len() as u32).to_be_bytes());
            disk[at + 8..at + 12].copy_from_slice(&start.to_be_bytes());
            disk[at + 12..at + 16].copy_from_slice(&blocks.to_be_bytes());
            disk[at + 16..at + 16 + name.len()].copy_from_slice(name.as_bytes());
            disk[at + 48..at + 48 + kind.len()].copy_from_slice(kind.as_bytes());
        }
        let mut at = 64 * 512;
        for volume in volumes {
            disk[at..at + volume.len()].copy_from_slice(volume);
            at += volume.len();
        }
        disk
    }

    fn hfs_fixture() -> Vec<u8> {
        super::super::tests::fixture("hfs.img")
    }

    #[test]
    fn hfs_disks_list_their_folders_forks_and_fragmented_files() {
        let volume = open(Arc::new(hfs_fixture()), "fixture.hfs").unwrap();
        assert_eq!(volume.describe().split(" \u{B7} made").next().unwrap(), "Macintosh 800K floppy \u{B7} HFS \u{B7} Fixture");
        let top = volume.list(None).unwrap();
        assert_eq!(&names(&top)[..4], ["Docs", "Big", "F1", "F11"], "folders first, then files by name");
        let docs = volume.list(Some(find(&top, "Docs"))).unwrap();
        assert_eq!(names(&docs), ["Read Me", "Read Me.rsrc"], "the resource fork beside its file");
        let readme = find(&docs, "Read Me");
        assert!(readme.kind == "Text" && readme.detail.starts_with("TEXT \u{B7} ttxt"), "{}", readme.detail);
        assert_eq!(volume.read(readme).unwrap(), b"Hello from HFS\r");
        assert_eq!(volume.text_preset(readme), Some(MAC_TEXT_PRESET));
        let fork = find(&docs, "Read Me.rsrc");
        assert_eq!(fork.kind, "Resource fork");
        assert_eq!(volume.text_preset(fork), None);
        assert!(resource_fork_holds(&volume.read(fork).unwrap()));
        // Big lies in twelve pieces, past the three a catalog record holds, so the extents tree has the rest.
        let big = volume.read(find(&top, "Big")).unwrap();
        assert!(big.len() == 11 * 512 + 100 && big.iter().enumerate().all(|(i, b)| *b == (i % 251) as u8));
        assert_eq!(volume.read(find(&top, "F39")).unwrap(), vec![39u8; 512]);
    }

    #[test]
    fn mfs_disks_follow_their_block_map() {
        let volume = open(Arc::new(mfs_disk()), "games.dsk").unwrap();
        assert_eq!(volume.describe(), "Macintosh 400K floppy \u{B7} MFS \u{B7} MFSTest");
        let files = volume.list(None).unwrap();
        assert_eq!(names(&files), ["Game", "ReadMe", "ReadMe.rsrc"]);
        let text = volume.read(find(&files, "ReadMe")).unwrap();
        assert!(text.len() == 1500 && text.starts_with(b"Hello from MFS\r") && text.ends_with(b"Hello from MFS\r"[..1500 % 15].as_ref()), "two blocks, out of order, in the map's order");
        assert_eq!(volume.read(find(&files, "ReadMe.rsrc")).unwrap(), small_resource_fork());
        let game = find(&files, "Game");
        assert_eq!((game.kind.as_str(), game.detail.as_str()), ("Application", "APPL \u{B7} GAME \u{B7} 1992-09-22 09:46"));
        assert_eq!(volume.read(game).unwrap(), b"GAME CODE!");
        assert_eq!(volume.notes().len(), 1, "the Finder's folders are said to be in its Desktop file");
        // A CPC disk is still a CPC disk, though both are .dsk.
        assert!(!open(Arc::new(b"MV - CPCEMU Disk-File\r\nDisk-Info\r\n".iter().copied().chain(std::iter::repeat_n(0, 300)).collect::<Vec<u8>>()), "x.dsk").is_ok_and(|v| v.describe().contains("MFS")));
    }

    #[test]
    fn disk_copy_images_open_by_their_header_whatever_their_name() {
        let image = disk_copy("System Tools", &hfs_fixture(), false);
        for name in ["6.0.2 System Tools", "System Tools", "tools.image", "tools.dsk"] {
            assert!(is_image(name, &image[..0x8800], image.len() as u64), "{name}");
            let volume = open(Arc::new(image.clone()), name).unwrap();
            assert!(volume.describe().starts_with("Disk Copy 4.2 image \u{201C}System Tools\u{201D}, 800K \u{B7} HFS \u{B7} Fixture"), "{name}");
            assert!(volume.notes().is_empty());
        }
        assert!(!is_image("notes", b"just some words", 15), "a file with no ending is only an image when Disk Copy's header says so");
        let spoiled = open(Arc::new(disk_copy("Bad", &mfs_disk(), true)), "bad.image").unwrap();
        assert_eq!(spoiled.notes()[0], "The image's checksum doesn't match its data, so some of it may be damaged.");
        assert_eq!(names(&spoiled.list(None).unwrap()), ["Game", "ReadMe", "ReadMe.rsrc"], "it still opens");
    }

    #[test]
    fn partitioned_disks_show_their_hfs_partitions() {
        let hfs = hfs_fixture();
        // One HFS partition, as a CD-ROM has: its files at once.
        let one = open(Arc::new(partitioned(&[&hfs])), "cd.toast").unwrap();
        assert!(one.describe().contains("partition 3 \u{B7} HFS \u{B7} Fixture"));
        assert_eq!(one.read(find(&one.list(None).unwrap(), "F1")).unwrap(), vec![1u8; 512]);
        // Two: the map's partitions as folders, and the files inside them.
        let two = open(Arc::new(partitioned(&[&hfs, &hfs])), "disk.hda.img").unwrap();
        let parts = two.list(None).unwrap();
        assert_eq!(names(&parts), ["Apple", "Macintosh", "Disk 1", "Disk 2"]);
        assert!(!parts[1].is_dir && parts[1].kind == "Apple_Driver43");
        let inside = two.list(Some(&parts[3])).unwrap();
        let docs = two.list(Some(find(&inside, "Docs"))).unwrap();
        assert_eq!(two.read(find(&docs, "Read Me")).unwrap(), b"Hello from HFS\r");
        assert!(resource_fork_holds(&two.read(find(&docs, "Read Me.rsrc")).unwrap()));
        assert_eq!(two.read(find(&inside, "Big")).unwrap().len(), 11 * 512 + 100);
    }

    #[test]
    fn hfs_plus_with_nothing_in_it_is_named_and_refused() {
        let mut disk = vec![0u8; 819_200];
        disk[1024..1026].copy_from_slice(b"H+");
        assert!(open(Arc::new(disk), "new.img").err().unwrap().contains("HFS+"));
    }

    #[test]
    fn damaged_mac_disks_give_errors_never_panics() {
        for (name, image) in [("d.dsk", mfs_disk()), ("d.hfs", hfs_fixture()), ("d.toast", partitioned(&[&hfs_fixture()])), ("d.image", disk_copy("D", &mfs_disk(), false))] {
            let base = if name == "d.toast" { 64 * 512 } else if name == "d.image" { 84 } else { 0 };
            for seed in 0..60u32 {
                let mut bytes = image.clone();
                let mut state = seed.wrapping_mul(2_654_435_761);
                // The volume information, map, directory and catalog are in the disk's first 24K.
                for _ in 0..48 {
                    state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    bytes[base + 1024 + state as usize % (23 * 1024)] = (state >> 16) as u8;
                }
                if let Ok(volume) = open(Arc::new(bytes), name) {
                    let _ = volume.describe();
                    for entry in volume.list(None).unwrap_or_default().iter().take(60) {
                        let _ = if entry.is_dir { volume.list(Some(entry)).map(|_| Vec::new()) } else { volume.read(entry) };
                    }
                }
            }
        }
    }

    fn walk(volume: &dyn Volume, dir: Option<&Entry>, depth: usize, forks: &mut usize, wrong: &mut Vec<String>) {
        if depth > 10 {
            return;
        }
        for entry in volume.list(dir).unwrap_or_default() {
            if entry.is_dir {
                walk(volume, Some(&entry), depth + 1, forks, wrong);
                continue;
            }
            let Ok(bytes) = volume.read(&entry) else {
                wrong.push(format!("{} can't be read", entry.name));
                continue;
            };
            if entry.extra & 0xFF == RESOURCE_FORK {
                *forks += 1;
                if !resource_fork_holds(&bytes) {
                    wrong.push(entry.name.clone());
                }
            } else if entry.kind == "Text" && bytes.contains(&0) {
                wrong.push(format!("{} (text with zero bytes)", entry.name));
            } else if is_nested_image(&entry.open_name, &bytes)
                && let Ok(inner) = open(Arc::new(bytes), &entry.open_name)
            {
                walk(&*inner, None, depth + 1, forks, wrong);
            }
        }
    }

    /// The real Mac disks in ..\testmedia, when it's there: every resource fork, on MFS disks that no other tool
    /// here reads and on HFS ones, is a whole resource fork.
    #[test]
    fn real_mac_disks_resource_forks_are_whole() {
        let root = Path::new("../testmedia");
        let Ok(folders) = std::fs::read_dir(root) else {
            return;
        };
        let (mut forks, mut wrong, mut disks) = (0, Vec::new(), 0);
        for folder in folders.flatten().map(|f| f.path()).filter(|p| p.is_dir()) {
            for file in std::fs::read_dir(&folder).unwrap().flatten().map(|f| f.path()).filter(|p| p.is_file()) {
                let name = file.file_name().unwrap().to_string_lossy().into_owned();
                let bytes = std::fs::read(&file).unwrap();
                let mac = looks_like_disk_copy(&bytes[..bytes.len().min(84)], bytes.len() as u64) || name.ends_with(".zip") && folder.ends_with("mac128k");
                if mac && let Ok(volume) = open(Arc::new(bytes), &name) {
                    disks += 1;
                    walk(&*volume, None, 0, &mut forks, &mut wrong);
                }
            }
        }
        assert!(wrong.is_empty(), "{forks} forks: {wrong:#?}");
        eprintln!("{disks} Mac disks, {forks} resource forks, all whole");
    }
}
