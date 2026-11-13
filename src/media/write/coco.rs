//! Color Computer (and Dragon, under its Disk BASIC for the CoCo) RS-DOS disks in JVC images, as Disk Extended
//! Color BASIC writes them: the disk in granules of nine sectors, half a track each, with track 17 kept for the
//! granule table (sector 2) and the directory (sectors 3 to 11). A file's directory entry gives its 8.3 name, its
//! type, whether it's ASCII, its first granule and the bytes used in its last sector; the table chains its granules,
//! the last marked &C0 plus the sectors it uses. A deleted entry's first byte is 0, as BASIC's KILL leaves it.

use super::{Addition, Entry, ImageWriter, Kind, already};

const BASIC: &str = "BASIC program";
const BASIC_ASCII: &str = "BASIC program (ASCII)";
const DATA: &str = "BASIC data";
const CODE: &str = "Machine code";
const TEXT: &str = "Text";

pub struct Disk {
    bytes: Vec<u8>,
    header: usize,
    granules: usize,
}

/// A blank 35-track disk, as DSKINI leaves one: every sector &FF, the directory track's first sector zeros.
pub fn blank() -> Vec<u8> {
    let mut bytes = vec![0xFF; 35 * 18 * 256];
    let at = |sector: usize| (17 * 18 + sector - 1) * 256;
    bytes[at(1)..at(1) + 256].fill(0);
    bytes[at(2) + 68..at(2) + 256].fill(0);
    bytes
}

impl Disk {
    pub fn open(bytes: Vec<u8>, ending: &str) -> Result<Self, String> {
        if ending == "vdk" || bytes.starts_with(b"dk") {
            return Err("Only JVC images of RS-DOS disks are changed here, not VDK.".into());
        }
        let header = bytes.len() % 256;
        if header >= 1 && bytes[0] != 18 || header >= 2 && bytes[1] != 1 {
            return Err("Only single-sided RS-DOS disks of 18 sectors a track are changed here.".into());
        }
        let tracks = (bytes.len() - header) / (18 * 256);
        if !(35..=80).contains(&tracks) || !(bytes.len() - header).is_multiple_of(18 * 256) {
            return Err("This disk image isn't a whole number of 18-sector tracks.".into());
        }
        // The reader decides what's on the disk: only RS-DOS is written.
        let volume = super::super::coco::Coco::open(std::sync::Arc::new(bytes.clone()), ending)?;
        if !super::super::Volume::describe(&volume).contains("RS-DOS") {
            return Err("This disk isn't RS-DOS (it's Dragon DOS or OS-9), so it's only read.".into());
        }
        Ok(Self { bytes, header, granules: ((tracks - 1) * 2).min(256 - 64) })
    }

    fn at(&self, track: usize, sector: usize) -> usize {
        self.header + (track * 18 + sector - 1) * 256
    }

    fn table(&self) -> usize {
        self.at(17, 2)
    }

    fn next(&self, granule: usize) -> u8 {
        self.bytes[self.table() + granule]
    }

    fn set_next(&mut self, granule: usize, value: u8) {
        let at = self.table() + granule;
        self.bytes[at] = value;
    }

    /// Where a granule's first sector is: half a track, the directory's track 17 skipped.
    fn granule_at(&self, granule: usize) -> usize {
        let track = granule / 2 + usize::from(granule >= 34);
        self.at(track, if granule.is_multiple_of(2) { 1 } else { 10 })
    }

    fn free_granules(&self) -> usize {
        (0..self.granules).filter(|g| self.next(*g) == 0xFF).count()
    }

