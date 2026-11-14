//! Hints for Amiga code from Hyperion's Native Developer Kit for AmigaOS 3.2: each system function's library,
//! offset (its _LVO), the registers its arguments go in, its C prototype, and its autodoc, shown when the pointer
//! rests on the function's name in 68000 or C source.
//!
//! The NDK is Hyperion's, free to download but not Limoniter's to hand out, so Limoniter doesn't carry it: at the
//! person's request it downloads it from Aminet and unpacks it beside the settings, or reads one already on the
//! computer. Offsets and registers come from its SFD files (each function six bytes further down the library's jump
//! table from its ==bias, an ==alias or ==varargs line sharing the slot before it, ==reserve skipping slots), and
//! the words from its Autodocs, a page per function between form feeds.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Where Aminet keeps the NDK.
pub const ADDRESS: &str = "https://aminet.net/dev/misc/NDK3.2.lha";
const MOST_BYTES: u64 = 64 << 20;

/// One function of a library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub library: String,
    /// Its place in the library's jump table, below the base: AllocMem's is -198.
    pub offset: i32,
    /// Each argument's name and register.
    pub arguments: Vec<(String, String)>,
    pub prototype: String,
    /// The library version it came in, when the SFD says.
    pub version: Option<u32>,
    pub private: bool,
}

/// What the NDK knows, by function name.
#[derive(Debug, Default)]
pub struct Index {
    pub functions: BTreeMap<String, Function>,
    /// Each function's autodoc page (and devices' and others' that have no SFD), by name.
    pub docs: BTreeMap<String, (String, String)>,
    /// The NDK's folder.
    pub folder: PathBuf,
}

/// The arguments' names in a C prototype: in `ULONG (*hook)()` the name in brackets, otherwise each one's last word.
fn argument_names(prototype: &str) -> Vec<String> {
    let Some(open) = prototype.find('(') else {
        return Vec::new();
    };
    // The argument list runs to the bracket that closes the first.
    let inside = &prototype[open + 1..];
    let mut depth = 0;
    let mut end = inside.len();
    for (i, c) in inside.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => {
                end = i;
                break;
            }
            ')' => depth -= 1,
            _ => {}
        }
    }
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0, 0);
    let list = &inside[..end];
    for (i, c) in list.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&list[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&list[start..]);
    let word = |s: &str| s.split(|c: char| !(c.is_alphanumeric() || c == '_')).rfind(|w| !w.is_empty()).map(str::to_string);
    parts
        .iter()
        .filter(|p| !p.trim().is_empty() && p.trim() != "VOID" && p.trim() != "void")
        .filter_map(|p| match p.find("(*") {
            Some(at) => word(&p[at + 2..p[at..].find(')').map_or(p.len(), |e| at + e)]),
            None => word(p),
        })
        .collect()
}

