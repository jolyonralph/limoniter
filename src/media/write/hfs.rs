//! Mac HFS volumes, as Inside Macintosh: Files lays them out: in a plain disk image, a Disk Copy 4.2 image (whose
//! checksum is worked out again) or the one HFS partition of a disk with an Apple partition map.
//!
//! The master directory block (sector 2, copied near the volume's end) counts the volume's files and folders and
//! its free allocation blocks, and says where the volume bitmap (a bit set for each block in use) and the two
//! B-trees are. The catalog tree holds a record for every file and folder, keyed by its parent folder's number and
//! its name, and a thread record for every folder, keyed by its own number; the extents tree holds the pieces of a
//! fork past the three its catalog record has room for. A B-tree here is a header node (with a map of the nodes in
//! use), leaves holding the records in order and chained together, and index nodes above them pointing at each node
//! below by its first key.
//!
//! Each change is made to the records, and the trees are then built again in the space their files have (grown
//! from free blocks when they need more, past the master directory block's three extents into the extents tree for
//! the catalog): leaves packed in order, and as many levels of index nodes above them as they need, a tree the Mac
//! goes on adding to and splitting as usual. Names sort as the Mac's file system sorts them: by its own order of
//! the Mac Roman characters, upper and lower case alike. Nothing is changed on a volume whose trees, bitmap or
//! order of names don't agree with this, nor on a locked one; the last allocation block is never used, as Apple's
//! checker counts a file reaching it as wrong.
//!
//! A file's data fork is the file as other machines see it; its resource fork goes on as `name.rsrc` beside it,
//! as the reader shows it. Its type and creator come from the kind chosen, or from its ending.

use super::{Addition, Entry, ImageWriter, Kind, already};
use crate::media::mac::{self, RESOURCE_FORK};
use crate::media::tidy_name;
use std::cmp::Ordering;
use std::sync::OnceLock;

const NODE: usize = 512;
const INDEX: u8 = 0;
const HEADER: u8 = 1;
const MAP: u8 = 2;
const LEAF: u8 = 0xFF;
/// The node bits the header node's map record holds, and a map node's.
const HEADER_MAP_BITS: usize = 256 * 8;
const MAP_NODE_BITS: usize = (NODE - 14 - 4) * 8;
const ROOT: u32 = 2;
const FOLDER: u8 = 1;
const FILE: u8 = 2;
const FOLDER_THREAD: u8 = 3;
const FILE_THREAD: u8 = 4;
const FIRST_FREE_ID: u32 = 16;

/// A B-tree record: its key (without its length byte) and its data.
type Record = (Vec<u8>, Vec<u8>);

/// Extents: (first allocation block, count).
type Run = (usize, usize);

// ---------- Names ----------

/// The Mac's order of its characters for names, lowest first, letters that sort together in one string. Upper and
/// lower case sort alike, but accented letters apart from their plain ones; the control characters come first.
const ORDER: [&str; 184] = [
    " \u{A0}", "!", "\"", "\u{201C}", "\u{201D}", "\u{AB}", "\u{BB}", "#", "$", "%", "&", "'", "\u{2018}", "\u{2019}", "(", ")", "*", "+", ",", "-", ".", "/", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", ":", ";", "<", "=",
    ">", "?", "@", "Aa", "\u{E0}\u{C0}", "\u{C4}\u{E4}", "\u{E3}\u{C3}", "\u{C5}\u{E5}", "\u{C6}\u{E6}", "`", "\u{E1}", "\u{E2}", "\u{AA}", "Bb", "Cc", "\u{C7}\u{E7}", "Dd", "Ee", "\u{C9}\u{E9}", "\u{E8}", "\u{EA}", "\u{EB}", "Ff",
    "Gg", "Hh", "Ii", "\u{ED}", "\u{EC}", "\u{EE}", "\u{EF}", "Jj", "Kk", "Ll", "Mm", "Nn", "\u{D1}\u{F1}", "Oo", "\u{D6}\u{F6}", "\u{F5}\u{D5}", "\u{D8}\u{F8}", "\u{152}\u{153}", "\u{F3}", "\u{F2}", "\u{F4}", "\u{BA}", "Pp", "Qq",
    "Rr", "Ss", "\u{DF}", "Tt", "Uu", "\u{DC}\u{FC}", "\u{FA}", "\u{F9}", "\u{FB}", "Vv", "Ww", "Xx", "Yy", "\u{FF}", "Zz", "[", "\\", "]", "^", "_", "{", "|", "}", "~", "\u{7F}", "\u{2020}", "\u{B0}", "\u{A2}", "\u{A3}", "\u{A7}",
    "\u{2022}", "\u{B6}", "\u{AE}", "\u{A9}", "\u{2122}", "\u{B4}", "\u{A8}", "\u{2260}", "\u{221E}", "\u{B1}", "\u{2264}", "\u{2265}", "\u{A5}", "\u{B5}", "\u{2202}", "\u{2211}", "\u{220F}", "\u{3C0}", "\u{222B}", "\u{3A9}", "\u{BF}",
    "\u{A1}", "\u{AC}", "\u{221A}", "\u{192}", "\u{2248}", "\u{2206}", "\u{2026}", "\u{2013}", "\u{2014}", "\u{F7}", "\u{25CA}", "\u{178}", "\u{2044}", "\u{20AC}", "\u{2039}", "\u{203A}", "\u{FB01}", "\u{FB02}", "\u{2021}", "\u{B7}",
    "\u{201A}", "\u{201E}", "\u{2030}", "\u{C2}", "\u{CA}", "\u{C1}", "\u{CB}", "\u{C8}", "\u{CD}", "\u{CE}", "\u{CF}", "\u{CC}", "\u{D3}", "\u{D4}", "\u{F8FF}", "\u{D2}", "\u{DA}", "\u{DB}", "\u{D9}", "\u{131}", "\u{2C6}", "\u{2DC}",
    "\u{AF}", "\u{2D8}", "\u{2D9}", "\u{2DA}", "\u{B8}", "\u{2DD}", "\u{2DB}", "\u{2C7}",
];

/// Mac Roman's characters, by byte, and each byte's place in the Mac's order.
struct Roman {
    chars: [char; 256],
    rank: [u16; 256],
}

fn roman() -> &'static Roman {
    static ROMAN: OnceLock<Roman> = OnceLock::new();
    ROMAN.get_or_init(|| {
        let mut chars = ['\0'; 256];
        for (b, c) in chars.iter_mut().enumerate() {
            *c = mac::mac_roman(&[b as u8]).chars().next().unwrap_or('?');
        }
        // DEL is a control character the decoding leaves out; its place is in the order all the same.
        chars[0x7F] = '\u{7F}';
        let mut rank = [u16::MAX; 256];
        for (b, r) in rank.iter_mut().enumerate().take(0x20) {
            *r = b as u16;
        }
        for (place, group) in ORDER.iter().enumerate() {
            for c in group.chars() {
                if let Some(b) = (0x20..256).find(|b| chars[*b] == c) {
                    rank[b] = 0x20 + place as u16;
                }
            }
        }
        Roman { chars, rank }
    })
}

/// A name in Mac Roman, as HFS keeps it: at most `most` characters, and no colon, which the Mac uses between the
/// folders of a path.
fn mac_name(name: &str, most: usize) -> Result<Vec<u8>, String> {
    let name = name.trim();
    let roman = roman();
    let mut out = Vec::new();
    for c in name.chars() {
        if c == ':' {
            return Err(format!("\"{name}\" can't be a Mac name: the Mac keeps colons for between the folders of a path."));
        }
        let byte = (0x20..256).find(|b| roman.chars[*b] == c && *b != 0x7F).ok_or_else(|| format!("\"{name}\" has a character, {c}, that a Mac name can't hold."))?;
        out.push(byte as u8);
    }
    if out.is_empty() {
        return Err("A Mac name needs at least one character.".into());
    }
    if out.len() > most {
        return Err(format!("\"{name}\" is longer than the {most} characters a Mac name can be."));
    }
    Ok(out)
}

fn compare_names(a: &[u8], b: &[u8]) -> Ordering {
    let rank = &roman().rank;
    a.iter().map(|c| rank[usize::from(*c)]).cmp(b.iter().map(|c| rank[usize::from(*c)]))
}

fn shown(name: &[u8]) -> String {
    tidy_name(&mac::mac_roman(name))
}

// ---------- Keys and records ----------

