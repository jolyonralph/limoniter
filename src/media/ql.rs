//! Sinclair QL microdrive cartridges, as QL emulators keep them (.mdv): QLAY's format, which sQLux, Q-emuLator,
//! MAME and the MiSTer's QL read. The image is the tape's 255 sectors of 686 bytes in the order they pass the
//! head, each as the microdrive records it:
//!
//! - 12 bytes of preamble (ten zeros, two $FF), then the sector's header: $FF, its number, the cartridge's name in
//!   ten characters, two random bytes the QL chose when it formatted it, and a checksum of those 14 bytes;
//! - another 12 of preamble, then the block's header: the file it's part of and which of its blocks it is, and a
//!   checksum of those two;
//! - 8 bytes of preamble (six zeros, two $FF), the 512 bytes of the block, their checksum, and 120 bytes of gap.
//!
//! Each checksum is $0F0F plus the bytes, kept as a word with its low byte first. Sector 0 holds the map: for each
//! sector a file and block, $F8 for the map itself, $FD for a free sector and $FF for one QDOS won't use, its last
//! word twice the number of the sector last given out. File 0 is the directory, a 64-byte QDOS header for each file
//! by its number (the first, the directory's own, gives its length); each file's first block starts with its header
//! too, and its length counts it.
//!
//! The layout is MAME's (src/devices/imagedev/microdrv.cpp, "QLAY image format: sector byte layout"), checked
//! against mdvtool (github.com/xxoraa/mdvtool, written for the MiST's QL) and cartridges formatted by a QL; the file
//! header is the QL Technical Guide's (section 7.0). Real cartridges give their directory entries the files' names
//! and lengths, and keep a program's type and data space in the header in its first block, which is read first.

use super::{Entry, Source, Volume, tidy_name};
use crate::ql::{HEADER_LEN, Header};
use std::sync::Arc;

pub const SECTORS: usize = 255;
pub const SECTOR_LEN: usize = 686;
/// How long a QLAY image is: every sector, whether the tape had it or not.
pub const IMAGE_LEN: usize = SECTORS * SECTOR_LEN;

const HEADER: usize = 12;
const BLOCK_HEADER: usize = 40;
const DATA: usize = 52;
const DATA_CHECK: usize = DATA + 512;
const GAP: usize = DATA_CHECK + 2;

/// The map's own file number, a free sector's, and one not to be used.
pub const MAP_FILE: u8 = 0xF8;
pub const FREE_FILE: u8 = 0xFD;
pub const BAD_FILE: u8 = 0xFF;
/// The file number the map's block header carries, as the QL writes it.
const MAP_BLOCK_FILE: u8 = 0x80;
/// The most files a cartridge's directory numbers: those above are the system's.
pub const MOST_FILES: usize = 127;

/// The sum the microdrive checks each part by.
pub fn checksum(bytes: &[u8]) -> [u8; 2] {
    bytes.iter().fold(0x0F0Fu16, |sum, b| sum.wrapping_add(u16::from(*b))).to_le_bytes()
}

/// Whether bytes are a QLAY image: the right length, and the first sector's preamble where it should be.
pub fn looks_like(head: &[u8], len: u64) -> bool {
    len == IMAGE_LEN as u64 && head.get(..12).is_some_and(|p| p[..10].iter().all(|b| *b == 0) && p[10..] == [0xFF, 0xFF])
}

/// A file on a cartridge: its number, its header (from its first block, or else its directory entry) and its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub number: usize,
    pub header: Header,
    pub bytes: Vec<u8>,
}

/// A cartridge's image, read and changed in place.
pub struct Cartridge {
    pub bytes: Vec<u8>,
    /// Where each sector is in the image, by its number.
    at: Vec<Option<usize>>,
}