/// Reads one SFD file's functions.
pub fn read_sfd(text: &str) -> Vec<Function> {
    let mut found = Vec::new();
    let (mut library, mut bias, mut private, mut version) = (String::new(), 30i32, false, None);
    // A function's declaration runs over lines until its registers, in brackets at the end, are reached.
    let mut pending = String::new();
    // The next function shares the slot of the one before.
    let mut shared = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if !pending.is_empty() {
            pending.push(' ');
            pending.push_str(trimmed);
        } else if let Some(rest) = trimmed.strip_prefix("==") {
            let (word, value) = rest.split_once(char::is_whitespace).map_or((rest, ""), |(w, v)| (w, v.trim()));
            match word {
                "libname" => library = value.to_string(),
                "bias" => bias = value.parse().unwrap_or(bias),
                "private" => private = true,
                "public" => private = false,
                "version" => version = value.parse().ok(),
                "reserve" => bias += 6 * value.parse::<i32>().unwrap_or(1),
                "alias" | "varargs" => shared = true,
                "end" => break,
                _ => {}
            }
            continue;
        } else if trimmed.is_empty() || trimmed.starts_with('*') {
            continue;
        } else {
            pending = trimmed.to_string();
        }
        // Complete once it ends with its registers' brackets, after the argument list's.
        let complete = pending.ends_with(')') && pending.matches('(').count() == pending.matches(')').count() && {
            let registers_open = pending.rfind('(').unwrap_or(0);
            let before = pending[..registers_open].trim_end();
            before.ends_with(')')
        };
        if !complete {
            continue;
        }
        let declaration = std::mem::take(&mut pending);
        let registers_open = declaration.rfind('(').unwrap_or(0);
        let registers: Vec<String> = declaration[registers_open + 1..declaration.len() - 1].split([',', '/']).map(|r| r.trim().to_ascii_lowercase()).filter(|r| !r.is_empty()).collect();
        let prototype = declaration[..registers_open].split_whitespace().collect::<Vec<_>>().join(" ").replace("( ", "(").replace(" )", ")");
        let name_end = prototype.find('(').unwrap_or(prototype.len());
        let name = prototype[..name_end].rsplit(|c: char| !(c.is_alphanumeric() || c == '_')).next().unwrap_or("").to_string();
        let offset = if shared { bias - 6 } else { bias };
        if !shared {
            bias += 6;
        }
        shared = false;
        if name.is_empty() {
            continue;
        }
        let names = argument_names(&prototype);
        let arguments = registers.iter().enumerate().map(|(i, r)| (names.get(i).cloned().unwrap_or_else(|| format!("argument {}", i + 1)), r.clone())).collect();
        found.push(Function { name, library: library.clone(), offset: -offset, arguments, prototype, version, private });
    }
    found
}

/// Reads one autodoc file's pages: (library or device, function name, the page).
pub fn read_autodoc(text: &str) -> Vec<(String, String, String)> {
    let mut pages = Vec::new();
    for page in text.split('\x0c') {
        let mut lines = page.lines().skip_while(|l| l.trim().is_empty());
        let Some(header) = lines.next() else {
            continue;
        };
        let Some(title) = header.split_whitespace().next() else {
            continue;
        };
        let Some((library, name)) = title.rsplit_once('/') else {
            continue;
        };
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let body: Vec<&str> = lines.collect();
        let body = body.join("\n");
        pages.push((library.to_string(), name.to_string(), body.trim_matches('\n').to_string()));
    }
    pages
}

fn read_text(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|bytes| bytes.iter().map(|&b| b as char).collect())
}

impl Index {
    /// Reads an NDK's SFD and Autodocs folders.
    pub fn read(folder: &Path) -> Result<Index, String> {
        let mut index = Index { folder: folder.to_path_buf(), ..Index::default() };
        let listed = |name: &str| -> Vec<PathBuf> {
            let mut files: Vec<PathBuf> = std::fs::read_dir(folder.join(name)).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
            files.sort();
            files
        };
        for path in listed("SFD").iter().filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("sfd"))) {
            for function in read_text(path).map(|t| read_sfd(&t)).unwrap_or_default() {
                index.functions.entry(function.name.clone()).or_insert(function);
            }
        }
        for path in listed("Autodocs").iter().filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("doc"))) {
            for (library, name, page) in read_text(path).map(|t| read_autodoc(&t)).unwrap_or_default() {
                index.docs.entry(name).or_insert((library, page));
            }
        }
        if index.functions.is_empty() {
            return Err("There's no NDK there: no SFD folder with libraries' functions in it.".into());
        }
        Ok(index)
    }

    /// What to show for a word: the function's prototype, library, offset and registers, and its autodoc.
    pub fn describe(&self, word: &str) -> Option<String> {
        let name = word.strip_prefix("_LVO").unwrap_or(word);
        let function = self.functions.get(name);
        let doc = self.docs.get(name);
        if function.is_none() && doc.is_none() {
            return None;
        }
        let mut out = String::new();
        if let Some(f) = function {
            out.push_str(&f.prototype);
            out.push_str(&format!("\n{}, _LVO{} = {}", f.library, f.name, f.offset));
            if let Some(v) = f.version {
                out.push_str(&format!(", from V{v}"));
            }
            if f.private {
                out.push_str(", private");
            }
            if !f.arguments.is_empty() {
                let registers: Vec<String> = f.arguments.iter().map(|(n, r)| format!("{n} in {r}")).collect();
                out.push_str(&format!("\n{}", registers.join(", ")));
            }
        }
        if let Some((library, page)) = doc {
            if function.is_none() {
                out.push_str(library);
            }
            // The page as written, shortened for a hover.
            let lines: Vec<&str> = page.lines().collect();
            let shown: Vec<String> = lines.iter().take(40).map(|l| l.replace('\t', "    ")).collect();
            out.push_str("\n\n");
            out.push_str(shown.join("\n").trim_end());
            if lines.len() > 40 {
                out.push_str("\n\u{2026}");
            }
        }
        Some(out)
    }

    /// Names that finish a prefix: functions, or `_LVO` names when that's how it was started.
    pub fn complete(&self, prefix: &str) -> Vec<String> {
        let (lvo, rest) = match prefix.strip_prefix("_LVO") {
            Some(rest) => (true, rest),
            None => (false, prefix),
        };
        if rest.len() < 2 && !lvo {
            return Vec::new();
        }
        let lower = rest.to_lowercase();
        self.functions
            .values()
            .filter(|f| !f.private && f.name.to_lowercase().starts_with(&lower) && f.name != rest)
            .map(|f| if lvo { format!("_LVO{}", f.name) } else { f.name.clone() })
            .take(40)
            .collect()
    }
}

