//! The Amiga's executables: AmigaDOS load files, made of hunks. Each SECTION of the source (code, data or BSS,
//! in chip or fast memory or either) is a hunk of its own, put wherever there's room when it's loaded; the
//! longs in a hunk that hold addresses in another (or the same) are listed in HUNK_RELOC32, for the loader to add
//! where that hunk went. The file is written as vasm writes it with -Fhunkexe -kick1hunks, so Kickstart 1.3
//! loads it too.
//!
//! To find those longs, the program is assembled twice, as the Atari ST's is: once with each section at an
//! address of its own, far apart, and once with them all moved on by `SHIFT`. A long that moved by exactly that
//! is an address; its value says which section it's in. Anything else that moved can't be moved by the loader.

use super::{Assembly, Problem};

/// How far apart sections are put while assembling: each may be up to half of this.
pub const SPACING: i64 = 0x0200_0000;
/// How far the second assembly moves every section: an address's top byte and its low word both change, so a
/// long holding one is told apart from a word or a byte that would need moving too, and can't be.
pub const SHIFT: i64 = 0x0100_0100;

pub const HUNK_HEADER: u32 = 0x3F3;
pub const HUNK_CODE: u32 = 0x3E9;
pub const HUNK_DATA: u32 = 0x3EA;
pub const HUNK_BSS: u32 = 0x3EB;
pub const HUNK_RELOC32: u32 = 0x3EC;
pub const HUNK_SYMBOL: u32 = 0x3F0;
pub const HUNK_END: u32 = 0x3F2;
/// HUNK_DREL32, which executables use for 16-bit relocations (from Kickstart 2.0): not written, nor read here.
#[cfg(test)]
pub const HUNK_RELOC32SHORT: u32 = 0x3F7;

/// What a section holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Code,
    Data,
    Bss,
}

/// The memory a section must be loaded into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Memory {
    Any,
    /// Chip memory, which the custom chips can see: for pictures, sound, copper lists and sprites.
    Chip,
    Fast,
}

impl Memory {
    /// The bits saying so, at the top of a hunk's size.
    fn flags(self) -> u32 {
        match self {
            Memory::Any => 0,
            Memory::Chip => 0x4000_0000,
            Memory::Fast => 0x8000_0000,
        }
    }
}

/// A section of the source, as assembled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub kind: Kind,
    pub memory: Memory,
    /// Where it was put while assembling, and where it got to.
    pub start: i64,
    pub end: i64,
}

impl Section {
    pub fn size(&self) -> i64 {
        self.end - self.start
    }
}

/// Where section number `index` is put while assembling, moved on by `shift`.
pub fn section_start(index: usize, shift: i64) -> i64 {
    (index as i64 + 1) * SPACING + shift
}

/// What a SECTION line (or CODE, DATA_C and the like) says: its name, kind and memory. `directive` is the word,
/// in lower case; `operands` what follows.
pub fn parse_section(directive: &str, operands: &str) -> Result<(String, Kind, Memory), String> {
    let kind_of = |word: &str| -> Result<(Kind, Memory), String> {
        let word = word.trim().trim_matches('"').to_ascii_lowercase();
        let (kind, memory) = match word.split_once('_') {
            Some((kind, "c" | "chip")) => (kind.to_string(), Memory::Chip),
            Some((kind, "f" | "fast")) => (kind.to_string(), Memory::Fast),
            Some((kind, "p" | "a" | "public")) => (kind.to_string(), Memory::Any),
            Some(_) => return Err(format!("{word} isn't a kind of section: CODE, DATA or BSS, with _C for chip memory or _F for fast")),
            None => (word, Memory::Any),
        };
        let kind = match kind.as_str() {
            "code" | "text" => Kind::Code,
            "data" => Kind::Data,
            "bss" => Kind::Bss,
            _ => return Err(format!("{kind} isn't a kind of section: CODE, DATA or BSS, with _C for chip memory or _F for fast")),
        };
        Ok((kind, memory))
    };
    if directive != "section" {
        let (kind, memory) = kind_of(directive)?;
        return Ok((directive.to_string(), kind, memory));
    }
    let items = super::split_operands(operands);
    let name = items.first().map(|n| n.trim().trim_matches('"').to_string()).filter(|n| !n.is_empty()).ok_or("SECTION needs a name")?;
    let (kind, mut memory) = match items.get(1) {
        Some(kind) => kind_of(kind)?,
        // A section with no kind: Devpac takes the name for it if it is one, else it's code.
        None => kind_of(&name).unwrap_or((Kind::Code, Memory::Any)),
    };
    if let Some(third) = items.get(2) {
        memory = match third.trim().to_ascii_lowercase().as_str() {
            "chip" => Memory::Chip,
            "fast" => Memory::Fast,
            other => return Err(format!("{other} isn't a kind of memory: CHIP or FAST")),
        };
    }
    Ok((name, kind, memory))
}