fn be16(b: &[u8], at: usize) -> usize {
    usize::from(u16::from_be_bytes([b[at], b[at + 1]]))
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn put16(b: &mut [u8], at: usize, value: usize) {
    b[at..at + 2].copy_from_slice(&(value as u16).to_be_bytes());
}

fn put32(b: &mut [u8], at: usize, value: u32) {
    b[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

/// A catalog key, as a leaf holds it: a reserved byte, the parent's number, the name.
fn catalog_key(parent: u32, name: &[u8]) -> Vec<u8> {
    let mut key = vec![0];
    key.extend(parent.to_be_bytes());
    key.push(name.len() as u8);
    key.extend(name);
    key
}

fn key_parent(key: &[u8]) -> u32 {
    be32(key, 1)
}

fn key_name(key: &[u8]) -> &[u8] {
    &key[6..6 + usize::from(key[5])]
}

fn compare_catalog(a: &[u8], b: &[u8]) -> Ordering {
    key_parent(a).cmp(&key_parent(b)).then_with(|| compare_names(key_name(a), key_name(b)))
}

/// An extents key: the fork (0 data, &FF resource), the file's number, and the fork's block its extents start at.
fn extents_key(resource: bool, id: u32, start: usize) -> Vec<u8> {
    let mut key = vec![if resource { 0xFF } else { 0 }];
    key.extend(id.to_be_bytes());
    key.extend((start as u16).to_be_bytes());
    key
}

fn compare_extents(a: &[u8], b: &[u8]) -> Ordering {
    be32(a, 1).cmp(&be32(b, 1)).then(a[0].cmp(&b[0])).then(be16(a, 5).cmp(&be16(b, 5)))
}

fn runs_of(bytes: &[u8]) -> [Run; 3] {
    std::array::from_fn(|i| (be16(bytes, i * 4), be16(bytes, i * 4 + 2)))
}

fn put_runs(bytes: &mut [u8], runs: &[Run]) {
    bytes[..12].fill(0);
    for (i, (start, count)) in runs.iter().take(3).enumerate() {
        put16(bytes, i * 4, *start);
        put16(bytes, i * 4 + 2, *count);
    }
}

/// Now, as the Mac keeps dates: seconds since the start of 1904, in local time.
fn now() -> u32 {
    let local = chrono::Local::now().naive_local().and_utc().timestamp();
    (local + 2_082_844_800).clamp(0, i64::from(u32::MAX)) as u32
}

/// Where a file's fork is described in its catalog record: its length, its physical length and its extents.
struct ForkAt {
    length: usize,
    physical: usize,
    extents: usize,
}

const DATA_FORK: ForkAt = ForkAt { length: 26, physical: 30, extents: 74 };
const RESOURCE: ForkAt = ForkAt { length: 36, physical: 40, extents: 86 };

fn fork_at(resource: bool) -> &'static ForkAt {
    if resource { &RESOURCE } else { &DATA_FORK }
}

// ---------- The kinds of file ----------

/// The kinds offered when adding, each with its type and creator.
const KINDS: [(&str, &str, &[u8; 4], &[u8; 4]); 9] = [
    ("Text", "Plain text, opened by SimpleText or TeachText.", b"TEXT", b"ttxt"),
    ("Data", "Any other file; the Finder asks which program is to open it.", b"????", b"????"),
    ("Picture (PICT)", "A QuickDraw picture.", b"PICT", b"ttxt"),
    ("MacPaint picture", "A MacPaint document.", b"PNTG", b"MPNT"),
    ("JPEG picture", "Opened by PictureViewer.", b"JPEG", b"ogle"),
    ("GIF picture", "Opened by PictureViewer.", b"GIFf", b"ogle"),
    ("PNG picture", "Opened by PictureViewer.", b"PNGf", b"ogle"),
    ("StuffIt archive", "Opened by StuffIt Expander.", b"SIT!", b"SITx"),
    ("Disk Copy image", "A disk image, opened by Disk Copy.", b"dImg", b"dCpy"),
];

/// The kind a file most likely is, by its ending.
fn kind_for(name: &str) -> &'static str {
    let (_, ending) = super::stem_and_ending(name);
    match ending.to_lowercase().as_str() {
        "txt" | "text" | "asc" | "me" | "1st" | "md" | "htm" | "html" | "c" | "h" | "p" | "pas" | "a" | "s" | "asm" | "bas" | "csv" | "tex" | "hqx" => "Text",
        "pict" | "pct" | "pic" => "Picture (PICT)",
        "pntg" | "mac" | "mpnt" => "MacPaint picture",
        "jpg" | "jpeg" => "JPEG picture",
        "gif" => "GIF picture",
        "png" => "PNG picture",
        "sit" => "StuffIt archive",
        "image" | "img" | "dsk" | "dc42" => "Disk Copy image",
        _ => "Data",
    }
}

/// The type and creator of a kind, or of a name's ending when the kind isn't one of these.
fn type_and_creator(kind: &str, name: &str) -> ([u8; 4], [u8; 4]) {
    let kind = if KINDS.iter().any(|k| k.0 == kind) { kind } else { kind_for(name) };
    let found = KINDS.iter().find(|k| k.0 == kind).expect("every kind is in the table");
    (*found.2, *found.3)
}

// ---------- The volume ----------

/// Which B-tree: where its file is in the master directory block, its number, and its longest key.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tree {
    Extents,
    Catalog,
}

impl Tree {
    fn size_at(self) -> usize {
        if self == Tree::Extents { 130 } else { 146 }
    }
    fn runs_at(self) -> usize {
        self.size_at() + 4
    }
    fn clump_at(self) -> usize {
        if self == Tree::Extents { 74 } else { 78 }
    }
    fn id(self) -> u32 {
        if self == Tree::Extents { 3 } else { 4 }
    }
    fn key_length(self) -> usize {
        if self == Tree::Extents { 7 } else { 37 }
    }
}

pub struct Hfs {
    bytes: Vec<u8>,
    /// Where the volume is in the image, and its length.
    start: usize,
    length: usize,
    disk_copy: bool,
    block_size: usize,
    /// Where allocation block 0 is, from the volume's start.
    blocks_at: usize,
    blocks: usize,
    catalog: Vec<Record>,
    extents: Vec<Record>,
    /// Folders whose contents have changed, to be dated now.
    touched: Vec<u32>,
}

/// Where the HFS volume is in an image: a Disk Copy 4.2 image's disk, the one HFS partition in an Apple partition
/// map, or the image itself.
fn locate(bytes: &[u8]) -> Result<(usize, usize, bool), String> {
    let head = &bytes[..bytes.len().min(0x8800)];
    if mac::looks_like_disk_copy(head, bytes.len() as u64) {
        if head[81] == 0x24 {
            return Err("This Disk Copy image holds an Apple II disk, which isn't changed here.".into());
        }
        let length = be32(head, 64) as usize;
        let (start, inner) = (84, &bytes[84..84 + length]);
        return match mac::kind_of(&inner[..inner.len().min(0x8800)]) {
            Some(mac::Kind::Hfs) => Ok((start, length, true)),
            other => Err(not_hfs(other)),
        };
    }
    match mac::kind_of(head) {
        Some(mac::Kind::Hfs) => Ok((0, bytes.len(), false)),
        Some(mac::Kind::Partitioned) => {
            let block = match be16(bytes, 2) {
                size if size >= 512 && size.is_multiple_of(512) => size,
                _ => 512,
            };
            let entry = |i: usize| bytes.get(block * (1 + i)..block * (1 + i) + 512).filter(|e| e[..2] == *b"PM");
            let count = entry(0).map_or(0, |e| be32(e, 4) as usize).min(256);
            let mut found = Vec::new();
            for i in 0..count {
                let Some(e) = entry(i) else { break };
                let kind = String::from_utf8_lossy(&e[48..80]).trim_end_matches('\0').to_string();
                if matches!(kind.as_str(), "Apple_HFS" | "Apple_HFSX" | "Apple_MFS") {
                    found.push((be32(e, 8) as usize * block, be32(e, 12) as usize * block));
                }
            }
            match found[..] {
                [(start, length)] if start + length <= bytes.len() && length >= 1536 => match mac::kind_of(&bytes[start..start + length.min(0x8800)]) {
                    Some(mac::Kind::Hfs) => Ok((start, length, false)),
                    other => Err(not_hfs(other)),
                },
                [] => Err("This disk's partition map has no Mac volume in it.".into()),
                [_] => Err("This disk's Mac partition runs past the end of the image, so it can't be changed.".into()),
                _ => Err("This disk has several Mac partitions; only a disk with one is changed here.".into()),
            }
        }
        other => Err(not_hfs(other)),
    }
}

fn not_hfs(kind: Option<mac::Kind>) -> String {
    match kind {
        Some(mac::Kind::Mfs) => "This is an MFS disk, the 400K floppies' older file system, which is only read.".into(),
        Some(mac::Kind::HfsPlus | mac::Kind::Gpt) => "This is an HFS+ (Mac OS Extended) disk, which is only read; plain HFS disks can be changed.".into(),
        _ => "This isn't a Mac HFS disk Limoniter can change.".into(),
    }
}

