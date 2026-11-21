//! The structure templates there are to choose from: the ones that come with Limoniter, and your own, kept as
//! `.structure` files in the `structures` folder in Limoniter's data folder. Yours are tried first, so a copy of a
//! built-in one changed and kept there takes its place.

use crate::structure::{Problem, Source, Template};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The templates that come with Limoniter, in the order they're tried: the more particular before the more
/// general, so an Atari ST program is never taken for a Commodore one.
pub const BUILT_IN: [(&str, &str); 16] = [
    ("amiga-hunk.structure", include_str!("../data/structures/amiga-hunk.structure")),
    ("adf.structure", include_str!("../data/structures/adf.structure")),
    ("iff.structure", include_str!("../data/structures/iff.structure")),
    ("zx-tap.structure", include_str!("../data/structures/zx-tap.structure")),
    ("zx-tzx.structure", include_str!("../data/structures/zx-tzx.structure")),
    ("zx-sna.structure", include_str!("../data/structures/zx-sna.structure")),
    ("zx-z80.structure", include_str!("../data/structures/zx-z80.structure")),
    ("atari-st-prg.structure", include_str!("../data/structures/atari-st-prg.structure")),
    ("c64-prg.structure", include_str!("../data/structures/c64-prg.structure")),
    ("c64-d64.structure", include_str!("../data/structures/c64-d64.structure")),
    ("bmp.structure", include_str!("../data/structures/bmp.structure")),
    ("png.structure", include_str!("../data/structures/png.structure")),
    ("zip.structure", include_str!("../data/structures/zip.structure")),
    ("elf.structure", include_str!("../data/structures/elf.structure")),
    ("mz-pe.structure", include_str!("../data/structures/mz-pe.structure")),
    ("wav.structure", include_str!("../data/structures/wav.structure")),
];

/// How the template language is written, shown as a page from the Structure panel.
pub const LANGUAGE: &str = include_str!("../data/structures/LANGUAGE.md");

/// The ending template files have.
pub const ENDING: &str = "structure";

/// Where your own templates are kept.
pub fn own_folder() -> Option<PathBuf> {
    if let Some(folder) = std::env::var_os("LIMONITER_STRUCTURES") {
        return Some(folder.into());
    }
    // Tests never pick up templates someone has put on the machine running them.
    if cfg!(test) {
        return None;
    }
    crate::settings::data_folder().map(|dir| dir.join("structures"))
}

/// One template: its file's name, where it is if it's one of yours, and what it says or what's wrong with it.
pub struct Entry {
    pub file: String,
    pub path: Option<PathBuf>,
    pub text: String,
    pub template: Result<Template, Problem>,
}

impl Entry {
    fn new(file: &str, path: Option<PathBuf>, text: String) -> Self {
        let template = Template::parse(&text);
        Entry { file: file.to_string(), path, text, template }
    }

    /// The template's own name, or its file's for one that couldn't be read.
    pub fn title(&self) -> String {
        match &self.template {
            Ok(t) => t.name.clone(),
            Err(_) => self.file.clone(),
        }
    }

    pub fn is_own(&self) -> bool {
        self.path.is_some()
    }
}

pub struct Library {
    /// Your own first, then the built-in ones.
    pub entries: Vec<Entry>,
    /// The files in your folder as last read: name, size and when changed, to notice a template being saved.
    seen: Vec<(PathBuf, u64, Option<SystemTime>)>,
    folder: Option<PathBuf>,
}

impl Default for Library {
    fn default() -> Self {
        Library::load(own_folder())
    }
}

/// The template files in a folder, in name order, with their sizes and times, for noticing changes.
fn listing(folder: Option<&Path>) -> Vec<(PathBuf, u64, Option<SystemTime>)> {
    let Some(Ok(entries)) = folder.map(std::fs::read_dir) else {
        return Vec::new();
    };
    let mut files: Vec<(PathBuf, u64, Option<SystemTime>)> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ENDING)))
        .map(|p| {
            let meta = std::fs::metadata(&p).ok();
            let (size, time) = meta.map_or((0, None), |m| (m.len(), m.modified().ok()));
            (p, size, time)
        })
        .collect();
    files.sort();
    files
}