/// The sections being assembled into, and the one in use.
#[derive(Clone, Debug)]
pub struct Layout {
    pub shift: i64,
    pub sections: Vec<Section>,
    pub current: usize,
}

impl Layout {
    /// Before any SECTION, code goes in a section of its own, as vasm's does.
    pub fn new(shift: i64) -> Self {
        let start = section_start(0, shift);
        Layout { shift, sections: vec![Section { name: String::new(), kind: Kind::Code, memory: Memory::Any, start, end: start }], current: 0 }
    }

    /// Leaves the section in use at `pc`, and gives where to carry on in the one named: where it got to, if it's
    /// been used before, or its start.
    pub fn switch(&mut self, pc: i64, name: &str, kind: Kind, memory: Memory) -> Result<i64, String> {
        self.sections[self.current].end = pc;
        let found = self.sections.iter().position(|s| s.name.eq_ignore_ascii_case(name) && !s.name.is_empty());
        let index = match found {
            Some(index) if self.sections[index].kind != kind || self.sections[index].memory != memory => {
                return Err(format!("the section {name} was a different kind before"));
            }
            Some(index) => index,
            None => {
                let start = section_start(self.sections.len(), self.shift);
                self.sections.push(Section { name: name.to_string(), kind, memory, start, end: start });
                self.sections.len() - 1
            }
        };
        self.current = index;
        Ok(self.sections[index].end)
    }

}

/// The sections that hold anything, which become the hunks, in order.
fn hunks(assembly: &Assembly) -> Vec<&Section> {
    assembly.sections.iter().filter(|s| s.size() > 0).collect()
}