/// A blank HFS floppy of 400K, 800K or 1.4MB, as the Finder's Erase Disk leaves one (not made bootable).
pub fn blank(kilobytes: usize, label: &str) -> Vec<u8> {
    let sectors = kilobytes * 2;
    let mut bytes = vec![0u8; sectors * 512];
    let (bitmap_at, first_block) = (3usize, 4usize);
    // The volume ends with the alternate master directory block and a sector kept spare.
    let blocks = sectors - first_block - 2;
    let tree_blocks = (blocks / 128).max(4);
    let label = mac_name(if label.trim().is_empty() { "Untitled" } else { label }, 27).or_else(|_| mac_name("Untitled", 27)).expect("a plain name is one");
    let date = now();
    let mdb = 1024;
    let m = &mut bytes[mdb..mdb + 512];
    m[..2].copy_from_slice(b"BD");
    put32(m, 2, date);
    put32(m, 6, date);
    put16(m, 10, 0x0100);
    put16(m, 14, bitmap_at);
    put16(m, 16, 2 * tree_blocks);
    put16(m, 18, blocks);
    put32(m, 20, 512);
    put32(m, 24, 4 * 512);
    put16(m, 28, first_block);
    put32(m, 30, FIRST_FREE_ID);
    put16(m, 34, blocks - 2 * tree_blocks);
    m[36] = label.len() as u8;
    m[37..37 + label.len()].copy_from_slice(&label);
    for tree in [Tree::Extents, Tree::Catalog] {
        let first = if tree == Tree::Extents { 0 } else { tree_blocks };
        put32(m, tree.clump_at(), (tree_blocks * 512) as u32);
        put32(m, tree.size_at(), (tree_blocks * 512) as u32);
        put_runs(&mut m[tree.runs_at()..], &[(first, tree_blocks)]);
    }
    for block in 0..2 * tree_blocks {
        bytes[bitmap_at * 512 + block / 8] |= 0x80 >> (block % 8);
    }
    let mut hfs = Hfs { bytes, start: 0, length: sectors * 512, disk_copy: false, block_size: 512, blocks_at: first_block * 512, blocks, catalog: Vec::new(), extents: Vec::new(), touched: Vec::new() };
    let mut root = vec![0u8; 70];
    root[0] = FOLDER;
    put32(&mut root, 6, ROOT);
    put32(&mut root, 10, date);
    put32(&mut root, 14, date);
    hfs.catalog = vec![(catalog_key(1, &label), root), (catalog_key(ROOT, &[]), thread(FOLDER_THREAD, 1, &label))];
    hfs.flush().expect("a blank volume's trees fit");
    hfs.bytes
}

/// A thread record: the folder's (or file's) parent and name, found by its own number.
fn thread(kind: u8, parent: u32, name: &[u8]) -> Vec<u8> {
    let mut data = vec![0u8; 46];
    data[0] = kind;
    put32(&mut data, 10, parent);
    data[14] = name.len() as u8;
    data[15..15 + name.len()].copy_from_slice(name);
    data
}

impl Hfs {
    pub fn open(bytes: Vec<u8>) -> Result<Self, String> {
        let (start, length, disk_copy) = locate(&bytes)?;
        if length < 4096 || start + length > bytes.len() {
            return Err("This HFS volume is too small to be a whole one, so it isn't changed.".into());
        }
        let mdb = &bytes[start + 1024..start + 1536];
        if mdb[124..126] == *b"H+" {
            return Err("This is an HFS+ volume inside an HFS wrapper, which is only read.".into());
        }
        let attributes = be16(mdb, 10);
        if attributes & 0x8080 != 0 {
            return Err("This volume is locked, so it's left as it is.".into());
        }
        // The bit saying the volume was put away properly isn't asked for: Apple's own master floppies don't have
        // it. The trees and the bitmap are checked instead, and nothing is changed if they don't agree.
        let block_size = be32(mdb, 20) as usize;
        let blocks = be16(mdb, 18);
        let blocks_at = be16(mdb, 28) * 512;
        let bitmap = be16(mdb, 14) * 512;
        if block_size == 0 || !block_size.is_multiple_of(512) || blocks == 0 || blocks_at + blocks * block_size > length || bitmap + blocks.div_ceil(8) > blocks_at.max(bitmap + 1) || bitmap < 1536 {
            return Err("This HFS volume's master directory block doesn't make sense, so it isn't changed.".into());
        }
        let mut hfs = Self { bytes, start, length, disk_copy, block_size, blocks_at, blocks, catalog: Vec::new(), extents: Vec::new(), touched: Vec::new() };
        hfs.extents = hfs.read_tree(Tree::Extents)?;
        hfs.catalog = hfs.read_tree(Tree::Catalog)?;
        hfs.check()?;
        Ok(hfs)
    }

    fn mdb(&self) -> usize {
        self.start + 1024
    }

    fn mdb16(&self, at: usize) -> usize {
        be16(&self.bytes, self.mdb() + at)
    }

    fn mdb32(&self, at: usize) -> u32 {
        be32(&self.bytes, self.mdb() + at)
    }

    fn set_mdb16(&mut self, at: usize, value: usize) {
        let mdb = self.mdb();
        put16(&mut self.bytes, mdb + at, value);
    }

    fn set_mdb32(&mut self, at: usize, value: u32) {
        let mdb = self.mdb();
        put32(&mut self.bytes, mdb + at, value);
    }

    // ----- Allocation blocks -----

    fn bit(&self, block: usize) -> (usize, u8) {
        (self.start + self.mdb16(14) * 512 + block / 8, 0x80 >> (block % 8))
    }

    fn is_used(&self, block: usize) -> bool {
        let (at, mask) = self.bit(block);
        self.bytes[at] & mask != 0
    }

    fn set_used(&mut self, block: usize, used: bool) {
        let (at, mask) = self.bit(block);
        if used {
            self.bytes[at] |= mask;
        } else {
            self.bytes[at] &= !mask;
        }
    }

    fn free_blocks(&self) -> usize {
        (0..self.blocks).filter(|b| !self.is_used(*b)).count()
    }

    /// The last allocation block is never given to a file: Apple's checker counts an extent reaching it as wrong.
    fn usable(&self) -> usize {
        self.blocks - 1
    }

    fn usable_free(&self) -> usize {
        (0..self.usable()).filter(|b| !self.is_used(*b)).count()
    }

    fn block_at(&self, block: usize) -> usize {
        self.start + self.blocks_at + block * self.block_size
    }

    /// Takes `count` free blocks: in one run if there's one long enough (the first, as the Mac looks from its
    /// allocation pointer), otherwise in the fewest runs from the start.
    fn allocate(&mut self, count: usize) -> Result<Vec<Run>, String> {
        if count == 0 {
            return Ok(Vec::new());
        }
        if count > self.usable_free() {
            return Err(format!("The disk is full: this needs {} K, and {} K is free.", (count * self.block_size).div_ceil(1024), self.usable_free() * self.block_size / 1024));
        }
        let mut free_runs: Vec<Run> = Vec::new();
        let mut block = 0;
        while block < self.usable() {
            if self.is_used(block) {
                block += 1;
                continue;
            }
            let first = block;
            while block < self.usable() && !self.is_used(block) {
                block += 1;
            }
            free_runs.push((first, block - first));
        }
        let pointer = self.mdb16(16);
        let runs = if let Some(run) = free_runs.iter().filter(|r| r.1 >= count).min_by_key(|r| (r.0 < pointer, r.0)) {
            vec![(run.0, count)]
        } else {
            free_runs.sort_by_key(|r| std::cmp::Reverse(r.1));
            let mut left = count;
            let mut out = Vec::new();
            for (first, length) in free_runs {
                if left == 0 {
                    break;
                }
                let take = length.min(left);
                out.push((first, take));
                left -= take;
            }
            out.sort();
            out
        };
        for (first, length) in &runs {
            for block in *first..first + length {
                self.set_used(block, true);
            }
        }
        if let Some((first, length)) = runs.last() {
            self.set_mdb16(16, first + length);
        }
        Ok(runs)
    }

    fn release(&mut self, runs: &[Run]) {
        for (first, count) in runs {
            for block in *first..(first + count).min(self.blocks) {
                self.set_used(block, false);
            }
        }
    }

    /// A fork's runs: the three given, then any in the extents tree, while they carry on from where the last left off.
    fn fork_runs(&self, id: u32, resource: bool, first: [Run; 3]) -> Vec<Run> {
        let mut runs: Vec<Run> = first.into_iter().filter(|r| r.1 > 0).collect();
        let mut covered: usize = runs.iter().map(|r| r.1).sum();
        for (key, data) in &self.extents {
            if be32(key, 1) == id && (key[0] == 0xFF) == resource && be16(key, 5) == covered {
                let more: Vec<Run> = runs_of(data).into_iter().filter(|r| r.1 > 0).collect();
                covered += more.iter().map(|r| r.1).sum::<usize>();
                runs.extend(more);
            }
        }
        runs
    }

