//! Changing images: adding files to a disk or tape image, taking them off, renaming them, and making new blank
//! images. The readers beside this module stay read only; a writer works on a whole image held in memory and
//! gives back the new bytes, which the browser saves over the image and opens again to show.
//!
//! Each writer keeps to its format's own rules, as its machine would: a Commodore disk's sectors are chained and
//! marked in its BAM, a Spectrum tape's blocks carry their checksums. What it can't do safely it refuses, saying
//! why, rather than leave an image the machine can't read.

mod acorn;
mod adfs;
mod amiga;
mod apple2;
mod atari;
mod cbm;
mod coco;
mod fat;
mod plus3;
mod tape;

use super::{Entry, Format};
use std::path::Path;

/// The most an image may be to be changed here, as it's held in memory while it is.
pub const MOST_WRITABLE_BYTES: u64 = 512 * 1024 * 1024;

/// A file to put on an image, and how its machine is to see it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Addition {
    /// The name it's to have, as the person gave it; the writer makes it fit the image's rules.
    pub name: String,
    pub bytes: Vec<u8>,
    /// One of the writer's kinds, or empty where the image has none.
    pub kind: String,
    /// Where it loads, for a kind that says.
    pub address: Option<u16>,
    /// The line a BASIC program runs from, for a kind that has one.
    pub autostart: Option<u16>,
    /// Whether a file already there by the same name is to be replaced; if not, adding it is refused.
    pub replace: bool,
}

/// How a writer's refusal to add a file whose name is taken begins, so the browser can offer to replace it.
pub const ALREADY: &str = "There's already a file called";

/// The refusal to add a file whose name is taken.
pub(crate) fn already(name: &str) -> String {
    format!("{ALREADY} {name} here.")
}

/// A kind of file an image can hold, and what more it needs to be told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind {
    pub name: &'static str,
    /// What it's for, said plainly, for the Add box.
    pub about: &'static str,
    pub address: bool,
    pub autostart: bool,
}

impl Kind {
    pub const fn plain(name: &'static str, about: &'static str) -> Self {
        Self { name, about, address: false, autostart: false }
    }
}

/// An image being changed.
pub trait ImageWriter: Send {
    /// The kinds of file this image holds; empty when all files are alike, as on a PC or Amiga disk.
    fn kinds(&self) -> Vec<Kind> {
        Vec::new()
    }
    /// How a file would most likely go on, from its name and bytes: its kind, load address and name.
    fn guess(&self, name: &str, bytes: &[u8]) -> Addition {
        Addition { name: name.to_string(), bytes: bytes.to_vec(), ..Default::default() }
    }
    /// Puts a file in a folder (or at the top for None).
    fn add(&mut self, folder: Option<&Entry>, file: &Addition) -> Result<(), String>;
    /// Takes a file, or an empty folder, off.
    fn delete(&mut self, folder: Option<&Entry>, entry: &Entry) -> Result<(), String>;
    /// Gives a file or folder a new name.
    fn rename(&mut self, folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String>;
    /// Makes a new, empty folder, on images that have folders.
    fn make_folder(&mut self, _folder: Option<&Entry>, _name: &str) -> Result<(), String> {
        Err("This kind of image has no folders.".into())
    }
    fn has_folders(&self) -> bool {
        false
    }
    /// Moves a file earlier (negative) or later in the image's own order, where order matters, as on a tape.
    fn reorder(&mut self, _entry: &Entry, _by: i32) -> Result<(), String> {
        Err("Files on this kind of image have no order to change.".into())
    }
    fn has_order(&self) -> bool {
        false
    }
    /// The bytes free for files, if the image says.
    fn free(&self) -> Option<u64> {
        None
    }
    /// The image as it now is.
    fn bytes(&self) -> &[u8];
}

/// A writer for an image, or why it can't be changed here.
pub fn writer(name: &str, bytes: Vec<u8>) -> Result<Box<dyn ImageWriter>, String> {
    if bytes.len() as u64 > MOST_WRITABLE_BYTES {
        return Err("The image is larger than the 512 MB that can be changed here.".into());
    }
    let ending = Path::new(name).extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    let head = &bytes[..bytes.len().min(0x8800)];
    match super::detect(head, bytes.len() as u64, &ending) {
        Some(Format::ZxTape) => Ok(Box::new(tape::Tape::open(bytes)?)),
        Some(Format::D64) => Ok(Box::new(cbm::Disk::open(bytes, cbm::Kind::D64)?)),
        Some(Format::D71) => Ok(Box::new(cbm::Disk::open(bytes, cbm::Kind::D71)?)),
        Some(Format::D81) => Ok(Box::new(cbm::Disk::open(bytes, cbm::Kind::D81)?)),
        Some(Format::Fat) => Ok(Box::new(fat::Fat::open(bytes)?)),
        Some(Format::Adf) => Ok(Box::new(amiga::Adf::open(bytes)?)),
        Some(Format::Atari8) => Ok(Box::new(atari::Disk::open(bytes)?)),
        Some(Format::Coco) => Ok(Box::new(coco::Disk::open(bytes, &ending)?)),
        Some(Format::Dsk) => Ok(Box::new(plus3::Disk::open(bytes)?)),
        Some(Format::Apple2) => apple2::open(bytes, &ending),
        Some(Format::Dfs | Format::DoubleDfs) => Ok(Box::new(acorn::Dfs::open(bytes, ending == "dsd")?)),
        // An Amiga disk that only looked like ADFS is still changed as the Amiga disk it is.
        Some(Format::Adfs) => match adfs::Adfs::open(bytes.clone(), &ending) {
            Ok(adfs) => Ok(Box::new(adfs)),
            Err(problem) => amiga::Adf::open(bytes).map(|adf| Box::new(adf) as Box<dyn ImageWriter>).map_err(|_| problem),
        },
        Some(_) => Err("Limoniter can read this kind of image, but can't change it yet.".into()),
        None => Err("This isn't an image Limoniter can change.".into()),
    }
}

/// Whether an image of this name and these first bytes is one a writer may be able to change, without trying.
pub fn may_write(name: &str, head: &[u8], len: u64) -> bool {
    let ending = Path::new(name).extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    len <= MOST_WRITABLE_BYTES
        && matches!(
            super::detect(head, len, &ending),
            Some(Format::ZxTape | Format::D64 | Format::D71 | Format::D81 | Format::Fat | Format::Adf | Format::Atari8 | Format::Coco | Format::Dsk | Format::Apple2 | Format::Dfs | Format::DoubleDfs | Format::Adfs)
        )
}

/// A new, empty image of a kind, freshly formatted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blank {
    SpectrumTap,
    SpectrumTzx,
    Plus3,
    D64,
    D71,
    D81,
    AmigaDd,
    AmigaDdFfs,
    AmigaHd,
    Atari90,
    Atari130,
    CocoSingle,
    PcFloppy360,
    PcFloppy720,
    PcFloppy1440,
    AtariSt720,
    AppleDos,
    AppleProdos,
    AppleProdos800,
    BbcSsd40,
    BbcSsd80,
    BbcDsd80,
    AdfsS,
    AdfsM,
    AdfsL,
}

