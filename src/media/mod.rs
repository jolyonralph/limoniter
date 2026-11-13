//! Disk, tape and CD images and archives, read as the file systems they hold, so their files can be browsed and
//! opened. Nothing here writes: an image is only ever read, and a damaged or unexpected one gives an error rather
//! than a wrong file. Each format has its own module; this one finds which format an image is in.
//!
//! Large images aren't read into memory: a `Source` reads the parts it's asked for, from the file on disk.

mod acorn;
mod apple2;
mod atari8;
mod amiga;
mod cbm;
mod coco;
pub mod cpm;
mod cpm_formats;
mod fat;
mod iso;
mod lha;
mod lif;
mod mac;
mod rp9;
mod sectors;
mod trsdos;
pub(crate) mod spectrum;
mod zip;

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// The most a file taken out of an image may be, as it's opened in memory.
pub const MOST_FILE_BYTES: u64 = 1024 * 1024 * 1024;

/// Bytes that can be read at any offset: a file on disk, or bytes already in memory.
pub trait Source: Send + Sync {
    fn len(&self) -> u64;
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String>;

    fn read_vec(&self, offset: u64, length: usize) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; length];
        self.read_at(offset, &mut buf)?;
        Ok(buf)
    }
}

impl Source for Vec<u8> {
    fn len(&self) -> u64 {
        self.as_slice().len() as u64
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        let start = usize::try_from(offset).map_err(|_| "past the end of the image".to_string())?;
        let bytes = self.get(start..start + buf.len()).ok_or("past the end of the image")?;
        buf.copy_from_slice(bytes);
        Ok(())
    }
}

/// An image read from its file, a part at a time.
pub struct FileSource {
    file: Mutex<std::fs::File>,
    len: u64,
}

impl FileSource {
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        Ok(Self { file: Mutex::new(file), len })
    }
}

impl Source for FileSource {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        if offset + buf.len() as u64 > self.len {
            return Err("past the end of the image".into());
        }
        let mut file = self.file.lock().map_err(|_| "the image couldn't be read".to_string())?;
        file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
        file.read_exact(buf).map_err(|e| e.to_string())
    }
}

/// Part of another source, such as a partition of a hard disk image.
pub struct Slice {
    pub inner: Arc<dyn Source>,
    pub start: u64,
    pub len: u64,
}

impl Source for Slice {
    fn len(&self) -> u64 {
        self.len
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        if offset + buf.len() as u64 > self.len {
            return Err("past the end of the partition".into());
        }
        self.inner.read_at(self.start + offset, buf)
    }
}

/// A file or folder in an image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// As the image names it, made readable.
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    /// The kind of file, in the image's own terms (PRG, SEQ, a DOS attribute...), for the browser's Type column.
    pub kind: String,
    /// Anything else worth showing: a date, attributes, a load address.
    pub detail: String,
    /// The name to open it under, with an ending that says what it is, so previews and listings find it.
    pub open_name: String,
    /// Where the format keeps it: a sector, a cluster, an index. Each format reads its own.
    pub location: u64,
    /// A second number a format may need, such as a directory's length.
    pub extra: u64,
}

impl Entry {
    pub fn file(name: String, size: u64, location: u64) -> Self {
        Self { open_name: name.clone(), name, size, is_dir: false, kind: String::new(), detail: String::new(), location, extra: 0 }
    }

    pub fn dir(name: String, location: u64, extra: u64) -> Self {
        Self { open_name: name.clone(), name, size: 0, is_dir: true, kind: "Folder".into(), detail: String::new(), location, extra }
    }
}