    // ----- B-trees -----

    fn tree_runs(&self, tree: Tree) -> Vec<Run> {
        let mdb = &self.bytes[self.mdb()..self.mdb() + 512];
        self.fork_runs(tree.id(), false, runs_of(&mdb[tree.runs_at()..]))
    }

    /// Where each node of a tree's file is in the image, as many as its length holds.
    fn node_places(&self, tree: Tree) -> Vec<usize> {
        let size = self.mdb32(tree.size_at()) as usize;
        let mut out = Vec::new();
        for (first, count) in self.tree_runs(tree) {
            for block in first..first + count {
                for part in 0..self.block_size / NODE {
                    out.push(self.block_at(block) + part * NODE);
                }
            }
        }
        out.truncate(size / NODE);
        out
    }

    fn read_tree(&self, tree: Tree) -> Result<Vec<Record>, String> {
        let damaged = |what: &str| format!("The {} tree {what}, so the volume is damaged; let the Mac check it first.", if tree == Tree::Catalog { "catalog" } else { "extents" });
        let places = self.node_places(tree);
        if places.is_empty() || places.iter().any(|p| p + NODE > self.start + self.length) {
            return Err(damaged("lies outside the volume"));
        }
        let header = &self.bytes[places[0]..places[0] + NODE];
        if header[8] != HEADER || be16(header, 32) != NODE || be16(header, 34) != tree.key_length() {
            return Err(damaged("has no header node of the kind HFS has"));
        }
        let mut node = be32(header, 24) as usize;
        let mut out: Vec<Record> = Vec::new();
        let mut seen = 0;
        while node != 0 {
            seen += 1;
            let at = *places.get(node).filter(|_| seen <= places.len()).ok_or_else(|| damaged("has a chain of leaves that leaves it or loops"))?;
            let bytes = &self.bytes[at..at + NODE];
            if bytes[8] != LEAF || bytes[9] != 1 {
                return Err(damaged("has a leaf that isn't one"));
            }
            let count = be16(bytes, 10);
            if 14 + 2 * (count + 1) > NODE {
                return Err(damaged("has a node with more records than fit"));
            }
            for i in 0..count {
                let (start, end) = (be16(bytes, NODE - 2 * (i + 1)), be16(bytes, NODE - 2 * (i + 2)));
                if start < 14 || end > NODE - 2 * (count + 1) || start >= end {
                    return Err(damaged("has a record outside its node"));
                }
                let key_length = usize::from(bytes[start]);
                let data_at = (start + 1 + key_length + 1) & !1;
                if data_at > end || key_length < 6 {
                    return Err(damaged("has a record whose key doesn't fit"));
                }
                let key = &bytes[start + 1..start + 1 + key_length];
                let key = if tree == Tree::Catalog {
                    if usize::from(key[5]) > 31 || 6 + usize::from(key[5]) > key_length {
                        return Err(damaged("has a name longer than HFS allows"));
                    }
                    catalog_key(key_parent(key), key_name(key))
                } else {
                    if key_length != 7 {
                        return Err(damaged("has a key of the wrong length"));
                    }
                    key.to_vec()
                };
                out.push((key, bytes[data_at..end].to_vec()));
            }
            node = be32(bytes, 0) as usize;
        }
        // The records must be in the order the trees are built in, or a name sorts differently here than on the
        // Mac, and building the tree again would lose it.
        let compare = if tree == Tree::Catalog { compare_catalog } else { compare_extents };
        if out.windows(2).any(|w| compare(&w[0].0, &w[1].0) != Ordering::Less) {
            return Err(damaged("has records out of order, or a name Limoniter sorts differently from the Mac"));
        }
        Ok(out)
    }

    /// Checks the catalog makes sense and that every block a file or tree has is marked in use, so nothing is put
    /// over it.
    fn check(&self) -> Result<(), String> {
        if !self.catalog.iter().any(|(k, d)| key_parent(k) == 1 && d.len() >= 70 && d[0] == FOLDER && be32(d, 6) == ROOT) {
            return Err("This HFS volume's catalog has no top folder, so it's damaged.".into());
        }
        let mut runs = self.tree_runs(Tree::Extents);
        runs.extend(self.tree_runs(Tree::Catalog));
        for (_, data) in &self.catalog {
            if data[0] == FILE {
                if data.len() < 102 {
                    return Err("A file's catalog record is too short, so the volume is damaged.".into());
                }
                for resource in [false, true] {
                    runs.extend(self.fork_runs(be32(data, 20), resource, runs_of(&data[fork_at(resource).extents..])));
                }
            } else if data[0] == FOLDER && data.len() < 70 {
                return Err("A folder's catalog record is too short, so the volume is damaged.".into());
            }
        }
        for (first, count) in runs {
            if first + count > self.blocks || (first..first + count).any(|b| !self.is_used(b)) {
                return Err("This volume's bitmap doesn't mark all of its files' blocks in use; let the Mac check it with Disk First Aid first.".into());
            }
        }
        Ok(())
    }

    /// Builds a tree again from its records, in its file, growing the file if it must.
    fn build(&mut self, tree: Tree) -> Result<(), String> {
        let records = if tree == Tree::Catalog { self.catalog.clone() } else { self.extents.clone() };
        // The leaves, packed in order.
        let mut leaves: Vec<Vec<Vec<u8>>> = Vec::new();
        let mut used = NODE;
        for (key, data) in &records {
            let mut record = vec![key.len() as u8];
            record.extend(key);
            if record.len() % 2 == 1 {
                record.push(0);
            }
            record.extend(data);
            if used + record.len() + 2 > NODE - 2 {
                leaves.push(Vec::new());
                used = 14;
            }
            used += record.len() + 2;
            leaves.last_mut().expect("a leaf was started").push(record);
        }
        // The index levels above, each pointing at the level below by the first key of each node.
        let index_key = |key: &[u8]| {
            let mut out = vec![tree.key_length() as u8];
            out.extend(key);
            out.resize(1 + tree.key_length(), 0);
            out
        };
        let per_index = (NODE - 14 - 2) / (1 + tree.key_length() + 1 + 4 + 2);
        let mut levels: Vec<Vec<Vec<Vec<u8>>>> = vec![leaves];
        let mut firsts: Vec<Vec<u8>> = Vec::new();
        {
            let mut i = 0;
            for leaf in &levels[0] {
                firsts.push(records[i].0.clone());
                i += leaf.len();
            }
        }
        let mut level_firsts = vec![firsts];
        while levels.last().expect("a level").len() > 1 {
            let below = level_firsts.last().expect("a level").clone();
            let nodes: Vec<Vec<Vec<u8>>> = below.chunks(per_index).map(|keys| keys.iter().map(|k| index_key(k)).collect()).collect();
            level_firsts.push(below.chunks(per_index).map(|keys| keys[0].clone()).collect());
            levels.push(nodes);
        }
        let content: usize = levels.iter().map(Vec::len).sum();
        // Map nodes, past the header's own map.
        let mut total = self.mdb32(tree.size_at()) as usize / NODE;
        let maps_for = |total: usize| if total > HEADER_MAP_BITS { (total - HEADER_MAP_BITS).div_ceil(MAP_NODE_BITS) } else { 0 };
        while 1 + maps_for(total) + content > total {
            self.grow(tree, (1 + maps_for(total) + content) * NODE)?;
            total = self.mdb32(tree.size_at()) as usize / NODE;
        }
        let maps = maps_for(total);
        let places = self.node_places(tree);
        // Numbers: the header, its map nodes, the leaves, then each index level up to the root.
        let mut next = 1 + maps;
        let mut numbers: Vec<Vec<usize>> = Vec::new();
        for level in &levels {
            numbers.push((next..next + level.len()).collect());
            next += level.len();
        }
        let header_at = places[0];
        let old_header = self.bytes[header_at..header_at + NODE].to_vec();
        for &place in &places {
            self.bytes[place..place + NODE].fill(0);
        }
        // Index records point at the nodes below them.
        for (height, level) in levels.iter().enumerate() {
            for (i, node_records) in level.iter().enumerate() {
                let mut records = node_records.clone();
                if height > 0 {
                    let below = &numbers[height - 1];
                    for (j, record) in records.iter_mut().enumerate() {
                        record.extend((below[i * per_index + j] as u32).to_be_bytes());
                    }
                }
                let same = &numbers[height];
                let forward = same.get(i + 1).copied().unwrap_or(0);
                let back = if i == 0 { 0 } else { same[i - 1] };
                let kind = if height == 0 { LEAF } else { INDEX };
                let node = node_bytes(forward, back, kind, height as u8 + 1, &records);
                let at = places[same[i]];
                self.bytes[at..at + NODE].copy_from_slice(&node);
            }
        }
        // The map of nodes in use: the header, the map nodes and those just written.
        let in_use = 1 + maps + content;
        let bits = |from: usize, count: usize| -> Vec<u8> {
            let mut out = vec![0u8; count / 8];
            for node in from..(from + count).min(in_use) {
                out[(node - from) / 8] |= 0x80 >> ((node - from) % 8);
            }
            out
        };
        for m in 0..maps {
            let forward = if m + 1 < maps { m + 2 } else { 0 };
            let record = bits(HEADER_MAP_BITS + m * MAP_NODE_BITS, MAP_NODE_BITS);
            let node = node_bytes(forward, 0, MAP, 0, &[record]);
            let at = places[1 + m];
            self.bytes[at..at + NODE].copy_from_slice(&node);
        }
        let depth = if records.is_empty() { 0 } else { levels.len() };
        let mut head = old_header[14..14 + 106].to_vec();
        put16(&mut head, 0, depth);
        put32(&mut head, 2, if records.is_empty() { 0 } else { *numbers.last().expect("a level")[..].first().expect("a root") as u32 });
        put32(&mut head, 6, records.len() as u32);
        put32(&mut head, 10, if records.is_empty() { 0 } else { numbers[0][0] as u32 });
        put32(&mut head, 14, if records.is_empty() { 0 } else { *numbers[0].last().expect("a leaf") as u32 });
        put16(&mut head, 18, NODE);
        put16(&mut head, 20, tree.key_length());
        put32(&mut head, 22, total as u32);
        put32(&mut head, 26, (total - in_use) as u32);
        let user = old_header[120..248].to_vec();
        let node = node_bytes(if maps > 0 { 1 } else { 0 }, 0, HEADER, 0, &[head, user, bits(0, HEADER_MAP_BITS)]);
        self.bytes[header_at..header_at + NODE].copy_from_slice(&node);
        Ok(())
    }