/// An AmigaDOS executable from a program assembled with its sections apart (`normal`), and again with them all
/// moved on by `SHIFT` (`moved`). With `symbols`, each hunk's labels go in a HUNK_SYMBOL, for debuggers.
pub fn executable(normal: &Assembly, moved: &Assembly, symbols: bool) -> Result<Vec<u8>, Problem> {
    let hunks = hunks(normal);
    if hunks.is_empty() {
        return Err(Problem { file: 0, line: 0, message: "nothing was assembled".into() });
    }
    // Which hunk an address is in, by the section it was put in, and how far into it.
    let section_of = |value: i64| -> Option<(usize, i64)> {
        let index = (value + SPACING / 2) / SPACING - 1;
        let section = normal.sections.get(usize::try_from(index).ok()?)?;
        let hunk = hunks.iter().position(|h| h.start == section.start)?;
        Some((hunk, value - section.start))
    };
    let mut file = Vec::new();
    let long = |file: &mut Vec<u8>, value: u32| file.extend(value.to_be_bytes());
    long(&mut file, HUNK_HEADER);
    long(&mut file, 0);
    long(&mut file, hunks.len() as u32);
    long(&mut file, 0);
    long(&mut file, hunks.len() as u32 - 1);
    for hunk in &hunks {
        long(&mut file, (hunk.size() as u32).div_ceil(4) | hunk.memory.flags());
    }
    for hunk in &hunks {
        let size = hunk.size();
        let longs = (size as u32).div_ceil(4);
        let mut bytes = normal.image(hunk.start, hunk.end);
        let there = moved.image(hunk.start + SHIFT, hunk.end + SHIFT);
        // The addresses in it: for each hunk they're in, where they are.
        let mut relocations: Vec<Vec<u32>> = vec![Vec::new(); hunks.len()];
        let mut at = 0;
        while at < bytes.len() {
            if bytes[at] == there[at] {
                at += 1;
                continue;
            }
            let address = (at + 4 <= bytes.len()).then(|| (u32::from_be_bytes(bytes[at..at + 4].try_into().expect("four")), u32::from_be_bytes(there[at..at + 4].try_into().expect("four"))));
            let problem = |message: String| {
                let (file, line) = normal.line_at(hunk.start + at as i64).unwrap_or((0, 0));
                Problem { file, line, message }
            };
            let Some((value, _)) = address.filter(|(a, b)| i64::from(b.wrapping_sub(*a)) == SHIFT) else {
                return Err(problem(format!("an address here (${at:X} into the {} section) can't be moved with the program: only a long (as DC.L label, or MOVE.L #label) can hold an address in an Amiga program, and only the same section can be reached relative to the PC", name_of(hunk))));
            };
            let Some((target, offset)) = section_of(i64::from(value)) else {
                return Err(problem(format!("the long here (${at:X} into the {} section) isn't an address in any section of the program", name_of(hunk))));
            };
            if hunk.kind == Kind::Bss {
                return Err(problem(format!("the {} section is BSS, which only leaves room: it can't hold an address", name_of(hunk))));
            }
            bytes[at..at + 4].copy_from_slice(&(offset as u32).to_be_bytes());
            relocations[target].push(at as u32);
            at += 4;
        }
        match hunk.kind {
            Kind::Bss => {
                if let Some(at) = bytes.iter().position(|b| *b != 0) {
                    let (file, line) = normal.line_at(hunk.start + at as i64).unwrap_or((0, 0));
                    return Err(Problem { file, line, message: format!("the {} section is BSS, which only leaves room: use a DATA section for what's put here", name_of(hunk)) });
                }
                long(&mut file, HUNK_BSS);
                long(&mut file, longs);
            }
            kind => {
                long(&mut file, if kind == Kind::Code { HUNK_CODE } else { HUNK_DATA });
                long(&mut file, longs);
                // Code is padded with NOPs when it can be.
                let pad = longs as usize * 4 - bytes.len();
                if kind == Kind::Code && pad == 2 && bytes.len().is_multiple_of(2) {
                    bytes.extend([0x4E, 0x71]);
                }
                bytes.resize(longs as usize * 4, 0);
                file.extend(&bytes);
                if relocations.iter().any(|r| !r.is_empty()) {
                    long(&mut file, HUNK_RELOC32);
                    for (target, places) in relocations.iter().enumerate().filter(|(_, p)| !p.is_empty()) {
                        long(&mut file, places.len() as u32);
                        long(&mut file, target as u32);
                        for place in places {
                            long(&mut file, *place);
                        }
                    }
                    long(&mut file, 0);
                }
            }
        }
        if symbols {
            let labels = labels_in(normal, hunk);
            if !labels.is_empty() {
                long(&mut file, HUNK_SYMBOL);
                for (name, offset) in labels {
                    let mut text = name.into_bytes();
                    text.truncate(0x00FF_FFFF);
                    let longs = text.len().div_ceil(4);
                    text.resize(longs * 4, 0);
                    long(&mut file, longs as u32);
                    file.extend(text);
                    long(&mut file, offset as u32);
                }
                long(&mut file, 0);
            }
        }
        long(&mut file, HUNK_END);
    }
    Ok(file)
}

/// Assembles 68000 source into an Amiga executable: the program as assembled (with its sections apart, for its
/// problems and labels), with the file if it could be made; if it couldn't, why is among the problems.
pub fn assemble_executable(path: &std::path::Path, text: &str, read: &dyn Fn(&std::path::Path) -> Result<Vec<u8>, String>, symbols: bool, options: &super::Options) -> (Assembly, Option<Vec<u8>>) {
    let isa = &super::m68k::M68k;
    let mut normal = super::assemble_sections(isa, path, text, read, 0, options);
    if !normal.ok() {
        return (normal, None);
    }
    let moved = super::assemble_sections(isa, path, text, read, SHIFT, options);
    match executable(&normal, &moved, symbols) {
        Ok(file) => (normal, Some(file)),
        Err(problem) => {
            normal.problems.push(problem);
            (normal, None)
        }
    }
}

fn name_of(section: &Section) -> String {
    if section.name.is_empty() { "first".to_string() } else { section.name.clone() }
}

/// A hunk's labels and where each is in it, in the order they're defined: not local labels, which only mean
/// something near where they are, nor constants.
fn labels_in(assembly: &Assembly, section: &Section) -> Vec<(String, i64)> {
    assembly
        .definitions
        .iter()
        .filter(|d| !d.written.starts_with('.') && !d.written.ends_with('$') && !d.name.contains('.'))
        .filter_map(|d| assembly.symbols.get(&d.name).map(|v| (d.name.clone(), *v)))
        .filter(|(_, v)| *v >= section.start && *v <= section.end)
        .map(|(name, v)| (name, v - section.start))
        .collect()
}