impl Blank {
    pub const ALL: [Blank; 25] = [
        Blank::SpectrumTap,
        Blank::SpectrumTzx,
        Blank::Plus3,
        Blank::D64,
        Blank::D71,
        Blank::D81,
        Blank::AmigaDd,
        Blank::AmigaDdFfs,
        Blank::AmigaHd,
        Blank::Atari90,
        Blank::Atari130,
        Blank::CocoSingle,
        Blank::PcFloppy360,
        Blank::PcFloppy720,
        Blank::PcFloppy1440,
        Blank::AtariSt720,
        Blank::AppleDos,
        Blank::AppleProdos,
        Blank::AppleProdos800,
        Blank::BbcSsd40,
        Blank::BbcSsd80,
        Blank::BbcDsd80,
        Blank::AdfsS,
        Blank::AdfsM,
        Blank::AdfsL,
    ];

    /// What it is, for the New image box.
    pub fn name(self) -> &'static str {
        match self {
            Blank::SpectrumTap => "ZX Spectrum tape (.tap)",
            Blank::SpectrumTzx => "ZX Spectrum tape (.tzx)",
            Blank::Plus3 => "Spectrum +3 disk, 173K (.dsk)",
            Blank::D64 => "Commodore 1541 disk, 170K (.d64)",
            Blank::D71 => "Commodore 1571 disk, 340K (.d71)",
            Blank::D81 => "Commodore 1581 disk, 800K (.d81)",
            Blank::AmigaDd => "Amiga floppy, 880K, OFS (.adf)",
            Blank::AmigaDdFfs => "Amiga floppy, 880K, FFS (.adf)",
            Blank::AmigaHd => "Amiga HD floppy, 1.76MB, FFS (.adf)",
            Blank::Atari90 => "Atari 8-bit disk, 90K, DOS 2.0 (.atr)",
            Blank::Atari130 => "Atari 8-bit disk, 130K, DOS 2.5 (.atr)",
            Blank::CocoSingle => "Color Computer / Dragon disk, 156K, Disk BASIC (.dsk)",
            Blank::PcFloppy360 => "PC floppy, 360K, FAT12 (.img)",
            Blank::PcFloppy720 => "PC floppy, 720K, FAT12 (.img)",
            Blank::PcFloppy1440 => "PC floppy, 1.44MB, FAT12 (.img)",
            Blank::AtariSt720 => "Atari ST floppy, 720K (.st)",
            Blank::AppleDos => "Apple II disk, 140K, DOS 3.3 (.dsk)",
            Blank::AppleProdos => "Apple II disk, 140K, ProDOS (.po)",
            Blank::AppleProdos800 => "Apple II 3.5-inch disk, 800K, ProDOS (.po)",
            Blank::BbcSsd40 => "BBC Micro disk, 100K, DFS (.ssd)",
            Blank::BbcSsd80 => "BBC Micro disk, 200K, DFS (.ssd)",
            Blank::BbcDsd80 => "BBC Micro disk, double-sided, 400K, DFS (.dsd)",
            Blank::AdfsS => "BBC Micro / Master disc, 160K, ADFS S (.ads)",
            Blank::AdfsM => "BBC Micro / Master disc, 320K, ADFS M (.adm)",
            Blank::AdfsL => "BBC Micro / Master disc, 640K, ADFS L (.adl)",
        }
    }

    pub fn ending(self) -> &'static str {
        match self {
            Blank::SpectrumTap => "tap",
            Blank::SpectrumTzx => "tzx",
            Blank::Plus3 | Blank::CocoSingle | Blank::AppleDos => "dsk",
            Blank::AppleProdos | Blank::AppleProdos800 => "po",
            Blank::D64 => "d64",
            Blank::D71 => "d71",
            Blank::D81 => "d81",
            Blank::AmigaDd | Blank::AmigaDdFfs | Blank::AmigaHd => "adf",
            Blank::Atari90 | Blank::Atari130 => "atr",
            Blank::PcFloppy360 | Blank::PcFloppy720 | Blank::PcFloppy1440 => "img",
            Blank::AtariSt720 => "st",
            Blank::BbcSsd40 | Blank::BbcSsd80 => "ssd",
            Blank::BbcDsd80 => "dsd",
            Blank::AdfsS => "ads",
            Blank::AdfsM => "adm",
            Blank::AdfsL => "adl",
        }
    }

    /// The formatted image, its disk named `label` where the format keeps a name.
    pub fn make(self, label: &str) -> Vec<u8> {
        match self {
            Blank::SpectrumTap => Vec::new(),
            Blank::SpectrumTzx => tape::blank_tzx(),
            Blank::Plus3 => plus3::blank(),
            Blank::D64 => cbm::blank(cbm::Kind::D64, label),
            Blank::D71 => cbm::blank(cbm::Kind::D71, label),
            Blank::D81 => cbm::blank(cbm::Kind::D81, label),
            Blank::AmigaDd => amiga::blank(label, false, false),
            Blank::AmigaDdFfs => amiga::blank(label, true, false),
            Blank::AmigaHd => amiga::blank(label, true, true),
            Blank::Atari90 => atari::blank(false),
            Blank::Atari130 => atari::blank(true),
            Blank::CocoSingle => coco::blank(),
            Blank::PcFloppy360 => fat::blank(fat::Floppy::Pc360, label),
            Blank::PcFloppy720 => fat::blank(fat::Floppy::Pc720, label),
            Blank::PcFloppy1440 => fat::blank(fat::Floppy::Pc1440, label),
            Blank::AtariSt720 => fat::blank(fat::Floppy::St720, label),
            Blank::AppleDos => apple2::blank_dos(),
            Blank::AppleProdos => apple2::blank_prodos(280, label),
            Blank::AppleProdos800 => apple2::blank_prodos(1600, label),
            Blank::BbcSsd40 => acorn::blank(40, false, label),
            Blank::BbcSsd80 => acorn::blank(80, false, label),
            Blank::BbcDsd80 => acorn::blank(80, true, label),
            Blank::AdfsS => adfs::blank(640, label),
            Blank::AdfsM => adfs::blank(1280, label),
            Blank::AdfsL => adfs::blank(2560, label),
        }
    }
}

