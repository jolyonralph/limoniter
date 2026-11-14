//! HFS+ (Mac OS Extended) and HFSX, its case-sensitive form: the Mac's file system from Mac OS 8.1 until APFS took
//! over in macOS 10.13 and 10.14. The layout is Apple's Technical Note TN1150, "HFS Plus Volume Format". A volume
//! may stand alone, sit in an Apple partition map or a GUID partition table, or be wrapped inside an ordinary HFS
//! volume (the "HFS wrapper"), so that Macs whose ROMs only knew HFS could still start up from it.
//!
//! Everything is kept in B-trees stored as files: the catalog (every file and folder, keyed by its parent folder's
//! number and its name), the extents overflow file (where a fragmented file's pieces past its first eight are), and
//! the attributes file (extended attributes, among them HFS+ compression's). A volume can be many gigabytes with a
//! catalog of tens of megabytes, so nothing is read whole: a folder is found by walking down the catalog's index
//! nodes to its first leaf, and its entries are read from there.
//!
//! As on HFS, a file's resource fork is listed beside it as `name.rsrc`. Hard links, symbolic links and files
//! compressed by HFS+ compression (10.6 onwards) are read as the files they stand for.

use super::mac::{self, MAC_TEXT_PRESET, RESOURCE_FORK};
use super::{Entry, InfoSection, MOST_FILE_BYTES, Slice, Source, Volume, tidy_name};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use unicode_normalization::UnicodeNormalization;

fn be(bytes: &[u8]) -> u64 {
    mac::be(bytes)
}

fn le32(bytes: &[u8]) -> u64 {
    u64::from(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// The folder number of a volume's root folder.
const ROOT: u32 = 2;

/// The catalog numbers of the B-tree files themselves, as the extents overflow file knows them.
const CATALOG_FILE: u32 = 4;
const ATTRIBUTES_FILE: u32 = 8;

/// The hidden folders that hold hard links' real files and folders, at the top of the volume.
const PRIVATE_FILES: &str = "\0\0\0\0HFS+ Private Data";
const PRIVATE_FOLDERS: &str = ".HFS+ Private Directory Data\r";

/// The most steps a walk through a B-tree may take, so a damaged tree can't keep it going for ever.
const MOST_NODES: usize = 1 << 20;

/// A fork as the volume header and catalog give it: its length in bytes and its extents, each (first allocation
/// block, block count). The first eight are in the record; any more are in the extents overflow file.
#[derive(Clone, Debug, Default)]
struct Fork {
    length: u64,
    extents: Vec<(u64, u64)>,
}

/// An HFSPlusForkData: the length, the clump size and block total (not needed here), then eight extents.
fn fork_data(bytes: &[u8]) -> Fork {
    let extents = (0..8).map(|i| (be(&bytes[16 + i * 8..20 + i * 8]), be(&bytes[20 + i * 8..24 + i * 8]))).filter(|(_, count)| *count > 0).collect();
    Fork { length: be(&bytes[0..8]), extents }
}

/// Which B-tree a node belongs to, for the node cache.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum TreeId {
    Extents,
    Catalog,
    Attributes,
}

/// A B-tree file: its fork, and from its header node the node size, the root and whether index keys are their own
/// length or all padded to the longest.
struct Tree {
    fork: Fork,
    node_size: usize,
    root: u32,
    nodes: u32,
    max_key: usize,
    variable_index_keys: bool,
}

/// One record of a node: its key (after the key's length) and its data.
struct NodeRecord<'a> {
    key: &'a [u8],
    data: &'a [u8],
}

/// A node's kind byte, from its descriptor.
const LEAF: u8 = 0xFF;
const INDEX: u8 = 0;

/// A file or folder from the catalog.
#[derive(Clone, Debug, Default)]
struct Item {
    id: u32,
    name: String,
    is_dir: bool,
    /// A folder's number of items.
    valence: u64,
    created: u64,
    modified: u64,
    kind: [u8; 4],
    creator: [u8; 4],
    finder_flags: u16,
    /// The BSD flags and mode: the owner's flags say a file is compressed; the mode, a symbolic link.
    owner_flags: u8,
    mode: u16,
    /// A hard link's iNode number, from the BSD information's special field.
    special: u32,
    /// The catalog record's flags.
    flags: u16,
    data: Fork,
    resource: Fork,
}

/// The catalog record flag of a hard link, or a file that hard links name.
const HAS_LINK_CHAIN: u16 = 0x20;

/// The owner flag that says a file's data is compressed by HFS+ compression, as BSD's UF_COMPRESSED.
const UF_COMPRESSED: u8 = 0x20;

impl Item {
    fn is_symlink(&self) -> bool {
        self.mode & 0xF000 == 0xA000
    }
    fn is_file_link(&self) -> bool {
        &self.kind == b"hlnk" && &self.creator == b"hfs+"
    }
    /// A hard link to a folder has the Finder's type and creator for an alias to one, so it's told apart by the
    /// record's flag saying it's in a chain of links.
    fn is_folder_link(&self) -> bool {
        &self.kind == b"fdrp" && &self.creator == b"MACS" && self.special != 0 && self.flags & HAS_LINK_CHAIN != 0
    }
}

/// How a compressed file's data is kept, from its `com.apple.decmpfs` attribute.
struct Compressed {
    method: u32,
    size: u64,
    /// The bytes after the attribute's 16-byte header: the data itself for the methods that keep it there.
    inline: Vec<u8>,
}

fn method_name(method: u32) -> &'static str {
    match method {
        1 | 9 | 10 => "stored",
        3 | 4 => "zlib",
        7 | 8 => "LZVN",
        11 | 12 => "LZFSE",
        13 | 14 => "LZBITMAP",
        _ => "an unknown method",
    }
}

pub struct HfsPlus {
    source: Arc<dyn Source>,
    what: String,
    name: String,
    format: &'static str,
    block_size: u64,
    created: u64,
    files: u64,
    folders: u64,
    last_mounted: String,
    extents: Option<Tree>,
    catalog: Tree,
    attributes: Option<Tree>,
    /// The hidden folders of hard links' files and folders, by number, if the volume has them.
    private_files: Option<u32>,
    private_folders: Option<u32>,
    /// The hard links' files, by iNode number, read from the hidden folder when first needed.
    inodes: Mutex<Option<HashMap<u32, Item>>>,
    cache: Mutex<HashMap<(TreeId, u32), Arc<Vec<u8>>>>,
    notes: Vec<String>,
}

/// Opens an HFS+ or HFSX volume at the start of `source`.
pub fn open(source: Arc<dyn Source>, what: &str) -> Result<Box<dyn Volume>, String> {
    Ok(Box::new(HfsPlus::open(source, what, false)?))
}

/// Opens the HFS+ volume inside an HFS wrapper: the wrapper's master directory block says where it is, as an extent
/// of the wrapper's own allocation blocks.
pub fn open_wrapped(source: Arc<dyn Source>, what: &str) -> Result<Box<dyn Volume>, String> {
    let mdb = source.read_vec(1024, 162).map_err(|_| "The image is too short for an HFS wrapper.".to_string())?;
    let (block_size, blocks_at) = (be(&mdb[20..24]), be(&mdb[28..30]) * 512);
    let (first, count) = (be(&mdb[126..128]), be(&mdb[128..130]));
    if block_size == 0 || !block_size.is_multiple_of(512) || count == 0 {
        return Err("The HFS wrapper doesn't say sensibly where its HFS+ volume is, so the disk is damaged.".into());
    }
    let start = blocks_at + first * block_size;
    let len = count * block_size;
    if start + len > source.len() + block_size {
        return Err("The HFS+ volume inside the HFS wrapper runs past the end of the image.".into());
    }
    let inner: Arc<dyn Source> = Arc::new(Slice { inner: source.clone(), start, len: len.min(source.len().saturating_sub(start)) });
    Ok(Box::new(HfsPlus::open(inner, what, true)?))
}

impl HfsPlus {
    fn open(source: Arc<dyn Source>, what: &str, wrapped: bool) -> Result<Self, String> {
        let header = source.read_vec(1024, 512).map_err(|_| "The image is too short for an HFS+ volume.".to_string())?;
        let format = match &header[..2] {
            b"H+" if wrapped => "HFS+ in an HFS wrapper",
            b"H+" => "HFS+",
            b"HX" => "HFSX",
            _ => return Err("This isn't an HFS+ volume.".into()),
        };
        let block_size = be(&header[40..44]);
        if block_size < 512 || !block_size.is_power_of_two() || block_size > 1 << 24 {
            return Err("This HFS+ volume's allocation block size doesn't make sense, so it can't be read.".into());
        }
        let last_mounted = match &header[8..12] {
            b"10.0" => "Mac OS X".to_string(),
            b"HFSJ" => "Mac OS X, journaled".to_string(),
            b"FSK!" | b"fsck" => "fsck_hfs".to_string(),
            b"8.10" => "Mac OS 8.1 to 9.2.2".to_string(),
            other => mac::code(other),
        };
        let empty = || Tree { fork: Fork::default(), node_size: 512, root: 0, nodes: 0, max_key: 0, variable_index_keys: true };
        let mut volume = Self {
            source,
            what: what.to_string(),
            name: String::new(),
            format,
            block_size,
            created: be(&header[16..20]),
            files: be(&header[32..36]),
            folders: be(&header[36..40]),
            last_mounted,
            extents: None,
            catalog: empty(),
            attributes: None,
            private_files: None,
            private_folders: None,
            inodes: Mutex::new(None),
            cache: Mutex::new(HashMap::new()),
            notes: Vec::new(),
        };
        // The extents overflow file first, as the other B-trees' own extents past their first eight are in it. Its
        // own can't overflow: it has only the eight in the volume header.
        let extents_fork = fork_data(&header[192..272]);
        if extents_fork.length > 0 {
            volume.extents = Some(volume.tree(extents_fork)?);
        }
        let catalog_fork = volume.whole_fork(CATALOG_FILE, false, fork_data(&header[272..352]))?;
        volume.catalog = volume.tree(catalog_fork)?;
        let attributes_fork = fork_data(&header[352..432]);
        if attributes_fork.length > 0 {
            let attributes_fork = volume.whole_fork(ATTRIBUTES_FILE, false, attributes_fork)?;
            // A damaged attributes file only costs compressed files, so the rest of the volume still opens.
            match volume.tree(attributes_fork) {
                Ok(tree) => volume.attributes = Some(tree),
                Err(problem) => volume.notes.push(format!("The attributes file can't be read, so compressed files can't be either: {problem}")),
            }
        }
        // The volume's name is its root folder's, from the root's thread record.
        let mut name = None;
        volume.scan_catalog(ROOT, &mut |key, data| {
            if catalog_parent(key) == Some(ROOT) && key.get(4..6) == Some(&[0, 0]) {
                if data.len() >= 10 && be(&data[0..2]) == 3 {
                    name = Some(unicode(&data[8..]));
                }
                return false;
            }
            catalog_parent(key).is_some_and(|p| p <= ROOT)
        })?;
        volume.name = name.ok_or("This HFS+ volume's catalog has no root folder, so the disk is damaged.")?;
        for item in volume.children(ROOT)? {
            if item.is_dir && item.name == PRIVATE_FILES {
                volume.private_files = Some(item.id);
            } else if item.is_dir && item.name == PRIVATE_FOLDERS {
                volume.private_folders = Some(item.id);
            }
        }
        volume.check_journal(&header);
        Ok(volume)
    }

