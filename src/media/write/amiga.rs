//! Amiga floppies (.adf), OFS and FFS, international or not (DOS\0 to DOS\3), written as the ADF-FAQ lays the
//! blocks out: a file's header block, listing its data blocks from the end of its table backwards, with extension
//! blocks for more than 72; a folder's header with a hash table of the entries in it, each the head of a chain;
//! the bitmap of free blocks, from the root block's list. Every header block's checksum makes its longs add to zero.
//!
//! Disks with directory caches (DOS\4, DOS\5) or long names (DOS\6, DOS\7) are only read, as a cache left stale
//! would show the wrong files.

use super::{Addition, Entry, ImageWriter, already};

const T_HEADER: u32 = 2;
const T_LIST: u32 = 16;
const T_DATA: u32 = 8;
const ST_ROOT: u32 = 1;
const ST_USERDIR: u32 = 2;
const ST_FILE: u32 = (-3i32) as u32;
const BLOCK: usize = 512;
const TABLE: usize = 72;

pub struct Adf {
    bytes: Vec<u8>,
    blocks: u32,
    root: u32,
    ffs: bool,
    international: bool,
}

/// A date now, as AmigaDOS keeps it: days since 1978, minutes, ticks.
fn now() -> [u32; 3] {
    let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let since_1978 = seconds.saturating_sub(252_460_800);
    [(since_1978 / 86_400) as u32, ((since_1978 % 86_400) / 60) as u32, ((since_1978 % 60) * 50) as u32]
}

/// A blank floppy as the Workbench's Format leaves it (not made bootable): the boot block's DOS type, the root in
/// the middle, and the bitmap after it.
pub fn blank(label: &str, ffs: bool, high_density: bool) -> Vec<u8> {
    let blocks: u32 = if high_density { 3520 } else { 1760 };
    let root = blocks / 2;
    let mut adf = Adf { bytes: vec![0; blocks as usize * BLOCK], blocks, root, ffs, international: false };
    adf.bytes[..4].copy_from_slice(&[b'D', b'O', b'S', u8::from(ffs)]);
    adf.bytes[8..12].copy_from_slice(&root.to_be_bytes());
    let bitmap = root + 1;
    adf.set_long(root, 0, T_HEADER);
    adf.set_long(root, 3, TABLE as u32);
    adf.set_long(root, 78, 0xFFFF_FFFF);
    adf.set_long(root, 79, bitmap);
    let date = now();
    // The root's own date, the disk's last change and its creation.
    for (at, value) in [105, 106, 107, 118, 119, 120, 121, 122, 123].iter().zip(date.iter().cycle()) {
        adf.set_long(root, *at, *value);
    }
    adf.set_name(root, if label.trim().is_empty() { "Empty" } else { label });
    adf.set_long(root, 127, ST_ROOT);
    for block in 2..blocks {
        adf.set_free(block, block != root && block != bitmap);
    }
    adf.seal(root);
    adf.bytes
}

impl Adf {
    pub fn open(bytes: Vec<u8>) -> Result<Self, String> {
        if !bytes.starts_with(b"DOS") || bytes.len() < 4 {
            return Err("This Amiga disk has no AmigaDOS boot block.".into());
        }
        let dos = bytes[3];
        if dos > 3 {
            return Err(format!("This disk is DOS\\{dos}, {}; Limoniter only reads it, so the cache or names aren't left wrong.", if dos <= 5 { "with a directory cache" } else { "with long names" }));
        }
        if !matches!(bytes.len(), 901_120 | 1_802_240) {
            return Err("Only standard 880K and 1.76MB floppy images are changed here.".into());
        }
        let blocks = (bytes.len() / BLOCK) as u32;
        let adf = Self { root: blocks / 2, blocks, ffs: dos & 1 == 1, international: dos & 2 == 2, bytes };
        if adf.long(adf.root, 0) != T_HEADER || adf.long(adf.root, 127) != ST_ROOT {
            return Err("The disk has no root block where AmigaDOS keeps one.".into());
        }
        if adf.long(adf.root, 78) != 0xFFFF_FFFF {
            return Err("The disk's bitmap is marked out of date (it wasn't put away properly); let the Amiga validate it first.".into());
        }
        Ok(adf)
    }

    fn at(block: u32, index: usize) -> usize {
        block as usize * BLOCK + index * 4
    }

    fn long(&self, block: u32, index: usize) -> u32 {
        let at = Self::at(block, index);
        u32::from_be_bytes([self.bytes[at], self.bytes[at + 1], self.bytes[at + 2], self.bytes[at + 3]])
    }