impl Cartridge {
    pub fn open(bytes: Vec<u8>) -> Result<Self, String> {
        if bytes.len() != IMAGE_LEN {
            return Err(crate::i18n::tf!("A QL microdrive image is {IMAGE_LEN} bytes, and this is {}.", bytes.len()));
        }
        let mut at = vec![None; 256];
        for place in 0..SECTORS {
            let sector = &bytes[place * SECTOR_LEN..(place + 1) * SECTOR_LEN];
            // A place the tape had no sector at has no header.
            if sector[10..12] != [0xFF, 0xFF] || sector[HEADER] != 0xFF {
                continue;
            }
            let number = usize::from(sector[HEADER + 1]);
            if at[number].is_none() {
                at[number] = Some(place * SECTOR_LEN);
            }
        }
        if at[0].is_none() {
            return Err(crate::i18n::tr("This microdrive image has no sector 0, where the cartridge's map is.").into());
        }
        Ok(Self { bytes, at })
    }

    /// A cartridge as a QL formats it, named `name`: the map in sector 0, an empty directory, every other sector
    /// free but 254, which QDOS's FORMAT leaves unused.
    pub fn blank(name: &str, random: [u8; 2]) -> Vec<u8> {
        let mut label = [b' '; 10];
        for (slot, c) in label.iter_mut().zip(name.chars().filter(char::is_ascii).map(|c| c as u8)) {
            *slot = c;
        }
        let mut bytes = vec![0u8; IMAGE_LEN];
        // The sectors pass the head in falling order, as on a real tape.
        for place in 0..SECTORS {
            let number = (SECTORS - 1 - place) as u8;
            let sector = &mut bytes[place * SECTOR_LEN..(place + 1) * SECTOR_LEN];
            sector[10] = 0xFF;
            sector[11] = 0xFF;
            sector[HEADER] = 0xFF;
            sector[HEADER + 1] = number;
            sector[HEADER + 2..HEADER + 12].copy_from_slice(&label);
            sector[HEADER + 12..HEADER + 14].copy_from_slice(&random);
            let sum = checksum(&sector[HEADER..HEADER + 14]);
            sector[HEADER + 14..HEADER + 16].copy_from_slice(&sum);
            sector[BLOCK_HEADER - 2] = 0xFF;
            sector[BLOCK_HEADER - 1] = 0xFF;
            sector[DATA - 2] = 0xFF;
            sector[DATA - 1] = 0xFF;
            // What a QL's FORMAT leaves in a block, and in the gap.
            let (block, gap) = sector.split_at_mut(GAP);
            for (n, b) in block[DATA..DATA_CHECK].iter_mut().chain(gap.iter_mut()).enumerate() {
                *b = if n % 2 == 0 { 0xAA } else { 0x55 };
            }
        }
        let mut cartridge = Self::open(bytes).expect("a blank cartridge has its sectors");
        let mut map = [0u8; 512];
        for sector in 0..256 {
            map[sector * 2] = FREE_FILE;
        }
        map[0] = MAP_FILE;
        map[254 * 2] = BAD_FILE;
        cartridge.write_block(0, MAP_BLOCK_FILE, 0, &map);
        for sector in 1..SECTORS {
            cartridge.write_block(sector, FREE_FILE, 0, &[0xAA, 0x55].repeat(256));
        }
        // The directory's first block, holding only its own header, where FORMAT puts it.
        let mut directory = [0u8; 512];
        directory[..HEADER_LEN].copy_from_slice(&Header { length: HEADER_LEN as u32, ..Header::default() }.to_bytes());
        cartridge.write_block(245, 0, 0, &directory);
        cartridge.set_map(245, 0, 0);
        cartridge.bytes
    }

    /// The cartridge's name, as every sector's header has it.
    pub fn name(&self) -> String {
        self.at[0].map(|at| String::from_utf8_lossy(&self.bytes[at + HEADER + 2..at + HEADER + 12]).trim_end().to_string()).unwrap_or_default()
    }

    fn data(&self, sector: usize) -> &[u8] {
        let at = self.at[sector].unwrap_or(0);
        &self.bytes[at + DATA..at + DATA_CHECK]
    }