/// A hunk of an executable, read back: its kind, memory, bytes (or BSS size) and relocations.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedHunk {
    pub kind: Kind,
    pub memory: Memory,
    /// The bytes, or for BSS, as many zeros as it leaves room for.
    pub bytes: Vec<u8>,
    /// For each place in it holding an address: where, and in which hunk the address is.
    pub relocations: Vec<(u32, u32)>,
    pub symbols: Vec<(String, u32)>,
}

/// Reads an executable's hunks, as LoadSeg would, to check one.
#[cfg(test)]
pub fn read_executable(file: &[u8]) -> Result<Vec<LoadedHunk>, String> {
    let mut at = 0;
    let mut long = || -> Result<u32, String> {
        let value = file.get(at..at + 4).ok_or("The file ends too soon.")?;
        at += 4;
        Ok(u32::from_be_bytes(value.try_into().expect("four")))
    };
    if long()? != HUNK_HEADER {
        return Err("It isn't an Amiga executable: it doesn't start with HUNK_HEADER.".into());
    }
    while long()? != 0 {}
    let count = long()?;
    let (first, last) = (long()?, long()?);
    if count > 0xFFFF || last < first || last - first + 1 != count {
        return Err("Its header doesn't add up.".into());
    }
    let mut sizes = Vec::new();
    for _ in 0..count {
        let size = long()?;
        let memory = match size >> 30 {
            1 => Memory::Chip,
            2 => Memory::Fast,
            0 => Memory::Any,
            _ => {
                long()?;
                Memory::Any
            }
        };
        sizes.push(((size & 0x3FFF_FFFF) as usize * 4, memory));
    }
    let mut hunks: Vec<LoadedHunk> = Vec::new();
    let mut current: Option<LoadedHunk> = None;
    let mut at_short = None;
    while let Ok(id) = long() {
        match id & 0x3FFF_FFFF {
            HUNK_CODE | HUNK_DATA | HUNK_BSS => {
                let index = hunks.len();
                let (size, memory) = *sizes.get(index).ok_or("It has more hunks than its header says.")?;
                let length = long()? as usize * 4;
                let kind = match id & 0x3FFF_FFFF {
                    HUNK_CODE => Kind::Code,
                    HUNK_DATA => Kind::Data,
                    _ => Kind::Bss,
                };
                let mut bytes = if kind == Kind::Bss {
                    Vec::new()
                } else {
                    let mut taken = Vec::with_capacity(length);
                    for _ in 0..length / 4 {
                        taken.extend(long()?.to_be_bytes());
                    }
                    taken
                };
                bytes.resize(size.max(bytes.len()), 0);
                current = Some(LoadedHunk { kind, memory, bytes, relocations: Vec::new(), symbols: Vec::new() });
            }
            HUNK_RELOC32 => loop {
                let n = long()?;
                if n == 0 {
                    break;
                }
                let target = long()?;
                for _ in 0..n {
                    let place = long()?;
                    current.as_mut().ok_or("A relocation comes before its hunk.")?.relocations.push((place, target));
                }
            },
            HUNK_RELOC32SHORT => at_short = Some(()),
            HUNK_SYMBOL => loop {
                let n = long()?;
                if n == 0 {
                    break;
                }
                let mut name = Vec::new();
                for _ in 0..n & 0x00FF_FFFF {
                    name.extend(long()?.to_be_bytes());
                }
                while name.last() == Some(&0) {
                    name.pop();
                }
                let value = long()?;
                current.as_mut().ok_or("A symbol comes before its hunk.")?.symbols.push((String::from_utf8_lossy(&name).into_owned(), value));
            },
            HUNK_END => hunks.extend(current.take()),
            other => return Err(format!("It has a hunk of a kind not read here (${other:X}).")),
        }
        if at_short.take().is_some() {
            return Err("It has 16-bit relocations (HUNK_RELOC32SHORT), not read here.".into());
        }
    }
    hunks.extend(current.take());
    if hunks.len() != count as usize {
        return Err(format!("Its header says {count} hunks, but it has {}.", hunks.len()));
    }
    Ok(hunks)
}