    fn set_long(&mut self, block: u32, index: usize, value: u32) {
        let at = Self::at(block, index);
        self.bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
    }

    fn clear(&mut self, block: u32) {
        let at = block as usize * BLOCK;
        self.bytes[at..at + BLOCK].fill(0);
    }

    /// Sets a block's checksum (long 5, or long 0 in a bitmap block), so its longs add to zero.
    fn checksum_at(&mut self, block: u32, index: usize) {
        self.set_long(block, index, 0);
        let sum = (0..BLOCK / 4).fold(0u32, |sum, i| sum.wrapping_add(self.long(block, i)));
        self.set_long(block, index, sum.wrapping_neg());
    }

    fn seal(&mut self, block: u32) {
        self.checksum_at(block, 5);
        if block == self.root {
            for page in 0..25 {
                let bitmap = self.long(self.root, 79 + page);
                if bitmap != 0 {
                    self.checksum_at(bitmap, 0);
                }
            }
        }
    }

    fn set_name(&mut self, block: u32, name: &str) {
        let at = block as usize * BLOCK + 432;
        let bytes: Vec<u8> = name.chars().map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')).take(30).collect();
        self.bytes[at..at + 32].fill(0);
        self.bytes[at] = bytes.len() as u8;
        self.bytes[at + 1..at + 1 + bytes.len()].copy_from_slice(&bytes);
    }

    fn name_of(&self, block: u32) -> String {
        let at = block as usize * BLOCK + 432;
        let length = usize::from(self.bytes[at]).min(30);
        self.bytes[at + 1..at + 1 + length].iter().map(|b| char::from(*b)).collect()
    }

    /// The bitmap block and bit for a block: the bitmap starts at block 2.
    fn bit(&self, block: u32) -> (u32, usize, u32) {
        let index = (block - 2) as usize;
        let page = index / (127 * 32);
        let bitmap = self.long(self.root, 79 + page);
        (bitmap, 1 + (index % (127 * 32)) / 32, 1 << (index % 32))
    }

    fn is_free(&self, block: u32) -> bool {
        let (bitmap, long, mask) = self.bit(block);
        self.long(bitmap, long) & mask != 0
    }

    fn set_free(&mut self, block: u32, free: bool) {
        let (bitmap, long, mask) = self.bit(block);
        let value = self.long(bitmap, long);
        self.set_long(bitmap, long, if free { value | mask } else { value & !mask });
    }

    fn free_blocks(&self) -> usize {
        (2..self.blocks).filter(|b| self.is_free(*b)).count()
    }

    /// Takes free blocks, from just past the root upwards, then from the start, as AmigaDOS fills a floppy.
    fn allocate(&mut self, count: usize) -> Result<Vec<u32>, String> {
        let order = (self.root + 1..self.blocks).chain(2..self.root);
        let found: Vec<u32> = order.filter(|b| self.is_free(*b)).take(count).collect();
        if found.len() < count {
            return Err(format!("The disk is full: this needs {} blocks, and {} are free.", count, self.free_blocks()));
        }
        for block in &found {
            self.set_free(*block, false);
        }
        Ok(found)
    }

    /// The hash of a name in a folder's table, as AmigaDOS works it out (upper-casing accented letters too on an
    /// international disk).
    fn hash(&self, name: &str) -> usize {
        let mut hash = name.chars().count() as u32;
        for c in name.chars() {
            let mut b = u32::from(c) & 0xFF;
            if (u32::from(b'a')..=u32::from(b'z')).contains(&b) || (self.international && (224..=254).contains(&b) && b != 247) {
                b -= 32;
            }
            hash = (hash.wrapping_mul(13).wrapping_add(b)) & 0x7FF;
        }
        hash as usize % TABLE
    }

    fn same_name(&self, a: &str, b: &str) -> bool {
        let fold = |s: &str| -> Vec<u32> {
            s.chars()
                .map(|c| {
                    let b = u32::from(c);
                    if (97..=122).contains(&b) || (self.international && (224..=254).contains(&b) && b != 247) { b - 32 } else { b }
                })
                .collect()
        };
        fold(a) == fold(b)
    }

    fn folder_block(&self, folder: Option<&Entry>) -> u32 {
        folder.map_or(self.root, |f| f.location as u32)
    }

    /// The header blocks in a folder, by every hash chain.
    fn children(&self, folder: u32) -> Vec<u32> {
        let mut out = Vec::new();
        for slot in 0..TABLE {
            let mut next = self.long(folder, 6 + slot);
            while next != 0 && next < self.blocks && !out.contains(&next) {
                out.push(next);
                next = self.long(next, 124);
            }
        }
        out
    }