impl Library {
    /// The built-in templates and the ones in this folder.
    pub fn load(folder: Option<PathBuf>) -> Self {
        let seen = listing(folder.as_deref());
        let mut entries: Vec<Entry> = seen
            .iter()
            .map(|(path, _, _)| {
                let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let text = match std::fs::read(path) {
                    Ok(bytes) => String::from_utf8_lossy(&bytes).replace("\r\n", "\n"),
                    Err(e) => return Entry { file, path: Some(path.clone()), text: String::new(), template: Err(Problem { line: 1, message: crate::i18n::tf!("Couldn't read it: {e}") }) },
                };
                Entry::new(&file, Some(path.clone()), text)
            })
            .collect();
        entries.extend(BUILT_IN.iter().map(|(file, text)| Entry::new(file, None, text.replace("\r\n", "\n"))));
        Library { entries, seen, folder }
    }

    /// Reads your templates again if any has been added, changed or taken away. Says whether anything changed.
    pub fn refresh(&mut self) -> bool {
        if listing(self.folder.as_deref()) == self.seen {
            return false;
        }
        *self = Library::load(self.folder.clone());
        true
    }

    pub fn folder(&self) -> Option<&Path> {
        self.folder.as_deref()
    }

    /// The first template that fits this file, yours before the built-in ones, by the file's ending and its bytes.
    pub fn choose(&self, source: &dyn Source, ending: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.template.as_ref().is_ok_and(|t| t.fits(source, ending)))
    }

    pub fn find(&self, file: &str, own: bool) -> Option<usize> {
        self.entries.iter().position(|e| e.file == file && e.is_own() == own)
    }

    /// Puts a copy of a built-in template in your folder, to be changed there, unless there's one already. Gives
    /// where it is.
    pub fn copy_to_own(&mut self, index: usize) -> Result<PathBuf, String> {
        let folder = self.folder.clone().ok_or_else(|| crate::i18n::tr("There's no data folder to keep templates in.").to_string())?;
        let entry = &self.entries[index];
        if let Some(path) = &entry.path {
            return Ok(path.clone());
        }
        std::fs::create_dir_all(&folder).map_err(|e| crate::i18n::tf!("Couldn't make the folder {}: {e}", folder.display()))?;
        let path = folder.join(&entry.file);
        if !path.exists() {
            std::fs::write(&path, &entry.text).map_err(|e| crate::i18n::tf!("Couldn't write {}: {e}", path.display()))?;
        }
        self.refresh();
        Ok(path)
    }

    /// A new template of your own to start from, made in your folder. Gives where it is.
    pub fn new_own(&mut self) -> Result<PathBuf, String> {
        let folder = self.folder.clone().ok_or_else(|| crate::i18n::tr("There's no data folder to keep templates in.").to_string())?;
        std::fs::create_dir_all(&folder).map_err(|e| crate::i18n::tf!("Couldn't make the folder {}: {e}", folder.display()))?;
        let path = (1..1000).map(|n| folder.join(if n == 1 { format!("my-format.{ENDING}") } else { format!("my-format-{n}.{ENDING}") })).find(|p| !p.exists()).unwrap_or_else(|| folder.join(format!("my-format.{ENDING}")));
        let starter = "# A template of your own. See \"How templates are written\" in the Structure panel.\nname \"My format\"\nendings dat\n# magic str(0, 4) == \"ABCD\"\norder little\n\nstruct Header {\n    char signature[4]\n    u16 version\n    u16 count\n}\n\nHeader header\nu32 table[header.count] : hex\nu8 rest[]\n";
        std::fs::write(&path, starter).map_err(|e| crate::i18n::tf!("Couldn't write {}: {e}", path.display()))?;
        self.refresh();
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structure::{Bytes, NodeKind, Tree};
    use std::sync::Arc;

    /// The sample files, kept beside the source as ../testmedia, or where LIMONITER_TESTMEDIA says.
    fn media(path: &str) -> Option<Vec<u8>> {
        let root = std::env::var_os("LIMONITER_TESTMEDIA").map_or_else(|| PathBuf::from("../testmedia"), PathBuf::from);
        std::fs::read(root.join(path)).ok()
    }

    /// A file from inside a disk image or archive in the sample files.
    fn inside(image: &str, path: &[&str]) -> Option<Vec<u8>> {
        let bytes = media(image)?;
        let mut volume = crate::media::open(Arc::new(bytes), image).ok()?;
        let mut dir: Option<crate::media::Entry> = None;
        for (n, part) in path.iter().enumerate() {
            let entry = volume.list(dir.as_ref()).ok()?.into_iter().find(|e| e.name.eq_ignore_ascii_case(part))?;
            if n + 1 == path.len() {
                return volume.read(&entry).ok();
            }
            if entry.is_dir {
                dir = Some(entry);
            } else {
                // An image inside the image, such as a floppy in a ZIP.
                volume = crate::media::open(Arc::new(volume.read(&entry).ok()?), part).ok()?;
                dir = None;
            }
        }
        None
    }

    fn library() -> Library {
        Library::load(None)
    }

    /// Reads a file with the template chosen for it, checking it was the one expected.
    fn read_as(bytes: &[u8], ending: &str, expected: &str) -> Tree {
        let lib = library();
        let chosen = lib.choose(&Bytes(bytes), ending).map(|i| lib.entries[i].file.clone());
        assert_eq!(chosen.as_deref(), Some(expected));
        let tree = lib.entries[lib.find(expected, false).unwrap()].template.as_ref().unwrap().read(&Bytes(bytes));
        assert_eq!(tree.error, None, "{expected}");
        tree
    }

    fn get<'t>(tree: &'t Tree, path: &str) -> &'t crate::structure::Node {
        let i = (1..tree.nodes.len()).find(|i| tree.path(*i) == path).unwrap_or_else(|| panic!("no {path}"));
        &tree.nodes[i]
    }

    fn shown(tree: &Tree, path: &str) -> String {
        get(tree, path).shown.clone()
    }

    fn warnings(tree: &Tree) -> Vec<String> {
        tree.nodes.iter().filter(|n| n.kind == NodeKind::Warning).map(|n| n.shown.clone()).collect()
    }

    /// The labels of a list's entries.
    fn labels(tree: &Tree, path: &str) -> Vec<String> {
        get(tree, path).children.iter().map(|c| tree.nodes[*c].shown.clone()).collect()
    }

    #[test]
    fn every_built_in_template_is_read_without_a_mistake() {
        for (file, text) in BUILT_IN {
            if let Err(p) = Template::parse(text) {
                panic!("{file}, line {}: {}", p.line, p.message);
            }
        }
        // The language's description is there to be shown.
        assert!(LANGUAGE.contains("[while"));
    }

    #[test]
    fn amiga_hunk_executables() {
        let programs = [media("amiga-demos/SAE-MULater"), inside("amiga-floppy-fredfish-801/amiga-floppy-fredfish-801.adf", &["c", "MuchMore"])];
        let mut read = 0;
        for bytes in programs.into_iter().flatten() {
            let tree = read_as(&bytes, "", "amiga-hunk.structure");
            assert_eq!(tree.nodes[0].end, bytes.len() as u64);
            let kinds = labels(&tree, "hunks");
            assert!(kinds[0].starts_with("HUNK_CODE"), "{kinds:?}");
            assert!(kinds.last().unwrap().starts_with("HUNK_END"));
            let hunk_count = kinds.iter().filter(|k| k.starts_with("HUNK_END")).count();
            assert_eq!(shown(&tree, "table_size"), hunk_count.to_string());
            read += 1;
        }
        // A small one made here, so the template is tested without the sample files too.
        let mut bytes = Vec::new();
        for long in [0x3F3u32, 0, 1, 0, 0, 0x4000_0002, 0x3E9, 2, 0x4E75_4E75, 0x4E71_4E71, 0x3EC, 1, 0, 4, 0, 0x3F2] {
            bytes.extend(long.to_be_bytes());
        }
        let tree = read_as(&bytes, "", "amiga-hunk.structure");
        assert_eq!(labels(&tree, "hunks"), ["HUNK_CODE (0x3E9)", "HUNK_RELOC32 (0x3EC)", "HUNK_END (0x3F2)"]);
        assert_eq!(shown(&tree, "hunk_sizes"), "[0x40000002]");
        assert_eq!(shown(&tree, "hunks[1].groups[0].offsets"), "[0x00000004]");
        assert_eq!(shown(&tree, "hunks[0].memory"), "none (0x0)");
        assert!(read > 0 || media("amiga-demos/SAE-MULater").is_none());
    }

    #[test]
    fn amiga_disk_images() {
        let Some(disk) = media("amiga-floppy-fredfish-801/amiga-floppy-fredfish-801.adf") else {
            return;
        };
        let tree = read_as(&disk, "adf", "adf.structure");
        assert_eq!(shown(&tree, "boot.disk_type"), "\"DOS\"");
        assert_eq!(shown(&tree, "root.secondary_type"), "ST_ROOT (1)");
        assert_eq!(get(&tree, "root").start, 880 * 512);
        assert_eq!(shown(&tree, "root"), "\"AmigaLibDisk801\"");
        assert_eq!(shown(&tree, "root.created.days"), "1993-01-24");
        // An Archimedes disk is .adf too, but not an AmigaDOS one.
        if let Some(acorn) = media("acorn-user-coverdisk-february-1994/Acorn User (February 1994).adf") {
            assert!(library().choose(&Bytes(&acorn), "adf").is_none_or(|i| library().entries[i].file != "adf.structure"));
        }
    }

    /// A small ILBM picture, made here: a header, four colours, the view modes and a body.
    fn ilbm() -> Vec<u8> {
        let mut chunks = Vec::new();
        let mut chunk = |id: &[u8], body: &[u8]| {
            chunks.extend_from_slice(id);
            chunks.extend((body.len() as u32).to_be_bytes());
            chunks.extend_from_slice(body);
            if body.len() % 2 == 1 {
                chunks.push(0);
            }
        };
        let mut bmhd = Vec::new();
        bmhd.extend(320u16.to_be_bytes());
        bmhd.extend(200u16.to_be_bytes());
        bmhd.extend([0, 0, 0, 0, 2, 0, 1, 0, 0, 0, 10, 11]);
        bmhd.extend(320u16.to_be_bytes());
        bmhd.extend(200u16.to_be_bytes());
        chunk(b"BMHD", &bmhd);
        chunk(b"CMAP", &[0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 0, 255]);
        chunk(b"CAMG", &0x8804u32.to_be_bytes());
        chunk(b"ANNO", b"odd");
        chunk(b"BODY", &[0x81, 0x00, 0x81, 0x00]);
        let mut file = b"FORM".to_vec();
        file.extend((chunks.len() as u32 + 4).to_be_bytes());
        file.extend(b"ILBM");
        file.extend(chunks);
        file
    }

    #[test]
    fn iff_pictures_and_sounds() {
        let picture = ilbm();
        let tree = read_as(&picture, "iff", "iff.structure");
        assert_eq!(labels(&tree, "form.chunks"), ["\"BMHD\"", "\"CMAP\"", "\"CAMG\"", "\"ANNO\"", "\"BODY\""]);
        assert_eq!(shown(&tree, "form.chunks[0].width"), "320 (0x140)");
        assert_eq!(shown(&tree, "form.chunks[0].compression"), "byte_run1 (1)");
        assert_eq!(get(&tree, "form.chunks[1].colours").children.len(), 4);
        assert_eq!(shown(&tree, "form.chunks[2].view_modes"), "LACE | HAM | HIRES (0x8804)");
        assert_eq!(shown(&tree, "form.chunks[3].text"), "\"odd\"");
        // The odd chunk's padding byte is its own, and the next chunk starts after it.
        assert_eq!(get(&tree, "form.chunks[4]").start, get(&tree, "form.chunks[3].padding").end);
        assert_eq!(tree.nodes[0].end, picture.len() as u64);
        if let Some(sound) = media("sounds/tone.aiff") {
            let tree = read_as(&sound, "aiff", "iff.structure");
            assert_eq!(shown(&tree, "form.kind"), "\"AIFF\"");
            assert!(labels(&tree, "form.chunks").contains(&"\"COMM\"".to_string()));
        }
    }

    /// A Spectrum tape block: its length, flag, bytes and checksum.
    fn tap_block(flag: u8, bytes: &[u8]) -> Vec<u8> {
        let mut block = ((bytes.len() + 2) as u16).to_le_bytes().to_vec();
        block.push(flag);
        block.extend_from_slice(bytes);
        block.push(bytes.iter().fold(flag, |a, b| a ^ b));
        block
    }

    #[test]
    fn spectrum_tapes() {
        let mut header = vec![3];
        header.extend(b"screen    ");
        header.extend(6912u16.to_le_bytes());
        header.extend(16384u16.to_le_bytes());
        header.extend(32768u16.to_le_bytes());
        let mut tape = tap_block(0, &header);
        tape.extend(tap_block(0xFF, &[7; 6912]));
        let tree = read_as(&tape, "tap", "zx-tap.structure");
        assert_eq!(labels(&tree, "blocks"), ["\"screen\"", "6912"]);
        assert_eq!(shown(&tree, "blocks[0].header.type"), "bytes (3)");
        assert_eq!(shown(&tree, "blocks[0].header.start_address"), "0x4000");
        assert!(warnings(&tree).is_empty());
        // A wrong checksum is pointed out.
        let last = tape.len() - 1;
        tape[last] ^= 1;
        let tree = read_as(&tape, "tap", "zx-tap.structure");
        assert_eq!(warnings(&tree), ["The checksum doesn't match the block's bytes"]);
        // Oric tapes are .tap files too, and aren't read as the Spectrum's.
        if let Some(oric) = media("oric/ZZZ-UNK-Poker (F) (19xx).tap") {
            assert!(library().choose(&Bytes(&oric), "tap").is_none());
        }
        let mut count = 0;
        for name in ["astrocode-zx-spectrum/Astrocode.tzx", "zx_Dont_Buy_This/Don't Buy This (1985)(Firebird Software).tzx", "micro-PROLOG_SideA/micro-PROLOG_SideA.tzx"] {
            let Some(tzx) = media(name) else {
                continue;
            };
            let tree = read_as(&tzx, "tzx", "zx-tzx.structure");
            assert_eq!(tree.nodes[0].end, tzx.len() as u64, "{name}");
            assert_eq!(shown(&tree, "signature"), "\"ZXTape!\"");
            assert!(!labels(&tree, "blocks").is_empty());
            count += 1;
        }
        if count > 0 {
            let tzx = media("astrocode-zx-spectrum/Astrocode.tzx").unwrap();
            let tree = read_as(&tzx, "tzx", "zx-tzx.structure");
            assert!(shown(&tree, "blocks[0].id").starts_with("standard_speed") || shown(&tree, "blocks[0].id").starts_with("text_description") || shown(&tree, "blocks[0].id").starts_with("archive_info"), "{}", shown(&tree, "blocks[0].id"));
        }
    }

    #[test]
    fn spectrum_snapshots() {
        let mut sna = vec![0u8; 49179];
        sna[23..25].copy_from_slice(&0x8000u16.to_le_bytes());
        sna[25] = 1;
        sna[26] = 2;
        // The program counter, on the stack at 0x8000.
        sna[0x8000 - 0x4000 + 27..0x8000 - 0x4000 + 29].copy_from_slice(&0x1234u16.to_le_bytes());
        let tree = read_as(&sna, "sna", "zx-sna.structure");
        assert_eq!(shown(&tree, "registers.SP"), "0x8000");
        assert_eq!(shown(&tree, "registers.interrupt_mode"), "IM1 (1)");
        assert_eq!(shown(&tree, "PC"), "0x1234");
        assert_eq!(tree.nodes[0].end, sna.len() as u64);
        let mut read = 0;
        for name in ["zx_128k_Music_1989_Simon_Owen_128K/128k_Music_1989_Simon_Owen_128K.z80", "zx_Games_List_19xx_/Games_List_19xx__.z80", "zx_Sgt._Pepper_03_1988_Sgt._Pepper_a/Sgt._Pepper_03_1988_Sgt._Pepper_a.z80"] {
            let Some(z80) = media(name) else {
                continue;
            };
            let tree = read_as(&z80, "z80", "zx-z80.structure");
            assert_eq!(tree.nodes[0].end, z80.len() as u64, "{name}");
            if shown(&tree, "version") != "1" {
                let pages = labels(&tree, "pages");
                assert!(pages.len() >= 3, "{name}: {pages:?}");
            }
            read += 1;
        }
        let _ = read;
        // A version 1 file made here.
        let mut z80 = vec![0u8; 30];
        z80[6] = 0x34;
        z80[7] = 0x12;
        z80[12] = 0b0000_0110;
        z80.extend([0u8; 49152]);
        let tree = read_as(&z80, "z80", "zx-z80.structure");
        assert_eq!((shown(&tree, "version").as_str(), shown(&tree, "border").as_str(), shown(&tree, "PC").as_str()), ("1", "3", "0x1234"));
    }

    #[test]
    fn atari_st_programs() {
        if let Some(program) = inside("atari-st-gemdrive-21grpdg/21GRPDG.ZIP", &["21GRPDG", "BNOID.TOS"]) {
            let tree = read_as(&program, "tos", "atari-st-prg.structure");
            assert_eq!(shown(&tree, "header.branch"), "0x601A");
            assert_eq!(get(&tree, "text").end - get(&tree, "text").start, u64::from(u32::from_be_bytes(program[2..6].try_into().unwrap())));
            assert_eq!(tree.nodes[0].end, program.len() as u64);
        }
        let mut program = vec![0x60, 0x1A];
        for long in [4u32, 2, 100, 0, 0, 1] {
            program.extend(long.to_be_bytes());
        }
        program.extend([0, 0]);
        program.extend([0x4E, 0x75, 0x4E, 0x75, 0xAB, 0xCD]);
        program.extend(2u32.to_be_bytes());
        program.extend([0]);
        let tree = read_as(&program, "prg", "atari-st-prg.structure");
        assert_eq!(shown(&tree, "header.flags"), "FASTLOAD (0x1)");
        assert_eq!(shown(&tree, "data"), "AB CD");
        assert_eq!(shown(&tree, "first_relocation"), "0x00000002");
        assert_eq!(tree.nodes[0].end, program.len() as u64);
    }

    #[test]
    fn commodore_programs_and_disks() {
        let mut basic = 0x0801u16.to_le_bytes().to_vec();
        basic.extend(0x080Bu16.to_le_bytes());
        basic.extend(10u16.to_le_bytes());
        basic.extend([0x99, b' ', b'"', b'H', b'I', b'"', 0]);
        basic.extend([0, 0]);
        let tree = read_as(&basic, "prg", "c64-prg.structure");
        assert_eq!(shown(&tree, "load_address"), "0x0801");
        assert_eq!(labels(&tree, "lines"), ["10 (0xA)"]);
        assert_eq!(shown(&tree, "lines[0].text"), "\"\u{B7} \"HI\"\"");
        assert_eq!(tree.nodes[0].end, basic.len() as u64);
        let Some(disk) = media("d64_Biorytm64_1991_SYS_Public_Domain_sv/Biorytm64_1991_SYS_Public_Domain_sv.d64") else {
            return;
        };
        let tree = read_as(&disk, "d64", "c64-d64.structure");
        assert_eq!(get(&tree, "bam").start, 0x16500);
        assert_eq!(shown(&tree, "bam.dos_type"), "\"2A\"");
        // The directory names the same files as the disk browser.
        let volume = crate::media::open(Arc::new(disk.clone()), "x.d64").unwrap();
        let listed: Vec<String> = volume.list(None).unwrap().iter().map(|e| e.name.clone()).collect();
        let first = shown(&tree, "directory.entries[0].name");
        assert!(listed.iter().any(|name| first.contains(name.trim())), "{first} not in {listed:?}");
        // A program from the disk is read as a Commodore program, not an Atari one.
        let entry = volume.list(None).unwrap().into_iter().find(|e| !e.is_dir).unwrap();
        let program = volume.read(&entry).unwrap();
        let tree = read_as(&program, "prg", "c64-prg.structure");
        assert_eq!(tree.nodes[0].end, program.len() as u64);
    }

    #[test]
    fn bitmaps() {
        let mut bmp = b"BM".to_vec();
        let pixels_at = 14 + 40 + 8;
        bmp.extend((pixels_at as u32 + 8).to_le_bytes());
        bmp.extend([0, 0, 0, 0]);
        bmp.extend((pixels_at as u32).to_le_bytes());
        bmp.extend(40u32.to_le_bytes());
        bmp.extend(4i32.to_le_bytes());
        bmp.extend(2i32.to_le_bytes());
        bmp.extend(1u16.to_le_bytes());
        bmp.extend(1u16.to_le_bytes());
        bmp.extend([0; 24]);
        bmp.extend([0, 0, 0, 0, 255, 255, 255, 0]);
        bmp.extend([0x50, 0, 0, 0, 0xA0, 0, 0, 0]);
        let tree = read_as(&bmp, "bmp", "bmp.structure");
        assert_eq!(shown(&tree, "compression"), "BI_RGB (0)");
        assert_eq!(labels(&tree, "palette"), ["0", "16777215"]);
        assert_eq!(get(&tree, "pixels").start, pixels_at as u64);
        assert_eq!(tree.nodes[0].end, bmp.len() as u64);
    }

    #[test]
    fn png_pictures() {
        let Some(png) = media("pictures/mandrill.png") else {
            return;
        };
        let tree = read_as(&png, "png", "png.structure");
        let chunks = labels(&tree, "chunks");
        assert_eq!(chunks.first().map(String::as_str), Some("\"IHDR\""));
        assert_eq!(chunks.last().map(String::as_str), Some("\"IEND\""));
        assert!(warnings(&tree).is_empty(), "{:?}", warnings(&tree));
        assert_eq!(tree.nodes[0].end, png.len() as u64);
        // A damaged chunk's CRC is pointed out.
        let mut damaged = png.clone();
        damaged[20] ^= 0xFF;
        let tree = read_as(&damaged, "png", "png.structure");
        assert_eq!(warnings(&tree), ["The CRC doesn't match the chunk"]);
    }

    #[test]
    fn zip_archives() {
        let Some(zip) = media("atari-st-gemdrive-21grpdg/21GRPDG.ZIP") else {
            return;
        };
        let tree = read_as(&zip, "zip", "zip.structure");
        let local = labels(&tree, "files");
        let central = labels(&tree, "directory");
        assert_eq!(local, central);
        assert_eq!(shown(&tree, "end.entries"), format!("{0} (0x{0:X})", local.len()));
        assert!(local.contains(&"\"21GRPDG/BNOID.TOS\"".to_string()));
        assert_eq!(shown(&tree, "files[1].date"), "2021-10-11");
    }

    #[test]
    fn executables() {
        // A small 32-bit big-endian ELF header, made here.
        let mut elf = b"\x7FELF\x01\x02\x01\x00".to_vec();
        elf.extend([0; 8]);
        elf.extend(2u16.to_be_bytes());
        elf.extend(4u16.to_be_bytes());
        elf.extend(1u32.to_be_bytes());
        elf.extend(0x8000_0000u32.to_be_bytes());
        elf.extend([0; 8]);
        elf.extend([0; 4]);
        elf.extend(52u16.to_be_bytes());
        elf.extend([0; 10]);
        let tree = read_as(&elf, "", "elf.structure");
        assert_eq!(shown(&tree, "machine"), "m68k (4)");
        assert_eq!(shown(&tree, "entry"), "0x80000000");
        // Limoniter itself is an ELF on Linux and a PE on Windows, with its sections named.
        let me = std::fs::read(std::env::current_exe().unwrap()).unwrap();
        if cfg!(target_os = "linux") || cfg!(windows) {
            let tree = read_as(&me, if cfg!(windows) { "exe" } else { "" }, if cfg!(windows) { "mz-pe.structure" } else { "elf.structure" });
            let path = if cfg!(windows) { "pe.sections" } else { "sections" };
            let names = labels(&tree, path);
            assert!(names.iter().any(|n| n == "\".text\""), "{names:?}");
        }
        // A small MS-DOS program, made here, with one relocation.
        let mut mz = b"MZ".to_vec();
        for word in [0x40u16, 1, 1, 2, 0, 0xFFFF, 0, 0x100, 0, 0, 0, 0x1C, 0] {
            mz.extend(word.to_le_bytes());
        }
        mz.extend([0x10, 0, 0, 0]);
        mz.extend([0xB8, 0x00, 0x4C, 0xCD, 0x21]);
        let tree = read_as(&mz, "exe", "mz-pe.structure");
        assert_eq!(shown(&tree, "relocations[0].offset"), "0x0010");
        assert_eq!(get(&tree, "code").start, 32);
        if let Some(command) = inside("FD12FLOPPY/FD12FLOPPY.zip", &["FLOPPY.img", "COMMAND.COM"]).filter(|b| b.starts_with(b"MZ")) {
            read_as(&command, "com", "mz-pe.structure");
        }
    }

    #[test]
    fn wav_sounds() {
        let mut read = 0;
        for name in ["sounds/tone-adpcm.wav", "amiga-demos/mulater-study/rip/mulater-first45s.wav"] {
            let Some(wav) = media(name) else {
                continue;
            };
            let tree = read_as(&wav, "wav", "wav.structure");
            let chunks = labels(&tree, "chunks");
            let fmt = chunks.iter().position(|c| c == "\"fmt\"").unwrap();
            assert!(chunks.contains(&"\"data\"".to_string()));
            assert!(!shown(&tree, &format!("chunks[{fmt}].format")).contains("not one listed"));
            assert_eq!(tree.nodes[0].end, wav.len() as u64);
            read += 1;
        }
        let _ = read;
    }

    #[test]
    fn your_own_templates_come_first_and_their_mistakes_are_kept() {
        let folder = std::env::temp_dir().join(format!("limoniter-structures-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("mine.structure"), "name \"Mine\"\nmagic str(0, 4) == \"FORM\"\nchar form[4]\n").unwrap();
        std::fs::write(folder.join("broken.structure"), "name \"Broken\"\nu8 a\nwhatever b\n").unwrap();
        std::fs::write(folder.join("notes.txt"), "not a template").unwrap();
        let mut lib = Library::load(Some(folder.clone()));
        assert_eq!(lib.entries.len(), BUILT_IN.len() + 2);
        let broken = lib.find("broken.structure", true).unwrap();
        assert_eq!(lib.entries[broken].template.as_ref().err().map(|p| p.line), Some(3));
        let picture = ilbm();
        assert_eq!(lib.entries[lib.choose(&Bytes(&picture), "iff").unwrap()].title(), "Mine");
        // A copy of a built-in one goes in your folder, and is then the one used.
        let png = lib.find("png.structure", false).unwrap();
        let copied = lib.copy_to_own(png).unwrap();
        assert_eq!(copied, folder.join("png.structure"));
        assert!(lib.find("png.structure", true).is_some());
        assert!(!lib.refresh());
        std::fs::remove_file(folder.join("mine.structure")).unwrap();
        assert!(lib.refresh());
        assert_eq!(lib.entries[lib.choose(&Bytes(&picture), "iff").unwrap()].file, "iff.structure");
        let made = lib.new_own().unwrap();
        assert!(lib.entries[lib.find(&made.file_name().unwrap().to_string_lossy(), true).unwrap()].template.is_ok());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