    /// A journaled volume put away without its journal written back holds changes that only the journal has; what's
    /// read here is the volume as it was before them.
    fn check_journal(&mut self, header: &[u8]) {
        if be(&header[4..8]) & 0x2000 == 0 {
            return;
        }
        let info_at = be(&header[12..16]) * self.block_size;
        let Ok(info) = self.source.read_vec(info_at, 52) else {
            return;
        };
        // The journal info block: flags, then (past the device signature) the journal's offset and size on the volume.
        if be(&info[0..4]) & 1 == 0 {
            return;
        }
        let Ok(journal) = self.source.read_vec(be(&info[36..44]), 24) else {
            return;
        };
        // The journal header is written in the byte order of the Mac that wrote it; its magic number says which.
        let number = |bytes: &[u8], little: bool| if little { u64::from_le_bytes(bytes.try_into().unwrap()) } else { be(bytes) };
        let little = match &journal[0..4] {
            [0x4A, 0x4E, 0x4C, 0x78] => false,
            [0x78, 0x4C, 0x4E, 0x4A] => true,
            _ => return,
        };
        if number(&journal[8..16], little) != number(&journal[16..24], little) {
            self.notes.push("The volume's journal holds changes that were never written back, so the newest of its files may be missing or out of date.".into());
        }
    }

    /// The bytes of part of a fork: its extents, in order, mapped onto the volume's allocation blocks.
    fn fork_part(&self, fork: &Fork, offset: u64, length: usize) -> Result<Vec<u8>, String> {
        if offset + length as u64 > fork.length {
            return Err("A read runs past the end of its fork, so the disk is damaged.".into());
        }
        let mut out = Vec::with_capacity(length);
        let mut skip = offset;
        for (first, count) in &fork.extents {
            if out.len() == length {
                break;
            }
            let span = count * self.block_size;
            if skip >= span {
                skip -= span;
                continue;
            }
            let want = (span - skip).min((length - out.len()) as u64);
            let at = first * self.block_size + skip;
            if at + want > self.source.len() {
                return Err("A fork runs past the end of the volume, so the disk is damaged.".into());
            }
            out.extend(self.source.read_vec(at, want as usize)?);
            skip = 0;
        }
        if out.len() < length {
            return Err("A fork's extents give it less space than its length, so the disk is damaged.".into());
        }
        Ok(out)
    }

    /// A fork with all its extents: the eight in its record, then those in the extents overflow file, each record
    /// of which starts at the fork's block where the last left off.
    fn whole_fork(&self, id: u32, resource: bool, mut fork: Fork) -> Result<Fork, String> {
        let mut covered: u64 = fork.extents.iter().map(|(_, count)| count).sum();
        let needed = fork.length.div_ceil(self.block_size);
        if covered >= needed {
            return Ok(fork);
        }
        let Some(tree) = &self.extents else {
            return Err("A fragmented file's extents should be in the extents overflow file, which the volume hasn't got.".into());
        };
        // The key: its length, the fork (0 data, &FF resource), a pad byte, the file's number and the fork's block.
        let fork_type = if resource { 0xFF } else { 0 };
        let first = covered;
        let compare = move |key: &[u8]| -> Ordering {
            if key.len() < 10 {
                return Ordering::Less;
            }
            (be(&key[2..6]) as u32, key[0], be(&key[6..10])).cmp(&(id, fork_type, first))
        };
        let start = self.find_leaf(TreeId::Extents, tree, &compare)?;
        let mut trouble = None;
        self.scan(TreeId::Extents, tree, start, &mut |key, data| {
            if key.len() < 10 || compare(key) == Ordering::Less {
                return true;
            }
            if (be(&key[2..6]) as u32, key[0]) != (id, fork_type) || be(&key[6..10]) != covered {
                return false;
            }
            if data.len() < 64 {
                trouble = Some("An extents overflow record is cut short, so the disk is damaged.");
                return false;
            }
            let more: Vec<(u64, u64)> = (0..8).map(|i| (be(&data[i * 8..i * 8 + 4]), be(&data[i * 8 + 4..i * 8 + 8]))).filter(|(_, c)| *c > 0).collect();
            if more.is_empty() {
                return false;
            }
            covered += more.iter().map(|(_, count)| count).sum::<u64>();
            fork.extents.extend(more);
            covered < needed
        })?;
        if let Some(problem) = trouble {
            return Err(problem.into());
        }
        if covered < needed {
            return Err("The extents overflow file gives a fragmented file less space than its length, so the disk is damaged.".into());
        }
        Ok(fork)
    }

    /// Reads a B-tree's header node.
    fn tree(&self, fork: Fork) -> Result<Tree, String> {
        let head = self.fork_part(&fork, 0, 512.min(fork.length as usize)).map_err(|_| "A B-tree's header node can't be read, so the disk is damaged.".to_string())?;
        if head.len() < 106 || head[8] != 1 {
            return Err("A B-tree's first node isn't its header, so the disk is damaged.".into());
        }
        let node_size = be(&head[32..34]) as usize;
        if !(512..=32_768).contains(&node_size) || !node_size.is_power_of_two() {
            return Err("A B-tree's node size doesn't make sense, so the disk is damaged.".into());
        }
        let nodes = (fork.length / node_size as u64).min(u64::from(u32::MAX)) as u32;
        Ok(Tree { root: be(&head[16..20]) as u32, max_key: be(&head[34..36]) as usize, variable_index_keys: be(&head[52..56]) & 4 != 0, node_size, nodes, fork })
    }

    /// A node of a B-tree, kept for a while, as listing a folder goes back to the same few.
    fn node(&self, which: TreeId, tree: &Tree, number: u32) -> Result<Arc<Vec<u8>>, String> {
        if number >= tree.nodes {
            return Err("A B-tree points past its end, so the disk is damaged.".into());
        }
        if let Some(node) = self.cache.lock().ok().and_then(|cache| cache.get(&(which, number)).cloned()) {
            return Ok(node);
        }
        let node = Arc::new(self.fork_part(&tree.fork, u64::from(number) * tree.node_size as u64, tree.node_size)?);
        if let Ok(mut cache) = self.cache.lock() {
            if cache.len() > 1024 {
                cache.clear();
            }
            cache.insert((which, number), node.clone());
        }
        Ok(node)
    }

    /// Goes down a B-tree from its root to the leaf where a key would be: at each index node, the last record whose
    /// key isn't past it. `compare` says how a key stands against the one sought.
    fn find_leaf(&self, which: TreeId, tree: &Tree, compare: &dyn Fn(&[u8]) -> Ordering) -> Result<u32, String> {
        let mut number = tree.root;
        for _ in 0..32 {
            let node = self.node(which, tree, number)?;
            match node[8] {
                LEAF => return Ok(number),
                INDEX => {}
                _ => return Err("A B-tree's index leads to a node that isn't one, so the disk is damaged.".into()),
            }
            let records = records(&node, tree, false)?;
            let chosen = records.iter().take_while(|r| compare(r.key) != Ordering::Greater).last().or(records.first()).ok_or("A B-tree index node is empty, so the disk is damaged.")?;
            number = chosen.data.get(..4).map(be).ok_or("A B-tree index record is cut short, so the disk is damaged.")? as u32;
        }
        Err("A B-tree is deeper than any could be, so the disk is damaged.".into())
    }

    /// Visits the records of a B-tree's leaves from one leaf on, following the chain of leaves, until `visit` says to
    /// stop by giving false.
    fn scan(&self, which: TreeId, tree: &Tree, start: u32, visit: &mut dyn FnMut(&[u8], &[u8]) -> bool) -> Result<(), String> {
        let mut number = start;
        let mut seen = HashSet::new();
        while number != 0 {
            if !seen.insert(number) || seen.len() > MOST_NODES {
                return Err("A B-tree's leaves loop back on themselves, so the disk is damaged.".into());
            }
            let node = self.node(which, tree, number)?;
            if node[8] != LEAF {
                return Err("A B-tree's chain of leaves leads to a node that isn't one, so the disk is damaged.".into());
            }
            for record in records(&node, tree, true)? {
                if !visit(record.key, record.data) {
                    return Ok(());
                }
            }
            number = be(&node[0..4]) as u32;
        }
        Ok(())
    }

    /// Visits the catalog's records from where those of a folder's children start.
    fn scan_catalog(&self, parent: u32, visit: &mut dyn FnMut(&[u8], &[u8]) -> bool) -> Result<(), String> {
        let start = self.find_leaf(TreeId::Catalog, &self.catalog, &|key| folder_order(key, parent))?;
        self.scan(TreeId::Catalog, &self.catalog, start, visit)
    }

    /// A folder's files and folders, in the catalog's order.
    fn children(&self, parent: u32) -> Result<Vec<Item>, String> {
        Ok(self.children_at(parent)?.into_iter().map(|(_, item)| item).collect())
    }