    fn find(&self, folder: u32, name: &str) -> Option<u32> {
        self.children(folder).into_iter().find(|b| self.same_name(&self.name_of(*b), name))
    }

    /// Puts a header in its folder's hash chain, at the head, and dates the folder.
    fn link(&mut self, folder: u32, header: u32) {
        let slot = self.hash(&self.name_of(header));
        let head = self.long(folder, 6 + slot);
        self.set_long(header, 124, head);
        self.seal(header);
        self.set_long(folder, 6 + slot, header);
        self.touch(folder);
    }

    /// Takes a header out of its folder's hash chain.
    fn unlink(&mut self, folder: u32, header: u32) -> Result<(), String> {
        let slot = self.hash(&self.name_of(header));
        let after = self.long(header, 124);
        if self.long(folder, 6 + slot) == header {
            self.set_long(folder, 6 + slot, after);
        } else {
            let mut at = self.long(folder, 6 + slot);
            let mut steps = 0;
            while at != 0 && self.long(at, 124) != header {
                at = self.long(at, 124);
                steps += 1;
                if steps > 10_000 {
                    return Err("The folder's hash chain loops.".into());
                }
            }
            if at == 0 {
                return Err("The entry isn't in its folder's hash chain.".into());
            }
            self.set_long(at, 124, after);
            self.seal(at);
        }
        self.set_long(header, 124, 0);
        self.touch(folder);
        Ok(())
    }

    /// Dates a folder as changed now, and the disk with it.
    fn touch(&mut self, folder: u32) {
        let date = now();
        for (i, value) in date.iter().enumerate() {
            self.set_long(folder, 105 + i, *value);
        }
        if folder != self.root {
            self.seal(folder);
        }
        for (i, value) in date.iter().enumerate() {
            self.set_long(self.root, 118 + i, *value);
        }
        self.seal(self.root);
    }

    fn check_name(name: &str) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() || name.contains(':') || name.contains('/') || name.chars().any(|c| c.is_control() || u32::from(c) > 255) {
            return Err(format!("\"{name}\" can't be an Amiga file name: no colons, slashes or characters outside Latin-1."));
        }
        if name.chars().count() > 30 {
            return Err(format!("\"{name}\" is longer than the 30 characters an Amiga name can have."));
        }
        Ok(name.to_string())
    }

    /// The data blocks a file's header and its extensions list, and the extension blocks themselves.
    fn file_blocks(&self, header: u32) -> (Vec<u32>, Vec<u32>) {
        let (mut data, mut lists) = (Vec::new(), Vec::new());
        let mut list = header;
        loop {
            let count = (self.long(list, 2) as usize).min(TABLE);
            data.extend((0..count).map(|i| self.long(list, 6 + TABLE - 1 - i)).filter(|b| *b != 0));
            let next = self.long(list, 126);
            if next == 0 || next >= self.blocks || lists.contains(&next) {
                break;
            }
            lists.push(next);
            list = next;
        }
        (data, lists)
    }

    fn remove(&mut self, folder: u32, header: u32) -> Result<(), String> {
        let kind = self.long(header, 127);
        if kind == ST_USERDIR && !self.children(header).is_empty() {
            return Err("The folder has files in it; take them off first.".into());
        }
        if kind != ST_USERDIR && kind != ST_FILE {
            return Err("Only files and folders are taken off here, not links.".into());
        }
        self.unlink(folder, header)?;
        if kind == ST_FILE {
            let (data, lists) = self.file_blocks(header);
            for block in data.into_iter().chain(lists) {
                self.set_free(block, true);
            }
        }
        self.set_free(header, true);
        self.seal(self.root);
        Ok(())
    }
}