/// The NDK being used, once read; `None` inside when there's none.
static CURRENT: RwLock<Option<Option<Arc<Index>>>> = RwLock::new(None);

/// Where a downloaded NDK goes.
pub fn download_folder() -> Option<PathBuf> {
    crate::settings::data_folder().map(|dir| dir.join("amiga-ndk"))
}

/// The small file that names an NDK folder chosen by hand.
fn choice_file() -> Option<PathBuf> {
    crate::settings::data_folder().map(|dir| dir.join("amiga-ndk-folder.txt"))
}

/// The NDK folder to use: one chosen by hand, or the one downloaded, if either is there.
pub fn chosen_folder() -> Option<PathBuf> {
    if let Some(named) = choice_file().and_then(|f| std::fs::read_to_string(f).ok()).map(|t| PathBuf::from(t.trim())).filter(|p| p.join("SFD").is_dir()) {
        return Some(named);
    }
    download_folder().filter(|f| f.join("SFD").is_dir())
}

/// Remembers a folder chosen by hand (or forgets it), and uses what's chosen from now on. Gives the functions known.
pub fn choose(folder: Option<&Path>) -> Result<Option<usize>, String> {
    if let Some(folder) = folder {
        Index::read(folder)?;
    }
    if let Some(file) = choice_file() {
        match folder {
            Some(folder) => {
                let _ = std::fs::create_dir_all(file.parent().expect("in a folder"));
                std::fs::write(&file, folder.display().to_string()).map_err(|e| format!("The choice couldn't be saved: {e}"))?;
            }
            None => {
                let _ = std::fs::remove_file(&file);
            }
        }
    }
    set_folder(chosen_folder().as_deref())
}

/// Uses the NDK in a folder (or none), from now on.
pub fn set_folder(folder: Option<&Path>) -> Result<Option<usize>, String> {
    let index = folder.map(Index::read).transpose()?.map(Arc::new);
    let count = index.as_ref().map(|i| i.functions.len());
    if let Ok(mut current) = CURRENT.write() {
        *current = Some(index);
    }
    Ok(count)
}

/// The NDK in use, read the first time it's asked for. None in tests, unless one is set.
pub fn current() -> Option<Arc<Index>> {
    if let Some(known) = CURRENT.read().ok().and_then(|c| c.clone()) {
        return known;
    }
    let folder = if cfg!(test) { None } else { chosen_folder() };
    let index = folder.and_then(|f| Index::read(&f).ok()).map(Arc::new);
    if let Ok(mut current) = CURRENT.write() {
        *current = Some(index.clone());
    }
    index
}