/// A file system in an image, read only.
pub trait Volume: Send + Sync {
    /// What the image is, for the browser's heading: its format, and its name or label if it has one.
    fn describe(&self) -> String;
    /// A folder's entries, or the top level's for None. Folders come first, then files, in the image's own order.
    fn list(&self, dir: Option<&Entry>) -> Result<Vec<Entry>, String>;
    /// A file's bytes.
    fn read(&self, entry: &Entry) -> Result<Vec<u8>, String>;
    /// What the reader couldn't follow, so it's never taken for the whole story.
    fn notes(&self) -> Vec<String> {
        Vec::new()
    }
    /// The text preset a file from here is read in when its own ending doesn't say, such as the Amiga's for a
    /// file off an Amiga disk. None leaves it to be worked out from the bytes.
    fn text_preset(&self, _entry: &Entry) -> Option<&'static str> {
        None
    }
    /// More about the image than its heading and notes, for File properties: sections of labelled values, such as
    /// all of a package's manifest. A row with no label is a paragraph of its own.
    fn info(&self) -> Vec<InfoSection> {
        Vec::new()
    }
    /// A file's bytes as they're to be opened to read, rather than saved: a CP/M text file ends at its Ctrl+Z.
    fn reading_bytes(&self, _entry: &Entry, bytes: Vec<u8>) -> Vec<u8> {
        bytes
    }
    /// A picture the image carries of itself, such as a package's screenshot, as a PNG.
    fn picture(&self) -> Option<Vec<u8>> {
        None
    }
}

/// A titled group of labelled values about an image, for File properties.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InfoSection {
    pub title: String,
    pub rows: Vec<(String, String)>,
}

/// The preset Amiga files are read in: the Amiga's own text encoding, with its console's control sequences.
pub const AMIGA_TEXT_PRESET: &str = "amiga-ansi";

/// File endings that say a file is an image or archive this can open.
pub const IMAGE_ENDINGS: [&str; 59] = [
    "iso", "bin", "cue", "img", "ima", "vfd", "st", "msa", "dsk", "d64", "d71", "d81", "t64", "zip", "hdf", "vhd", "trd", "scl", "mdr", "tap", "tzx", "z80", "sna", "adf", "adz", "dms", "lha", "lzh", "ssd", "dsd", "adl", "ads", "adm", "uef", "image", "hfs", "dc42", "toast", "cdr", "imd", "td0", "lif", "hpi", "rp9", "cpm", "do", "po", "2mg", "2img", "hdv", "atr", "xfd", "dmk", "jv1", "jv3", "dis", "vdk", "jvc", "os9",
];