    /// A folder's files and folders, each with where its record is: its leaf node and its place in the node.
    fn children_at(&self, parent: u32) -> Result<Vec<(u64, Item)>, String> {
        let start = self.find_leaf(TreeId::Catalog, &self.catalog, &|key| folder_order(key, parent))?;
        let mut out = Vec::new();
        let mut number = start;
        let mut seen = HashSet::new();
        'leaves: while number != 0 {
            if !seen.insert(number) || seen.len() > MOST_NODES {
                return Err("The catalog's leaves loop back on themselves, so the disk is damaged.".into());
            }
            let node = self.node(TreeId::Catalog, &self.catalog, number)?;
            if node[8] != LEAF {
                return Err("The catalog's chain of leaves leads to a node that isn't one, so the disk is damaged.".into());
            }
            for (index, record) in records(&node, &self.catalog, true)?.into_iter().enumerate() {
                match catalog_parent(record.key).map(|p| p.cmp(&parent)) {
                    Some(Ordering::Less) | None => continue,
                    Some(Ordering::Greater) => break 'leaves,
                    Some(Ordering::Equal) => {}
                }
                if let Some(item) = catalog_item(record.key, record.data) {
                    out.push(((u64::from(number) << 16) | index as u64, item));
                }
            }
            number = be(&node[0..4]) as u32;
        }
        Ok(out)
    }

    /// The file or folder whose record is at a place `children_at` gave.
    fn item_at(&self, place: u64) -> Result<Item, String> {
        let node = self.node(TreeId::Catalog, &self.catalog, (place >> 16) as u32)?;
        let records = records(&node, &self.catalog, true)?;
        let record = records.get((place & 0xFFFF) as usize).ok_or("That file isn't on the disk.")?;
        catalog_item(record.key, record.data).ok_or_else(|| "That file isn't on the disk.".into())
    }

    /// The file a hard link stands for: `iNode` and its number, in the hidden folder of hard links' files.
    fn link_target(&self, link: &Item) -> Result<Item, String> {
        let mut inodes = self.inodes.lock().map_err(|_| "The disk couldn't be read.".to_string())?;
        if inodes.is_none() {
            let mut map = HashMap::new();
            if let Some(folder) = self.private_files {
                for item in self.children(folder)? {
                    if let Some(number) = item.name.strip_prefix("iNode").and_then(|n| n.parse::<u32>().ok()) {
                        map.insert(number, item);
                    }
                }
            }
            *inodes = Some(map);
        }
        inodes.as_ref().and_then(|map| map.get(&link.special)).cloned().ok_or_else(|| "This hard link's file isn't in the volume's hidden folder of linked files, so the disk is damaged.".into())
    }

    /// A file's extended attribute, by name: its bytes, whether kept in the attributes file or in a fork of its own.
    fn attribute(&self, id: u32, name: &str) -> Result<Option<Vec<u8>>, String> {
        let Some(tree) = &self.attributes else {
            return Ok(None);
        };
        // The key: its length, a pad, the file's number, the block an extents record starts at, and the name.
        let wanted: Vec<u16> = name.encode_utf16().collect();
        let compare = |key: &[u8]| -> Ordering {
            if key.len() < 12 {
                return Ordering::Less;
            }
            (be(&key[2..6]) as u32).cmp(&id).then_with(|| attribute_name(key).cmp(&wanted)).then_with(|| be(&key[6..10]).cmp(&0))
        };
        let start = self.find_leaf(TreeId::Attributes, tree, &compare)?;
        let mut found: Option<Result<Vec<u8>, String>> = None;
        let mut fork: Option<Fork> = None;
        self.scan(TreeId::Attributes, tree, start, &mut |key, data| {
            if key.len() < 12 || be(&key[2..6]) as u32 != id || attribute_name(key) != wanted {
                return key.len() < 12 || compare(key) == Ordering::Less;
            }
            match data.get(..4).map(be) {
                // Inline data: the record type, eight reserved bytes, the length, and the data.
                Some(0x10) => {
                    let length = data.get(12..16).map_or(0, be) as usize;
                    found = Some(data.get(16..16 + length).map(<[u8]>::to_vec).ok_or_else(|| "A file's attribute is cut short, so the disk is damaged.".to_string()));
                    false
                }
                // Too big to be inline: a fork of the attribute's own, its extents past eight in records of type &30.
                Some(0x20) if data.len() >= 88 => {
                    fork = Some(fork_data(&data[8..88]));
                    true
                }
                Some(0x30) if data.len() >= 72 => {
                    if let Some(fork) = &mut fork {
                        fork.extents.extend((0..8).map(|i| (be(&data[8 + i * 8..12 + i * 8]), be(&data[12 + i * 8..16 + i * 8]))).filter(|(_, c)| *c > 0));
                    }
                    true
                }
                _ => false,
            }
        })?;
        if let Some(fork) = fork {
            if fork.length > MOST_FILE_BYTES {
                return Err("A file's attribute is larger than the 1 GB that can be opened.".into());
            }
            return self.fork_part(&fork, 0, fork.length as usize).map(Some);
        }
        found.transpose()
    }

    /// How a file is compressed, if it is.
    fn compression(&self, item: &Item) -> Result<Option<Compressed>, String> {
        if item.owner_flags & UF_COMPRESSED == 0 || item.is_dir {
            return Ok(None);
        }
        let Some(attribute) = self.attribute(item.id, "com.apple.decmpfs")? else {
            return Err("This file is marked as compressed but has no compression attribute, so the disk is damaged.".into());
        };
        // The header, little-endian: the magic 'fpmc', the method, and the file's length uncompressed.
        if attribute.len() < 16 || &attribute[0..4] != b"fpmc" {
            return Err("This file's compression attribute isn't one HFS+ compression writes, so the disk is damaged.".into());
        }
        let method = le32(&attribute[4..8]) as u32;
        let size = u64::from_le_bytes(attribute[8..16].try_into().unwrap());
        Ok(Some(Compressed { method, size, inline: attribute[16..].to_vec() }))
    }

    /// A fork's bytes, whole.
    fn fork_bytes(&self, id: u32, resource: bool, fork: &Fork) -> Result<Vec<u8>, String> {
        if fork.length > MOST_FILE_BYTES {
            return Err("This file is larger than the 1 GB that can be opened.".into());
        }
        let fork = self.whole_fork(id, resource, fork.clone())?;
        self.fork_part(&fork, 0, fork.length as usize)
    }

    /// A compressed file's data, uncompressed: from the attribute itself for the smaller files, or from the
    /// resource fork, in 64K blocks, for the larger.
    fn uncompress(&self, item: &Item, compressed: &Compressed) -> Result<Vec<u8>, String> {
        if compressed.size > MOST_FILE_BYTES {
            return Err("This file is larger than the 1 GB that can be opened.".into());
        }
        let size = compressed.size as usize;
        let inline = &compressed.inline;
        let out = match compressed.method {
            1 => inline.clone(),
            // Stored in the attribute, after a mark byte of &CC.
            9 if inline.first() == Some(&0xCC) => inline[1..].to_vec(),
            // zlib in the attribute; a first byte with &F in its low half says the rest is stored as it is.
            3 if inline.first().is_some_and(|b| b & 0x0F == 0x0F) => inline[1..].to_vec(),
            3 => inflate(inline, size)?,
            7 if inline.first() == Some(&0x06) => inline[1..].to_vec(),
            7 => unlzvn(inline, size)?,
            11 if inline.starts_with(b"bvx") => unlzfse(inline, size)?,
            11 if inline.first().is_some_and(|b| *b == 0xFF || *b == 0x06) => inline[1..].to_vec(),
            4 | 8 | 12 => {
                let fork = self.fork_bytes(item.id, true, &item.resource)?;
                let blocks = if compressed.method == 4 { zlib_blocks(&fork)? } else { offset_blocks(&fork)? };
                let mut out = Vec::with_capacity(size);
                for (start, end) in blocks {
                    let block = fork.get(start..end).ok_or("A compressed file's block lies outside its resource fork, so the disk is damaged.")?;
                    let want = (size - out.len().min(size)).min(0x10000);
                    let raw = match compressed.method {
                        4 if block.first().is_some_and(|b| b & 0x0F == 0x0F) => block[1..].to_vec(),
                        4 => inflate(block, want)?,
                        _ if block.first() == Some(&0x06) => block[1..].to_vec(),
                        8 => unlzvn(block, want)?,
                        _ if block.starts_with(b"bvx") => unlzfse(block, want)?,
                        _ => block.get(1..).unwrap_or_default().to_vec(),
                    };
                    out.extend(raw);
                    if out.len() > size {
                        break;
                    }
                }
                out
            }
            other => return Err(format!("This file is compressed by HFS+ compression using {} (method {other}), which Limoniter can't read yet.", method_name(other))),
        };
        if out.len() != size {
            return Err("This compressed file doesn't come out at the length it says, so the disk is damaged.".into());
        }
        Ok(out)
    }

    /// The entries for a file: its data, and its resource fork beside it if it has one.
    fn file_entries(&self, place: u64, item: &Item) -> Vec<Entry> {
        let name = tidy_name(&item.name);
        let mut about = Vec::new();
        // A hard link's file has its own type, dates and length; the link is only a name for it.
        let (shown, link_trouble) = if item.is_file_link() {
            match self.link_target(item) {
                Ok(target) => (target, None),
                Err(problem) => (item.clone(), Some(problem)),
            }
        } else {
            (item.clone(), None)
        };
        let (mut kind, ending) = if shown.kind == [0; 4] { ("File", None) } else { mac::file_kind(&shown.kind) };
        if shown.kind != [0; 4] || shown.creator != [0; 4] {
            about.push(format!("{} \u{B7} {}", mac::code(&shown.kind), mac::code(&shown.creator)));
        }
        let date = mac::mac_date(if shown.modified != 0 { shown.modified } else { shown.created });
        if !date.is_empty() {
            about.push(date);
        }
        let mut size = shown.data.length;
        let mut compressed_fork = false;
        if item.is_file_link() {
            about.push(link_trouble.unwrap_or_else(|| "hard link".into()));
        } else if item.is_symlink() {
            kind = "Symbolic link";
            let target = if item.data.length <= 1024 { self.fork_bytes(item.id, false, &item.data).ok() } else { None };
            about.push(target.map_or_else(|| "symbolic link".into(), |t| format!("\u{2192} {}", String::from_utf8_lossy(&t))));
        }
        match self.compression(&shown) {
            Ok(Some(compressed)) => {
                size = compressed.size;
                compressed_fork = matches!(compressed.method, 4 | 8 | 12);
                about.push(format!("compressed ({})", method_name(compressed.method)));
            }
            Ok(None) => {}
            Err(problem) => about.push(problem),
        }
        if shown.finder_flags & 0x4000 != 0 {
            about.push("invisible".into());
        }
        let mut data = Entry::file(name.clone(), size, place);
        data.kind = kind.into();
        data.detail = about.join(" \u{B7} ");
        if let Some(ending) = ending
            && !name.to_lowercase().ends_with(&format!(".{ending}"))
        {
            data.open_name = format!("{name}.{ending}");
        }
        let mut out = vec![data];
        // A compressed file's resource fork holds its compressed data, which is no resource fork to show.
        if shown.resource.length > 0 && !compressed_fork {
            let mut resource = Entry::file(format!("{name}.rsrc"), shown.resource.length, place);
            resource.extra = RESOURCE_FORK;
            resource.kind = "Resource fork".into();
            resource.detail = format!("of {name}");
            out.push(resource);
        }
        out
    }
}

/// How a catalog key stands against the first key of a folder's children: its thread record, keyed by the folder's
/// number and an empty name. Only numbers need comparing, as an empty name comes before any other.
fn folder_order(key: &[u8], parent: u32) -> Ordering {
    match catalog_parent(key) {
        None => Ordering::Less,
        Some(p) => p.cmp(&parent).then(if key.get(4..6) == Some(&[0, 0]) { Ordering::Equal } else { Ordering::Greater }),
    }
}

/// The parent folder's number in a catalog key.
fn catalog_parent(key: &[u8]) -> Option<u32> {
    key.get(..4).map(|b| be(b) as u32)
}

/// An attribute key's name, as UTF-16 code units.
fn attribute_name(key: &[u8]) -> Vec<u16> {
    let length = key.get(10..12).map_or(0, be) as usize;
    key.get(12..12 + length * 2).unwrap_or_default().chunks(2).map(|p| u16::from_be_bytes([p[0], p[1]])).collect()
}

/// An HFSUniStr255: a length, then that many UTF-16 code units, big-endian. Names are stored decomposed, as Unicode's
/// NFD has them; they're shown composed, as they're typed.
fn unicode(bytes: &[u8]) -> String {
    let length = bytes.get(..2).map_or(0, be) as usize;
    let units: Vec<u16> = bytes.get(2..2 + length.min(255) * 2).unwrap_or_default().chunks(2).map(|p| u16::from_be_bytes([p[0], p[1]])).collect();
    String::from_utf16_lossy(&units).nfc().collect()
}

/// A catalog leaf record as a file or folder; None for thread records and anything cut short.
fn catalog_item(key: &[u8], data: &[u8]) -> Option<Item> {
    let name = unicode(key.get(4..)?);
    let kind = be(data.get(..2)?);
    let common = |is_dir: bool| Item {
        id: be(&data[8..12]) as u32,
        name: name.clone(),
        is_dir,
        created: be(&data[12..16]),
        modified: be(&data[16..20]),
        owner_flags: data[41],
        mode: be(&data[42..44]) as u16,
        special: be(&data[44..48]) as u32,
        flags: be(&data[2..4]) as u16,
        kind: data[48..52].try_into().unwrap(),
        creator: data[52..56].try_into().unwrap(),
        finder_flags: be(&data[56..58]) as u16,
        ..Item::default()
    };
    match kind {
        1 if data.len() >= 88 => Some(Item { valence: be(&data[4..8]), kind: [0; 4], creator: [0; 4], ..common(true) }),
        2 if data.len() >= 248 => Some(Item { data: fork_data(&data[88..168]), resource: fork_data(&data[168..248]), ..common(false) }),
        _ => None,
    }
}

/// A node's records: the offsets at the node's end say where each starts, and the next where it ends. In a leaf the
/// data follows the key; in an index node it's the child's number, after a key that's either its own length or
/// padded to the tree's longest.
fn records<'a>(node: &'a [u8], tree: &Tree, leaf: bool) -> Result<Vec<NodeRecord<'a>>, String> {
    let size = node.len();
    let count = be(&node[10..12]) as usize;
    if 14 + 2 * (count + 1) > size {
        return Err("A B-tree node says it holds more records than fit in it, so the disk is damaged.".into());
    }
    let offset = |i: usize| be(&node[size - 2 * (i + 1)..size - 2 * i]) as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let (start, end) = (offset(i), offset(i + 1));
        if start < 14 || end > size - 2 * (count + 1) || start + 2 > end {
            return Err("A B-tree record lies outside its node, so the disk is damaged.".into());
        }
        let key_length = be(&node[start..start + 2]) as usize;
        let stored = if leaf || tree.variable_index_keys { key_length } else { tree.max_key };
        let data_at = start + 2 + stored;
        if start + 2 + key_length > end || data_at > end {
            return Err("A B-tree record's key is longer than the record, so the disk is damaged.".into());
        }
        out.push(NodeRecord { key: &node[start + 2..start + 2 + key_length], data: &node[data_at..end] });
    }
    Ok(out)
}