    fn map(&self) -> &[u8] {
        self.data(0)
    }

    /// What the map says a sector holds: its file and block.
    pub fn map_entry(&self, sector: usize) -> (u8, u8) {
        let map = self.map();
        (map[sector * 2], map[sector * 2 + 1])
    }

    fn set_map(&mut self, sector: usize, file: u8, block: u8) {
        let mut map = self.map().to_vec();
        map[sector * 2] = file;
        map[sector * 2 + 1] = block;
        if file != FREE_FILE {
            map[510..512].copy_from_slice(&((sector * 2) as u16).to_be_bytes());
        }
        self.write_block(0, MAP_BLOCK_FILE, 0, &map);
    }

    /// Writes a sector's block: its header (file and block) and its 512 bytes, with their checksums.
    fn write_block(&mut self, sector: usize, file: u8, block: u8, data: &[u8]) {
        let Some(at) = self.at[sector] else {
            return;
        };
        let s = &mut self.bytes[at..at + SECTOR_LEN];
        s[BLOCK_HEADER] = file;
        s[BLOCK_HEADER + 1] = block;
        let sum = checksum(&s[BLOCK_HEADER..BLOCK_HEADER + 2]);
        s[BLOCK_HEADER + 2..BLOCK_HEADER + 4].copy_from_slice(&sum);
        s[DATA..DATA + data.len().min(512)].copy_from_slice(&data[..data.len().min(512)]);
        let sum = checksum(&s[DATA..DATA_CHECK]);
        s[DATA_CHECK..GAP].copy_from_slice(&sum);
    }

    /// The sectors holding a file's blocks, in order, as their block headers say.
    fn chain(&self, file: usize) -> Vec<usize> {
        let mut blocks: Vec<(u8, usize)> = (0..256)
            .filter(|s| self.at[*s].is_some())
            .filter_map(|s| {
                let at = self.at[s]?;
                let sector = &self.bytes[at..at + SECTOR_LEN];
                (sector[BLOCK_HEADER - 2..BLOCK_HEADER] == [0xFF, 0xFF] && usize::from(sector[BLOCK_HEADER]) == file && s != 0).then_some((sector[BLOCK_HEADER + 1], s))
            })
            .collect();
        blocks.sort();
        blocks.dedup_by_key(|b| b.0);
        // Only the blocks from 0 on, with none missing.
        blocks.iter().enumerate().take_while(|(n, (block, _))| usize::from(*block) == *n).map(|(_, (_, s))| *s).collect()
    }

    fn file_bytes(&self, file: usize) -> Vec<u8> {
        self.chain(file).into_iter().flat_map(|s| self.data(s).to_vec()).collect()
    }

    fn directory(&self) -> Vec<u8> {
        let bytes = self.file_bytes(0);
        let length = Header::read(&bytes).map_or(0, |h| h.length as usize);
        bytes[..length.min(bytes.len())].to_vec()
    }

    /// The files in the directory, by number.
    pub fn files(&self) -> Vec<File> {
        let directory = self.directory();
        let mut out = Vec::new();
        for (number, entry) in directory.as_chunks::<HEADER_LEN>().0.iter().enumerate().skip(1) {
            let Some(listed) = Header::read(entry) else { continue };
            if listed.name.is_empty() || listed.length == 0 {
                continue;
            }
            let bytes = self.file_bytes(number);
            // The header in the file's first block has its type and data space where the directory may not.
            let header = match Header::read(&bytes) {
                Some(own) if own.length == listed.length => Header { name: listed.name.clone(), ..own },
                _ => listed.clone(),
            };
            let content = bytes.get(HEADER_LEN..header.length as usize).unwrap_or(bytes.get(HEADER_LEN..).unwrap_or_default()).to_vec();
            out.push(File { number, header, bytes: content });
        }
        out
    }