/// Whether a file is an image to browse, going by its ending and, for the less certain endings, its first bytes.
pub fn is_image(name: &str, head: &[u8], len: u64) -> bool {
    let ending = Path::new(name).extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    // Mac disk images were often saved with no ending, or with a name whose last dot isn't one ("6.0.2 System
    // Tools"); Disk Copy's header says what they are, closely enough to believe whatever the name.
    if !IMAGE_ENDINGS.contains(&ending.as_str()) {
        return mac::looks_like_disk_copy(head, len);
    }
    match ending.as_str() {
        // These endings are used for plain data too, so the bytes have to say so.
        "bin" | "img" | "dsk" | "cpm" | "vdk" | "jvc" | "os9" | "dmk" | "jv1" | "jv3" | "dis" | "atr" | "xfd" | "do" | "po" | "2mg" | "2img" | "hdv" | "ima" | "vhd" | "hdf" | "tap" | "trd" | "sna" | "z80" | "adf" | "adz" | "lha" | "lzh" | "uef" | "image" | "hfs" | "dc42" | "toast" | "cdr" | "imd" | "td0" | "lif" | "hpi" => detect(head, len, &ending).is_some(),
        _ => true,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Format {
    Iso,
    RawCd,
    Fat,
    Msa,
    D64,
    D71,
    D81,
    T64,
    Zip,
    Dsk,
    TrDos,
    Scl,
    Microdrive,
    ZxTape,
    Z80,
    Sna,
    Adf,
    Adz,
    Dms,
    AmigaHardDisk,
    Lha,
    Dfs,
    DoubleDfs,
    Adfs,
    Uef,
    MacDiskCopy,
    MacDisk,
    ImageDisk,
    Teledisk,
    Lif,
    /// A plain image of a CP/M disk, whose format is worked out from its directory.
    Cpm,
    /// An Apple II disk: DOS 3.3 or ProDOS, in either sector order, or a 2IMG.
    Apple2,
    /// An Atari 8-bit disk: ATR or XFD, with Atari DOS 2, MyDOS or SpartaDOS.
    Atari8,
    /// A TRS-80 sector image: DMK, JV3 or JV1.
    SectorImage(sectors::Kind),
    /// A Color Computer or Dragon disk: JVC, VDK or OS-9 image.
    Coco,
}

/// Which format an image is in: its signature where it has one, its size for the Commodore disks, its ending last.
fn detect(head: &[u8], len: u64, ending: &str) -> Option<Format> {
    let at = |offset: usize, bytes: &[u8]| head.get(offset..offset + bytes.len()) == Some(bytes);
    if at(0, b"PK\x03\x04") || at(0, b"PK\x05\x06") {
        return Some(Format::Zip);
    }
    if at(0, b"C64 tape image") || at(0, b"C64S tape") || at(0, b"C64 tape") {
        return Some(Format::T64);
    }
    if at(0x8001, b"CD001") {
        return Some(Format::Iso);
    }
    if at(0, &[0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]) {
        return Some(Format::RawCd);
    }
    if at(0, b"MV - CPC") || at(0, b"EXTENDED CPC DSK") {
        return Some(Format::Dsk);
    }
    if lha::looks_like_lha(head) {
        return Some(Format::Lha);
    }
    // Acorn's ADFS floppies are `.adf` too; they have a root directory or disc record where an Amiga's has none.
    if matches!(ending, "adf" | "adl" | "ads" | "adm" | "hdf") && !at(0, b"DOS") && acorn::looks_like_adfs(head) {
        return Some(Format::Adfs);
    }
    match ending {
        "ssd" => return Some(Format::Dfs),
        "dsd" => return Some(Format::DoubleDfs),
        "uef" if at(0, b"UEF File!\0") || at(0, &[0x1F, 0x8B]) => return Some(Format::Uef),
        _ => {}
    }
    // Amiga floppies start with their boot block's DOS type; one that isn't AmigaDOS's still opens, to say so.
    match ending {
        "adf" if len.is_multiple_of(512) && len >= 1024 => return Some(Format::Adf),
        "adz" if at(0, &[0x1F, 0x8B]) => return Some(Format::Adz),
        _ if at(0, b"DMS!") => return Some(Format::Dms),
        "hdf" if (0..16).any(|b| at(b * 512, b"RDSK")) || (at(0, b"DOS") && head.get(3).is_some_and(|t| *t <= 7)) => return Some(Format::AmigaHardDisk),
        _ => {}
    }
    if at(0, b"SINCLAIR") {
        return Some(Format::Scl);
    }
    if at(0, b"ZXTape!\x1A") {
        return Some(Format::ZxTape);
    }
    match ending {
        // A C64 tape is pulses, not files; it opens as itself.
        "tap" if !at(0, b"C64-TAPE-RAW") && head.len() >= 3 && usize::from(u16::from_le_bytes([head[0], head[1]])) >= 2 => return Some(Format::ZxTape),
        "trd" if head.get(0x8E7) == Some(&0x10) => return Some(Format::TrDos),
        "mdr" if len >= 543 => return Some(Format::Microdrive),
        "z80" if len >= 30 => return Some(Format::Z80),
        "sna" if len == 49_179 || len == 131_103 || len == 147_487 => return Some(Format::Sna),
        _ => {}
    }
    // Sector-by-sector floppy images: ImageDisk's text header, or Teledisk's two letters with its ending.
    if sectors::looks_like_imd(head) {
        return Some(Format::ImageDisk);
    }
    if sectors::looks_like_dmk(head, len) {
        return Some(Format::SectorImage(sectors::Kind::Dmk));
    }
    if matches!(ending, "dsk" | "jv3" | "dis") && sectors::looks_like_jv3(head, len) {
        return Some(Format::SectorImage(sectors::Kind::Jv3));
    }
    if matches!(ending, "dsk" | "jv1" | "dis") && sectors::looks_like_jv1(head, len) {
        return Some(Format::SectorImage(sectors::Kind::Jv1));
    }
    if ending == "td0" && sectors::looks_like_td0(head) {
        return Some(Format::Teledisk);
    }
    // HP's LIF: its &8000 mark, a label and a directory within the image.
    if lif::looks_like_lif(head, len) {
        return Some(Format::Lif);
    }
    // Mac disks: a Disk Copy 4.2 image by its header, a raw disk by the signature of its volume information.
    if mac::looks_like_disk_copy(head, len) {
        return Some(Format::MacDiskCopy);
    }
    if mac::kind_of(head).is_some() {
        return Some(Format::MacDisk);
    }
    if at(0, &[0x0E, 0x0F]) && ending == "msa" {
        return Some(Format::Msa);
    }
    match (ending, len) {
        ("d64", _) | (_, 174_848 | 175_531 | 196_608 | 197_376) => return Some(Format::D64),
        ("d71", _) | (_, 349_696 | 351_062) => return Some(Format::D71),
        ("d81", _) => return Some(Format::D81),
        _ => {}
    }
    if fat::looks_like_fat(head) || fat::partition_of(head).is_some() {
        return Some(Format::Fat);
    }
    // A BBC disk kept as `.img`: only a catalogue of a whole 40 or 80-track disk with files on it is believed.
    if ending == "img" && acorn::looks_like_dfs(head, true) {
        return Some(Format::Dfs);
    }
    if matches!(ending, "atr" | "xfd") && atari8::looks_like(head, len, ending) {
        return Some(Format::Atari8);
    }
    // An Apple II disk, by its ProDOS volume or 2IMG header, or a 140K image with an Apple ending to try.
    if matches!(ending, "dsk" | "do" | "po" | "2mg" | "2img" | "hdv") && apple2::looks_like(head, len, ending) {
        return Some(Format::Apple2);
    }
    // A Color Computer or Dragon disk, by its VDK header or its size; tried when opened, with CP/M to fall back on.
    if matches!(ending, "dsk" | "jvc" | "vdk" | "os9") && coco::looks_like(head, len, ending) {
        return Some(Format::Coco);
    }
    // A CP/M disk says nothing of itself: only a directory that makes sense in one of the known formats is believed.
    if matches!(ending, "img" | "dsk" | "cpm" | "ima") && cpm::looks_like(head, len) {
        return Some(Format::Cpm);
    }
    None
}

/// Opens an image or archive as the file system it holds.
pub fn open(source: Arc<dyn Source>, name: &str) -> Result<Box<dyn Volume>, String> {
    let ending = Path::new(name).extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    let head = source.read_vec(0, source.len().min(0x8800) as usize)?;
    if ending == "cue" {
        return Err("A .cue sheet only says how to read a .bin image beside it: open the .bin itself.".into());
    }
    match detect(&head, source.len(), &ending) {
        Some(Format::Zip) if ending == "rp9" => Ok(Box::new(rp9::Rp9::open(source)?)),
        Some(Format::Zip) => Ok(Box::new(zip::Zip::open(source)?)),
        Some(Format::T64) => Ok(Box::new(cbm::Tape::open(source)?)),
        Some(Format::Iso) => Ok(Box::new(iso::Iso::open(source, "ISO 9660")?)),
        Some(Format::RawCd) => {
            let cooked: Arc<dyn Source> = Arc::new(iso::RawCd::new(source)?);
            Ok(Box::new(iso::Iso::open(cooked, "ISO 9660 in a raw CD image")?))
        }
        Some(Format::Msa) => {
            let raw: Arc<dyn Source> = Arc::new(fat::unpack_msa(&*source)?);
            Ok(Box::new(fat::Fat::open(raw, "Atari ST disk (MSA)")?))
        }
        Some(Format::D64) => Ok(Box::new(cbm::Disk::open(source, cbm::Kind::D64)?)),
        Some(Format::D71) => Ok(Box::new(cbm::Disk::open(source, cbm::Kind::D71)?)),
        Some(Format::D81) => Ok(Box::new(cbm::Disk::open(source, cbm::Kind::D81)?)),
        Some(Format::Fat) => Ok(Box::new(fat::Fat::open(source, if ending == "st" { "Atari ST disk" } else { "FAT" })?)),
        Some(Format::Adf) => Ok(Box::new(amiga::open_adf(source)?)),
        Some(Format::Adz) => Ok(Box::new(amiga::open_adf(Arc::new(amiga::gunzip(&*source)?))?)),
        Some(Format::Dms) => Ok(Box::new(amiga::AmigaFs::open(Arc::new(amiga::undms(&*source)?), 512, "Amiga floppy (DMS)")?)),
        Some(Format::AmigaHardDisk) => Ok(Box::new(amiga::HardDisk::open(source)?)),
        Some(Format::Lha) => Ok(Box::new(lha::Lha::open(source)?)),
        Some(Format::Dfs) => Ok(Box::new(acorn::Dfs::open(source, false)?)),
        Some(Format::DoubleDfs) => Ok(Box::new(acorn::Dfs::open(source, true)?)),
        // An Amiga disk that only looked like ADFS still opens as the Amiga disk it is.
        Some(Format::Adfs) if ending == "adl" => Ok(Box::new(acorn::Adfs::open(Arc::new(acorn::AdlOrder { inner: source }))?)),
        Some(Format::Adfs) => match acorn::Adfs::open(source.clone()) {
            Ok(adfs) => Ok(Box::new(adfs)),
            Err(_) if ending == "adf" && amiga::open_adf(source.clone()).is_ok() => Ok(Box::new(amiga::open_adf(source)?)),
            Err(problem) => Err(problem),
        },
        Some(Format::Uef) => Ok(Box::new(acorn::Uef::open(source)?)),
        Some(Format::MacDiskCopy) => mac::open_disk_copy(source),
        Some(Format::ImageDisk) => sectors::open(source, sectors::Kind::Imd),
        Some(Format::Teledisk) => sectors::open(source, sectors::Kind::Td0),
        Some(Format::SectorImage(kind)) => sectors::open(source, kind),
        Some(Format::Lif) => Ok(Box::new(lif::Lif::open(source)?)),
        Some(Format::MacDisk) => {
            let what = match source.len() {
                409_600 => "Macintosh 400K floppy".to_string(),
                819_200 => "Macintosh 800K floppy".to_string(),
                1_474_560 => "Macintosh 1.4MB floppy".to_string(),
                size => format!("Macintosh disk image, {} MB", size.div_ceil(1024 * 1024)),
            };
            mac::open_disk(source, &what)
        }
        Some(Format::Dsk) => Ok(Box::new(spectrum::CpmDisk::open(source)?)),
        Some(Format::Cpm) => Ok(Box::new(cpm::open_guessed(source, None, "")?)),
        Some(Format::Atari8) => Ok(Box::new(atari8::Atari8::open(source)?)),
        Some(Format::Coco) => match coco::Coco::open(source.clone(), &ending) {
            Ok(coco) => Ok(Box::new(coco)),
            Err(problem) => cpm::open_guessed(source, None, "").map(|cpm| Box::new(cpm) as Box<dyn Volume>).map_err(|_| problem),
        },
        // A 140K disk that's neither DOS 3.3 nor ProDOS may be the Apple II's CP/M.
        Some(Format::Apple2) => match apple2::Apple2::open(source.clone(), &ending) {
            Ok(apple) => Ok(Box::new(apple)),
            Err(problem) => cpm::open_guessed(source, None, "").map(|cpm| Box::new(cpm) as Box<dyn Volume>).map_err(|_| problem),
        },
        Some(Format::TrDos) => Ok(Box::new(spectrum::TrDos::open(source, false)?)),
        Some(Format::Scl) => Ok(Box::new(spectrum::TrDos::open(source, true)?)),
        Some(Format::Microdrive) => Ok(Box::new(spectrum::Microdrive::open(source)?)),
        Some(Format::ZxTape) => Ok(Box::new(spectrum::Tape::open(source)?)),
        Some(Format::Z80) => Ok(Box::new(spectrum::Snapshot::open(source, false)?)),
        Some(Format::Sna) => Ok(Box::new(spectrum::Snapshot::open(source, true)?)),
        None => Err("This isn't an image or archive Limoniter can read yet.".into()),
    }
}

/// Opens a plain disk whose file name says nothing, such as the sectors of an ImageDisk or Teledisk image, as
/// whatever file system its bytes show: FAT, HP's LIF, a Mac disk, a BBC disk, an Amiga or Acorn floppy.
pub fn open_plain_disk(source: Arc<dyn Source>) -> Result<Box<dyn Volume>, String> {
    let head = source.read_vec(0, source.len().min(0x8800) as usize)?;
    for ending in ["img", "adf"] {
        // CP/M is left to the caller, which may know the disk's sector size and track length to judge it by.
        if detect(&head, source.len(), ending).is_some_and(|f| f != Format::Cpm)
            && let Ok(volume) = open(source.clone(), &format!("disk.{ending}"))
        {
            return Ok(volume);
        }
    }
    Err("No file system Limoniter knows is on this disk.".into())
}

/// Opens an image file from disk.
pub fn open_file(path: &Path) -> Result<Box<dyn Volume>, String> {
    let source: Arc<dyn Source> = Arc::new(FileSource::open(path)?);
    open(source, &path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
}

/// Whether bytes taken out of an image are themselves an image or archive, to open in the browser too.
pub fn is_nested_image(name: &str, bytes: &[u8]) -> bool {
    is_image(name, &bytes[..bytes.len().min(0x8800)], bytes.len() as u64)
}

/// A name made safe to show and to open under, from one an image gives.
pub fn tidy_name(name: &str) -> String {
    let tidy: String = name.chars().map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c }).collect();
    let tidy = tidy.trim().to_string();
    if tidy.is_empty() { "_".to_string() } else { tidy }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// The test images, made with c1541, mtools, xorriso, amitools and hfsutils, and packed into one ZIP.
    pub fn fixture(name: &str) -> Vec<u8> {
        let archive: Arc<dyn Source> = Arc::new(include_bytes!("../../data/testmedia/fixtures.zip").to_vec());
        let zip = zip::Zip::open(archive).expect("the fixtures open");
        let entry = zip.list(None).unwrap().into_iter().find(|e| e.name == name).unwrap_or_else(|| panic!("{name} is in the fixtures"));
        zip.read(&entry).unwrap()
    }

    pub fn volume(name: &str) -> Box<dyn Volume> {
        let bytes = fixture(name);
        open(Arc::new(bytes), name).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    /// An HP 150 disk in an ImageDisk image, with a MemoMaker (WordStar) letter on it, for the browser's tests.
    pub fn hp150_imd_with_letter() -> Vec<u8> {
        let mut disk = sectors::tests::hp150_disk();
        // HELLO.TXT's cluster holds a WordStar letter instead: a marked word end, a soft return, underlining.
        let letter = b"Dear Dad,\r\nTh\xE5 dec\xEB chair\xF3 \x8D\nar\xE5 \x13late\x13 \x8D\nagain.\r\n\x1A";
        let data = 512 + 2 * 768 + 128 * 32;
        disk[data..data + 256].fill(0x1A);
        disk[data..data + letter.len()].copy_from_slice(letter);
        disk[512 + 2 * 768 + 32 + 28..][..4].copy_from_slice(&(letter.len() as u32).to_le_bytes());
        sectors::tests::imd_of(&disk, (99, 99), (99, 99))
    }

    pub fn names(entries: &[Entry]) -> Vec<String> {
        entries.iter().map(|e| e.name.clone()).collect()
    }

    pub fn find<'a>(entries: &'a [Entry], name: &str) -> &'a Entry {
        entries.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("{name} in {:?}", names(entries)))
    }

    #[test]
    fn images_are_told_apart_by_their_bytes_not_only_their_endings() {
        assert!(is_image("x.d64", &[], 174_848));
        assert!(is_image("GAME.ZIP", b"PK\x03\x04", 100));
        assert!(!is_image("notes.txt", b"PK\x03\x04", 100), "a text file is never browsed");
        assert!(!is_image("data.bin", b"just some bytes", 15), "a .bin is only a CD image when it starts like one");
        assert!(is_image("cd.bin", &fixture("test.bin")[..0x8800], 449_232));
        assert!(is_image("floppy.img", &fixture("fat12.img")[..0x8800], 368_640));
        assert!(!is_image("photo.img", &[0u8; 0x8800], 0x8800));
    }

    #[test]
    fn odd_and_damaged_images_give_errors_never_panics() {
        for (name, bytes) in [("x.iso", vec![0u8; 100]), ("x.d64", vec![0xFFu8; 174_848]), ("x.zip", b"PK\x03\x04garbage".to_vec()), ("x.t64", b"C64 tape image".to_vec()), ("x.msa", vec![0x0E, 0x0F, 0, 9]), ("x.img", vec![0u8; 512])]
            .into_iter()
            .chain(odd_retro_images())
        {
            if let Ok(volume) = open(Arc::new(bytes), name) {
                let _ = volume.describe();
                if let Ok(entries) = volume.list(None) {
                    for entry in entries.iter().take(20) {
                        let _ = if entry.is_dir { volume.list(Some(entry)).map(|_| Vec::new()) } else { volume.read(entry) };
                    }
                }
            }
        }
        // A real image with random damage still answers every question it's asked.
        for seed in 0..40u32 {
            for name in ["test.d64", "fat12.img", "test.iso", "nested.zip", "test.t64", "ofs.adf", "ffs-longnames.adf", "rdb.hdf"] {
                let mut bytes = fixture(name);
                let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(name.len() as u32);
                // An Amiga volume's folders and file headers are by its root, in the middle, so the damage goes there.
                let (from, span) = match name {
                    "rdb.hdf" => (0, bytes.len()),
                    _ if name.ends_with(".adf") => (878 * 512, 24 * 512),
                    _ => (0, bytes.len()),
                };
                for _ in 0..64 {
                    state = state.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    let at = from + state as usize % span;
                    bytes[at] = (state >> 16) as u8;
                }
                if let Ok(volume) = open(Arc::new(bytes), name) {
                    walk(&*volume, None, 0);
                }
            }
        }
    }

    /// Spectrum, Amiga and archive inputs that are wrong in the ways a damaged or mislabelled file is.
    fn odd_retro_images() -> Vec<(&'static str, Vec<u8>)> {
        let mut trd = vec![0u8; 4096];
        trd[0x8E7] = 0x10;
        trd[..16].copy_from_slice(b"HUGE    B\xFF\xFF\xFF\xFF\xFF\xFF\xFF");
        let mut dsk = b"EXTENDED CPC DSK File\r\nDisk-Info\r\n".to_vec();
        dsk.resize(256, 0);
        dsk[0x30] = 40;
        dsk[0x31] = 2;
        dsk[0x34] = 0x13;
        let mut root_loop = vec![0u8; 901_120];
        root_loop[..4].copy_from_slice(b"DOS\x01");
        let root = 880 * 512;
        root_loop[root + 3] = 2;
        root_loop[root + 508..root + 512].copy_from_slice(&1u32.to_be_bytes());
        // Every hash slot points back at the root.
        for slot in 0..72 {
            root_loop[root + 24 + slot * 4..root + 28 + slot * 4].copy_from_slice(&880u32.to_be_bytes());
        }
        vec![
            ("x.trd", trd),
            ("x.scl", b"SINCLAIR\xFF".to_vec()),
            ("x.dsk", dsk),
            ("x.tap", vec![0x13, 0x00, 0x00, 0x00]),
            ("x.tzx", b"ZXTape!\x1A\x01\x14\x10\x00\x00\xFF\xFF".to_vec()),
            ("x.z80", vec![0u8; 40]),
            ("x.sna", vec![0xFFu8; 49_179]),
            ("x.mdr", vec![0xFFu8; 543 * 3]),
            ("x.adf", vec![0u8; 901_120]),
            ("loop.adf", root_loop),
            ("x.adz", vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 0, 0, 0, 0xFF]),
            ("x.dms", b"DMS!".iter().copied().chain(std::iter::repeat_n(0xFF, 200)).collect()),
            ("x.hdf", b"RDSK".iter().copied().chain(std::iter::repeat_n(0xFF, 8188)).collect()),
            ("x.lha", b"\x16\x00-lh5-\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF\x00\x00\x00\x00\x20\x00\x01a\x00\x00garbage".to_vec()),
        ]
    }

    fn walk(volume: &dyn Volume, dir: Option<&Entry>, depth: usize) {
        if depth > 6 {
            return;
        }
        if let Ok(entries) = volume.list(dir) {
            for entry in entries.iter().take(50) {
                if entry.is_dir {
                    walk(volume, Some(entry), depth + 1);
                } else {
                    let _ = volume.read(entry);
                }
            }
        }
    }

    /// Real images from archive.org, kept in ..\testmedia outside the repository: every file in every image is
    /// read, and a report of each one's size and SHA-256 is written beside them, to compare with what the tools
    /// that made such images (c1541, mtools, unzip, xorriso) read. Skipped when the folder isn't there.
    #[test]
    fn real_images_read_without_trouble_and_report_their_files() {
        use sha2::Digest;
        let root = Path::new("../testmedia");
        if !root.is_dir() {
            return;
        }
        let mut report = String::new();
        let mut images = Vec::new();
        let mut folders = vec![root.to_path_buf()];
        while let Some(folder) = folders.pop() {
            for item in std::fs::read_dir(&folder).unwrap().flatten() {
                let path = item.path();
                if path.is_dir() {
                    // Limoniter's own output, such as unpacked DMS disks, isn't media to read.
                    if path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("limoniter-")) {
                        continue;
                    }
                    folders.push(path);
                } else if path.file_name().is_some_and(|n| !n.to_string_lossy().ends_with(".txt")) {
                    images.push(path);
                }
            }
        }
        images.sort();
        let mut problems = Vec::new();
        for path in images {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let bytes = std::fs::read(&path).unwrap();
            if !is_image(&name, &bytes[..bytes.len().min(0x8800)], bytes.len() as u64) {
                continue;
            }
            let shown = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            match open(Arc::new(bytes), &name) {
                Ok(volume) => {
                    report.push_str(&format!("# {shown}: {}\n", volume.describe()));
                    collect(&*volume, None, "", &shown, &mut report, &mut problems, 0, &mut |bytes| format!("{:x}", sha2::Sha256::digest(bytes)));
                }
                Err(problem) => {
                    report.push_str(&format!("# {shown}: REFUSED: {problem}\n"));
                    problems.push(format!("{shown}: {problem}"));
                }
            }
        }
        std::fs::write(root.join("limoniter-report.txt"), &report).unwrap();
        // Images the independent tools also refuse are listed, one to a line, in known-refused.txt; they must still
        // be refused, and nothing else may be.
        // Windows keeps characters such as " in a name as private-use ones, which Linux shows as themselves; the
        // list may have been written on either.
        let plain = |text: &str| -> String { text.chars().map(|c| if ('\u{F001}'..='\u{F07F}').contains(&c) { char::from((c as u32 - 0xF000) as u8) } else { c }).collect() };
        let known: Vec<String> = std::fs::read_to_string(root.join("known-refused.txt")).unwrap_or_default().lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(plain).collect();
        let problems: Vec<String> = problems.iter().map(|p| plain(p)).collect();
        let unexpected: Vec<&String> = problems.iter().filter(|p| !known.iter().any(|k| p.starts_with(&format!("{k}: ")))).collect();
        assert!(unexpected.is_empty(), "{unexpected:#?}");
        for image in &known {
            assert!(problems.iter().any(|p| p.starts_with(&format!("{image}: "))), "{image} is listed as refused, and opened");
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn collect(volume: &dyn Volume, dir: Option<&Entry>, prefix: &str, image: &str, report: &mut String, problems: &mut Vec<String>, depth: usize, hash: &mut dyn FnMut(&[u8]) -> String) {
        if depth > 8 {
            return;
        }
        let entries = match volume.list(dir) {
            Ok(entries) => entries,
            Err(problem) => return problems.push(format!("{image} {prefix}: {problem}")),
        };
        for entry in entries {
            let path = format!("{prefix}{}", entry.name);
            if entry.kind.starts_with("DEL") {
                continue;
            }
            if entry.is_dir {
                collect(volume, Some(&entry), &format!("{path}/"), image, report, problems, depth + 1, hash);
                continue;
            }
            match volume.read(&entry) {
                Ok(bytes) => {
                    report.push_str(&format!("{image}\t{path}\t{}\t{}\n", bytes.len(), hash(&bytes)));
                    // An image or archive inside is read through as well, so its trouble is found too.
                    if is_nested_image(&entry.open_name, &bytes)
                        && let Ok(inner) = open(Arc::new(bytes), &entry.open_name)
                    {
                        collect(&*inner, None, &format!("{path}!/"), image, report, problems, depth + 1, hash);
                    }
                }
                Err(problem) => problems.push(format!("{image} {path}: {problem}")),
            }
        }
    }

    #[test]
    fn names_are_made_safe_to_open_under() {
        assert_eq!(tidy_name("A/B:C"), "A_B_C");
        assert_eq!(tidy_name("  "), "_");
    }
}