/// zlib data, inflated to the length expected.
fn inflate(bytes: &[u8], size: usize) -> Result<Vec<u8>, String> {
    miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(bytes, size.max(1)).map_err(|_| "This compressed file's zlib data is damaged.".to_string())
}

/// LZVN, the method of Apple's LZFSE library for small blocks, read by putting it in an LZFSE stream's LZVN block:
/// the mark 'bvxn', the lengths out and in, the data, and the end-of-stream mark.
fn unlzvn(bytes: &[u8], size: usize) -> Result<Vec<u8>, String> {
    let mut stream = Vec::with_capacity(bytes.len() + 16);
    stream.extend_from_slice(b"bvxn");
    stream.extend_from_slice(&(size as u32).to_le_bytes());
    stream.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    stream.extend_from_slice(bytes);
    stream.extend_from_slice(b"bvx$");
    unlzfse(&stream, size).map_err(|_| "This compressed file's LZVN data is damaged.".to_string())
}

/// An LZFSE stream, expanded.
/// An LZFSE stream, expanded, though never past one byte more than the length expected, so that damaged data
/// claiming to expand to gigabytes stops there.
pub(super) fn unlzfse(bytes: &[u8], size: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut out = Vec::with_capacity(size);
    let mut decoder = lzfse_rust::LzfseRingDecoder::default();
    decoder.reader_bytes(bytes).take(size as u64 + 1).read_to_end(&mut out).map_err(|_| "This compressed file's LZFSE data is damaged.".to_string())?;
    Ok(out)
}

/// The blocks of a zlib-compressed resource fork: after the usual 256-byte resource fork header, a length, then a
/// little-endian count and table of (offset, length) pairs, the offsets from the count's own place.
fn zlib_blocks(fork: &[u8]) -> Result<Vec<(usize, usize)>, String> {
    let damaged = || "A compressed file's table of blocks is damaged.".to_string();
    let data_at = fork.get(..4).map(be).ok_or_else(damaged)? as usize;
    let table = data_at + 4;
    let count = fork.get(table..table + 4).map(le32).ok_or_else(damaged)? as usize;
    if count > fork.len() / 8 {
        return Err(damaged());
    }
    (0..count)
        .map(|i| {
            let at = table + 4 + i * 8;
            let entry = fork.get(at..at + 8).ok_or_else(damaged)?;
            let start = table + le32(&entry[0..4]) as usize;
            Ok((start, start + le32(&entry[4..8]) as usize))
        })
        .collect()
}

/// The blocks of an LZVN or LZFSE-compressed resource fork: a little-endian table of offsets from the fork's start,
/// the first of which is the table's own length, each block running to the next offset.
fn offset_blocks(fork: &[u8]) -> Result<Vec<(usize, usize)>, String> {
    let damaged = || "A compressed file's table of blocks is damaged.".to_string();
    let table = fork.get(..4).map(le32).ok_or_else(damaged)? as usize;
    if table < 8 || !table.is_multiple_of(4) || table > fork.len() {
        return Err(damaged());
    }
    let offsets: Vec<usize> = fork[..table].chunks(4).map(|c| le32(c) as usize).collect();
    Ok(offsets.windows(2).map(|w| (w[0], w[1])).collect())
}

impl Volume for HfsPlus {
    fn describe(&self) -> String {
        let made = mac::mac_date(self.created);
        format!("{} \u{B7} {} \u{B7} {}{}", self.what, self.format, tidy_name(&self.name), if made.is_empty() { String::new() } else { format!(" \u{B7} made {}", &made[..10]) })
    }

    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String> {
        let parent = dir.map_or(ROOT, |d| d.location as u32);
        let mut folders = Vec::new();
        let mut files = Vec::new();
        for (place, item) in self.children_at(parent)? {
            // The hidden folders of hard links' files and folders are the volume's workings, not its files.
            if parent == ROOT && (Some(item.id) == self.private_files || Some(item.id) == self.private_folders) {
                continue;
            }
            if item.is_dir {
                let mut entry = Entry::dir(tidy_name(&item.name), u64::from(item.id), 0);
                entry.detail = format!("{} item(s){}", item.valence, if item.modified == 0 { String::new() } else { format!(" \u{B7} {}", mac::mac_date(item.modified)) });
                folders.push(entry);
            } else if item.is_folder_link() {
                // A hard link to a folder, as Time Machine makes: the folder itself, kept in the hidden folder.
                let mut entry = Entry::dir(tidy_name(&item.name), u64::from(item.special), 0);
                entry.detail = "hard link to a folder".into();
                folders.push(entry);
            } else {
                files.push((place, item));
            }
        }
        let mut out = folders;
        for (place, item) in files {
            out.extend(self.file_entries(place, &item));
        }
        Ok(out)
    }

    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        if entry.is_dir {
            return Err("That's a folder.".into());
        }
        let mut item = self.item_at(entry.location)?;
        if item.is_dir {
            return Err("That file isn't on the disk.".into());
        }
        if item.is_file_link() {
            item = self.link_target(&item)?;
        }
        if entry.extra & 0xFF == RESOURCE_FORK {
            return self.fork_bytes(item.id, true, &item.resource);
        }
        match self.compression(&item)? {
            Some(compressed) => self.uncompress(&item, &compressed),
            None => self.fork_bytes(item.id, false, &item.data),
        }
    }

    fn notes(&self) -> Vec<String> {
        self.notes.clone()
    }

    fn text_preset(&self, entry: &Entry) -> Option<&'static str> {
        // Mac OS X's own files are UTF-8 with Unix line ends; only a classic text file is Mac Roman.
        if entry.extra & 0xFF == RESOURCE_FORK || !entry.detail.starts_with("TEXT") {
            return None;
        }
        Some(MAC_TEXT_PRESET)
    }

    fn info(&self) -> Vec<InfoSection> {
        let rows = vec![
            ("Format".to_string(), self.format.to_string()),
            ("Name".to_string(), self.name.clone()),
            ("Created".to_string(), mac::mac_date(self.created)),
            ("Files".to_string(), self.files.to_string()),
            ("Folders".to_string(), self.folders.to_string()),
            ("Allocation block".to_string(), format!("{} bytes", self.block_size)),
            ("Last mounted by".to_string(), self.last_mounted.clone()),
        ];
        vec![InfoSection { title: "Volume".into(), rows }]
    }
}

// ---------- GUID partition tables ----------

/// The partition type of an HFS+ volume in a GUID partition table, as its bytes are stored (the first three parts
/// of the GUID little-endian): 48465300-0000-11AA-AA11-00306543ECAC. APFS's, to name it.
const GPT_HFS_PLUS: [u8; 16] = [0x00, 0x53, 0x46, 0x48, 0x00, 0x00, 0xAA, 0x11, 0xAA, 0x11, 0x00, 0x30, 0x65, 0x43, 0xEC, 0xAC];
const GPT_APFS: [u8; 16] = [0xEF, 0x57, 0x34, 0x7C, 0x00, 0x00, 0xAA, 0x11, 0xAA, 0x11, 0x00, 0x30, 0x65, 0x43, 0xEC, 0xAC];

/// A GUID partition table's partitions, from the start of a disk: (name, type, first byte, length).
fn gpt_partitions(head: &[u8]) -> Option<Vec<(String, [u8; 16], u64, u64)>> {
    let header = head.get(512..512 + 92)?;
    if &header[..8] != b"EFI PART" {
        return None;
    }
    let le = |b: &[u8]| b.iter().rev().fold(0u64, |n, x| (n << 8) | u64::from(*x));
    let (at, count, size) = (le(&header[72..80]).checked_mul(512)?, le(&header[80..84]), le(&header[84..88]) as usize);
    if size < 128 || count > 1024 || at > 1 << 20 {
        return None;
    }
    let mut out = Vec::new();
    for i in 0..count as usize {
        let Some(entry) = head.get(at as usize + i * size..at as usize + i * size + 128) else {
            break;
        };
        if entry[..16] == [0; 16] {
            continue;
        }
        let units: Vec<u16> = entry[56..128].chunks(2).map(|p| u16::from_le_bytes([p[0], p[1]])).take_while(|u| *u != 0).collect();
        let (first, last) = (le(&entry[32..40]), le(&entry[40..48]));
        out.push((String::from_utf16_lossy(&units), entry[..16].try_into().unwrap(), first.saturating_mul(512), last.saturating_add(1).saturating_sub(first).saturating_mul(512)));
    }
    Some(out)
}

/// Whether a disk starts with a GUID partition table holding an HFS+ volume.
pub fn looks_like_gpt(head: &[u8]) -> bool {
    gpt_partitions(head).is_some_and(|parts| parts.iter().any(|p| p.1 == GPT_HFS_PLUS))
}

/// A disk with a GUID partition table, its HFS+ partitions opened.
pub fn open_gpt(source: Arc<dyn Source>, what: &str) -> Result<Box<dyn Volume>, String> {
    let head = source.read_vec(0, source.len().min(0x8800) as usize)?;
    let parts = gpt_partitions(&head).ok_or("The disk's GUID partition table can't be read.")?;
    let mut out = Vec::new();
    for (i, (name, kind, start, length)) in parts.into_iter().enumerate() {
        let mut part = mac::Partition { name, kind: gpt_kind(&kind), volume: None, problem: None, blocks: length, raw: None };
        if kind == GPT_HFS_PLUS {
            let inner: Arc<dyn Source> = Arc::new(Slice { inner: source.clone(), start, len: length.min(source.len().saturating_sub(start)) });
            match mac::open_partition(inner, &format!("{what}, partition {}", i + 1)) {
                Ok(volume) => part.volume = Some(volume),
                Err(problem) => part.problem = Some(problem),
            }
        } else if kind == GPT_APFS {
            part.problem = Some("APFS, which Limoniter can't read yet".into());
        }
        out.push(part);
    }
    Ok(mac::Partitioned::new(what, "GUID partition table", out).into_volume())
}

/// What a GPT partition type is called, for the ones a Mac disk has.
fn gpt_kind(kind: &[u8; 16]) -> String {
    match kind {
        k if *k == GPT_HFS_PLUS => "Apple_HFS".into(),
        k if *k == GPT_APFS => "Apple_APFS".into(),
        [0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, ..] => "EFI system partition".into(),
        [0x00, 0x6F, 0x6F, 0x42, 0x00, 0x00, 0xAA, 0x11, ..] => "Apple_Boot".into(),
        _ => "GPT partition".into(),
    }
}

#[cfg(test)]
pub mod tests {
    use super::super::{is_image, open};
    use super::*;
    use std::collections::BTreeMap;
    use std::path::Path;