    pub fn find(&self, name: &str) -> Option<File> {
        self.files().into_iter().find(|f| f.header.name.eq_ignore_ascii_case(name))
    }

    /// The sectors free for files.
    pub fn free_sectors(&self) -> Vec<usize> {
        (1..SECTORS).filter(|s| self.at[*s].is_some() && self.map_entry(*s).0 == FREE_FILE).collect()
    }

    /// The free sectors in the order they're given out: as QDOS does, some way along the tape from the last given
    /// out (the sectors pass in falling order), so the QL has time to write each before the next comes by.
    fn free_order(&self) -> Vec<usize> {
        let map = self.map();
        let last = usize::from(u16::from_be_bytes([map[510], map[511]])) / 2;
        let free = self.free_sectors();
        let start = (last + SECTORS * 2 - 13) % SECTORS;
        let mut order: Vec<usize> = (0..SECTORS).map(|i| (start + SECTORS - i) % SECTORS).filter(|s| free.contains(s)).collect();
        order.dedup();
        order
    }

    /// Writes a file's bytes into its blocks, giving out free sectors for blocks it hasn't yet.
    fn write_file(&mut self, file: usize, bytes: &[u8]) -> Result<(), String> {
        let mut chain = self.chain(file);
        let needed = bytes.len().div_ceil(512).max(1);
        if needed > chain.len() {
            let free = self.free_order();
            if needed - chain.len() > free.len() {
                return Err(crate::i18n::tf!("That needs {} more sectors, and the cartridge has {} free.", needed - chain.len(), free.len()));
            }
            chain.extend(free.into_iter().take(needed - chain.len()));
        }
        for (block, sector) in chain.iter().enumerate() {
            if block >= needed {
                self.write_block(*sector, FREE_FILE, 0, &[0; 512]);
                self.set_map(*sector, FREE_FILE, 0);
                continue;
            }
            let mut data = [0u8; 512];
            let part = &bytes[(block * 512).min(bytes.len())..((block + 1) * 512).min(bytes.len())];
            data[..part.len()].copy_from_slice(part);
            self.write_block(*sector, file as u8, block as u8, &data);
            self.set_map(*sector, file as u8, block as u8);
        }
        Ok(())
    }

    /// Puts a file on the cartridge, with a header saying what it is, in a directory slot left by a deleted file or
    /// a new one at the end.
    pub fn add(&mut self, name: &str, kind: u8, data_space: u32, content: &[u8]) -> Result<(), String> {
        if self.find(name).is_some() {
            return Err(super::write::already(name));
        }
        let mut directory = self.directory();
        let number = directory.as_chunks::<HEADER_LEN>().0.iter().enumerate().skip(1).find(|(_, e)| Header::read(e.as_slice()).is_some_and(|h| h.name.is_empty() || h.length == 0)).map_or(directory.len() / HEADER_LEN, |(n, _)| n);
        let number = number.max(1);
        if number > MOST_FILES {
            return Err(crate::i18n::tf!("The directory is full: a cartridge holds {MOST_FILES} files."));
        }
        let header = Header::new(name, content.len(), kind, data_space);
        let mut file = header.to_bytes().to_vec();
        file.extend_from_slice(content);
        // Room for the file, and for the directory if it grows by a block.
        let grows = (number + 1) * HEADER_LEN > directory.len().div_ceil(512).max(1) * 512;
        let needed = file.len().div_ceil(512) + usize::from(grows);
        let free = self.free_sectors().len();
        if needed > free {
            return Err(crate::i18n::tf!("{name} needs {needed} sectors, and the cartridge has {free} free."));
        }
        self.write_file(number, &file)?;
        if directory.len() < (number + 1) * HEADER_LEN {
            directory.resize((number + 1) * HEADER_LEN, 0);
        }
        directory[number * HEADER_LEN..(number + 1) * HEADER_LEN].copy_from_slice(&header.to_bytes());
        self.write_directory(directory)
    }