impl ImageWriter for Adf {
    fn add(&mut self, folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        let name = Self::check_name(&file.name)?;
        let parent = self.folder_block(folder);
        if let Some(existing) = self.find(parent, &name) {
            if !file.replace {
                return Err(already(&name));
            }
            if self.long(existing, 127) != ST_FILE {
                return Err(format!("{name} is a folder here, so a file can't replace it."));
            }
            self.remove(parent, existing)?;
        }
        let per_block = if self.ffs { BLOCK } else { BLOCK - 24 };
        let data_count = file.bytes.len().div_ceil(per_block);
        let list_count = data_count.saturating_sub(TABLE).div_ceil(TABLE);
        let blocks = self.allocate(1 + list_count + data_count)?;
        let header = blocks[0];
        let lists = &blocks[1..1 + list_count];
        let data = &blocks[1 + list_count..];
        // The header and each extension list up to 72 data blocks, the first at the end of the table.
        let mut owners = vec![header];
        owners.extend_from_slice(lists);
        for (n, owner) in owners.iter().enumerate() {
            self.clear(*owner);
            let part = &data[(n * TABLE).min(data.len())..((n + 1) * TABLE).min(data.len())];
            self.set_long(*owner, 0, if n == 0 { T_HEADER } else { T_LIST });
            self.set_long(*owner, 1, *owner);
            self.set_long(*owner, 2, part.len() as u32);
            for (i, block) in part.iter().enumerate() {
                self.set_long(*owner, 6 + TABLE - 1 - i, *block);
            }
            self.set_long(*owner, 125, if n == 0 { parent } else { header });
            self.set_long(*owner, 126, owners.get(n + 1).copied().unwrap_or(0));
            self.set_long(*owner, 127, ST_FILE);
            if n > 0 {
                self.seal(*owner);
            }
        }
        self.set_long(header, 4, data.first().copied().unwrap_or(0));
        self.set_long(header, 81, file.bytes.len() as u32);
        for (i, value) in now().iter().enumerate() {
            self.set_long(header, 105 + i, *value);
        }
        self.set_name(header, &name);
        for (i, block) in data.iter().enumerate() {
            let part = &file.bytes[i * per_block..((i + 1) * per_block).min(file.bytes.len())];
            self.clear(*block);
            let at = *block as usize * BLOCK;
            if self.ffs {
                self.bytes[at..at + part.len()].copy_from_slice(part);
            } else {
                self.set_long(*block, 0, T_DATA);
                self.set_long(*block, 1, header);
                self.set_long(*block, 2, i as u32 + 1);
                self.set_long(*block, 3, part.len() as u32);
                self.set_long(*block, 4, data.get(i + 1).copied().unwrap_or(0));
                self.bytes[at + 24..at + 24 + part.len()].copy_from_slice(part);
                self.checksum_at(*block, 5);
            }
        }
        self.link(parent, header);
        self.seal(self.root);
        Ok(())
    }

    fn delete(&mut self, folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        let parent = self.folder_block(folder);
        let header = entry.location as u32;
        if !self.children(parent).contains(&header) {
            return Err(format!("{} isn't in this folder.", entry.name));
        }
        self.remove(parent, header)
    }

    fn rename(&mut self, folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        let name = Self::check_name(name)?;
        let parent = self.folder_block(folder);
        let header = entry.location as u32;
        if !self.children(parent).contains(&header) {
            return Err(format!("{} isn't in this folder.", entry.name));
        }
        if self.find(parent, &name).is_some_and(|other| other != header) {
            return Err(already(&name));
        }
        self.unlink(parent, header)?;
        self.set_name(header, &name);
        self.link(parent, header);
        self.seal(self.root);
        Ok(())
    }

    fn make_folder(&mut self, folder: Option<&Entry>, name: &str) -> Result<(), String> {
        let name = Self::check_name(name)?;
        let parent = self.folder_block(folder);
        if self.find(parent, &name).is_some() {
            return Err(already(&name));
        }
        let header = self.allocate(1)?[0];
        self.clear(header);
        self.set_long(header, 0, T_HEADER);
        self.set_long(header, 1, header);
        for (i, value) in now().iter().enumerate() {
            self.set_long(header, 105 + i, *value);
        }
        self.set_name(header, &name);
        self.set_long(header, 125, parent);
        self.set_long(header, 127, ST_USERDIR);
        self.link(parent, header);
        self.seal(self.root);
        Ok(())
    }

    fn has_folders(&self) -> bool {
        true
    }

    fn free(&self) -> Option<u64> {
        Some((self.free_blocks() * if self.ffs { BLOCK } else { BLOCK - 24 }) as u64)
    }

    fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{entry, names, reopened};
    use super::*;
    use std::process::Command;

    fn add(adf: &mut Adf, folder: Option<&Entry>, name: &str, bytes: &[u8]) {
        adf.add(folder, &Addition { name: name.into(), bytes: bytes.to_vec(), ..Default::default() }).unwrap();
    }

