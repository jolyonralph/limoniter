//! Sinclair QL microdrive cartridges (.mdv), changed as QDOS changes them: each file a header and its blocks, in
//! free sectors given out from the map, its entry in the directory (see `media::ql` for the format).
//!
//! A program goes on as an executable (type 1) with its data space: from the header it was kept with (XTcc after
//! it, or Q-emuLator's before it, as the browser and F9 give programs), or the usual 4K if it has none.

use super::{Addition, Entry, ImageWriter, Kind};
use crate::media::ql::{Cartridge, MOST_FILES};
use crate::ql::{self, EXECUTABLE};

const PROGRAM: &str = "Program";
const DATA: &str = "Data";

pub struct Mdv {
    cartridge: Cartridge,
}

impl Mdv {
    pub fn open(bytes: Vec<u8>) -> Result<Self, String> {
        Ok(Self { cartridge: Cartridge::open(bytes)? })
    }
}

/// A new cartridge, formatted with this name, its random bytes from the time it was made, as a QL's are.
pub fn blank(name: &str) -> Vec<u8> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    Cartridge::blank(name, [(nanos >> 8) as u8, (nanos >> 16) as u8])
}

impl ImageWriter for Mdv {
    fn kinds(&self) -> Vec<Kind> {
        vec![
            Kind::plain(PROGRAM, "An executable program (type 1), which EXEC loads and starts as a job, with its data space."),
            Kind::plain(DATA, "Any other file: a SuperBASIC program as text, a BOOT file, data."),
        ]
    }

    fn guess(&self, name: &str, bytes: &[u8]) -> Addition {
        let kind = if ql::is_program(bytes) { PROGRAM } else { DATA };
        Addition { name: ql::qdos_name(name), bytes: bytes.to_vec(), kind: kind.into(), ..Default::default() }
    }

    fn add(&mut self, _folder: Option<&Entry>, file: &Addition) -> Result<(), String> {
        let name = ql::qdos_name(&file.name);
        let name = if file.name.eq_ignore_ascii_case("boot") { "BOOT".to_string() } else { name };
        // The header a file was kept with is taken off and put in the QDOS header instead.
        let kept = ql::kept(&file.bytes);
        let (kind, data_space, content) = match (file.kind.as_str(), kept) {
            (PROGRAM, Some(kept)) => (EXECUTABLE, kept.data_space, kept.code),
            (PROGRAM, None) => (EXECUTABLE, ql::DATA_SPACE, file.bytes.as_slice()),
            (_, Some(kept)) if kept.keeping == ql::Keeping::Qemulator => (kept.kind, kept.data_space, kept.code),
            _ => (0, 0, file.bytes.as_slice()),
        };
        if self.cartridge.find(&name).is_some() {
            if !file.replace {
                return Err(super::already(&name));
            }
            self.cartridge.delete(&name)?;
        }
        if self.cartridge.files().len() >= MOST_FILES {
            return Err(crate::i18n::tf!("The directory is full: a cartridge holds {MOST_FILES} files."));
        }
        self.cartridge.add(&name, kind, data_space, content)
    }

    fn delete(&mut self, _folder: Option<&Entry>, entry: &Entry) -> Result<(), String> {
        self.cartridge.delete(&entry.name)
    }

    fn rename(&mut self, _folder: Option<&Entry>, entry: &Entry, name: &str) -> Result<(), String> {
        self.cartridge.rename(&entry.name, &ql::qdos_name(name))
    }

    fn free(&self) -> Option<u64> {
        Some(self.cartridge.free_sectors().len() as u64 * 512)
    }

    fn bytes(&self) -> &[u8] {
        &self.cartridge.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{entry, names, reopened};
    use super::*;

    /// Files added in the browser come back out of the image as they went in: a program with its header kept, which
    /// goes on as an executable with that data space, and a BOOT file as text.
    #[test]
    fn an_mdv_image_round_trips_through_the_browser() {
        let mut mdv = Mdv::open(blank("Limoniter")).unwrap();
        let program = ql::with_xtcc(&[0x60, 0x0E, 0, 0, 0, 0, 0x4A, 0xFB, 0, 2, b'h', b'i', 0x4E, 0x75], 2048);
        for (name, bytes) in [("hello_exe", program.clone()), ("boot", b"10 EXEC mdv1_hello_exe\n".to_vec()), ("notes.txt", b"QL".to_vec())] {
            let addition = mdv.guess(name, &bytes);
            mdv.add(None, &addition).unwrap();
        }
        let volume = reopened(&mdv, "t.mdv");
        assert_eq!(names(&*volume, None), ["hello_exe", "BOOT", "notes_txt"]);
        let shown = entry(&*volume, None, "hello_exe");
        assert_eq!((shown.kind.as_str(), shown.size), ("Program", 14));
        assert_eq!(shown.detail, "data space 2048 bytes, the job hi");
        assert_eq!(volume.read(&shown).unwrap(), program, "the program comes out as it went in");
        assert_eq!(volume.read(&entry(&*volume, None, "BOOT")).unwrap(), b"10 EXEC mdv1_hello_exe\n");
        let cartridge = Cartridge::open(mdv.bytes().to_vec()).unwrap();
        let file = cartridge.find("hello_exe").unwrap();
        assert_eq!((file.header.kind, file.header.data_space), (1, 2048));
        // The same name again is refused, or replaces it if asked.
        let again = mdv.guess("hello_exe", &program);
        assert!(mdv.add(None, &again).is_err());
        mdv.add(None, &Addition { replace: true, ..again }).unwrap();
        mdv.rename(None, &entry(&*reopened(&mdv, "t.mdv"), None, "notes_txt"), "readme").unwrap();
        mdv.delete(None, &entry(&*reopened(&mdv, "t.mdv"), None, "BOOT")).unwrap();
        assert_eq!(names(&*reopened(&mdv, "t.mdv"), None), ["hello_exe", "readme"]);
        assert_eq!(mdv.free(), Some((252 - 2) * 512));
    }
}