    /// Makes a tree's file at least `want` bytes: its last extent carried on where the blocks after it are free,
    /// otherwise new extents in the master directory block's three, and for the catalog, once those are used, in
    /// the extents tree as the Mac puts them.
    fn grow(&mut self, tree: Tree, want: usize) -> Result<(), String> {
        let size = self.mdb32(tree.size_at()) as usize;
        let clump = (self.mdb32(tree.clump_at()) as usize).max(self.block_size);
        let more = (want.saturating_sub(size)).max(clump).div_ceil(self.block_size);
        let full = || format!("The {} has no room to grow on this disk.", if tree == Tree::Catalog { "catalog (the list of files)" } else { "extents tree" });
        let mdb = self.mdb();
        let mut slots = runs_of(&self.bytes[mdb + tree.runs_at()..]);
        let all = self.tree_runs(tree);
        let covered: usize = all.iter().map(|r| r.1).sum();
        if size != covered * self.block_size {
            return Err(full());
        }
        let mut overflowing = all.len() > slots.iter().filter(|r| r.1 > 0).count();
        if !overflowing && let Some(last) = slots.iter().rposition(|r| r.1 > 0) {
            let (first, count) = slots[last];
            let end = first + count;
            if end + more <= self.usable() && count + more <= 0xFFFF && (end..end + more).all(|b| !self.is_used(b)) {
                for block in end..end + more {
                    self.set_used(block, true);
                }
                slots[last].1 += more;
                put_runs(&mut self.bytes[mdb + tree.runs_at()..], &slots);
                self.set_mdb32(tree.size_at(), (size + more * self.block_size) as u32);
                return Ok(());
            }
        }
        let mut start = covered;
        for run in self.allocate(more)? {
            match slots.iter().position(|r| r.1 == 0) {
                Some(free) if !overflowing => slots[free] = run,
                _ if tree == Tree::Catalog => {
                    overflowing = true;
                    // The last of the catalog's own extent records, if it has room, or a new one.
                    let last = self.extents.iter().rposition(|(k, _)| k[0] == 0 && be32(k, 1) == tree.id());
                    match last.filter(|at| runs_of(&self.extents[*at].1).iter().any(|r| r.1 == 0)) {
                        Some(at) => {
                            let mut runs: Vec<Run> = runs_of(&self.extents[at].1).into_iter().filter(|r| r.1 > 0).collect();
                            runs.push(run);
                            put_runs(&mut self.extents[at].1, &runs);
                        }
                        None => {
                            let key = extents_key(false, tree.id(), start);
                            let mut data = vec![0u8; 12];
                            put_runs(&mut data, &[run]);
                            let at = self.extents.binary_search_by(|(k, _)| compare_extents(k, &key)).unwrap_or_else(|at| at);
                            self.extents.insert(at, (key, data));
                        }
                    }
                }
                _ => return Err(full()),
            }
            start += run.1;
        }
        put_runs(&mut self.bytes[mdb + tree.runs_at()..], &slots);
        self.set_mdb32(tree.size_at(), (size + more * self.block_size) as u32);
        Ok(())
    }

    /// Writes the trees, the counts and dates, the alternate master directory block and Disk Copy's checksum.
    fn flush(&mut self) -> Result<(), String> {
        let date = now();
        // Every folder's count of what's in it, and the volume's.
        let mut counts: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        let (mut files, mut folders, mut root_files, mut root_folders) = (0, 0, 0, 0);
        for (key, data) in &self.catalog {
            if matches!(data[0], FILE | FOLDER) {
                *counts.entry(key_parent(key)).or_default() += 1;
                let top = key_parent(key) == ROOT;
                if data[0] == FILE {
                    files += 1;
                    root_files += usize::from(top);
                } else if key_parent(key) != 1 {
                    folders += 1;
                    root_folders += usize::from(top);
                }
            }
        }
        let touched = std::mem::take(&mut self.touched);
        for (_, data) in &mut self.catalog {
            if data[0] == FOLDER {
                let id = be32(data, 6);
                put16(data, 4, counts.get(&id).copied().unwrap_or(0).min(0xFFFF));
                if touched.contains(&id) {
                    put32(data, 14, date);
                }
            }
        }
        // The catalog first, as growing it may give the extents tree another record.
        self.build(Tree::Catalog)?;
        self.build(Tree::Extents)?;
        self.set_mdb32(6, date);
        self.set_mdb16(12, root_files.min(0xFFFF));
        self.set_mdb16(34, self.free_blocks());
        let writes = self.mdb32(70).wrapping_add(1);
        self.set_mdb32(70, writes);
        self.set_mdb16(82, root_folders.min(0xFFFF));
        self.set_mdb32(84, files as u32);
        self.set_mdb32(88, folders as u32);
        // The alternate master directory block, in the volume's second sector from the end.
        let (mdb, alternate) = (self.mdb(), self.start + self.length - 1024);
        let copy = self.bytes[mdb..mdb + 512].to_vec();
        self.bytes[alternate..alternate + 512].copy_from_slice(&copy);
        if self.disk_copy {
            let mut sum = 0u32;
            for pair in self.bytes[84..84 + self.length].chunks(2) {
                sum = sum.wrapping_add(u32::from(pair[0]) << 8 | u32::from(*pair.get(1).unwrap_or(&0))).rotate_right(1);
            }
            put32(&mut self.bytes, 72, sum);
        }
        Ok(())
    }

    // ----- The catalog -----

    fn position(&self, key: &[u8]) -> Result<usize, usize> {
        self.catalog.binary_search_by(|(k, _)| compare_catalog(k, key))
    }

    fn insert(&mut self, key: Vec<u8>, data: Vec<u8>) {
        let at = self.position(&key).unwrap_or_else(|at| at);
        self.catalog.insert(at, (key, data));
    }

    fn folder_id(&self, folder: Option<&Entry>) -> Result<u32, String> {
        let Some(folder) = folder else {
            return Ok(ROOT);
        };
        let id = folder.location as u32;
        if folder.is_dir && self.catalog.iter().any(|(_, d)| d[0] == FOLDER && be32(d, 6) == id) {
            Ok(id)
        } else {
            Err(format!("The folder {} isn't on the disk.", folder.name))
        }
    }

    /// The index of an entry's catalog record: a folder's by its number; a file's as the reader counts them, the
    /// nth file record, which must also have the entry's name.
    fn record_of(&self, parent: u32, entry: &Entry) -> Result<usize, String> {
        let missing = || format!("{} isn't on the disk.", entry.name);
        if entry.is_dir {
            let id = entry.location as u32;
            return self.catalog.iter().position(|(k, d)| d[0] == FOLDER && be32(d, 6) == id && key_parent(k) == parent).ok_or_else(missing);
        }
        let resource = entry.extra & 0xFF == RESOURCE_FORK;
        let name = if resource { entry.name.strip_suffix(".rsrc").unwrap_or(&entry.name) } else { &entry.name };
        let index = self.catalog.iter().enumerate().filter(|(_, (_, d))| d[0] == FILE).nth(entry.location as usize).map(|(i, _)| i);
        match index {
            Some(i) if key_parent(&self.catalog[i].0) == parent && shown(key_name(&self.catalog[i].0)) == name => Ok(i),
            _ => self.catalog.iter().position(|(k, d)| d[0] == FILE && key_parent(k) == parent && shown(key_name(k)) == name).ok_or_else(missing),
        }
    }