/// Lays an executable's hunks out in memory as LoadSeg would, each at the address given, with its relocations
/// done: what a program finds when it runs. For comparing two executables that should do the same.
#[cfg(test)]
pub fn load(hunks: &[LoadedHunk], addresses: &[u32]) -> Vec<Vec<u8>> {
    hunks
        .iter()
        .map(|hunk| {
            let mut bytes = hunk.bytes.clone();
            for (place, target) in &hunk.relocations {
                let at = *place as usize;
                if let (Some(slot), Some(base)) = (bytes.get_mut(at..at + 4), addresses.get(*target as usize)) {
                    let value = u32::from_be_bytes((&*slot).try_into().expect("four")).wrapping_add(*base);
                    slot.copy_from_slice(&value.to_be_bytes());
                }
            }
            bytes
        })
        .collect()
}

/// Whether 68000 source is for an Amiga executable: it has sections, or uses the Amiga's chips or system
/// libraries, and isn't put at an address of its own with ORG (as a boot block or a trackloaded part is).
pub fn is_amiga_source(text: &str) -> bool {
    let mut amiga = false;
    for line in text.lines().take(4000) {
        let code = super::strip_comment(line).to_ascii_lowercase();
        let words: Vec<&str> = code.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).collect();
        let first_op = if line.starts_with(|c: char| c.is_whitespace()) { words.first() } else { words.get(1) }.copied();
        if first_op.is_some_and(|op| matches!(op, "org" | ".org")) {
            return false;
        }
        amiga = amiga || amiga_line(&code, first_op);
    }
    amiga
}