    fn write_directory(&mut self, mut directory: Vec<u8>) -> Result<(), String> {
        let own = Header { length: directory.len() as u32, ..Header::default() };
        directory[..HEADER_LEN].copy_from_slice(&own.to_bytes());
        self.write_file(0, &directory)
    }

    /// Takes a file off: its sectors freed in the map and its directory entry cleared, as QDOS's DELETE leaves it.
    pub fn delete(&mut self, name: &str) -> Result<(), String> {
        let file = self.find(name).ok_or_else(|| crate::i18n::tf!("{name} isn't on the cartridge."))?;
        for sector in self.chain(file.number) {
            self.write_block(sector, FREE_FILE, 0, &[0; 512]);
            self.set_map(sector, FREE_FILE, 0);
        }
        let mut directory = self.directory();
        directory[file.number * HEADER_LEN..(file.number + 1) * HEADER_LEN].fill(0);
        self.write_directory(directory)
    }

    /// Gives a file a new name, in its directory entry and in the header in its first block.
    pub fn rename(&mut self, name: &str, new: &str) -> Result<(), String> {
        let file = self.find(name).ok_or_else(|| crate::i18n::tf!("{name} isn't on the cartridge."))?;
        if self.find(new).is_some_and(|other| other.number != file.number) {
            return Err(super::write::already(new));
        }
        let header = Header { name: new.chars().take(36).collect(), ..file.header };
        let mut directory = self.directory();
        directory[file.number * HEADER_LEN..(file.number + 1) * HEADER_LEN].copy_from_slice(&header.to_bytes());
        let mut bytes = header.to_bytes().to_vec();
        bytes.extend_from_slice(&file.bytes);
        self.write_file(file.number, &bytes)?;
        self.write_directory(directory)
    }
}

/// What a file of this type is, in the browser's Type column.
pub fn kind_name(kind: u8) -> &'static str {
    match kind {
        0 => "Data",
        1 => "Program",
        2 => "Relocatable object",
        255 => "Directory",
        _ => "Other",
    }
}

/// The name to open a file under: a QL name's last part after an underscore is its ending (boot_bas is BASIC).
fn open_name(name: &str) -> String {
    let tidy = tidy_name(name);
    match tidy.rsplit_once('_') {
        Some((stem, ending)) if !stem.is_empty() && matches!(ending.to_ascii_lowercase().as_str(), "bas" | "asm" | "s" | "txt" | "doc" | "c" | "h" | "scr" | "pic") => format!("{stem}.{ending}"),
        _ if tidy.eq_ignore_ascii_case("boot") => format!("{tidy}.bas"),
        _ => tidy,
    }
}

/// A cartridge, for the image browser.
pub struct Microdrive {
    name: String,
    files: Vec<File>,
    free: usize,
}

impl Microdrive {
    pub fn open(source: Arc<dyn Source>) -> Result<Self, String> {
        let cartridge = Cartridge::open(source.read_vec(0, source.len().min(IMAGE_LEN as u64) as usize)?)?;
        Ok(Self { name: cartridge.name(), files: cartridge.files(), free: cartridge.free_sectors().len() })
    }
}