    fn new_id(&mut self) -> Result<u32, String> {
        let id = self.mdb32(30).max(FIRST_FREE_ID);
        if id == u32::MAX {
            return Err("This volume has run out of file numbers.".into());
        }
        self.set_mdb32(30, id + 1);
        Ok(id)
    }

    /// Takes a fork's blocks and extents off, leaving it empty.
    fn clear_fork(&mut self, index: usize, resource: bool) {
        let data = &self.catalog[index].1;
        let id = be32(data, 20);
        let at = fork_at(resource);
        let runs = self.fork_runs(id, resource, runs_of(&data[at.extents..]));
        self.release(&runs);
        self.extents.retain(|(k, _)| !(be32(k, 1) == id && (k[0] == 0xFF) == resource));
        let data = &mut self.catalog[index].1;
        put32(data, at.length, 0);
        put32(data, at.physical, 0);
        data[at.extents..at.extents + 12].fill(0);
    }

    /// Writes a fork's bytes into newly taken blocks, its first three extents in the record and the rest in the
    /// extents tree.
    fn write_fork(&mut self, index: usize, resource: bool, bytes: &[u8]) -> Result<(), String> {
        let count = bytes.len().div_ceil(self.block_size);
        if count > 0xFFFF {
            return Err("The file is larger than an HFS volume can hold.".into());
        }
        let runs = self.allocate(count)?;
        let mut written = 0;
        for (first, length) in &runs {
            let at = self.block_at(*first);
            let room = length * self.block_size;
            self.bytes[at..at + room].fill(0);
            let part = &bytes[written..(written + room).min(bytes.len())];
            self.bytes[at..at + part.len()].copy_from_slice(part);
            written += part.len();
        }
        let id = be32(&self.catalog[index].1, 20);
        let mut covered: usize = runs.iter().take(3).map(|r| r.1).sum();
        for more in runs.get(3..).unwrap_or_default().chunks(3) {
            let mut data = vec![0u8; 12];
            put_runs(&mut data, more);
            let key = extents_key(resource, id, covered);
            let at = self.extents.binary_search_by(|(k, _)| compare_extents(k, &key)).unwrap_or_else(|at| at);
            self.extents.insert(at, (key, data));
            covered += more.iter().map(|r| r.1).sum::<usize>();
        }
        let at = fork_at(resource);
        let data = &mut self.catalog[index].1;
        put32(data, at.length, bytes.len() as u32);
        put32(data, at.physical, (count * self.block_size) as u32);
        put_runs(&mut data[at.extents..], &runs);
        put32(data, 48, now());
        Ok(())
    }

    /// A new, empty file's catalog record, its index.
    fn new_file(&mut self, parent: u32, name: &[u8], kind: [u8; 4], creator: [u8; 4]) -> Result<usize, String> {
        let id = self.new_id()?;
        let date = now();
        let mut data = vec![0u8; 102];
        data[0] = FILE;
        data[4..8].copy_from_slice(&kind);
        data[8..12].copy_from_slice(&creator);
        put32(&mut data, 20, id);
        put32(&mut data, 44, date);
        put32(&mut data, 48, date);
        let key = catalog_key(parent, name);
        self.insert(key.clone(), data);
        Ok(self.position(&key).expect("just put in"))
    }
}

/// A B-tree node: its descriptor (forward and back links, kind, height, count), records and their offsets.
fn node_bytes(forward: usize, back: usize, kind: u8, height: u8, records: &[Vec<u8>]) -> [u8; NODE] {
    let mut node = [0u8; NODE];
    put32(&mut node, 0, forward as u32);
    put32(&mut node, 4, back as u32);
    node[8] = kind;
    node[9] = height;
    put16(&mut node, 10, records.len());
    let mut at = 14;
    for (i, record) in records.iter().enumerate() {
        put16(&mut node, NODE - 2 * (i + 1), at);
        node[at..at + record.len()].copy_from_slice(record);
        at += record.len();
    }
    put16(&mut node, NODE - 2 * (records.len() + 1), at);
    node
}

/// A name the person gave split into the file's name and whether it's the resource fork: `name.rsrc` is `name`'s.
fn fork_of(name: &str) -> (&str, bool) {
    let trimmed = name.trim();
    match trimmed.len().checked_sub(5).filter(|at| *at > 0 && trimmed.is_char_boundary(*at)) {
        Some(at) if trimmed[at..].eq_ignore_ascii_case(".rsrc") => (&trimmed[..at], true),
        _ => (trimmed, false),
    }
}

/// The changes themselves, each leaving the records and bitmap as they're to be; `flush` writes the rest.
impl Hfs {
    /// Makes a change and writes it out, or, where it can't be made whole, leaves the image as it was.
    fn atomically(&mut self, change: impl FnOnce(&mut Self) -> Result<(), String>) -> Result<(), String> {
        let saved = (self.bytes.clone(), self.catalog.clone(), self.extents.clone());
        let result = change(self).and_then(|()| self.flush());
        if result.is_err() {
            (self.bytes, self.catalog, self.extents) = saved;
            self.touched.clear();
        }
        result
    }

    fn add_file(&mut self, folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        let parent = self.folder_id(folder)?;
        let (name, resource) = fork_of(&file.name);
        let mac = mac_name(name, 31)?;
        let fork_name = if resource { format!("{}.rsrc", shown(&mac)) } else { shown(&mac) };
        let (kind, creator) = type_and_creator(&file.kind, name);
        let index = match self.position(&catalog_key(parent, &mac)) {
            Ok(index) => {
                let data = &self.catalog[index].1;
                if data[0] != FILE {
                    return Err(format!("{} is a folder here, so a file can't replace it.", shown(&mac)));
                }
                if be32(data, fork_at(resource).length) > 0 && !file.replace {
                    return Err(already(&fork_name));
                }
                if data[2] & 1 != 0 {
                    return Err(format!("{} is locked; unlock it in the Finder first.", shown(&mac)));
                }
                self.clear_fork(index, resource);
                index
            }
            Err(_) => self.new_file(parent, &mac, kind, creator)?,
        };
        if !resource {
            let data = &mut self.catalog[index].1;
            data[4..8].copy_from_slice(&kind);
            data[8..12].copy_from_slice(&creator);
        }
        self.write_fork(index, resource, &file.bytes)?;
        self.touched.push(parent);
        Ok(())
    }

    fn delete_entry(&mut self, folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        let parent = self.folder_id(folder)?;
        let index = self.record_of(parent, entry)?;
        let data = self.catalog[index].1.clone();
        if data[0] == FOLDER {
            let id = be32(&data, 6);
            if self.catalog.iter().any(|(k, d)| key_parent(k) == id && matches!(d[0], FILE | FOLDER)) {
                return Err("The folder has files in it; take them off first.".into());
            }
            self.catalog.remove(index);
            if let Ok(thread) = self.position(&catalog_key(id, &[])) {
                self.catalog.remove(thread);
            }
        } else {
            if data[2] & 1 != 0 {
                return Err(format!("{} is locked; unlock it in the Finder first.", entry.name));
            }
            let resource_only = entry.extra & 0xFF == RESOURCE_FORK;
            self.clear_fork(index, true);
            if !resource_only {
                self.clear_fork(index, false);
                let id = be32(&data, 20);
                self.catalog.remove(index);
                if let Ok(thread) = self.position(&catalog_key(id, &[]))
                    && self.catalog[thread].1[0] == FILE_THREAD
                {
                    self.catalog.remove(thread);
                }
            }
        }
        self.touched.push(parent);
        Ok(())
    }

    fn rename_entry(&mut self, folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        if entry.extra & 0xFF == RESOURCE_FORK {
            return Err("A resource fork is named after its file: rename the file, and its resource fork goes with it.".into());
        }
        let parent = self.folder_id(folder)?;
        let index = self.record_of(parent, entry)?;
        let mac = mac_name(name, 31)?;
        let key = catalog_key(parent, &mac);
        if self.position(&key).is_ok_and(|other| other != index) {
            return Err(already(&shown(&mac)));
        }
        let (_, data) = self.catalog.remove(index);
        let id = be32(&data, if data[0] == FOLDER { 6 } else { 20 });
        self.insert(key, data);
        // A folder's thread (and a file's, where it has one) carries its name too.
        if let Ok(at) = self.position(&catalog_key(id, &[])) {
            let kind = self.catalog[at].1[0];
            self.catalog[at].1 = thread(kind, parent, &mac);
        }
        self.touched.push(parent);
        Ok(())
    }