/// A name's stem and ending, split at its last dot.
pub(crate) fn stem_and_ending(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot + 1..]),
        _ => (name, ""),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::super::{Volume, open};
    use super::*;
    use std::sync::Arc;

    /// The image a writer has made, opened by the reader as the browser would open it.
    pub fn reopened(writer: &dyn ImageWriter, name: &str) -> Box<dyn Volume> {
        open(Arc::new(writer.bytes().to_vec()), name).unwrap_or_else(|e| panic!("{name} no longer opens: {e}"))
    }

    pub fn names(volume: &dyn Volume, folder: Option<&Entry>) -> Vec<String> {
        volume.list(folder).unwrap().into_iter().map(|e| e.name).collect()
    }

    pub fn entry(volume: &dyn Volume, folder: Option<&Entry>, name: &str) -> Entry {
        volume.list(folder).unwrap().into_iter().find(|e| e.name == name).unwrap_or_else(|| panic!("{name} isn't there"))
    }

    /// A reference tool, if it's installed, for the tests that check a writer's images against it.
    pub fn tool(name: &str) -> Option<std::path::PathBuf> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path).map(|dir| dir.join(name)).find(|p| p.is_file())
    }

    #[test]
    fn every_blank_image_opens_as_the_kind_it_says_and_is_empty() {
        for blank in Blank::ALL {
            let bytes = blank.make("BLANK");
            let name = format!("new.{}", blank.ending());
            if blank == Blank::SpectrumTap {
                assert!(bytes.is_empty());
                continue;
            }
            let volume = open(Arc::new(bytes.clone()), &name).unwrap_or_else(|e| panic!("{blank:?}: {e}"));
            // A double-sided DFS disk's two sides are folders, each empty.
            assert!(volume.list(None).unwrap().iter().all(|side| side.is_dir && volume.list(Some(side)).unwrap().is_empty()), "{blank:?} isn't empty");
            assert!(writer(&name, bytes).is_ok(), "{blank:?} can't be written to");
        }
    }
}