impl Volume for Microdrive {
    fn describe(&self) -> String {
        crate::i18n::tf!("Sinclair QL microdrive cartridge (MDV) \u{B7} {} \u{B7} {} sectors free", if self.name.is_empty() { "no name" } else { &self.name }, self.free)
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
                let mut entry = Entry::file(tidy_name(&file.header.name), file.bytes.len() as u64, index as u64);
                entry.kind = crate::i18n::tr(kind_name(file.header.kind)).to_string();
                if file.header.kind == crate::ql::EXECUTABLE {
                    entry.detail = match crate::ql::job_name(&file.bytes) {
                        Some(job) => crate::i18n::tf!("data space {} bytes, the job {job}", file.header.data_space),
                        None => crate::i18n::tf!("data space {} bytes", file.header.data_space),
                    };
                }
                entry.open_name = open_name(&file.header.name);
                entry
            })
            .collect())
    }

    /// A file's bytes; a program's with the XTcc trailer after them, so its type and data space go with it, as QL
    /// emulators read them from a file.
    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String> {
        let file = self.files.get(entry.location as usize).ok_or(crate::i18n::tr("That file isn't on the cartridge."))?;
        if file.header.kind == crate::ql::EXECUTABLE {
            return Ok(crate::ql::with_xtcc(&file.bytes, file.header.data_space));
        }
        Ok(file.bytes.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A blank cartridge is as a QL's FORMAT leaves one: every sector's header and checksums right, the map with
    /// itself and the directory in it, and 252 sectors free.
    #[test]
    fn a_blank_cartridge_is_formatted_as_a_ql_formats_one() {
        let bytes = Cartridge::blank("Limoniter", [0x81, 0x30]);
        assert_eq!(bytes.len(), 174_930);
        assert!(looks_like(&bytes, bytes.len() as u64));
        // The first sector in the image is 254, and they fall from there.
        assert_eq!(&bytes[..12], &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF]);
        assert_eq!(&bytes[12..26], &[0xFF, 254, b'L', b'i', b'm', b'o', b'n', b'i', b't', b'e', b'r', b' ', 0x81, 0x30]);
        assert_eq!(bytes[26..28], checksum(&bytes[12..26]));
        assert_eq!(bytes[SECTOR_LEN + 13], 253);
        let cartridge = Cartridge::open(bytes).unwrap();
        assert_eq!(cartridge.name(), "Limoniter");
        assert_eq!(cartridge.map_entry(0), (MAP_FILE, 0));
        assert_eq!(cartridge.map_entry(245), (0, 0), "the directory");
        assert_eq!(cartridge.map_entry(254), (BAD_FILE, 0));
        assert_eq!(cartridge.free_sectors().len(), 252);
        assert!(cartridge.files().is_empty());
        for place in 0..SECTORS {
            let s = &cartridge.bytes[place * SECTOR_LEN..(place + 1) * SECTOR_LEN];
            assert_eq!(s[26..28], checksum(&s[12..26]), "sector header at {place}");
            assert_eq!(s[42..44], checksum(&s[40..42]), "block header at {place}");
            assert_eq!(s[564..566], checksum(&s[52..564]), "block at {place}");
        }
        // The map's block header is file $80, as a QL writes it.
        let zero = 254 * SECTOR_LEN;
        assert_eq!((cartridge.bytes[zero + 13], cartridge.bytes[zero + 40]), (0, 0x80));
    }

    /// The checksum against a sector header from a cartridge a QL formatted, named "fmt": FF 00 66 6D 74 20 ... 81 30
    /// sums to $12E6, kept as E6 12.
    #[test]
    fn the_checksum_is_the_microdrive_s() {
        let header = [0xFF, 0x00, b'f', b'm', b't', b' ', b' ', b' ', b' ', b' ', b' ', b' ', 0x81, 0x30];
        assert_eq!(checksum(&header), [0xE6, 0x12]);
        assert_eq!(checksum(&[0xFD, 0x00]), [0x0C, 0x10]);
    }

    /// Files go on, come back the same, are renamed and taken off; a program keeps its type and data space; a file
    /// bigger than a block spans several; the directory grows past one block.
    #[test]
    fn files_go_on_a_cartridge_and_come_back() {
        let mut cartridge = Cartridge::open(Cartridge::blank("test", [1, 2])).unwrap();
        let code: Vec<u8> = (0..1500u32).map(|i| (i * 7) as u8).collect();
        cartridge.add("hello_exe", crate::ql::EXECUTABLE, 4096, &code).unwrap();
        cartridge.add("BOOT", 0, 0, b"10 EXEC mdv1_hello_exe\n").unwrap();
        for n in 0..10 {
            cartridge.add(&format!("file{n}"), 0, 0, format!("number {n}").as_bytes()).unwrap();
        }
        assert!(cartridge.add("boot", 0, 0, b"again").unwrap_err().starts_with(super::super::write::ALREADY), "names are the same whatever their capitals");
        let reopened = Cartridge::open(cartridge.bytes.clone()).unwrap();
        let files = reopened.files();
        assert_eq!(files.len(), 12);
        let program = reopened.find("hello_exe").unwrap();
        assert_eq!((program.header.kind, program.header.data_space, program.header.length), (1, 4096, 1500 + 64));
        assert_eq!(program.bytes, code);
        assert_eq!(reopened.find("boot").unwrap().bytes, b"10 EXEC mdv1_hello_exe\n");
        assert_eq!(reopened.find("file9").unwrap().number, 12, "the directory's second block");
        // 1564 bytes take four sectors; twelve files and the directory's two blocks.
        assert_eq!(reopened.free_sectors().len(), 252 - 4 - 11 - 1);
        // Every block a file has is in the map under it.
        for file in &files {
            for (block, sector) in reopened.chain(file.number).iter().enumerate() {
                assert_eq!(reopened.map_entry(*sector), (file.number as u8, block as u8));
            }
        }
        cartridge.rename("file3", "renamed").unwrap();
        cartridge.delete("hello_exe").unwrap();
        let reopened = Cartridge::open(cartridge.bytes.clone()).unwrap();
        assert!(reopened.find("hello_exe").is_none() && reopened.find("file3").is_none());
        assert_eq!(reopened.find("renamed").unwrap().bytes, b"number 3");
        assert_eq!(reopened.free_sectors().len(), 252 - 11 - 1);
        // A new file takes the slot the deleted one left.
        cartridge.add("again", 0, 0, b"x").unwrap();
        assert_eq!(Cartridge::open(cartridge.bytes.clone()).unwrap().find("again").unwrap().number, 1);
    }

    #[test]
    fn a_full_cartridge_says_so() {
        let mut cartridge = Cartridge::open(Cartridge::blank("full", [0, 0])).unwrap();
        let big = vec![0x42; 300 * 512];
        assert!(cartridge.add("big", 0, 0, &big).unwrap_err().contains("252 free"));
        let fits = vec![0x42; 252 * 512 - 64];
        cartridge.add("fits", 0, 0, &fits).unwrap();
        assert_eq!(cartridge.find("fits").unwrap().bytes, fits);
        assert!(cartridge.add("more", 0, 0, b"no room").is_err());
    }

    #[test]
    fn the_browser_lists_a_cartridge() {
        let mut cartridge = Cartridge::open(Cartridge::blank("games", [9, 9])).unwrap();
        cartridge.add("game_exe", 1, 2048, &[0x4E, 0x75]).unwrap();
        cartridge.add("game_bas", 0, 0, b"10 PRINT 1\n").unwrap();
        cartridge.add("BOOT", 0, 0, b"10 LRUN mdv1_game_bas\n").unwrap();
        let volume = super::super::open(Arc::new(cartridge.bytes.clone()), "games.mdv").unwrap();
        assert!(volume.describe().contains("QL microdrive") && volume.describe().contains("games"), "{}", volume.describe());
        let entries = volume.list(None).unwrap();
        let names: Vec<(&str, &str, &str)> = entries.iter().map(|e| (e.name.as_str(), e.open_name.as_str(), e.kind.as_str())).collect();
        assert_eq!(names, [("game_exe", "game_exe", "Program"), ("game_bas", "game.bas", "Data"), ("BOOT", "BOOT.bas", "Data")]);
        assert_eq!(entries[0].detail, "data space 2048 bytes");
        // A program comes out with its header kept after it.
        assert_eq!(volume.read(&entries[0]).unwrap(), crate::ql::with_xtcc(&[0x4E, 0x75], 2048));
        assert_eq!(volume.read(&entries[1]).unwrap(), b"10 PRINT 1\n");
    }
}