/// Whether a line of source (without its comment, in lower case) is the Amiga's.
fn amiga_line(code: &str, first_op: Option<&str>) -> bool {
    if first_op.is_some_and(|op| matches!(op, "section" | "code_c" | "data_c" | "bss_c" | "code_f" | "data_f" | "bss_f")) {
        return true;
    }
    if first_op.is_some_and(|op| matches!(op, "include" | "incdir")) && ["exec/", "hardware/", "graphics/", "dos/", "intuition/", "lvo/", "libraries/", "_lib.i"].iter().any(|h| code.contains(h)) {
        return true;
    }
    code.contains("$dff") || code.contains("_lvo") || code.contains("execbase") || code.contains("4.w,a6")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn no_files(path: &Path) -> Result<Vec<u8>, String> {
        Err(format!("{} isn't there", path.display()))
    }

    fn made(source: &str, symbols: bool) -> Result<Vec<u8>, String> {
        let (assembly, file) = assemble_executable(Path::new("t.s"), source, &no_files, symbols, &Default::default());
        file.ok_or_else(|| assembly.problems.iter().map(|p| format!("line {}: {}", p.line + 1, p.message)).collect::<Vec<_>>().join("\n"))
    }

    /// Programs with code, data and BSS sections, in chip and fast memory, addresses between them, sections used
    /// twice and empty ones, and every instruction form, made byte for byte as vasm 2.0f makes them (-Fhunkexe
    /// -no-opt -kick1hunks).
    #[test]
    fn amiga_executables_match_vasms() {
        // Every instruction form, in a section rather than at an ORG, with addresses into it from a data section.
        let all = include_str!("testdata/m68k_all.asm").replace("\torg $1000\n", "\tsection all,code\n").replace("\tdc.w start,$1234\n", "\tdc.w $4321,$1234\n") + "\tsection\ttable,data\n\tdc.l\tstart,start+2,table\ntable:\tdc.w\t0\n";
        for (name, source, expected) in [
            ("all", all.as_str(), &include_bytes!("testdata/amiga_all.exe")[..]),
            ("demo", include_str!("testdata/amiga_demo.s"), &include_bytes!("testdata/amiga_demo.exe")[..]),
            ("merge", include_str!("testdata/amiga_merge.s"), &include_bytes!("testdata/amiga_merge.exe")[..]),
            ("plain", include_str!("testdata/amiga_plain.s"), &include_bytes!("testdata/amiga_plain.exe")[..]),
            ("hello", include_str!("testdata/amiga_hello.s"), &include_bytes!("testdata/amiga_hello.exe")[..]),
        ] {
            let ours = made(source, true).unwrap();
            if ours != expected {
                let first = ours.iter().zip(expected).position(|(a, b)| a != b).unwrap_or(ours.len().min(expected.len()));
                let kept = std::env::temp_dir().join(format!("limoniter-{name}.exe"));
                let _ = std::fs::write(&kept, &ours);
                panic!("{name} differs from vasm's from byte ${first:X} ({} bytes to vasm's {}); ours is in {}", ours.len(), expected.len(), kept.display());
            }
            // Read back and loaded, it's what vasm's is too.
            let hunks = read_executable(&ours).unwrap();
            let at: Vec<u32> = (0..hunks.len() as u32).map(|n| 0x10000 * (n + 1)).collect();
            assert_eq!(load(&hunks, &at), load(&read_executable(expected).unwrap(), &at), "{name}");
        }
    }

    #[test]
    fn the_demos_hunks_are_where_and_what_they_should_be() {
        let hunks = read_executable(&made(include_str!("testdata/amiga_demo.s"), false).unwrap()).unwrap();
        let kinds: Vec<(Kind, Memory)> = hunks.iter().map(|h| (h.kind, h.memory)).collect();
        assert_eq!(kinds, [(Kind::Code, Memory::Any), (Kind::Data, Memory::Any), (Kind::Data, Memory::Chip), (Kind::Bss, Memory::Any), (Kind::Bss, Memory::Chip)]);
        assert_eq!(hunks[4].bytes.len(), 40 * 256);
        assert_eq!(hunks[3].bytes.len(), 1008, "2 + 4 + 1001 bytes, rounded up to a long");
        assert!(hunks.iter().all(|h| h.symbols.is_empty()), "no symbols unless asked for");
        // pointers: dc.l start,copper,frames,gfxname, loaded with the code at $10000 and so on.
        let loaded = load(&hunks, &[0x10000, 0x20000, 0x30000, 0x40000, 0x50000]);
        let longs: Vec<u32> = loaded[1][4..20].chunks(4).map(|c| u32::from_be_bytes(c.try_into().unwrap())).collect();
        assert_eq!(longs, [0x10000, 0x30000, 0x40002, 0x10000 + 0x88]);
    }

    #[test]
    fn what_cant_be_moved_is_a_problem() {
        let word = made("\tsection a,code\nstart:\tnop\n\tsection b,data\n\tdc.w\tstart\n", false);
        assert!(word.as_ref().is_err_and(|e| e.contains("only a long") && e.starts_with("line 4:")), "{word:?}");
        let bss = made("\tsection a,code\nstart:\tnop\n\tsection b,bss\n\tdc.l\t5\n", false);
        assert!(bss.as_ref().is_err_and(|e| e.contains("BSS")), "{bss:?}");
        let (assembly, _) = assemble_executable(Path::new("t.s"), "\torg $1000\n\tnop\n", &no_files, false, &Default::default());
        assert!(assembly.problems.iter().any(|p| p.message.contains("ORG")));
        let (assembly, _) = assemble_executable(Path::new("t.s"), "\tsection a,code\n\tnop\n\tsection a,data\n", &no_files, false, &Default::default());
        assert!(!assembly.ok(), "a section can't change its kind");
    }

    #[test]
    fn sections_are_read_as_vasm_and_devpac_write_them() {
        assert_eq!(parse_section("section", "main,code_c"), Ok(("main".into(), Kind::Code, Memory::Chip)));
        assert_eq!(parse_section("section", "pic,DATA,chip"), Ok(("pic".into(), Kind::Data, Memory::Chip)));
        assert_eq!(parse_section("section", "vars,bss_f"), Ok(("vars".into(), Kind::Bss, Memory::Fast)));
        assert_eq!(parse_section("section", "text"), Ok(("text".into(), Kind::Code, Memory::Any)));
        assert_eq!(parse_section("section", "\"x\",data"), Ok(("x".into(), Kind::Data, Memory::Any)));
        assert_eq!(parse_section("bss_c", ""), Ok(("bss_c".into(), Kind::Bss, Memory::Chip)));
        assert!(parse_section("section", "x,rodata").is_err());
        assert!(parse_section("section", "").is_err());
    }

    #[test]
    fn amiga_source_is_told_from_the_atari_sts() {
        assert!(is_amiga_source("\tsection code,code_c\n\tmoveq #0,d0\n"));
        assert!(is_amiga_source("\tlea $dff000,a5\n"));
        assert!(is_amiga_source("\tmove.l 4.w,a6\n\tjsr _LVOOpenLibrary(a6)\n"));
        assert!(is_amiga_source("\tinclude \"exec/exec_lib.i\"\n"));
        assert!(!is_amiga_source("start:\tpea message\n\tmove.w #9,-(sp)\n\ttrap #1\n"));
        assert!(!is_amiga_source("; a section about sections\n\tnop\n"));
        assert!(!is_amiga_source("\torg $40000\n\tmove.w #$0f00,$dff180\n"), "with an ORG, it's put where it says");
    }
}