    /// Free granules in the order Disk BASIC takes them: nearest the directory track first, outwards.
    fn free_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.granules).collect();
        order.sort_by_key(|g| (if *g >= 34 { *g - 34 } else { 33 - *g }) * 2 + usize::from(*g < 34));
        order.into_iter().filter(|g| self.next(*g) == 0xFF).collect()
    }

    /// The directory's 72 slots, where each is.
    fn slots(&self) -> Vec<usize> {
        (0..72).map(|n| self.at(17, 3 + n / 8) + (n % 8) * 32).collect()
    }

    fn name(&self, slot: usize) -> String {
        let text = |b: &[u8]| b.iter().map(|&c| char::from(c & 0x7F)).collect::<String>().trim_end().to_string();
        let (stem, ext) = (text(&self.bytes[slot..slot + 8]), text(&self.bytes[slot + 8..slot + 11]));
        if ext.is_empty() { stem } else { format!("{stem}.{ext}") }
    }

    /// The slots holding files, up to the directory's end.
    fn files(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for slot in self.slots() {
            match self.bytes[slot] {
                0xFF => break,
                0 => {}
                _ => out.push(slot),
            }
        }
        out
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.files().into_iter().find(|slot| self.name(*slot).eq_ignore_ascii_case(name))
    }

    fn remove(&mut self, slot: usize) {
        let mut granule = usize::from(self.bytes[slot + 13]);
        for _ in 0..self.granules {
            if granule >= self.granules {
                break;
            }
            let next = self.next(granule);
            self.set_next(granule, 0xFF);
            if next >= 0xC0 {
                break;
            }
            granule = usize::from(next);
        }
        self.bytes[slot] = 0;
    }
}

/// An 8.3 name as Disk BASIC keeps it, in capitals, padded with spaces.
fn dos_name(name: &str) -> Result<[u8; 11], String> {
    let (stem, ending) = super::stem_and_ending(name.trim());
    let part = |text: &str, most: usize| -> String { text.chars().map(|c| c.to_ascii_uppercase()).filter(|c| c.is_ascii_graphic() && !"./:\"".contains(*c)).take(most).collect() };
    let (stem, ending) = (part(stem, 8), part(ending, 3));
    if stem.is_empty() {
        return Err(format!("\"{name}\" has no characters an RS-DOS name can hold."));
    }
    let mut out = [b' '; 11];
    out[..stem.len()].copy_from_slice(stem.as_bytes());
    out[8..8 + ending.len()].copy_from_slice(ending.as_bytes());
    Ok(out)
}

fn shown(name: &[u8; 11]) -> String {
    let text = |b: &[u8]| String::from_utf8_lossy(b).trim_end().to_string();
    let (stem, ext) = (text(&name[..8]), text(&name[8..]));
    if ext.is_empty() { stem } else { format!("{stem}.{ext}") }
}

/// Whether bytes are already a Disk BASIC machine-code file: blocks of 0, a length and an address, then &FF, 0, 0
/// and where it runs.
fn is_loadm(bytes: &[u8]) -> bool {
    let mut at = 0;
    while at + 5 <= bytes.len() {
        match bytes[at] {
            0x00 => at += 5 + usize::from(u16::from_be_bytes([bytes[at + 1], bytes[at + 2]])),
            0xFF => return at + 5 == bytes.len() && bytes[at + 1..at + 3] == [0, 0],
            _ => return false,
        }
    }
    false
}

impl ImageWriter for Disk {
    fn kinds(&self) -> Vec<Kind> {
        vec![
            Kind::plain(BASIC, "A tokenised BASIC program, as SAVE writes one."),
            Kind::plain(BASIC_ASCII, "A BASIC listing as text, as SAVE \"name\",A writes one; it loads like any program."),
            Kind::plain(DATA, "A file BASIC's programs OPEN and read, kept as it is."),
            Kind { name: CODE, about: "A machine-code program for LOADM. Plain bytes are put in a block to load at the address.", address: true, autostart: false },
            Kind::plain(TEXT, "Text, such as assembler source, marked as ASCII."),
        ]
    }