/// Downloads the NDK from Aminet and unpacks it into a folder, replacing what was there. Gives the number of files.
pub fn download(into: &Path) -> Result<usize, String> {
    let response = ureq::get(ADDRESS).header("User-Agent", "Limoniter").call().map_err(|e| format!("Aminet couldn't be reached: {e}"))?;
    let bytes = response.into_body().with_config().limit(MOST_BYTES).read_to_vec().map_err(|e| format!("The NDK couldn't be downloaded: {e}"))?;
    unpack(bytes, into)
}

/// Unpacks the NDK's archive into a folder: into a new folder beside it first, so a failure leaves the old one.
pub fn unpack(archive: Vec<u8>, into: &Path) -> Result<usize, String> {
    let volume = crate::media::open(Arc::new(archive), "NDK3.2.lha")?;
    let fresh = into.with_extension("new");
    let _ = std::fs::remove_dir_all(&fresh);
    std::fs::create_dir_all(&fresh).map_err(|e| format!("The folder for the NDK couldn't be made: {e}"))?;
    let mut count = 0;
    let mut todo = vec![(None, fresh.clone())];
    while let Some((dir, at)) = todo.pop() {
        for entry in volume.list(dir.as_ref())? {
            let name = crate::media::tidy_name(&entry.name);
            if name == "." || name == ".." {
                continue;
            }
            let path = at.join(&name);
            if entry.is_dir {
                std::fs::create_dir_all(&path).map_err(|e| format!("{name} couldn't be made: {e}"))?;
                todo.push((Some(entry), path));
            } else {
                let bytes = volume.read(&entry)?;
                std::fs::write(&path, bytes).map_err(|e| format!("{name} couldn't be written: {e}"))?;
                count += 1;
            }
        }
    }
    Index::read(&fresh)?;
    let _ = std::fs::remove_dir_all(into);
    std::fs::rename(&fresh, into).map_err(|e| format!("The NDK couldn't be put in place: {e}"))?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SFD: &str = "==id $VER: exec_lib.sfd 47.5 (30.11.2021)\n* \"exec.library\"\n==base _SysBase\n==libname exec.library\n==bias 30\n==public\nULONG\tSupervisor( ULONG (*userFunction)() ) (a5)\n==private\nVOID\tExitIntr() ()\n==public\nstruct Library *MakeLibrary( CONST_APTR funcInit, CONST_APTR structInit, ULONG (*libInit)(),\n\tULONG dataSize, ULONG segList ) (a0,a1,a2,d0,d1)\n==reserve 2\n==version 33\nAPTR\tAllocMem( ULONG byteSize, ULONG requirements ) (d0,d1)\n==alias\nAPTR\tAllocMemAlias( ULONG byteSize, ULONG requirements ) (d0,d1)\nVOID\tFreeMem( APTR memoryBlock, ULONG byteSize ) (a1,d0)\n==end\n";

    #[test]
    fn an_sfd_gives_each_function_its_offset_registers_and_prototype() {
        let functions = read_sfd(SFD);
        let by_name = |n: &str| functions.iter().find(|f| f.name == n).unwrap().clone();
        assert_eq!(by_name("Supervisor").offset, -30);
        assert_eq!(by_name("Supervisor").arguments, [("userFunction".to_string(), "a5".to_string())]);
        assert!(by_name("ExitIntr").private);
        assert_eq!(by_name("ExitIntr").offset, -36);
        let make = by_name("MakeLibrary");
        assert_eq!(make.offset, -42, "a declaration over two lines");
        assert_eq!(make.arguments.iter().map(|(n, r)| format!("{n}:{r}")).collect::<Vec<_>>(), ["funcInit:a0", "structInit:a1", "libInit:a2", "dataSize:d0", "segList:d1"]);
        let alloc = by_name("AllocMem");
        assert_eq!(alloc.offset, -60, "two slots reserved after -42's");
        assert_eq!(alloc.version, Some(33));
        assert_eq!(alloc.prototype, "APTR AllocMem(ULONG byteSize, ULONG requirements)");
        assert_eq!(by_name("AllocMemAlias").offset, -60, "an alias shares the slot");
        assert_eq!(by_name("FreeMem").offset, -66);
    }

    #[test]
    fn an_autodoc_is_split_into_its_pages() {
        let doc = "TABLE OF CONTENTS\n\nexec.library/AllocMem\n\x0cexec.library/AllocMem                         exec.library/AllocMem\n\n   NAME\n\tAllocMem -- allocate memory\n\n   SYNOPSIS\n\tmemoryBlock = AllocMem(byteSize, attributes)\n\x0c";
        let pages = read_autodoc(doc);
        assert_eq!(pages.len(), 1, "the table of contents isn't a page");
        assert_eq!(pages[0].0, "exec.library");
        assert_eq!(pages[0].1, "AllocMem");
        assert!(pages[0].2.starts_with("   NAME"));
    }

    #[test]
    fn a_word_is_described_and_completed_from_the_index() {
        let mut index = Index::default();
        for f in read_sfd(SFD) {
            index.functions.insert(f.name.clone(), f);
        }
        index.docs.insert("AllocMem".into(), ("exec.library".into(), "   NAME\n\tAllocMem -- allocate memory".into()));
        let words = index.describe("_LVOAllocMem").unwrap();
        assert!(words.starts_with("APTR AllocMem(ULONG byteSize, ULONG requirements)\nexec.library, _LVOAllocMem = -60, from V33\nbyteSize in d0, requirements in d1\n\n   NAME"), "{words}");
        assert_eq!(index.describe("AllocMem"), Some(words));
        assert!(index.describe("Nothing").is_none());
        assert_eq!(index.complete("_LVOAll"), ["_LVOAllocMem", "_LVOAllocMemAlias"]);
        assert_eq!(index.complete("Fre"), ["FreeMem"]);
        assert!(index.complete("Exi").is_empty(), "private functions aren't offered");
        assert!(index.complete("F").is_empty(), "not from one letter");
    }

    /// Reads a real NDK and checks its offsets with its own FD files: LIMONITER_NDK=<folder> cargo test -- --ignored
    #[test]
    #[ignore]
    fn a_real_ndk_agrees_with_its_fd_files() {
        // The archive as Aminet has it, unpacked as Limoniter unpacks it, or a folder.
        if let Ok(archive) = std::env::var("LIMONITER_NDK_LHA") {
            let into = std::env::temp_dir().join(format!("limoniter-ndk-{}", std::process::id()));
            let files = unpack(std::fs::read(&archive).unwrap(), &into).unwrap();
            println!("{files} files unpacked");
            unsafe { std::env::set_var("LIMONITER_NDK", &into) };
        }
        let Ok(folder) = std::env::var("LIMONITER_NDK") else { return };
        let index = Index::read(Path::new(&folder)).unwrap();
        let mut checked = 0;
        for path in std::fs::read_dir(Path::new(&folder).join("FD")).unwrap().flatten().map(|e| e.path()) {
            let text = read_text(&path).unwrap();
            let mut bias = 0;
            for line in text.lines() {
                if let Some(b) = line.strip_prefix("##bias") {
                    bias = b.trim().parse().unwrap();
                } else if line.starts_with("##end") {
                    break;
                } else if !line.starts_with('*') && !line.starts_with("##") && line.contains('(') {
                    let name = &line[..line.find('(').unwrap()];
                    if let Some(f) = index.functions.get(name) {
                        assert_eq!(f.offset, -bias, "{name} in {}", path.display());
                        checked += 1;
                    }
                    bias += 6;
                }
            }
        }
        println!("{} functions, {} pages, {checked} offsets checked against FD files", index.functions.len(), index.docs.len());
        assert!(checked > 1000);
        assert!(index.describe("_LVOAllocMem").unwrap().contains("-198"));
    }
}