    #[test]
    fn files_and_folders_go_on_ofs_and_ffs_floppies() {
        for (ffs, hd) in [(false, false), (true, false), (true, true)] {
            let mut adf = Adf::open(blank("Work", ffs, hd)).unwrap();
            let free = adf.free_blocks();
            // More than 72 data blocks, so the file needs an extension block.
            let big: Vec<u8> = (0..60_000u32).map(|i| (i * 3) as u8).collect();
            add(&mut adf, None, "Big", &big);
            add(&mut adf, None, "s", b"startup");
            adf.make_folder(None, "Tools").unwrap();
            let volume = reopened(&adf, "t.adf");
            assert_eq!(names(&*volume, None), ["Tools", "Big", "s"], "ffs {ffs}");
            assert_eq!(volume.read(&entry(&*volume, None, "Big")).unwrap(), big, "ffs {ffs}");
            let tools = entry(&*volume, None, "Tools");
            add(&mut adf, Some(&tools), "Readme.info", &[0xE3, 0x10]);
            let volume = reopened(&adf, "t.adf");
            assert_eq!(volume.read(&entry(&*volume, Some(&tools), "Readme.info")).unwrap(), [0xE3, 0x10]);
            assert!(adf.delete(None, &tools).unwrap_err().contains("files in it"));
            adf.rename(None, &entry(&*volume, None, "s"), "Startup-Sequence").unwrap();
            adf.delete(None, &entry(&*volume, None, "Big")).unwrap();
            let volume = reopened(&adf, "t.adf");
            assert_eq!(names(&*volume, None), ["Tools", "Startup-Sequence"]);
            assert_eq!(volume.read(&entry(&*volume, None, "Startup-Sequence")).unwrap(), b"startup");
            // The folder, and a header and a data block for each of the two files left.
            assert_eq!(adf.free_blocks(), free - 5);
        }
    }

    /// amitools' xdftool reads the files back and finds the disk's blocks and bitmap consistent.
    #[test]
    fn xdftool_reads_and_checks_written_floppies() {
        let xdftool = std::path::Path::new("/root/reftools/bin/xdftool");
        if !xdftool.is_file() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("limoniter-adf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (ffs, hd) in [(false, false), (true, false), (true, true)] {
            let mut adf = Adf::open(blank("Checked", ffs, hd)).unwrap();
            let big: Vec<u8> = (0..60_000u32).map(|i| (i * 7) as u8).collect();
            add(&mut adf, None, "Big file", &big);
            adf.make_folder(None, "Sub").unwrap();
            let sub = entry(&*reopened(&adf, "t.adf"), None, "Sub");
            for n in 0..30 {
                add(&mut adf, Some(&sub), &format!("file {n}"), format!("{n}").as_bytes());
            }
            add(&mut adf, None, "Gone", &[1; 5000]);
            adf.delete(None, &entry(&*reopened(&adf, "t.adf"), None, "Gone")).unwrap();
            let path = dir.join(format!("t{ffs}{hd}.adf"));
            std::fs::write(&path, adf.bytes()).unwrap();
            // Its count of used blocks agrees with the bitmap written.
            let info = Command::new(xdftool).arg("-r").arg(&path).arg("info").output().unwrap();
            let info = String::from_utf8_lossy(&info.stdout).to_string();
            let used: usize = info.lines().find(|l| l.starts_with("used:")).and_then(|l| l.split_whitespace().nth(1)).and_then(|n| n.parse().ok()).unwrap();
            assert_eq!(used, adf.blocks as usize - adf.free_blocks(), "{info}");
            let out = dir.join("big.out");
            let read = Command::new(xdftool).arg(&path).arg("read").arg("Big file").arg(&out).output().unwrap();
            assert!(read.status.success(), "{}", String::from_utf8_lossy(&read.stderr));
            assert_eq!(std::fs::read(&out).unwrap(), big);
            let listed = Command::new(xdftool).arg(&path).arg("list").output().unwrap();
            let listed = String::from_utf8_lossy(&listed.stdout);
            assert!(listed.contains("file 29") && !listed.contains("Gone"), "{listed}");
            // A file xdftool writes after ours goes in blocks the bitmap says are free: ours are all still whole.
            let extra = dir.join("extra.in");
            std::fs::write(&extra, vec![0x5A; 40_000]).unwrap();
            let wrote = Command::new(xdftool).arg(&path).arg("write").arg(&extra).arg("Extra").output().unwrap();
            assert!(wrote.status.success(), "{}", String::from_utf8_lossy(&wrote.stderr));
            let volume = super::super::super::open(std::sync::Arc::new(std::fs::read(&path).unwrap()), "t.adf").unwrap();
            assert_eq!(volume.read(&entry(&*volume, None, "Big file")).unwrap(), big);
            assert_eq!(volume.read(&entry(&*volume, None, "Extra")).unwrap(), vec![0x5A; 40_000]);
            let sub = entry(&*volume, None, "Sub");
            assert_eq!(volume.read(&entry(&*volume, Some(&sub), "file 7")).unwrap(), b"7");
        }
        std::fs::remove_dir_all(dir).ok();
    }
}