    fn guess(&self, name: &str, bytes: &[u8]) -> Addition {
        let (_, ending) = super::stem_and_ending(name);
        let text = !bytes.is_empty() && bytes.iter().all(|b| b.is_ascii_graphic() || matches!(b, b' ' | b'\r' | b'\n' | b'\t'));
        let (kind, address) = match ending.to_lowercase().as_str() {
            "bas" if text => (BASIC_ASCII, None),
            "bas" => (BASIC, None),
            "bin" | "rom" | "ccc" if is_loadm(bytes) => (CODE, None),
            "bin" | "rom" | "ccc" => (CODE, Some(0x0E00)),
            "dat" => (DATA, None),
            _ if text => (TEXT, None),
            _ => (DATA, None),
        };
        Addition { name: name.to_string(), bytes: bytes.to_vec(), kind: kind.into(), address, ..Default::default() }
    }

    fn add(&mut self, _folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        let name = dos_name(&file.name)?;
        if let Some(slot) = self.find(&shown(&name)) {
            if !file.replace {
                return Err(already(&shown(&name)));
            }
            self.remove(slot);
        }
        let (kind, ascii) = match file.kind.as_str() {
            BASIC => (0u8, 0u8),
            BASIC_ASCII => (0, 0xFF),
            DATA => (1, 0),
            CODE => (2, 0),
            _ => (3, 0xFF),
        };
        // Plain bytes for LOADM are put in one block, loading and running at the address given.
        let mut bytes = file.bytes.clone();
        if kind == 2 && !is_loadm(&bytes) {
            let address = file.address.unwrap_or(0x0E00);
            if bytes.len() > 0xFFFF {
                return Err("A machine-code block can't be 64K or more.".into());
            }
            let mut wrapped = vec![0x00];
            wrapped.extend((bytes.len() as u16).to_be_bytes());
            wrapped.extend(address.to_be_bytes());
            wrapped.extend(&bytes);
            wrapped.extend([0xFF, 0, 0]);
            wrapped.extend(address.to_be_bytes());
            bytes = wrapped;
        }
        if kind == 0 && ascii == 0xFF || kind == 3 {
            // Disk BASIC's lines end with a carriage return alone.
            let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\r").replace('\n', "\r");
            bytes = text.into_bytes();
        }
        let sectors = bytes.len().div_ceil(256);
        let needed = sectors.div_ceil(9).max(1);
        let free = self.free_order();
        if needed > free.len() {
            return Err(format!("{} needs {needed} granules; the disk has {} free.", file.name, free.len()));
        }
        let slot = self.slots().into_iter().find(|s| matches!(self.bytes[*s], 0 | 0xFF)).ok_or("The directory is full: RS-DOS holds 72 files.")?;
        let chain = &free[..needed];
        for (i, granule) in chain.iter().enumerate() {
            let at = self.granule_at(*granule);
            let part = &bytes[(i * 2304).min(bytes.len())..((i + 1) * 2304).min(bytes.len())];
            self.bytes[at..at + 2304].fill(0xFF);
            self.bytes[at..at + part.len()].copy_from_slice(part);
            let value = match chain.get(i + 1) {
                Some(next) => *next as u8,
                None => 0xC0 | part.len().div_ceil(256) as u8,
            };
            self.set_next(*granule, value);
        }
        let last = if bytes.is_empty() { 0 } else { bytes.len() - (sectors - 1) * 256 };
        let entry = &mut self.bytes[slot..slot + 32];
        entry.fill(0);
        entry[..11].copy_from_slice(&name);
        entry[11] = kind;
        entry[12] = ascii;
        entry[13] = chain[0] as u8;
        entry[14..16].copy_from_slice(&(last as u16).to_be_bytes());
        Ok(())
    }

    fn delete(&mut self, _folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        let slot = self.find(&entry.name).ok_or_else(|| format!("{} isn't on the disk.", entry.name))?;
        self.remove(slot);
        Ok(())
    }