    /// A B-tree file holding the records given, each a key (with its length) and data, already in the tree's
    /// order: a header node, then the leaves, then index nodes level by level up to the root.
    pub fn btree(records: &[(Vec<u8>, Vec<u8>)], node_size: usize, max_key: u16, variable: bool) -> Vec<u8> {
        let mut nodes: Vec<Vec<u8>> = vec![Vec::new()];
        // Puts records into nodes of one level, returning each node's number and first key.
        let pack = |nodes: &mut Vec<Vec<u8>>, items: Vec<(Vec<u8>, Vec<u8>)>, kind: u8, height: u8| -> Vec<(u32, Vec<u8>)> {
            let mut out: Vec<(u32, Vec<u8>)> = Vec::new();
            let mut current: Vec<Vec<u8>> = Vec::new();
            let mut first_key = Vec::new();
            let flush = |nodes: &mut Vec<Vec<u8>>, current: &mut Vec<Vec<u8>>, first_key: &[u8], out: &mut Vec<(u32, Vec<u8>)>| {
                let mut node = vec![0u8; node_size];
                node[8] = kind;
                node[9] = height;
                node[10..12].copy_from_slice(&(current.len() as u16).to_be_bytes());
                let mut at = 14;
                for (i, record) in current.iter().enumerate() {
                    node[node_size - 2 * (i + 1)..node_size - 2 * i].copy_from_slice(&(at as u16).to_be_bytes());
                    node[at..at + record.len()].copy_from_slice(record);
                    at += record.len();
                }
                let i = current.len();
                node[node_size - 2 * (i + 1)..node_size - 2 * i].copy_from_slice(&(at as u16).to_be_bytes());
                out.push((nodes.len() as u32, first_key.to_vec()));
                nodes.push(node);
                current.clear();
            };
            let mut used = 14 + 2;
            for (key, data) in items {
                let record = [key.clone(), data].concat();
                if used + record.len() + 2 > node_size {
                    flush(nodes, &mut current, &first_key, &mut out);
                    used = 14 + 2;
                }
                if current.is_empty() {
                    first_key = key;
                }
                used += record.len() + 2;
                current.push(record);
            }
            if !current.is_empty() || out.is_empty() {
                flush(nodes, &mut current, &first_key, &mut out);
            }
            // Each level's nodes are chained.
            for pair in out.windows(2) {
                let (a, b) = (pair[0].0 as usize, pair[1].0 as usize);
                nodes[a][0..4].copy_from_slice(&(b as u32).to_be_bytes());
                nodes[b][4..8].copy_from_slice(&(a as u32).to_be_bytes());
            }
            out
        };
        let leaves = pack(&mut nodes, records.to_vec(), LEAF, 1);
        let (first_leaf, last_leaf) = (leaves[0].0, leaves[leaves.len() - 1].0);
        let mut level = leaves;
        let mut depth = 1;
        while level.len() > 1 {
            depth += 1;
            let items = level
                .iter()
                .map(|(number, key)| {
                    let mut key = key.clone();
                    if !variable {
                        key.resize(2 + usize::from(max_key), 0);
                        key[0..2].copy_from_slice(&max_key.to_be_bytes());
                    }
                    (key, number.to_be_bytes().to_vec())
                })
                .collect();
            level = pack(&mut nodes, items, INDEX, depth);
        }
        let root = level[0].0;
        let mut header = vec![0u8; node_size];
        header[8] = 1;
        header[10..12].copy_from_slice(&3u16.to_be_bytes());
        header[14..16].copy_from_slice(&(depth as u16).to_be_bytes());
        header[16..20].copy_from_slice(&root.to_be_bytes());
        header[20..24].copy_from_slice(&(records.len() as u32).to_be_bytes());
        header[24..28].copy_from_slice(&first_leaf.to_be_bytes());
        header[28..32].copy_from_slice(&last_leaf.to_be_bytes());
        header[32..34].copy_from_slice(&(node_size as u16).to_be_bytes());
        header[34..36].copy_from_slice(&max_key.to_be_bytes());
        header[36..40].copy_from_slice(&(nodes.len() as u32).to_be_bytes());
        header[51] = 0xCF;
        header[52..56].copy_from_slice(&(if variable { 6u32 } else { 2 }).to_be_bytes());
        nodes[0] = header;
        nodes.concat()
    }

    fn utf16(name: &str) -> Vec<u8> {
        let units: Vec<u16> = name.encode_utf16().collect();
        let mut out = (units.len() as u16).to_be_bytes().to_vec();
        out.extend(units.iter().flat_map(|u| u.to_be_bytes()));
        out
    }

    fn catalog_key(parent: u32, name: &str) -> Vec<u8> {
        let body = [parent.to_be_bytes().to_vec(), utf16(name)].concat();
        [(body.len() as u16).to_be_bytes().to_vec(), body].concat()
    }

    /// How a test file is made: its forks, its Finder type and creator, its BSD mode, flags and special number.
    #[derive(Clone, Default)]
    pub struct FileSpec {
        pub data: Vec<u8>,
        pub resource: Vec<u8>,
        pub kind: [u8; 4],
        pub creator: [u8; 4],
        pub mode: u16,
        pub owner_flags: u8,
        pub special: u32,
        /// The catalog record's flags, besides the one saying it has a thread record.
        pub flags: u16,
        /// Into how many pieces, with a gap between each, the data fork is put.
        pub pieces: usize,
        /// A `com.apple.decmpfs` attribute to give it.
        pub decmpfs: Option<Vec<u8>>,
    }

    /// An HFS+ volume put together a file at a time, with 512-byte allocation blocks and 512-byte B-tree nodes, so
    /// that a few dozen files make catalogs several levels deep.
    pub struct Builder {
        disk: Vec<u8>,
        next_block: u32,
        next_id: u32,
        folders: Vec<(u32, u32, String)>,
        files: Vec<(u32, u32, String, FileSpec, Fork, Fork)>,
        extents: Vec<(Vec<u8>, Vec<u8>)>,
        attributes: Vec<(Vec<u8>, Vec<u8>)>,
    }

    fn fork_record(fork: &Fork) -> Vec<u8> {
        let mut out = vec![0u8; 80];
        out[0..8].copy_from_slice(&fork.length.to_be_bytes());
        out[12..16].copy_from_slice(&(fork.extents.iter().map(|e| e.1).sum::<u64>() as u32).to_be_bytes());
        for (i, (first, count)) in fork.extents.iter().take(8).enumerate() {
            out[16 + i * 8..20 + i * 8].copy_from_slice(&(*first as u32).to_be_bytes());
            out[20 + i * 8..24 + i * 8].copy_from_slice(&(*count as u32).to_be_bytes());
        }
        out
    }

    impl Builder {
        pub fn new(name: &str) -> Self {
            Self { disk: vec![0u8; 4 * 512], next_block: 4, next_id: 100, folders: vec![(1, ROOT, name.to_string())], files: Vec::new(), extents: Vec::new(), attributes: Vec::new() }
        }

        /// Puts bytes in the volume's blocks, in `pieces` runs with a block left free between each.
        fn store(&mut self, bytes: &[u8], pieces: usize) -> Fork {
            let blocks = bytes.len().div_ceil(512).max(1);
            let pieces = pieces.clamp(1, blocks);
            let mut extents = Vec::new();
            let mut done = 0;
            for piece in 0..pieces {
                let count = blocks / pieces + usize::from(piece < blocks % pieces);
                let first = self.next_block;
                self.disk.resize((first as usize + count) * 512, 0);
                let part = &bytes[(done * 512).min(bytes.len())..((done + count) * 512).min(bytes.len())];
                self.disk[first as usize * 512..first as usize * 512 + part.len()].copy_from_slice(part);
                extents.push((u64::from(first), count as u64));
                done += count;
                self.next_block = first + count as u32 + u32::from(pieces > 1);
            }
            Fork { length: if bytes.is_empty() { 0 } else { bytes.len() as u64 }, extents: if bytes.is_empty() { Vec::new() } else { extents } }
        }

        pub fn folder(&mut self, parent: u32, name: &str) -> u32 {
            self.next_id += 1;
            self.folders.push((parent, self.next_id, name.to_string()));
            self.next_id
        }

        pub fn folder_with_id(&mut self, parent: u32, id: u32, name: &str) {
            self.folders.push((parent, id, name.to_string()));
        }

        pub fn file(&mut self, parent: u32, name: &str, spec: FileSpec) -> u32 {
            self.next_id += 1;
            let id = self.next_id;
            self.file_with_id(parent, id, name, spec);
            id
        }

        pub fn file_with_id(&mut self, parent: u32, id: u32, name: &str, spec: FileSpec) {
            let data = self.store(&spec.data, spec.pieces);
            let resource = self.store(&spec.resource, 1);
            // Extents past the first eight go in the extents overflow file, eight to a record.
            let mut start: u64 = data.extents.iter().take(8).map(|e| e.1).sum();
            for more in data.extents.iter().skip(8).collect::<Vec<_>>().chunks(8) {
                let key = [&10u16.to_be_bytes()[..], &[0, 0], &id.to_be_bytes(), &(start as u32).to_be_bytes()].concat();
                let mut record = vec![0u8; 64];
                for (i, (first, count)) in more.iter().enumerate() {
                    record[i * 8..i * 8 + 4].copy_from_slice(&(*first as u32).to_be_bytes());
                    record[i * 8 + 4..i * 8 + 8].copy_from_slice(&(*count as u32).to_be_bytes());
                }
                start += more.iter().map(|e| e.1).sum::<u64>();
                self.extents.push((key, record));
            }
            if let Some(attribute) = &spec.decmpfs {
                let name = utf16("com.apple.decmpfs");
                let body = [&[0u8, 0][..], &id.to_be_bytes(), &0u32.to_be_bytes(), &name].concat();
                let key = [(body.len() as u16).to_be_bytes().to_vec(), body].concat();
                let record = [&0x10u32.to_be_bytes()[..], &[0; 8], &(attribute.len() as u32).to_be_bytes(), attribute].concat();
                self.attributes.push((key, record));
            }
            self.files.push((parent, id, name.to_string(), spec, data, resource));
        }

        /// The finished volume, its B-trees in blocks after the files, with `signature` (H+ or HX) in its header.
        pub fn finish(mut self, signature: &[u8; 2]) -> Vec<u8> {
            let mut catalog: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
            for (parent, id, name) in &self.folders {
                let valence = self.folders.iter().filter(|f| f.0 == *id).count() + self.files.iter().filter(|f| f.0 == *id).count();
                let mut record = vec![0u8; 88];
                record[0..2].copy_from_slice(&1u16.to_be_bytes());
                record[2..4].copy_from_slice(&2u16.to_be_bytes());
                record[4..8].copy_from_slice(&(valence as u32).to_be_bytes());
                record[8..12].copy_from_slice(&id.to_be_bytes());
                record[16..20].copy_from_slice(&3_000_000_000u32.to_be_bytes());
                record[42..44].copy_from_slice(&0o040755u16.to_be_bytes());
                catalog.push((catalog_key(*parent, name), record));
                let thread = [&3u16.to_be_bytes()[..], &[0, 0], &parent.to_be_bytes(), &utf16(name)].concat();
                catalog.push((catalog_key(*id, ""), thread));
            }
            for (parent, id, name, spec, data, resource) in &self.files {
                let mut record = vec![0u8; 248];
                record[0..2].copy_from_slice(&2u16.to_be_bytes());
                record[2..4].copy_from_slice(&(2 | spec.flags).to_be_bytes());
                record[8..12].copy_from_slice(&id.to_be_bytes());
                record[12..16].copy_from_slice(&2_900_000_000u32.to_be_bytes());
                record[16..20].copy_from_slice(&3_100_000_000u32.to_be_bytes());
                record[41] = spec.owner_flags;
                record[42..44].copy_from_slice(&(if spec.mode == 0 { 0o100644 } else { spec.mode }).to_be_bytes());
                record[44..48].copy_from_slice(&spec.special.to_be_bytes());
                record[48..52].copy_from_slice(&spec.kind);
                record[52..56].copy_from_slice(&spec.creator);
                record[88..168].copy_from_slice(&fork_record(data));
                record[168..248].copy_from_slice(&fork_record(resource));
                catalog.push((catalog_key(*parent, name), record));
                let thread = [&4u16.to_be_bytes()[..], &[0, 0], &parent.to_be_bytes(), &utf16(name)].concat();
                catalog.push((catalog_key(*id, ""), thread));
            }
            // The catalog's order: by parent, then by name (plain code units do here, as the names are simple).
            let order = |key: &[u8]| (be(&key[2..6]), key[8..].chunks(2).map(|p| u16::from_be_bytes([p[0], p[1]])).collect::<Vec<_>>());
            catalog.sort_by_key(|(key, _)| order(key));
            self.extents.sort_by_key(|(key, _)| (be(&key[4..8]), key[2], be(&key[8..12])));
            self.attributes.sort_by_key(|(key, _)| (be(&key[4..8]), key[12..].to_vec()));
            let catalog_tree = btree(&catalog, 512, 516, true);
            let extents_tree = btree(&self.extents, 512, 10, false);
            let attributes_tree = btree(&self.attributes, 512, 266, true);
            let catalog_fork = self.store(&catalog_tree, 1);
            let extents_fork = self.store(&extents_tree, 1);
            let attributes_fork = self.store(&attributes_tree, 1);
            let total = self.next_block + 2;
            self.disk.resize(total as usize * 512, 0);
            let header = &mut self.disk[1024..1536];
            header[0..2].copy_from_slice(signature);
            header[2..4].copy_from_slice(&(if signature == b"HX" { 5u16 } else { 4 }).to_be_bytes());
            header[8..12].copy_from_slice(b"10.0");
            header[16..20].copy_from_slice(&3_000_000_000u32.to_be_bytes());
            header[32..36].copy_from_slice(&(self.files.len() as u32).to_be_bytes());
            header[36..40].copy_from_slice(&(self.folders.len() as u32 - 1).to_be_bytes());
            header[40..44].copy_from_slice(&512u32.to_be_bytes());
            header[44..48].copy_from_slice(&total.to_be_bytes());
            header[192..272].copy_from_slice(&fork_record(&extents_fork));
            header[272..352].copy_from_slice(&fork_record(&catalog_fork));
            header[352..432].copy_from_slice(&fork_record(&attributes_fork));
            self.disk
        }
    }