    fn new_folder(&mut self, folder: Option<&Entry>, name: &str) -> Result<(), String> {
        let parent = self.folder_id(folder)?;
        let mac = mac_name(name, 31)?;
        let key = catalog_key(parent, &mac);
        if self.position(&key).is_ok() {
            return Err(already(&shown(&mac)));
        }
        let id = self.new_id()?;
        let date = now();
        let mut data = vec![0u8; 70];
        data[0] = FOLDER;
        put32(&mut data, 6, id);
        put32(&mut data, 10, date);
        put32(&mut data, 14, date);
        self.insert(key, data);
        self.insert(catalog_key(id, &[]), thread(FOLDER_THREAD, parent, &mac));
        self.touched.push(parent);
        Ok(())
    }
}

impl ImageWriter for Hfs {
    fn kinds(&self) -> Vec<Kind> {
        KINDS.iter().map(|(name, about, _, _)| Kind::plain(name, about)).collect()
    }

    fn guess(&self, name: &str, bytes: &[u8]) -> Addition {
        let (file, _) = fork_of(name);
        Addition { name: name.to_string(), bytes: bytes.to_vec(), kind: kind_for(file).into(), ..Default::default() }
    }

    fn add(&mut self, folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        self.atomically(|hfs| hfs.add_file(folder, file))
    }

    fn delete(&mut self, folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        self.atomically(|hfs| hfs.delete_entry(folder, entry))
    }

    fn rename(&mut self, folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        self.atomically(|hfs| hfs.rename_entry(folder, entry, name))
    }

    fn make_folder(&mut self, folder: Option<&Entry>, name: &str) -> Result<(), String> {
        self.atomically(|hfs| hfs.new_folder(folder, name))
    }

    fn has_folders(&self) -> bool {
        true
    }

    fn free(&self) -> Option<u64> {
        Some((self.usable_free() * self.block_size) as u64)
    }

    fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{entry, names, reopened, tool};
    use super::*;
    use crate::media::mac::tests::{disk_copy, partitioned, small_resource_fork};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn add(hfs: &mut Hfs, folder: Option<&Entry>, name: &str, bytes: &[u8]) {
        let guessed = hfs.guess(name, bytes);
        hfs.add(folder, &guessed).unwrap_or_else(|e| panic!("{name}: {e}"));
    }