    fn rename(&mut self, _folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        let slot = self.find(&entry.name).ok_or_else(|| format!("{} isn't on the disk.", entry.name))?;
        let new = dos_name(name)?;
        if self.find(&shown(&new)).is_some_and(|other| other != slot) {
            return Err(already(&shown(&new)));
        }
        self.bytes[slot..slot + 11].copy_from_slice(&new);
        Ok(())
    }

    fn free(&self) -> Option<u64> {
        Some(self.free_granules() as u64 * 2304)
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

    fn add(disk: &mut Disk, name: &str, bytes: &[u8]) {
        let addition = disk.guess(name, bytes);
        disk.add(None, &addition).unwrap();
    }

    #[test]
    fn files_go_on_and_come_off_an_rsdos_disk() {
        let mut disk = Disk::open(blank(), "dsk").unwrap();
        assert_eq!(disk.free_granules(), 68);
        let big: Vec<u8> = (0..30_000u32).map(|i| (i * 9) as u8).collect();
        add(&mut disk, "hello.bas", b"10 PRINT \"HI\"\n20 GOTO 10\n");
        add(&mut disk, "game.bin", &[0x86, 0x41, 0x39]);
        add(&mut disk, "big.dat", &big);
        let volume = reopened(&disk, "t.dsk");
        assert_eq!(names(&*volume, None), ["HELLO.BAS", "GAME.BIN", "BIG.DAT"]);
        assert_eq!(volume.read(&entry(&*volume, None, "HELLO.BAS")).unwrap(), b"10 PRINT \"HI\"\r20 GOTO 10\r");
        assert_eq!(volume.read(&entry(&*volume, None, "GAME.BIN")).unwrap(), [0, 0, 3, 0x0E, 0, 0x86, 0x41, 0x39, 0xFF, 0, 0, 0x0E, 0]);
        assert_eq!(volume.read(&entry(&*volume, None, "BIG.DAT")).unwrap(), big);
        disk.rename(None, &entry(&*volume, None, "GAME.BIN"), "start.bin").unwrap();
        disk.delete(None, &entry(&*volume, None, "BIG.DAT")).unwrap();
        let volume = reopened(&disk, "t.dsk");
        assert_eq!(names(&*volume, None), ["HELLO.BAS", "START.BIN"]);
        assert_eq!(disk.free_granules(), 66);
    }

    /// ToolShed's decb lists and copies out what's written, as Disk BASIC would see it.
    #[test]
    fn decb_reads_a_written_disk() {
        let decb = std::path::Path::new("/root/reftools/bin/decb");
        if !decb.is_file() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("limoniter-coco-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut disk = Disk::open(blank(), "dsk").unwrap();
        let big: Vec<u8> = (0..40_000u32).map(|i| (i * 3) as u8).collect();
        add(&mut disk, "hello.bas", b"10 PRINT \"HI\"\n");
        add(&mut disk, "big.dat", &big);
        let path = dir.join("t.dsk");
        std::fs::write(&path, disk.bytes()).unwrap();
        let listed = Command::new(decb).arg("dir").arg(&path).output().unwrap();
        let listed = String::from_utf8_lossy(&listed.stdout);
        assert!(listed.contains("HELLO    BAS  0  A") && listed.contains("BIG      DAT  1  B"), "{listed}");
        let out = dir.join("big.out");
        let copied = Command::new(decb).arg("copy").arg(format!("{},BIG.DAT", path.display())).arg(&out).output().unwrap();
        assert!(copied.status.success(), "{}", String::from_utf8_lossy(&copied.stderr));
        assert_eq!(std::fs::read(&out).unwrap(), big);
        let free = Command::new(decb).arg("free").arg(&path).output().unwrap();
        let free = String::from_utf8_lossy(&free.stdout);
        assert!(free.contains(&format!("Free granules: {} ", disk.free_granules())), "{free}");
        std::fs::remove_dir_all(dir).ok();
    }
}