    /// A raw LZVN stream of literals only, as a block that didn't compress would be, with the end-of-stream mark.
    fn lzvn_literals(bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for chunk in bytes.chunks(271) {
            if chunk.len() < 16 {
                out.push(0xE0 | chunk.len() as u8);
            } else {
                out.extend([0xE0, (chunk.len() - 16) as u8]);
            }
            out.extend_from_slice(chunk);
        }
        out.extend([0x06, 0, 0, 0, 0, 0, 0, 0]);
        out
    }

    fn decmpfs(method: u32, size: usize, inline: &[u8]) -> Vec<u8> {
        [&b"fpmc"[..], &method.to_le_bytes(), &(size as u64).to_le_bytes(), inline].concat()
    }

    /// A resource fork holding zlib-compressed 64K blocks, as HFS+ compression's method 4 keeps a larger file.
    fn zlib_fork(data: &[u8]) -> Vec<u8> {
        let blocks: Vec<Vec<u8>> = data.chunks(0x10000).map(|b| miniz_oxide::deflate::compress_to_vec_zlib(b, 6)).collect();
        let table_length = 4 + blocks.len() * 8;
        let mut table = (blocks.len() as u32).to_le_bytes().to_vec();
        let mut at = table_length;
        for block in &blocks {
            table.extend((at as u32).to_le_bytes());
            table.extend((block.len() as u32).to_le_bytes());
            at += block.len();
        }
        let body = [table, blocks.concat()].concat();
        // The resource fork's header: where its data and map are, and their lengths; the map is 50 bytes.
        let data_length = body.len() as u32 + 4;
        let mut fork = [256, 256 + data_length, data_length, 50u32].iter().flat_map(|n| n.to_be_bytes()).collect::<Vec<u8>>();
        fork.resize(256, 0);
        fork.extend((body.len() as u32).to_be_bytes());
        fork.extend(body);
        // The map, as the Finder writes it: one type, 'cmpf', with one resource, number 1, at the data's start.
        let mut map = vec![0u8; 50];
        map[24..26].copy_from_slice(&28u16.to_be_bytes());
        map[26..28].copy_from_slice(&50u16.to_be_bytes());
        map[30..34].copy_from_slice(b"cmpf");
        map[36..38].copy_from_slice(&10u16.to_be_bytes());
        map[38..40].copy_from_slice(&1u16.to_be_bytes());
        map[40..42].copy_from_slice(&0xFFFFu16.to_be_bytes());
        fork.extend(map);
        fork
    }

    /// LZVN for bytes that repeat every `period`: the first period as literals, then copies from a period back,
    /// the first giving the distance (sml_d) and the rest reusing it (lrg_m and sml_m), then the end-of-stream mark.
    fn lzvn_repeating(bytes: &[u8], period: usize) -> Vec<u8> {
        assert!(bytes.len() > period + 3 && (16..=271).contains(&period) && bytes[period..].iter().enumerate().all(|(i, b)| *b == bytes[i]));
        let mut out = vec![0xE0, (period - 16) as u8];
        out.extend_from_slice(&bytes[..period]);
        let mut left = bytes.len() - period;
        let first = left.min(10);
        out.extend([((first - 3) << 3) as u8, period as u8]);
        left -= first;
        while left > 0 {
            let run = left.min(271);
            if run >= 16 {
                out.extend([0xF0, (run - 16) as u8]);
            } else {
                out.push(0xF0 | run as u8);
            }
            left -= run;
        }
        out.extend([0x06, 0, 0, 0, 0, 0, 0, 0]);
        out
    }

    /// A resource fork of LZVN blocks: a table of offsets, then the blocks, each 64K or the file's last.
    fn lzvn_fork(data: &[u8]) -> Vec<u8> {
        offset_fork(data.chunks(0x10000).enumerate().map(|(i, b)| if i % 2 == 0 { lzvn_repeating(b, 45) } else { [&[0x06][..], b].concat() }).collect())
    }

    /// The same, of LZFSE blocks.
    fn lzfse_fork(data: &[u8]) -> Vec<u8> {
        offset_fork(
            data.chunks(0x10000)
                .map(|b| {
                    let mut out = Vec::new();
                    lzfse_rust::encode_bytes(b, &mut out).unwrap();
                    out
                })
                .collect(),
        )
    }

    fn offset_fork(blocks: Vec<Vec<u8>>) -> Vec<u8> {
        let mut at = 4 * (blocks.len() + 1);
        let mut fork = Vec::new();
        for block in &blocks {
            fork.extend((at as u32).to_le_bytes());
            at += block.len();
        }
        fork.extend((at as u32).to_le_bytes());
        fork.extend(blocks.concat());
        fork
    }

    pub fn text(i: usize) -> Vec<u8> {
        (0..i).map(|n| b"The quick brown fox jumps over the lazy dog.\n"[n % 45]).collect()
    }

    /// A volume with most of what HFS+ has: folders, both forks, a decomposed name, a file in twelve pieces, a hard
    /// link, a symbolic link, compressed files of each kind, and enough files to need an index in the catalog.
    pub fn sample(signature: &[u8; 2]) -> Vec<u8> {
        let mut volume = Builder::new("Sample");
        let docs = volume.folder(ROOT, "Docs");
        volume.file(docs, "Read Me", FileSpec { data: b"Hello from HFS+\r".to_vec(), resource: crate::media::mac::tests::small_resource_fork(), kind: *b"TEXT", creator: *b"ttxt", ..Default::default() });
        // "Café" as HFS+ keeps it, decomposed: e and a combining acute accent.
        volume.file(docs, "Cafe\u{301}.txt", FileSpec { data: b"accents\n".to_vec(), ..Default::default() });
        let big: Vec<u8> = (0..12 * 512 - 100).map(|i| (i % 251) as u8).collect();
        volume.file(ROOT, "Big", FileSpec { data: big, pieces: 12, ..Default::default() });
        // A hard link: the file itself is iNode40 in the hidden folder, the link names it by number.
        volume.folder_with_id(ROOT, 30, PRIVATE_FILES);
        volume.file_with_id(30, 31, "iNode40", FileSpec { data: b"linked file\n".to_vec(), ..Default::default() });
        volume.file(ROOT, "Linked", FileSpec { kind: *b"hlnk", creator: *b"hfs+", special: 40, ..Default::default() });
        // A hard link to a folder, as Time Machine makes, and a Finder alias to one, which isn't one.
        volume.folder_with_id(ROOT, 32, PRIVATE_FOLDERS);
        volume.folder_with_id(32, 33, "dir_33");
        volume.file(33, "Inside", FileSpec { data: b"in a linked folder".to_vec(), ..Default::default() });
        volume.file(ROOT, "Folder link", FileSpec { kind: *b"fdrp", creator: *b"MACS", special: 33, flags: HAS_LINK_CHAIN, ..Default::default() });
        volume.file(ROOT, "Folder alias", FileSpec { kind: *b"fdrp", creator: *b"MACS", special: 7, resource: b"alis".to_vec(), ..Default::default() });
        volume.file(ROOT, "Shortcut", FileSpec { data: b"Docs/Read Me".to_vec(), mode: 0o120755, ..Default::default() });
        let small = text(3000);
        volume.file(ROOT, "Zlib small", FileSpec { owner_flags: UF_COMPRESSED, decmpfs: Some(decmpfs(3, small.len(), &miniz_oxide::deflate::compress_to_vec_zlib(&small, 6))), ..Default::default() });
        volume.file(ROOT, "Stored small", FileSpec { owner_flags: UF_COMPRESSED, decmpfs: Some(decmpfs(3, 5, b"\xFFplain")), ..Default::default() });
        let large = text(150_000);
        volume.file(ROOT, "Zlib large", FileSpec { owner_flags: UF_COMPRESSED, resource: zlib_fork(&large), decmpfs: Some(decmpfs(4, large.len(), &[])), ..Default::default() });
        volume.file(ROOT, "LZVN small", FileSpec { owner_flags: UF_COMPRESSED, decmpfs: Some(decmpfs(7, 300, &lzvn_literals(&text(300)))), ..Default::default() });
        volume.file(ROOT, "LZVN large", FileSpec { owner_flags: UF_COMPRESSED, resource: lzvn_fork(&large), decmpfs: Some(decmpfs(8, large.len(), &[])), ..Default::default() });
        volume.file(ROOT, "LZFSE large", FileSpec { owner_flags: UF_COMPRESSED, resource: lzfse_fork(&large), decmpfs: Some(decmpfs(12, large.len(), &[])), ..Default::default() });
        volume.file(ROOT, "Copied", FileSpec { owner_flags: UF_COMPRESSED, decmpfs: Some(decmpfs(9, 4, b"\xCCcopy")), ..Default::default() });
        volume.file(ROOT, "LZBITMAP", FileSpec { owner_flags: UF_COMPRESSED, decmpfs: Some(decmpfs(13, 10, b"??")), ..Default::default() });
        let many = volume.folder(ROOT, "Many");
        for i in 0..60 {
            volume.file(many, &format!("File {i:02}"), FileSpec { data: vec![i as u8; 10 + i], ..Default::default() });
        }
        volume.finish(signature)
    }

    fn names(entries: &[Entry]) -> Vec<String> {
        entries.iter().map(|e| e.name.clone()).collect()
    }