    /// Apple's own checker, fsck_hfs, as Linux has it: whether it finds the volume sound, and what it said.
    fn fsck(volume: &[u8], dir: &Path) -> Option<(bool, String)> {
        let fsck = tool("fsck.hfs").or_else(|| Some(PathBuf::from("/usr/sbin/fsck.hfs")).filter(|p| p.is_file()))?;
        let path = dir.join("check.hfs");
        std::fs::write(&path, volume).unwrap();
        let out = Command::new(fsck).args(["-n", "-f"]).arg(&path).output().unwrap();
        let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        Some((out.status.success() && said.contains("appears to be OK"), said))
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-hfs-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn assert_sound(volume: &[u8], dir: &Path) {
        if let Some((ok, said)) = fsck(volume, dir) {
            assert!(ok, "{said}");
        }
    }

    #[test]
    fn names_sort_as_the_mac_sorts_them() {
        let roman = roman();
        assert!(roman.rank.iter().all(|r| *r != u16::MAX), "every character has its place");
        let order = |a: &str, b: &str| compare_names(&mac_name(a, 31).unwrap(), &mac_name(b, 31).unwrap());
        assert_eq!(order("apple", "Apple"), Ordering::Equal);
        assert_eq!(order("Apple", "apples"), Ordering::Less);
        assert_eq!(order("\u{C9}t\u{E9}", "Ezra"), Ordering::Greater, "accented letters after their plain ones");
        assert_eq!(order("\u{C9}t\u{E9}", "F"), Ordering::Less);
        assert_eq!(order("Zebra", "_x"), Ordering::Less);
        assert!(mac_name("a:b", 31).is_err() && mac_name("\u{4E00}", 31).is_err() && mac_name(&"x".repeat(32), 31).is_err());
    }

    #[test]
    fn files_folders_and_forks_go_on_an_hfs_floppy() {
        let dir = scratch("floppy");
        let mut hfs = Hfs::open(blank(800, "Work")).unwrap();
        let free = hfs.free_blocks();
        add(&mut hfs, None, "Read Me.txt", b"Hello from Limoniter\r");
        add(&mut hfs, None, "Read Me.txt.rsrc", &small_resource_fork());
        add(&mut hfs, None, "Only a fork.rsrc", &small_resource_fork());
        hfs.make_folder(None, "Stuff").unwrap();
        let volume = reopened(&hfs, "w.dsk");
        assert!(volume.describe().contains("HFS \u{B7} Work"), "{}", volume.describe());
        assert_eq!(names(&*volume, None), ["Stuff", "Only a fork", "Only a fork.rsrc", "Read Me.txt", "Read Me.txt.rsrc"]);
        let readme = entry(&*volume, None, "Read Me.txt");
        assert!(readme.detail.starts_with("TEXT \u{B7} ttxt"), "{}", readme.detail);
        assert_eq!(volume.read(&readme).unwrap(), b"Hello from Limoniter\r");
        assert_eq!(volume.read(&entry(&*volume, None, "Read Me.txt.rsrc")).unwrap(), small_resource_fork());
        assert_eq!(volume.read(&entry(&*volume, None, "Only a fork")).unwrap(), b"");
        assert!(hfs.add(None, &hfs.guess("read me.TXT", b"again")).unwrap_err().starts_with(super::super::ALREADY), "names are alike whatever their case");
        assert_sound(hfs.bytes(), &dir);
        // Enough files in a folder to split the catalog's leaves several times and give it index nodes.
        let stuff = entry(&*volume, None, "Stuff");
        for n in 0..150 {
            add(&mut hfs, Some(&stuff), &format!("File {n:03}"), &vec![n as u8; 700 + n * 3]);
        }
        hfs.make_folder(Some(&stuff), "Inner").unwrap();
        let volume = reopened(&hfs, "w.dsk");
        let stuff = entry(&*volume, None, "Stuff");
        let listed = names(&*volume, Some(&stuff));
        assert_eq!((listed.len(), listed[0].as_str(), listed[150].as_str()), (151, "Inner", "File 149"));
        assert_eq!(volume.read(&entry(&*volume, Some(&stuff), "File 077")).unwrap(), vec![77u8; 700 + 77 * 3]);
        assert!(stuff.detail.starts_with("151 item(s)"), "{}", stuff.detail);
        assert_sound(hfs.bytes(), &dir);
        // Every other file off, then one that has to go in many pieces, past the three a catalog record holds.
        for n in (0..150).step_by(2) {
            hfs.delete(Some(&stuff), &entry(&*volume, Some(&stuff), &format!("File {n:03}"))).unwrap();
        }
        let left = hfs.usable_free();
        let big: Vec<u8> = (0..left * 512 - 300).map(|i| (i % 253) as u8).collect();
        add(&mut hfs, None, "Big", &big);
        assert_eq!(hfs.usable_free(), 0);
        assert!(!hfs.extents.is_empty(), "the extents tree holds the rest");
        let volume = reopened(&hfs, "w.dsk");
        assert_eq!(volume.read(&entry(&*volume, None, "Big")).unwrap(), big);
        assert!(hfs.add(None, &hfs.guess("more", b"x")).unwrap_err().contains("full"));
        assert_sound(hfs.bytes(), &dir);
        // Renaming, taking a resource fork off, and taking everything off again.
        hfs.rename(None, &entry(&*volume, None, "Read Me.txt"), "\u{C9}t\u{E9} notes").unwrap();
        hfs.rename(None, &entry(&*volume, None, "Stuff"), "Things").unwrap();
        let volume = reopened(&hfs, "w.dsk");
        hfs.delete(None, &entry(&*volume, None, "\u{C9}t\u{E9} notes.rsrc")).unwrap();
        let volume = reopened(&hfs, "w.dsk");
        assert_eq!(names(&*volume, None), ["Things", "Big", "Only a fork", "Only a fork.rsrc", "\u{C9}t\u{E9} notes"]);
        let things = entry(&*volume, None, "Things");
        assert!(hfs.delete(None, &things).is_err(), "a folder with files in it stays");
        assert_sound(hfs.bytes(), &dir);
        for name in names(&*volume, Some(&things)) {
            hfs.delete(Some(&things), &entry(&*volume, Some(&things), &name)).unwrap();
        }
        for name in ["Things", "Big", "\u{C9}t\u{E9} notes", "Only a fork"] {
            hfs.delete(None, &entry(&*volume, None, name)).unwrap();
        }
        assert!(reopened(&hfs, "w.dsk").list(None).unwrap().is_empty());
        let grown = (hfs.mdb32(Tree::Catalog.size_at()) as usize - 12 * 512) / 512;
        assert!(grown > 0 && hfs.catalog.len() == 2, "the catalog grew, and has only the top folder left");
        assert_eq!(hfs.free_blocks() + grown, free, "every block back but those the catalog keeps");
        assert_sound(hfs.bytes(), &dir);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn every_blank_floppy_is_sound() {
        let dir = scratch("blank");
        for kilobytes in [400, 800, 1440] {
            let bytes = blank(kilobytes, "Blank");
            assert_eq!(bytes.len(), kilobytes * 1024);
            assert_sound(&bytes, &dir);
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn disk_copy_images_and_partitioned_disks_are_changed_in_place() {
        let dir = scratch("wrapped");
        let image = disk_copy("Work", &blank(800, "Work"), false);
        let mut hfs = Hfs::open(image).unwrap();
        add(&mut hfs, None, "Note.txt", b"In a Disk Copy image\r");
        let volume = reopened(&hfs, "work.image");
        assert!(volume.notes().is_empty(), "the checksum is worked out again: {:?}", volume.notes());
        assert_eq!(volume.read(&entry(&*volume, None, "Note.txt")).unwrap(), b"In a Disk Copy image\r");
        assert_sound(&hfs.bytes()[84..], &dir);
        let disk = partitioned(&[&blank(1440, "Part")]);
        let mut hfs = Hfs::open(disk).unwrap();
        add(&mut hfs, None, "Note.txt", b"In a partition\r");
        let volume = reopened(&hfs, "disk.toast");
        assert_eq!(volume.read(&entry(&*volume, None, "Note.txt")).unwrap(), b"In a partition\r");
        assert_sound(&hfs.bytes()[64 * 512..], &dir);
        assert!(Hfs::open(partitioned(&[&blank(800, "A"), &blank(800, "B")])).is_err(), "only one partition");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn locked_damaged_and_other_mac_disks_are_refused() {
        let mut locked = blank(800, "L");
        locked[1024 + 10] |= 0x80;
        assert!(Hfs::open(locked).err().unwrap().contains("locked"));
        let short = blank(800, "S")[..600 * 1024].to_vec();
        assert!(Hfs::open(short).err().unwrap().contains("doesn't make sense"), "a volume longer than its image");
        assert!(Hfs::open(crate::media::mac::tests::mfs_disk()).err().unwrap().contains("MFS"));
        let mut hfs = Hfs::open(blank(800, "X")).unwrap();
        add(&mut hfs, None, "A file", &[1; 3000]);
        hfs.make_folder(None, "F").unwrap();
        let good = hfs.bytes().to_vec();
        for seed in 0..200u32 {
            let mut bytes = good.clone();
            let mut state = seed.wrapping_mul(2_654_435_761);
            for _ in 0..16 {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                bytes[1024 + state as usize % (30 * 512)] = (state >> 16) as u8;
            }
            if let Ok(mut hfs) = Hfs::open(bytes) {
                let _ = hfs.add(None, &hfs.guess("New", b"new"));
                let _ = hfs.make_folder(None, "G");
            }
        }
    }

    /// hfsutils, an independent reader, lists and copies out what's written, and its checker passes the volume.
    #[test]
    fn hfsutils_read_written_volumes() {
        let (Some(hmount), Some(hls), Some(hcopy)) = (tool("hmount"), tool("hls"), tool("hcopy")) else {
            return;
        };
        let dir = scratch("hfsutils");
        let mut hfs = Hfs::open(blank(1440, "Shared")).unwrap();
        hfs.make_folder(None, "Docs").unwrap();
        let volume = reopened(&hfs, "s.dsk");
        let docs = entry(&*volume, None, "Docs");
        let big: Vec<u8> = (0..400_000u32).map(|i| (i * 7 / 3) as u8).collect();
        add(&mut hfs, Some(&docs), "Letter.txt", b"Dear Mac,\r");
        add(&mut hfs, Some(&docs), "Letter.txt.rsrc", &small_resource_fork());
        add(&mut hfs, None, "Big.jpg", &big);
        let path = dir.join("s.dsk");
        std::fs::write(&path, hfs.bytes()).unwrap();
        let run = |tool: &Path, args: &[&str]| {
            let out = Command::new(tool).env("HOME", &dir).current_dir(&dir).args(args).output().unwrap();
            assert!(out.status.success(), "{tool:?} {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        run(&hmount, &[path.to_str().unwrap()]);
        let listed = run(&hls, &["-l", ":Docs"]);
        assert!(listed.contains("TEXT/ttxt") && listed.contains("Letter.txt"), "{listed}");
        assert!(run(&hls, &["-l"]).contains("JPEG/ogle"));
        run(&hcopy, &["-r", ":Big.jpg", "big.out"]);
        run(&hcopy, &["-m", ":Docs:Letter.txt", "letter.bin"]);
        assert_eq!(std::fs::read(dir.join("big.out")).unwrap(), big);
        // MacBinary: a 128-byte header with the forks' lengths, then each fork padded to 128 bytes.
        let macbinary = std::fs::read(dir.join("letter.bin")).unwrap();
        assert_eq!(&macbinary[128..138], b"Dear Mac,\r");
        assert_eq!(&macbinary[256..256 + small_resource_fork().len()], &small_resource_fork()[..]);
        if let Some(hfsck) = Some(PathBuf::from("/root/reftools/hfs/hfsck")).filter(|p| p.is_file()) {
            let out = Command::new(hfsck).arg("-n").arg(&path).output().unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
        }
        std::fs::remove_dir_all(dir).ok();
    }

    /// The test fixture, made with hfsutils: a file in twelve pieces (most in the extents tree) taken off, and new
    /// ones put on.
    #[test]
    fn the_hfsutils_fixture_takes_changes() {
        let mut hfs = Hfs::open(crate::media::tests::fixture("hfs.img")).unwrap();
        let volume = reopened(&hfs, "f.hfs");
        let free = hfs.free_blocks();
        hfs.delete(None, &entry(&*volume, None, "Big")).unwrap();
        assert!(hfs.free_blocks() > free && hfs.extents.is_empty(), "its pieces and their extents gone");
        add(&mut hfs, None, "New", b"new\r");
        let docs = entry(&*volume, None, "Docs");
        add(&mut hfs, Some(&docs), "Another", &[9; 5000]);
        let volume = reopened(&hfs, "f.hfs");
        let docs = entry(&*volume, None, "Docs");
        assert_eq!(names(&*volume, Some(&docs)), ["Another", "Read Me", "Read Me.rsrc"]);
        assert_eq!(volume.read(&entry(&*volume, Some(&docs), "Read Me")).unwrap(), b"Hello from HFS\r");
        assert_eq!(volume.read(&entry(&*volume, None, "F39")).unwrap(), vec![39u8; 512]);
    }

    /// Real Mac disks in ..\testmedia, when it's there (System 6 floppies and two HFS CD-ROMs): each one Apple's
    /// checker passes still passes once a file has been put on and another taken off.
    #[test]
    fn real_mac_disks_stay_sound() {
        let folders = ["all-mac-os-system-6-disk-images", "lpo94_hfs", "lausd-mac-cti"];
        let files: Vec<PathBuf> = folders.iter().filter_map(|f| std::fs::read_dir(Path::new("../testmedia").join(f)).ok()).flat_map(|d| d.flatten().map(|f| f.path())).collect();
        let dir = scratch("real");
        let mut checked = Vec::new();
        for path in files {
            let bytes = std::fs::read(&path).unwrap();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let mut hfs = match Hfs::open(bytes) {
                Ok(hfs) => hfs,
                Err(problem) => {
                    eprintln!("{name}: {problem}");
                    continue;
                }
            };
            let volume_of = |hfs: &Hfs| hfs.bytes()[hfs.start..hfs.start + hfs.length].to_vec();
            if !fsck(&volume_of(&hfs), &dir).is_some_and(|(ok, _)| ok) {
                continue;
            }
            let volume = reopened(&hfs, &name);
            let first = volume.list(None).unwrap().into_iter().find(|e| !e.is_dir && e.extra == 0).unwrap();
            hfs.add(None, &hfs.guess("Limoniter note.txt", b"Added\r")).unwrap();
            hfs.delete(None, &first).unwrap();
            let volume = reopened(&hfs, &name);
            assert!(volume.notes().is_empty(), "{name}: {:?}", volume.notes());
            assert_eq!(volume.read(&entry(&*volume, None, "Limoniter note.txt")).unwrap(), b"Added\r");
            assert_sound(&volume_of(&hfs), &dir);
            checked.push(name);
        }
        eprintln!("checked {checked:?}");
        std::fs::remove_dir_all(dir).ok();
    }
}