    fn find<'a>(entries: &'a [Entry], name: &str) -> &'a Entry {
        entries.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("{name} in {:?}", names(entries)))
    }

    #[test]
    fn hfs_plus_volumes_list_folders_forks_links_and_fragmented_files() {
        let volume = open(Arc::new(sample(b"H+")), "sample.img").unwrap();
        assert!(volume.describe().starts_with("Macintosh disk image, 1 MB \u{B7} HFS+ \u{B7} Sample \u{B7} made 1999-"), "{}", volume.describe());
        let top = volume.list(None).unwrap();
        assert_eq!(&names(&top)[..3], ["Docs", "Folder link", "Many"], "folders first, and the hidden folders of hard links' files and folders aren't shown");
        let link = find(&top, "Folder link");
        assert!(link.is_dir && link.detail == "hard link to a folder");
        assert_eq!(volume.read(find(&volume.list(Some(link)).unwrap(), "Inside")).unwrap(), b"in a linked folder");
        assert!(!find(&top, "Folder alias").is_dir && names(&top).contains(&"Folder alias.rsrc".to_string()), "an alias is a file");
        let docs = volume.list(Some(find(&top, "Docs"))).unwrap();
        assert_eq!(names(&docs), ["Café.txt", "Read Me", "Read Me.rsrc"], "a decomposed name is shown composed, and the resource fork is beside its file");
        let readme = find(&docs, "Read Me");
        assert!(readme.kind == "Text" && readme.detail.starts_with("TEXT \u{B7} ttxt \u{B7} 2002-"), "{}", readme.detail);
        assert_eq!(volume.read(readme).unwrap(), b"Hello from HFS+\r");
        assert_eq!(volume.text_preset(readme), Some(MAC_TEXT_PRESET));
        assert_eq!(volume.read(find(&docs, "Read Me.rsrc")).unwrap(), crate::media::mac::tests::small_resource_fork());
        assert_eq!(volume.read(find(&docs, "Café.txt")).unwrap(), b"accents\n");
        assert_eq!(volume.text_preset(find(&docs, "Café.txt")), None, "a file with no type is left to be worked out");
        // Big is in twelve pieces: eight in its catalog record, four in the extents overflow file.
        let big = volume.read(find(&top, "Big")).unwrap();
        assert!(big.len() == 12 * 512 - 100 && big.iter().enumerate().all(|(i, b)| *b == (i % 251) as u8));
        let linked = find(&top, "Linked");
        assert_eq!((linked.size, linked.detail.ends_with("hard link")), (12, true), "{}", linked.detail);
        assert_eq!(volume.read(linked).unwrap(), b"linked file\n");
        let shortcut = find(&top, "Shortcut");
        assert_eq!((shortcut.kind.as_str(), shortcut.detail.ends_with("\u{2192} Docs/Read Me")), ("Symbolic link", true), "{}", shortcut.detail);
        assert_eq!(volume.read(shortcut).unwrap(), b"Docs/Read Me");
        // Enough files for the catalog to need index nodes above its leaves, every one found.
        let many = volume.list(Some(find(&top, "Many"))).unwrap();
        assert_eq!(many.len(), 60);
        for (i, entry) in many.iter().enumerate() {
            assert_eq!(entry.name, format!("File {i:02}"));
            assert_eq!(volume.read(entry).unwrap(), vec![i as u8; 10 + i]);
        }
        assert!(find(&top, "Many").detail.starts_with("60 item(s)"));
        assert_eq!(volume.info()[0].rows[0], ("Format".to_string(), "HFS+".to_string()));
    }

    #[test]
    fn compressed_files_are_read_uncompressed() {
        let volume = open(Arc::new(sample(b"H+")), "sample.img").unwrap();
        let top = volume.list(None).unwrap();
        assert!(!names(&top).iter().any(|n| n.starts_with("Zlib large.rsrc")), "a compressed file's resource fork is its data, not shown");
        for (name, expected, method) in [("Zlib small", text(3000), "zlib"), ("Stored small", b"plain".to_vec(), "zlib"), ("Zlib large", text(150_000), "zlib"), ("LZVN small", text(300), "LZVN"), ("LZVN large", text(150_000), "LZVN"), ("LZFSE large", text(150_000), "LZFSE"), ("Copied", b"copy".to_vec(), "stored")] {
            let entry = find(&top, name);
            assert_eq!(entry.size, expected.len() as u64, "{name}: the size uncompressed");
            assert!(entry.detail.contains(&format!("compressed ({method})")), "{name}: {}", entry.detail);
            assert_eq!(volume.read(entry).unwrap(), expected, "{name}");
        }
        let unknown = volume.read(find(&top, "LZBITMAP")).unwrap_err();
        assert!(unknown.contains("LZBITMAP") && unknown.contains("can't read yet"), "{unknown}");
    }

    #[test]
    fn lzvn_from_a_real_encoder_is_read() {
        // LZFSE's encoder writes small inputs as one LZVN block; its payload is what HFS+ compression keeps.
        let input = text(2000);
        let mut stream = Vec::new();
        lzfse_rust::encode_bytes(&input, &mut stream).unwrap();
        assert_eq!(&stream[..4], b"bvxn");
        let length = le32(&stream[8..12]) as usize;
        assert!(length < input.len() / 4, "the text repeats, so the LZVN has copies in it");
        assert_eq!(unlzvn(&stream[12..12 + length], input.len()).unwrap(), input);
        assert!(unlzvn(&stream[12..12 + length / 2], input.len()).is_err(), "cut short");
        // Damaged streams of every kind give errors or the wrong length, never a panic or more than was expected.
        let mut lzfse = Vec::new();
        lzfse_rust::encode_bytes(&text(100_000), &mut lzfse).unwrap();
        let zlib = miniz_oxide::deflate::compress_to_vec_zlib(&text(100_000), 6);
        let mut state = 1u32;
        for round in 0..3000 {
            let mut bytes = match round % 3 {
                0 => stream[12..12 + length].to_vec(),
                1 => lzfse.clone(),
                _ => zlib.clone(),
            };
            for _ in 0..1 + round % 7 {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let at = (state >> 8) as usize % bytes.len();
                bytes[at] = (state >> 16) as u8;
            }
            let out = match round % 3 {
                0 => unlzvn(&bytes, 2000),
                1 => unlzfse(&bytes, 100_000),
                _ => inflate(&bytes, 100_000),
            };
            assert!(out.is_err() || out.unwrap().len() <= 100_001);
        }
    }

    #[test]
    fn hfsx_wrapped_and_partitioned_volumes_open() {
        let hfsx = open(Arc::new(sample(b"HX")), "x.img").unwrap();
        assert!(hfsx.describe().contains(" \u{B7} HFSX \u{B7} Sample"));
        // The HFS wrapper: an HFS master directory block whose embedded extent is the HFS+ volume, in its blocks.
        let inner = sample(b"H+");
        let mut wrapped = vec![0u8; 8 * 1024];
        let mdb = 1024;
        wrapped[mdb..mdb + 2].copy_from_slice(b"BD");
        wrapped[mdb + 20..mdb + 24].copy_from_slice(&1024u32.to_be_bytes());
        wrapped[mdb + 28..mdb + 30].copy_from_slice(&4u16.to_be_bytes());
        wrapped[mdb + 36..mdb + 44].copy_from_slice(b"\x07Wrapper");
        wrapped[mdb + 124..mdb + 126].copy_from_slice(b"H+");
        wrapped[mdb + 126..mdb + 128].copy_from_slice(&6u16.to_be_bytes());
        wrapped[mdb + 128..mdb + 130].copy_from_slice(&(inner.len().div_ceil(1024) as u16).to_be_bytes());
        wrapped.resize(4 * 512 + 6 * 1024, 0);
        wrapped.extend(&inner);
        wrapped.resize(wrapped.len().next_multiple_of(1024) + 1024, 0);
        let volume = open(Arc::new(wrapped), "os9.img").unwrap();
        assert!(volume.describe().contains("HFS+ in an HFS wrapper \u{B7} Sample"), "{}", volume.describe());
        assert_eq!(volume.read(find(&volume.list(None).unwrap(), "Big")).unwrap().len(), 12 * 512 - 100);
        // In an Apple partition map, alone and beside an HFS volume.
        let map = crate::media::mac::tests::partitioned(&[&inner]);
        let alone = open(Arc::new(map), "cd.toast").unwrap();
        assert!(alone.describe().contains("partition 3 \u{B7} HFS+ \u{B7} Sample"));
        let hfs = crate::media::tests::fixture("hfs.img");
        let both = open(Arc::new(crate::media::mac::tests::partitioned(&[&hfs, &inner])), "disk.img").unwrap();
        let parts = both.list(None).unwrap();
        let docs = both.list(Some(find(&both.list(Some(&parts[3])).unwrap(), "Docs"))).unwrap();
        assert_eq!(both.read(find(&docs, "Read Me")).unwrap(), b"Hello from HFS+\r");
        assert_eq!(both.text_preset(find(&docs, "Read Me")), Some(MAC_TEXT_PRESET));
        assert!(both.read(find(&docs, "Read Me.rsrc")).is_ok());
        // In a GUID partition table, as Intel Macs have.
        let gpt = gpt_disk(&inner);
        assert!(is_image("mac.img", &gpt[..0x8800], gpt.len() as u64));
        let volume = open(Arc::new(gpt), "mac.img").unwrap();
        assert!(volume.describe().contains("partition 2 \u{B7} HFS+ \u{B7} Sample"), "{}", volume.describe());
    }

    /// A disk with a GUID partition table: an EFI system partition, then the HFS+ volume.
    pub fn gpt_disk(volume: &[u8]) -> Vec<u8> {
        let start = 64u64;
        let mut disk = vec![0u8; 64 * 512];
        disk[446 + 4] = 0xEE;
        disk[510..512].copy_from_slice(&[0x55, 0xAA]);
        disk[512..520].copy_from_slice(b"EFI PART");
        disk[512 + 72..512 + 80].copy_from_slice(&2u64.to_le_bytes());
        disk[512 + 80..512 + 84].copy_from_slice(&128u32.to_le_bytes());
        disk[512 + 84..512 + 88].copy_from_slice(&128u32.to_le_bytes());
        let efi = 1024;
        disk[efi..efi + 8].copy_from_slice(&[0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11]);
        disk[efi + 16] = 1;
        disk[efi + 32..efi + 40].copy_from_slice(&40u64.to_le_bytes());
        disk[efi + 40..efi + 48].copy_from_slice(&63u64.to_le_bytes());
        let hfs = 1024 + 128;
        disk[hfs..hfs + 16].copy_from_slice(&GPT_HFS_PLUS);
        disk[hfs + 16] = 2;
        disk[hfs + 32..hfs + 40].copy_from_slice(&start.to_le_bytes());
        disk[hfs + 40..hfs + 48].copy_from_slice(&(start + volume.len() as u64 / 512 - 1).to_le_bytes());
        for (i, unit) in "Macintosh HD".encode_utf16().enumerate() {
            disk[hfs + 56 + i * 2..hfs + 58 + i * 2].copy_from_slice(&unit.to_le_bytes());
        }
        disk.extend_from_slice(volume);
        disk
    }

    #[test]
    fn damaged_hfs_plus_volumes_give_errors_never_panics() {
        let image = sample(b"H+");
        // The B-trees are stored after the files, the catalog first.
        let (header_and_trees, files) = (be(&image[1024 + 272 + 16..1024 + 272 + 20]) as usize * 512, image.len());
        for seed in 0..200u32 {
            let mut bytes = image.clone();
            let mut state = seed.wrapping_mul(2_654_435_761);
            for _ in 0..24 {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                // Mostly the B-trees at the end, sometimes the volume header.
                let at = if state % 5 == 0 { 1024 + (state >> 8) as usize % 512 } else { header_and_trees + (state >> 8) as usize % (files - header_and_trees) };
                bytes[at] = (state >> 16) as u8;
            }
            if let Ok(volume) = open(Arc::new(bytes), "d.img") {
                walk(&*volume, None, 0, &mut |_, _| {});
            }
        }
        // Partition maps with a partition that is the whole disk, map and all, are read once, not for ever.
        let mut apm = crate::media::mac::tests::partitioned(&[&image]);
        let blocks = (apm.len() / 512) as u32;
        apm[3 * 512 + 8..3 * 512 + 16].copy_from_slice(&[&0u32.to_be_bytes()[..], &blocks.to_be_bytes()].concat());
        let looped = open(Arc::new(apm), "loop.toast").unwrap();
        assert!(looped.list(None).unwrap()[2].detail.contains("another partition map"));
        let mut gpt = gpt_disk(&image);
        gpt[1024 + 128 + 32..1024 + 128 + 40].copy_from_slice(&0u64.to_le_bytes());
        let looped = open(Arc::new(gpt), "loop.img").unwrap();
        assert!(looped.list(None).unwrap()[1].detail.contains("another partition map"));
        // A catalog whose leaves point back at themselves stops with an error.
        let mut looped = image.clone();
        let catalog = be(&image[1024 + 272 + 16..1024 + 272 + 20]) as usize * 512;
        let first_leaf = be(&image[catalog + 24..catalog + 28]) as usize;
        looped[catalog + first_leaf * 512..catalog + first_leaf * 512 + 4].copy_from_slice(&(first_leaf as u32).to_be_bytes());
        if let Ok(volume) = open(Arc::new(looped), "loop.img") {
            walk(&*volume, None, 0, &mut |_, _| {});
        }
    }

    /// Every file of a volume, by its path, as `found` is given it.
    pub fn walk(volume: &dyn Volume, dir: Option<&Entry>, depth: usize, found: &mut dyn FnMut(String, Result<Vec<u8>, String>)) {
        walk_from(volume, dir, "", depth, found);
    }

    fn walk_from(volume: &dyn Volume, dir: Option<&Entry>, prefix: &str, depth: usize, found: &mut dyn FnMut(String, Result<Vec<u8>, String>)) {
        if depth > 24 {
            return;
        }
        let entries = match volume.list(dir) {
            Ok(entries) => entries,
            Err(problem) => return found(format!("{prefix}(listing)"), Err(problem)),
        };
        for entry in entries {
            let path = format!("{prefix}{}", entry.name);
            if entry.is_dir {
                if entry.detail != "hard link to a folder" {
                    walk_from(volume, Some(&entry), &format!("{path}/"), depth + 1, found);
                }
            } else {
                found(path, volume.read(&entry));
            }
        }
    }

    /// 7-Zip's own command-line 7-Zip, if it's installed: LIMONITER_7ZZ, or `7zz` on the path.
    pub fn modern_7zip() -> Option<String> {
        let modern = std::env::var("LIMONITER_7ZZ").unwrap_or_else(|_| "7zz".into());
        std::process::Command::new(&modern).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok().then_some(modern)
    }

    /// The test volume, compressed files and all, reads the same in 7-Zip as here, so the layouts it's built with are
    /// the ones 7-Zip knows from real volumes. Skipped without a 7-Zip new enough to read HFS+ compression.
    #[test]
    fn the_test_volume_reads_as_7zip_reads_it() {
        if modern_7zip().is_none() {
            return;
        }
        let path = std::env::temp_dir().join(format!("limoniter-hfsplus-sample-{}.img", std::process::id()));
        std::fs::write(&path, sample(b"H+")).unwrap();
        let differences = differences_from_7zip(&path);
        let _ = std::fs::remove_file(&path);
        let differences = differences.expect("7-Zip reads the test volume");
        // 7-Zip reads neither LZBITMAP nor LZFSE in a resource fork, and sees a folder hard link as a file; everything
        // else must match.
        assert!(differences.iter().all(|d| d.starts_with("LZBITMAP") || d.starts_with("LZFSE large") || d.starts_with("Folder link")), "{differences:#?}");
    }

    /// Every file's path and entry, without reading them.
    pub fn entries(volume: &dyn Volume, dir: Option<&Entry>, prefix: &str, depth: usize, out: &mut Vec<(String, Entry)>) {
        if depth > 24 {
            return;
        }
        for entry in volume.list(dir).unwrap_or_default() {
            let path = format!("{prefix}{}", entry.name);
            if entry.is_dir && entry.detail != "hard link to a folder" {
                entries(volume, Some(&entry), &format!("{path}/"), depth + 1, out);
            } else if !entry.is_dir {
                out.push((path, entry));
            }
        }
    }

    /// What 7-Zip makes of an image: each file it extracts, by its path without the volume's name in front, and the
    /// file's SHA-256. None when 7-Zip's own `7zz` (or the one LIMONITER_7ZZ names) isn't installed. p7zip's older
    /// `7z` isn't asked: it reads the stored blocks of compressed files wrongly, splits names with a slash in them
    /// into folders, and knows neither resource forks nor LZFSE.
    fn seven_zip(image: &Path) -> Option<BTreeMap<String, String>> {
        use sha2::Digest;
        let quiet = || std::process::Stdio::null();
        let seven = modern_7zip()?;
        let out = std::env::temp_dir().join(format!("limoniter-7z-{}-{}", std::process::id(), image.file_name()?.to_string_lossy()));
        let _ = std::fs::remove_dir_all(&out);
        let mut command = std::process::Command::new(seven);
        command.arg("x").arg("-y").arg("-sns").arg("-snld").arg(format!("-o{}", out.display()));
        command.arg(image).stdout(quiet()).stderr(quiet()).status().ok()?;
        let mut files = BTreeMap::new();
        let mut folders = vec![out.clone()];
        while let Some(folder) = folders.pop() {
            for item in std::fs::read_dir(&folder).ok()?.flatten() {
                let path = item.path();
                // 7-Zip rewrites or refuses symbolic links that lead outside the folder, so they aren't compared.
                if std::fs::read_link(&path).is_ok() {
                    continue;
                }
                if path.is_dir() {
                    folders.push(path);
                } else if let Ok(bytes) = std::fs::read(&path) {
                    let relative = path.strip_prefix(&out).unwrap().to_string_lossy().into_owned();
                    let relative: String = relative.nfc().collect();
                    // 7-Zip puts the volume's name first, and shows the hidden folders, which aren't files to compare.
                    let Some((_, inside)) = relative.split_once('/') else {
                        continue;
                    };
                    if inside.starts_with("[HFS+ Private Data]") || inside.starts_with(".HFS+ Private Directory Data") {
                        continue;
                    }
                    // A resource fork is listed here as `name.rsrc`; other streams are extended attributes.
                    let inside = match inside.rsplit_once(':') {
                        Some((file, "rsrc")) => format!("{file}.rsrc"),
                        Some(_) => continue,
                        None => inside.to_string(),
                    };
                    files.insert(inside.split('/').map(tidy_name).collect::<Vec<_>>().join("/"), format!("{:x}", sha2::Sha256::digest(&bytes)));
                }
            }
        }
        let _ = std::fs::remove_dir_all(&out);
        Some(files)
    }

    /// Compares every file of an image with 7-Zip's: each of 7-Zip's files must be listed and read with the same
    /// bytes. Gives what differs; None when 7-Zip isn't there to ask, or its idea of the image isn't comparable.
    pub fn differences_from_7zip(path: &Path) -> Option<Vec<String>> {
        use sha2::Digest;
        let theirs = seven_zip(path)?;
        if theirs.is_empty() {
            return None;
        }
        let volume = match super::super::open_file(path) {
            Ok(volume) => volume,
            Err(problem) => return Some(vec![format!("refused: {problem}")]),
        };
        // A disk image of another file system, such as APFS, is 7-Zip's to read, not this.
        if !volume.describe().contains("HFS") {
            return None;
        }
        let mut ours = BTreeMap::new();
        let mut problems = Vec::new();
        // 7-Zip can't always make a symbolic link, and writes something else in its place.
        let mut listed = Vec::new();
        entries(&*volume, None, "", 0, &mut listed);
        // Nor does it follow hard links, but gives the link's own empty data.
        let links: HashSet<String> = listed.into_iter().filter(|(_, e)| e.kind == "Symbolic link" || e.detail.ends_with("hard link")).map(|(path, _)| path).collect();
        walk(&*volume, None, 0, &mut |path, bytes| match bytes {
            Ok(bytes) => {
                ours.insert(path, format!("{:x}", sha2::Sha256::digest(&bytes)));
            }
            Err(problem) => problems.push(format!("{path}: {problem}")),
        });
        for (path, hash) in theirs.iter().filter(|(path, _)| !links.contains(*path)) {
            match ours.get(path) {
                Some(mine) if mine == hash => {}
                Some(_) => problems.push(format!("{path}: different bytes from 7-Zip's")),
                None => problems.push(format!("{path}: 7-Zip has it, Limoniter doesn't")),
            }
        }
        Some(problems)
    }

    /// The HFS+ disks and disk images in ..\testmedia\hfsplus, when it's there: every file of each read the same as
    /// 7-Zip reads it, where 7-Zip's 7zz is installed.
    #[test]
    fn real_hfs_plus_images_match_7zip() {
        let root = Path::new("../testmedia/hfsplus");
        let Ok(items) = std::fs::read_dir(root) else {
            return;
        };
        let mut wrong = Vec::new();
        let mut compared = 0;
        let mut images: Vec<_> = items.flatten().map(|i| i.path()).filter(|p| p.is_file() && !p.to_string_lossy().ends_with(".txt")).collect();
        images.sort();
        for image in images {
            match differences_from_7zip(&image) {
                Some(problems) => {
                    compared += 1;
                    wrong.extend(problems.into_iter().map(|p| format!("{}: {p}", image.display())));
                }
                // Without 7-Zip, the image must still open and read throughout.
                None => {
                    let volume = super::super::open_file(&image).unwrap_or_else(|e| panic!("{}: {e}", image.display()));
                    walk(&*volume, None, 0, &mut |path, bytes| {
                        if let Err(problem) = bytes {
                            wrong.push(format!("{} {path}: {problem}", image.display()));
                        }
                    });
                }
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
        eprintln!("{compared} HFS+ images compared with 7-Zip");
    }

    /// For a sweep of many images: set LIMONITER_HFSPLUS_SWEEP to a folder and run with --ignored.
    #[test]
    #[ignore]
    fn sweep_hfs_plus_images_against_7zip() {
        let Ok(root) = std::env::var("LIMONITER_HFSPLUS_SWEEP") else {
            return;
        };
        let mut folders = vec![std::path::PathBuf::from(root)];
        let mut images = Vec::new();
        while let Some(folder) = folders.pop() {
            for item in std::fs::read_dir(&folder).unwrap().flatten() {
                let path = item.path();
                if path.is_dir() {
                    folders.push(path);
                } else {
                    images.push(path);
                }
            }
        }
        images.sort();
        for image in images {
            let opened = super::super::open_file(&image);
            let described = opened.as_ref().map_or_else(|e| format!("REFUSED: {e}"), |v| v.describe());
            let differences = differences_from_7zip(&image);
            // What's in it of what's particular to HFS+.
            let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
            if let Ok(volume) = &opened {
                let mut listed = Vec::new();
                entries(&**volume, None, "", 0, &mut listed);
                for (_, entry) in listed {
                    for (mark, name) in [("compressed (", ""), ("hard link", "hard link"), ("\u{2192}", "symbolic link"), ("of ", "resource fork")] {
                        if let Some(at) = entry.detail.find(mark) {
                            let name = if name.is_empty() { entry.detail[at..].split(')').next().unwrap_or_default().to_string() + ")" } else { name.to_string() };
                            *kinds.entry(name).or_default() += 1;
                        }
                    }
                }
            }
            eprintln!("    {kinds:?}");
            eprintln!("{}\n    {described}\n    {}", image.display(), differences.map_or_else(|| "7-Zip can't compare".to_string(), |d| if d.is_empty() { "same as 7-Zip".to_string() } else { format!("{} differences: {:#?}", d.len(), &d[..d.len().min(8)]) }));
        }
    }
}
