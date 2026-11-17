//! Other people's packers, for Atari ST programs and data: ZX0 (salvador, or Einar Saukas's zx0), LZSA, LZ4,
//! Shrinkler in its data mode and UPX for TOS programs. They're found where they're installed or where the
//! person points, fetched or built only when asked, and run in the background; the 68000 calls to unpack what
//! they made are written here, naming the depacker's own source file rather than copying it. Also the YM2149's
//! note tables and envelope shapes, for writing music and sound effects by hand.

use crate::amiga_tools::{Outcome, group, run};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Salvador,
    Zx0,
    Lzsa,
    Lz4,
    Shrinkler,
    Upx,
}

impl Tool {
    pub const ALL: [Tool; 6] = [Tool::Salvador, Tool::Zx0, Tool::Lzsa, Tool::Lz4, Tool::Shrinkler, Tool::Upx];

    pub fn id(self) -> &'static str {
        match self {
            Tool::Salvador => "salvador",
            Tool::Zx0 => "zx0",
            Tool::Lzsa => "lzsa",
            Tool::Lz4 => "lz4",
            Tool::Shrinkler => "shrinkler",
            Tool::Upx => "upx",
        }
    }

    pub fn from_id(id: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.id() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            Tool::Salvador => "salvador (ZX0)",
            Tool::Zx0 => "zx0 (ZX0)",
            Tool::Lzsa => "LZSA",
            Tool::Lz4 => "LZ4",
            Tool::Shrinkler => "Shrinkler (data)",
            Tool::Upx => "UPX (TOS programs)",
        }
    }

    /// What the packed file is, for its depacker.
    pub fn format(self) -> &'static str {
        match self {
            Tool::Salvador | Tool::Zx0 => "ZX0",
            Tool::Lzsa => "LZSA",
            Tool::Lz4 => "LZ4 frame",
            Tool::Shrinkler => "Shrinkler data",
            Tool::Upx => "UPX",
        }
    }

    fn programs(self) -> &'static [&'static str] {
        match self {
            Tool::Salvador => &["salvador"],
            Tool::Zx0 => &["zx0"],
            Tool::Lzsa => &["lzsa"],
            Tool::Lz4 => &["lz4"],
            Tool::Shrinkler => &["Shrinkler", "shrinkler"],
            Tool::Upx => &["upx", "upx-ucl"],
        }
    }

    fn folder_hints(self) -> &'static [&'static str] {
        match self {
            Tool::Salvador => &["salvador"],
            Tool::Zx0 => &["zx0"],
            Tool::Lzsa => &["lzsa"],
            Tool::Lz4 => &["lz4"],
            Tool::Shrinkler => &["shrinkler"],
            Tool::Upx => &["upx"],
        }
    }

    pub fn website(self) -> &'static str {
        match self {
            Tool::Salvador => "https://github.com/emmanuel-marty/salvador",
            Tool::Zx0 => "https://github.com/einar-saukas/ZX0",
            Tool::Lzsa => "https://github.com/emmanuel-marty/lzsa",
            Tool::Lz4 => "https://github.com/lz4/lz4",
            Tool::Shrinkler => "https://github.com/askeksa/Shrinkler",
            Tool::Upx => "https://upx.github.io",
        }
    }

    /// Whose it is and on what terms, as its own package says.
    pub fn licence(self) -> &'static str {
        match self {
            Tool::Salvador => "Emmanuel Marty. zlib licence (its match finder CC0). Writes ZX0, Einar Saukas's format, packing harder than zx0 itself.",
            Tool::Zx0 => "Einar Saukas. BSD 3-clause licence.",
            Tool::Lzsa => "Emmanuel Marty. zlib licence (its match finder CC0).",
            Tool::Lz4 => "Yann Collet. The lz4 program is GNU GPL version 2; its library BSD 2-clause.",
            Tool::Shrinkler => "Aske Simon Christensen (Blueberry). Its own permissive licence; its data decompressor, ShrinklerDecompress.S, is free to use in whole or in part.",
            Tool::Upx => "Markus Oberhumer, L\u{e1}szl\u{f3} Moln\u{e1}r and John Reiser. GNU GPL version 2 or later, with an exception for the programs it packs.",
        }
    }

    /// The depacker for 68000 that unpacks what it makes: its file, where it comes from, and its terms.
    pub fn depacker(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Tool::Salvador | Tool::Zx0 => ("unzx0_68000.S (zx0_decompress, 88 bytes)", "https://github.com/emmanuel-marty/unzx0_68000", "Emmanuel Marty, zlib licence"),
            Tool::Lzsa => ("lzsa.s (lzsa_depack_stream)", "https://github.com/tattlemuss/lz4-m68k", "Steven Tattersall, MIT licence"),
            Tool::Lz4 => ("lz4_frame.asm (lz4_frame_depack)", "https://github.com/arnaud-carre/lz4-68k", "Arnaud Carr\u{e9} (Leonard/Oxygene), MIT licence"),
            Tool::Shrinkler => ("ShrinklerDecompress.S (ShrinklerDecompress)", "https://github.com/askeksa/Shrinkler", "Aske Simon Christensen, free to use"),
            Tool::Upx => ("none: a packed program unpacks itself when it runs", "https://upx.github.io", "UPX's own"),
        }
    }

    /// Where its release can be fetched from, for this system: the address and the program's path inside its zip
    /// (or None when the address is the program itself).
    pub fn release(self) -> Option<(&'static str, Option<&'static str>)> {
        if self == Tool::Shrinkler {
            return crate::amiga_tools::Tool::Shrinkler.release().map(|(a, inside)| (a, Some(inside)));
        }
        if !cfg!(windows) {
            return None;
        }
        Some(match self {
            Tool::Salvador => ("https://github.com/emmanuel-marty/salvador/releases/download/1.4.2/salvador_win64_1.4.2.zip", Some("salvador.exe")),
            Tool::Zx0 => ("https://raw.githubusercontent.com/einar-saukas/ZX0/main/win/zx0.exe", None),
            Tool::Lzsa => ("https://github.com/emmanuel-marty/lzsa/releases/download/1.4.1/lzsa_win64_1.4.1.zip", Some("lzsa.exe")),
            Tool::Lz4 => ("https://github.com/lz4/lz4/releases/download/v1.11.1/lz4_win64_v1_11_1.zip", Some("lz4.exe")),
            Tool::Upx => ("https://github.com/upx/upx/releases/download/v5.3.0/upx-5.3.0-win64.zip", Some("upx-5.3.0-win64/upx.exe")),
            Tool::Shrinkler => return None,
        })
    }

    /// The source to build it from with a C compiler, where that's simple: the zip, and the folder inside it.
    pub fn source(self) -> Option<(&'static str, &'static str)> {
        if cfg!(windows) {
            return None;
        }
        match self {
            Tool::Salvador => Some(("https://codeload.github.com/emmanuel-marty/salvador/zip/refs/tags/1.4.2", "salvador-1.4.2")),
            Tool::Lzsa => Some(("https://codeload.github.com/emmanuel-marty/lzsa/zip/refs/tags/1.4.1", "lzsa-1.4.1")),
            Tool::Zx0 => Some(("https://codeload.github.com/einar-saukas/ZX0/zip/refs/heads/main", "ZX0-main")),
            Tool::Lz4 => Some(("https://codeload.github.com/lz4/lz4/zip/refs/tags/v1.10.0", "lz4-1.10.0")),
            Tool::Shrinkler | Tool::Upx => None,
        }
    }

    /// How to get it when it can't be fetched for you.
    pub fn how_to_get(self) -> &'static str {
        match self {
            Tool::Salvador | Tool::Lzsa => "Download its Windows release from GitHub, or build it from its source with make (Limoniter can do either, on the Tools page).",
            Tool::Zx0 => "Its Windows program is in its GitHub repository; elsewhere, build it with cc -O2 -o zx0 src/*.c (Limoniter can do either, on the Tools page).",
            Tool::Lz4 => "Install it with your package manager (apt install lz4, brew install lz4), or download its Windows release (Limoniter can, on the Tools page).",
            Tool::Shrinkler => "Download its release from GitHub (Limoniter can, on the Tools page), or build it from the source with make. It's the same Shrinkler as Run on, Amiga crunchers and music uses.",
            Tool::Upx => "Install it with your package manager (apt install upx-ucl, brew install upx), or download its Windows release (Limoniter can, on the Tools page).",
        }
    }

    /// What a packed copy of `name` is called.
    pub fn packed_name(self, name: &str) -> String {
        match self {
            Tool::Salvador | Tool::Zx0 => format!("{name}.zx0"),
            Tool::Lzsa => format!("{name}.lzsa"),
            Tool::Lz4 => format!("{name}.lz4"),
            Tool::Shrinkler => format!("{name}.shr"),
            Tool::Upx => crate::amiga_tools::crunched_name(name),
        }
    }
}

/// How each packer is used, remembered between runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefs {
    pub paths: Vec<(Tool, PathBuf)>,
    /// LZSA's format: 1 unpacks faster, 2 packs smaller.
    pub lzsa_format: u8,
    /// LZ4's level, 1 to 12.
    pub lz4_level: u8,
    /// Shrinkler's effort, 1 to 9.
    pub shrinkler_preset: u8,
    /// zx0's quick, less thorough packing.
    pub zx0_quick: bool,
    /// UPX's --best (slower, smaller); else its default.
    pub upx_best: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { paths: Vec::new(), lzsa_format: 2, lz4_level: 12, shrinkler_preset: 3, zx0_quick: false, upx_best: true }
    }
}

fn prefs_file() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    crate::settings::data_folder().map(|d| d.join("st-tools.json"))
}

thread_local! {
    static TEST_PREFS: std::cell::RefCell<Prefs> = std::cell::RefCell::new(Prefs::default());
}

/// Where fetched and built tools are kept.
pub fn tools_folder() -> Option<PathBuf> {
    crate::settings::data_folder().map(|d| d.join("st-tools"))
}

fn exe_name(tool: Tool) -> String {
    let name = tool.programs()[0];
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

impl Prefs {
    pub fn load() -> Prefs {
        if cfg!(test) {
            return TEST_PREFS.with(|p| p.borrow().clone());
        }
        prefs_file().and_then(|f| std::fs::read_to_string(f).ok()).map(|t| Prefs::from_json(&t)).unwrap_or_default()
    }

    pub fn save(&self) {
        if cfg!(test) {
            TEST_PREFS.with(|p| *p.borrow_mut() = self.clone());
        }
        if let Some(file) = prefs_file() {
            let _ = file.parent().map(std::fs::create_dir_all);
            let _ = std::fs::write(file, self.to_json());
        }
    }

    pub fn path(&self, tool: Tool) -> Option<&Path> {
        self.paths.iter().find(|(t, _)| *t == tool).map(|(_, p)| p.as_path())
    }

    pub fn set_path(&mut self, tool: Tool, path: Option<PathBuf>) {
        self.paths.retain(|(t, _)| *t != tool);
        if let Some(path) = path {
            self.paths.push((tool, path));
        }
    }

    pub fn to_json(&self) -> String {
        use serde_json::json;
        let paths: serde_json::Map<String, serde_json::Value> = self.paths.iter().map(|(t, p)| (t.id().to_string(), json!(p.display().to_string()))).collect();
        let value = json!({ "paths": paths, "lzsa_format": self.lzsa_format, "lz4_level": self.lz4_level, "shrinkler_preset": self.shrinkler_preset, "zx0_quick": self.zx0_quick, "upx_best": self.upx_best });
        serde_json::to_string_pretty(&value).unwrap_or_default()
    }

    pub fn from_json(text: &str) -> Prefs {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            return Prefs::default();
        };
        let mut prefs = Prefs::default();
        if let Some(paths) = value["paths"].as_object() {
            for (id, path) in paths {
                if let (Some(tool), Some(path)) = (Tool::from_id(id), path.as_str()) {
                    prefs.paths.push((tool, PathBuf::from(path)));
                }
            }
        }
        let number = |key: &str, low: u8, high: u8, default: u8| value[key].as_u64().map_or(default, |n| (n.min(255) as u8).clamp(low, high));
        prefs.lzsa_format = number("lzsa_format", 1, 2, 2);
        prefs.lz4_level = number("lz4_level", 1, 12, 12);
        prefs.shrinkler_preset = number("shrinkler_preset", 1, 9, 3);
        prefs.zx0_quick = value["zx0_quick"].as_bool().unwrap_or(false);
        prefs.upx_best = value["upx_best"].as_bool().unwrap_or(true);
        prefs
    }

    /// Where a tool is: where the person pointed; else one fetched or built here; else where it's usually
    /// installed. Shrinkler is also found where the Amiga crunchers' box was pointed at it.
    pub fn find(&self, tool: Tool) -> Option<PathBuf> {
        if let Some(path) = self.path(tool).filter(|p| p.is_file()) {
            return Some(path.to_path_buf());
        }
        if let Some(made) = tools_folder().map(|f| f.join(tool.id()).join(exe_name(tool))).filter(|p| p.is_file()) {
            return Some(made);
        }
        if tool == Tool::Shrinkler
            && let Some(path) = crate::amiga_tools::Prefs::load().find(crate::amiga_tools::Tool::Shrinkler)
        {
            return Some(path);
        }
        crate::emulators::find_program(tool.programs(), tool.folder_hints())
    }
}

/// Whether bytes are a TOS program: it starts with $601A, a branch over its 28-byte header.
pub fn is_tos_program(bytes: &[u8]) -> bool {
    bytes.len() >= 28 && bytes[..2] == [0x60, 0x1A]
}

/// What to give a packer to pack `input` into `output`.
pub fn pack_arguments(tool: Tool, prefs: &Prefs, input: &Path, output: &Path) -> Vec<String> {
    let (i, o) = (input.display().to_string(), output.display().to_string());
    match tool {
        // -c checks the packed stream unpacks to the input before it's kept.
        Tool::Salvador => vec!["-c".into(), i, o],
        Tool::Zx0 => {
            let mut args = vec!["-f".to_string()];
            if prefs.zx0_quick {
                args.push("-q".into());
            }
            args.extend([i, o]);
            args
        }
        Tool::Lzsa => vec!["-c".into(), "-f".into(), prefs.lzsa_format.clamp(1, 2).to_string(), i, o],
        // A frame without its checksum, as lz4_frame.asm expects.
        Tool::Lz4 => vec![format!("-{}", prefs.lz4_level.clamp(1, 12)), "--no-frame-crc".into(), "-f".into(), i, o],
        Tool::Shrinkler => vec!["-d".into(), format!("-{}", prefs.shrinkler_preset.clamp(1, 9)), "-p".into(), i, o],
        Tool::Upx => {
            let mut args = vec!["-f".to_string(), "-q".into()];
            if prefs.upx_best {
                args.push("--best".into());
            }
            args.extend(["-o".into(), o, i]);
            args
        }
    }
}

/// Packs `input` into `output` with a packer, in the thread it's called from.
pub fn pack(tool: Tool, program: &Path, prefs: &Prefs, input: &Path, output: &Path, stop: &AtomicBool) -> Result<Outcome, String> {
    let before = std::fs::metadata(input).map_err(|e| crate::i18n::tf!("{} couldn't be read: {e}", input.display()))?.len();
    let _ = std::fs::remove_file(output);
    let started = Instant::now();
    let folder = output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let (ok, printed) = run(program, &pack_arguments(tool, prefs, input, output), folder, stop)?;
    let made: Vec<(PathBuf, u64)> = std::fs::metadata(output).ok().map(|m| (output.to_path_buf(), m.len())).into_iter().collect();
    Ok(Outcome { ok: ok && !made.is_empty(), printed, before, made, seconds: started.elapsed().as_secs_f64() })
}

/// The 68000 calls that unpack a file packed with a packer, as source to adapt. They call the depacker's own
/// routine and include its file, which comes from its own package.
pub fn depack_snippet(tool: Tool, packed_name: &str, unpacked_size: u64) -> String {
    let (file, from, terms) = tool.depacker();
    let routine = file.split(['(', ',', ')']).nth(1).unwrap_or("").trim().to_string();
    let include = file.split(' ').next().unwrap_or(file);
    let mut s = String::new();
    let mut line = |text: &str| {
        s.push_str(text);
        s.push('\n');
    };
    let room = unpacked_size.max(1);
    if tool == Tool::Upx {
        line(&format!("; --- {packed_name}, packed with UPX ---"));
        line("; A TOS program packed by UPX unpacks itself in memory when it's run, so there's nothing to call:");
        line("; run it as you would the original. Its unpacking needs a little more memory than the program did.");
        return s;
    }
    line(&format!("; --- Unpacking {packed_name} ({}) ---", tool.format()));
    line(&format!("; Needs {include} from {from} ({terms}),"));
    if tool == Tool::Lz4 {
        line("; and the block depacker it includes, lz4_smallest.asm (or change its include to lz4_normal.asm or");
        line("; lz4_fastest.asm, faster and bigger), from the same place.");
    }
    line("; Beside this source. Unpacked to the buffer below, which must hold the whole file.");
    line("depack:");
    line("\t\tlea\tpacked_data,a0");
    match tool {
        Tool::Lzsa => {
            line("\t\tlea\tunpacked_data,a2\t\t; lzsa_depack_stream writes from a2");
            line(&format!("\t\tbsr\t{routine}\t\t; d0 = 0 if it unpacked, -1 if it isn't an LZSA stream"));
        }
        Tool::Shrinkler => {
            line("\t\tlea\tunpacked_data,a1");
            line("\t\tsuba.l\ta2,a2\t\t\t; no progress callback");
            line("\t\tmoveq\t#1,d7\t\t\t; the parity context, as Shrinkler packs by default (0 for -b)");
            line(&format!("\t\tbsr\t{routine}\t\t; uses 3 KB of stack"));
        }
        _ => {
            line("\t\tlea\tunpacked_data,a1");
            line(&format!("\t\tbsr\t{routine}"));
        }
    }
    line("\t\trts");
    line("");
    line(&format!("\t\tinclude\t\"{include}\""));
    line("");
    line("\t\tsection\tdata");
    line("\t\teven");
    line(&format!("packed_data:\tincbin\t\"{packed_name}\""));
    if tool == Tool::Shrinkler {
        line("\t\tdc.l\t0\t\t\t; it may read a byte past the end");
    }
    line("\t\teven");
    line("");
    line("\t\tsection\tbss");
    line(&format!("unpacked_data:\tds.b\t{room}\t\t; {} bytes unpacked", group(room)));
    line("\t\teven");
    s
}

/// Fetches a tool's release, or builds it from its source with the C compiler here, and keeps it in
/// Limoniter's folder for tools. Only ever when asked.
pub fn fetch(tool: Tool) -> Result<PathBuf, String> {
    if tool == Tool::Shrinkler {
        return crate::amiga_tools::download(crate::amiga_tools::Tool::Shrinkler);
    }
    let folder = tools_folder().ok_or(crate::i18n::tr("There's nowhere to keep it: Limoniter's settings folder couldn't be found."))?.join(tool.id());
    if let Some((address, inside)) = tool.release() {
        let bytes = get(address)?;
        let program = match inside {
            Some(inside) => from_zip(bytes, inside)?,
            None => bytes,
        };
        std::fs::create_dir_all(&folder).map_err(|e| crate::i18n::tf!("{} couldn't be made: {e}", folder.display()))?;
        let path = folder.join(exe_name(tool));
        std::fs::write(&path, program).map_err(|e| crate::i18n::tf!("{} couldn't be saved: {e}", path.display()))?;
        return Ok(path);
    }
    build(tool, &folder)
}

fn get(address: &str) -> Result<Vec<u8>, String> {
    let response = ureq::get(address).header("User-Agent", "Limoniter").call().map_err(|e| crate::i18n::tf!("{address} couldn't be reached: {e}"))?;
    let mut bytes = Vec::new();
    response.into_body().into_reader().take(64 << 20).read_to_end(&mut bytes).map_err(|e| crate::i18n::tf!("The download stopped: {e}"))?;
    Ok(bytes)
}

fn from_zip(zip: Vec<u8>, inside: &str) -> Result<Vec<u8>, String> {
    let volume = crate::media::open(std::sync::Arc::new(zip), "release.zip")?;
    let mut dir: Option<crate::media::Entry> = None;
    let parts: Vec<&str> = inside.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        let found = volume.list(dir.as_ref())?.into_iter().find(|e| e.name.eq_ignore_ascii_case(part)).ok_or_else(|| crate::i18n::tf!("The release has no {inside} in it."))?;
        if i + 1 == parts.len() {
            return volume.read(&found);
        }
        dir = Some(found);
    }
    Err(crate::i18n::tf!("The release has no {inside} in it."))
}

/// Writes every file under a folder of a zip into `target`, keeping the folders.
fn unzip_folder(volume: &dyn crate::media::Volume, dir: Option<&crate::media::Entry>, target: &Path) -> Result<usize, String> {
    std::fs::create_dir_all(target).map_err(|e| crate::i18n::tf!("{} couldn't be made: {e}", target.display()))?;
    let mut count = 0;
    for entry in volume.list(dir)? {
        if entry.name.contains(['/', '\\']) || entry.name == ".." || entry.name == "." {
            continue;
        }
        let path = target.join(&entry.name);
        if entry.is_dir {
            count += unzip_folder(volume, Some(&entry), &path)?;
        } else {
            std::fs::write(&path, volume.read(&entry)?).map_err(|e| crate::i18n::tf!("{} couldn't be written: {e}", path.display()))?;
            count += 1;
        }
    }
    Ok(count)
}

/// A C compiler on this computer.
pub fn c_compiler() -> Option<PathBuf> {
    ["cc", "gcc", "clang"].into_iter().find_map(|name| {
        let path: Vec<PathBuf> = std::env::var_os("PATH").map(|paths| std::env::split_paths(&paths).collect()).unwrap_or_default();
        path.into_iter().map(|p| p.join(name)).find(|p| p.is_file())
    })
}

/// The C files and include folders a tool is built from, relative to its source's top folder.
fn build_recipe(tool: Tool, top: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let c_files = |dir: &Path| -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir).map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "c")).collect()).unwrap_or_default();
        files.sort();
        files
    };
    match tool {
        Tool::Salvador | Tool::Lzsa => {
            let src = top.join("src");
            let mut files = c_files(&src);
            files.extend(c_files(&src.join("libdivsufsort").join("lib")));
            (files, vec![src.clone(), src.join("libdivsufsort").join("include")])
        }
        Tool::Zx0 => (c_files(&top.join("src")).into_iter().filter(|p| !p.ends_with("dzx0.c")).collect(), Vec::new()),
        Tool::Lz4 => {
            let mut files = c_files(&top.join("lib"));
            files.extend(c_files(&top.join("programs")));
            (files, vec![top.join("lib")])
        }
        Tool::Shrinkler | Tool::Upx => (Vec::new(), Vec::new()),
    }
}

/// Fetches a tool's source and builds it with the C compiler here.
fn build(tool: Tool, folder: &Path) -> Result<PathBuf, String> {
    let (address, top) = tool.source().ok_or_else(|| crate::i18n::tf!("There's no {} release for this computer to download, and Limoniter doesn't build it. {}", tool.name(), tool.how_to_get()))?;
    let compiler = c_compiler().ok_or_else(|| crate::i18n::tf!("Building {} needs a C compiler (cc, gcc or clang), and none was found. {}", tool.name(), tool.how_to_get()))?;
    let zip = get(address)?;
    let volume = crate::media::open(std::sync::Arc::new(zip), "source.zip")?;
    let source = folder.join("source");
    let _ = std::fs::remove_dir_all(&source);
    let top_entry = volume.list(None)?.into_iter().find(|e| e.is_dir && e.name == top).ok_or_else(|| crate::i18n::tf!("The source has no {top} folder."))?;
    unzip_folder(&*volume, Some(&top_entry), &source)?;
    let (files, includes) = build_recipe(tool, &source);
    if files.is_empty() {
        return Err(crate::i18n::tf!("{}'s source isn't laid out as Limoniter expects, so it wasn't built.", tool.name()));
    }
    let program = folder.join(exe_name(tool));
    let mut args: Vec<String> = vec!["-O2".into(), "-o".into(), program.display().to_string()];
    args.extend(includes.iter().map(|i| format!("-I{}", i.display())));
    args.extend(files.iter().map(|f| f.display().to_string()));
    let (ok, printed) = run(&compiler, &args, &source, &AtomicBool::new(false))?;
    if !ok || !program.is_file() {
        let tail: Vec<String> = printed.iter().rev().take(8).rev().cloned().collect();
        return Err(crate::i18n::tf!("Building {} didn't work:\n{}", tool.name(), tail.join("\n")));
    }
    let _ = std::fs::remove_dir_all(&source);
    Ok(program)
}

/// The YM2149's clock on the machines that had it, in Hz.
pub const CLOCKS: [(&str, u32); 4] = [("Atari ST (2 MHz)", 2_000_000), ("ZX Spectrum 128 (1.7734 MHz)", 1_773_400), ("Amstrad CPC (1 MHz)", 1_000_000), ("MSX (1.7898 MHz)", 1_789_772)];

const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// What a table is of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableKind {
    /// Tone periods for registers 0-5: clock / (16 x frequency), 12 bits.
    Tone,
    /// Envelope periods for registers 11-12 that make the sawtooth shapes (8, 12) sound the note: clock / (256 x f).
    Buzzer,
    /// ProTracker's own Amiga periods, finetune 0, its three octaves.
    Amiga,
}

/// A frequency for a note counted from C in octave 0, with A in octave 4 tuned to `a4`.
pub fn note_frequency(note: i32, a4: f64) -> f64 {
    a4 * 2f64.powf(f64::from(note - 57) / 12.0)
}

/// ProTracker's period table, finetune 0, C-1 to B-3.
pub const PROTRACKER_PERIODS: [u16; 36] = [
    856, 808, 762, 720, 678, 640, 604, 570, 538, 508, 480, 453, 428, 404, 381, 360, 339, 320, 302, 285, 269, 254, 240, 226, 214, 202, 190, 180, 170, 160, 151, 143, 135, 127, 120, 113,
];

/// A table of periods as dc.w lines, an octave to a line, with the notes named.
pub fn period_table(kind: TableKind, clock: u32, a4: f64, first_octave: i32, octaves: i32, label: &str) -> String {
    let mut s = String::new();
    let mut line = |text: String| {
        s.push_str(&text);
        s.push('\n');
    };
    let label = if label.trim().is_empty() { "periods" } else { label.trim() };
    match kind {
        TableKind::Amiga => {
            line("; ProTracker's Amiga periods (finetune 0), C-1 to B-3: Paula's clock / (sample rate)".into());
            line(format!("{label}:"));
            for (octave, chunk) in PROTRACKER_PERIODS.chunks(12).enumerate() {
                let values: Vec<String> = chunk.iter().map(u16::to_string).collect();
                line(format!("\t\tdc.w\t{}\t; C-{} to B-{}", values.join(","), octave + 1, octave + 1));
            }
        }
        TableKind::Tone | TableKind::Buzzer => {
            let divisor = if kind == TableKind::Tone { 16.0 } else { 256.0 };
            let (most, what) = if kind == TableKind::Tone { (4095.0, "tone periods (registers 0-5)") } else { (65535.0, "envelope periods for shapes 8 and 12 (registers 11-12)") };
            line(format!("; YM2149 {what}: {} Hz clock, A-4 = {a4} Hz, clock / ({divisor} x frequency)", group(u64::from(clock))));
            let mut clamped = 0;
            line(format!("{label}:"));
            for octave in first_octave..first_octave + octaves.max(1) {
                let values: Vec<String> = (0..12)
                    .map(|n| {
                        let period = (f64::from(clock) / (divisor * note_frequency(octave * 12 + n, a4))).round();
                        if !(1.0..=most).contains(&period) {
                            clamped += 1;
                        }
                        (period.clamp(1.0, most) as u32).to_string()
                    })
                    .collect();
                line(format!("\t\tdc.w\t{}\t; {}{octave} to {}{octave}", values.join(","), NOTE_NAMES[0], NOTE_NAMES[11]));
            }
            if clamped > 0 {
                line(format!("; {clamped} notes were out of the chip's range and are held at its limit."));
            }
        }
    }
    s
}

/// The sixteen values of register 13, the envelope's shape, drawn as they sound: each as four levels over
/// time, from its first cycle to its last, where 0 is silent and 15 loud. Shapes 0-3 all sound as 9, and 4-7
/// as 15.
pub fn envelope_shape(shape: u8) -> [(u8, u8); 4] {
    // Each pair is a cycle: its level at the start and at the end.
    let continuing = shape & 8 != 0;
    let attack = shape & 4 != 0;
    let alternate = shape & 2 != 0;
    let hold = shape & 1 != 0;
    let first = if attack { (0, 15) } else { (15, 0) };
    if !continuing {
        return [first, (0, 0), (0, 0), (0, 0)];
    }
    if hold {
        // Held after the first cycle: where it ended, or the opposite when it alternates.
        let level = if alternate { 15 - first.1 } else { first.1 };
        return [first, (level, level), (level, level), (level, level)];
    }
    let mut cycles = [first; 4];
    if alternate {
        for (i, cycle) in cycles.iter_mut().enumerate() {
            if i % 2 == 1 {
                *cycle = (first.1, first.0);
            }
        }
    }
    cycles
}

/// A shape in words.
pub fn envelope_words(shape: u8) -> &'static str {
    match shape {
        0..=3 | 9 => "falls once, then silent",
        4..=7 | 15 => "rises once, then silent",
        8 => "falls again and again (sawtooth)",
        10 => "falls and rises (triangle)",
        11 => "falls once, then loud",
        12 => "rises again and again (sawtooth)",
        13 => "rises once, then stays loud",
        14 => "rises and falls (triangle)",
        _ => "",
    }
}

/// The 68000 that sets an envelope going on the ST: its period, then its shape, which restarts it.
pub fn envelope_snippet(shape: u8, period: u16, clock: u32) -> String {
    let hz = f64::from(clock) / (256.0 * f64::from(period.max(1)));
    let cycle = if matches!(shape, 10 | 14) { format!(" (a whole triangle is {:.2} Hz)", hz / 2.0) } else { String::new() };
    let mut s = format!("; YM2149 envelope: shape {shape}, {}; period {period}: {hz:.2} Hz{cycle}\n", envelope_words(shape));
    for (register, value) in [(11, period & 0xFF), (12, period >> 8), (13, u16::from(shape & 15))] {
        s.push_str(&format!("\t\tmove.b\t#{register},$ffff8800\t; register {register}\n\t\tmove.b\t#{value},$ffff8802\n"));
    }
    s.push_str("; Set a channel's volume (registers 8-10) to 16 for it to follow the envelope.\n");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packers_are_given_the_options_chosen() {
        let prefs = Prefs::default();
        let (i, o) = (Path::new("in.bin"), Path::new("out.zx0"));
        assert_eq!(pack_arguments(Tool::Salvador, &prefs, i, o), ["-c", "in.bin", "out.zx0"]);
        assert_eq!(pack_arguments(Tool::Zx0, &Prefs { zx0_quick: true, ..Prefs::default() }, i, o), ["-f", "-q", "in.bin", "out.zx0"]);
        assert_eq!(pack_arguments(Tool::Lzsa, &Prefs { lzsa_format: 1, ..Prefs::default() }, i, o), ["-c", "-f", "1", "in.bin", "out.zx0"]);
        assert_eq!(pack_arguments(Tool::Lz4, &prefs, i, o), ["-12", "--no-frame-crc", "-f", "in.bin", "out.zx0"]);
        assert_eq!(pack_arguments(Tool::Shrinkler, &prefs, i, o), ["-d", "-3", "-p", "in.bin", "out.zx0"]);
        assert_eq!(pack_arguments(Tool::Upx, &prefs, i, o), ["-f", "-q", "--best", "-o", "out.zx0", "in.bin"]);
        assert_eq!(Tool::Lzsa.packed_name("pic.pi1"), "pic.pi1.lzsa");
        assert_eq!(Tool::Upx.packed_name("demo.prg"), "demo_crunched.prg");
    }

    #[test]
    fn the_choices_are_remembered() {
        let mut prefs = Prefs { lzsa_format: 1, lz4_level: 9, shrinkler_preset: 7, zx0_quick: true, upx_best: false, ..Prefs::default() };
        prefs.set_path(Tool::Lz4, Some(PathBuf::from("/opt/lz4")));
        assert_eq!(Prefs::from_json(&prefs.to_json()), prefs);
        assert_eq!(Prefs::from_json("nonsense"), Prefs::default());
        assert_eq!(Prefs::from_json(r#"{"lz4_level": 99}"#).lz4_level, 12);
    }

    #[test]
    fn tone_tables_follow_the_tuning_and_the_clock() {
        // A-4 at 440 Hz on the ST: 2,000,000 / (16 x 440) = 284.09.
        let table = period_table(TableKind::Tone, 2_000_000, 440.0, 4, 1, "notes");
        assert!(table.contains("notes:\n"), "{table}");
        let values: Vec<u32> = table.lines().find(|l| l.contains("dc.w")).unwrap().split('\t').nth(3).unwrap().split(',').map(|v| v.parse().unwrap()).collect();
        assert_eq!(values.len(), 12);
        assert_eq!(values[9], 284);
        assert_eq!(values[0], 478, "C-4 is 261.63 Hz");
        // An octave down doubles every period.
        let lower = period_table(TableKind::Tone, 2_000_000, 440.0, 3, 1, "x");
        assert!(lower.contains("568"), "{lower}");
        // Out of range: held at 4095, and said.
        assert!(period_table(TableKind::Tone, 2_000_000, 440.0, 0, 1, "x").contains("out of the chip's range"));
        // The buzzer table: 2,000,000 / (256 x 440) = 17.76.
        assert!(period_table(TableKind::Buzzer, 2_000_000, 440.0, 4, 1, "x").contains(",18,"));
        let amiga = period_table(TableKind::Amiga, 0, 440.0, 0, 0, "pt");
        assert!(amiga.contains("dc.w\t856,808,762") && amiga.contains("120,113\t; C-3 to B-3"), "{amiga}");
    }

    #[test]
    fn envelope_shapes_are_drawn_as_the_chip_plays_them() {
        assert_eq!(envelope_shape(0), [(15, 0), (0, 0), (0, 0), (0, 0)]);
        assert_eq!(envelope_shape(4), [(0, 15), (0, 0), (0, 0), (0, 0)]);
        assert_eq!(envelope_shape(8), [(15, 0); 4]);
        assert_eq!(envelope_shape(9), [(15, 0), (0, 0), (0, 0), (0, 0)]);
        assert_eq!(envelope_shape(10), [(15, 0), (0, 15), (15, 0), (0, 15)]);
        assert_eq!(envelope_shape(11), [(15, 0), (15, 15), (15, 15), (15, 15)]);
        assert_eq!(envelope_shape(12), [(0, 15); 4]);
        assert_eq!(envelope_shape(13), [(0, 15), (15, 15), (15, 15), (15, 15)]);
        assert_eq!(envelope_shape(14), [(0, 15), (15, 0), (0, 15), (15, 0)]);
        assert_eq!(envelope_shape(15), [(0, 15), (0, 0), (0, 0), (0, 0)]);
        let snippet = envelope_snippet(14, 300, 2_000_000);
        assert!(snippet.contains("#13,$ffff8800") && snippet.contains("#14,$ffff8802") && snippet.contains("#44,$ffff8802") && snippet.contains("#1,$ffff8802"), "{snippet}");
        let path = std::env::temp_dir().join(format!("limoniter-env-{}.s", std::process::id()));
        let assembled = crate::assemblers::assemble_own(crate::assemblers::Cpu::M68k, &path, &snippet);
        assert!(assembled.assembly.problems.is_empty(), "{:?}", assembled.assembly.problems);
    }

    fn tool_for_test(tool: Tool) -> Option<PathBuf> {
        let variable = format!("LIMONITER_{}", tool.id().to_uppercase());
        std::env::var_os(variable).map(PathBuf::from).filter(|p| p.is_file())
    }

    /// Each packer named by LIMONITER_SALVADOR, LIMONITER_ZX0, LIMONITER_LZSA, LIMONITER_LZ4 or
    /// LIMONITER_SHRINKLER packs a file, and the sizes before and after are known.
    #[test]
    fn installed_packers_pack_data() {
        let dir = std::env::temp_dir().join(format!("limoniter-st-pack-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("data.bin");
        let data: Vec<u8> = (0..20_000u32).map(|i| ((i * 7) % 251) as u8 ^ (i / 300) as u8).collect();
        std::fs::write(&input, &data).unwrap();
        let mut packed_any = false;
        for tool in [Tool::Salvador, Tool::Zx0, Tool::Lzsa, Tool::Lz4, Tool::Shrinkler] {
            let Some(program) = tool_for_test(tool) else {
                eprintln!("LIMONITER_{} isn't set, so {} wasn't run.", tool.id().to_uppercase(), tool.name());
                continue;
            };
            let output = dir.join(tool.packed_name("data.bin"));
            let outcome = pack(tool, &program, &Prefs { shrinkler_preset: 1, ..Prefs::default() }, &input, &output, &AtomicBool::new(false)).unwrap();
            assert!(outcome.ok, "{}: {:?}", tool.name(), outcome.printed);
            assert_eq!(outcome.before, 20_000);
            assert!(outcome.made[0].1 < 20_000, "{}", tool.name());
            eprintln!("{}: {} in {:.2} s", tool.name(), outcome.sizes(), outcome.seconds);
            packed_any = true;
        }
        if !packed_any {
            eprintln!("No packer was named, so nothing was packed.");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The depacking calls, with each depacker's own file from a folder named by LIMONITER_ST_DEPACKERS, must
    /// assemble with Limoniter's 68000 assembler.
    #[test]
    fn depack_snippets_assemble_with_the_depackers_files() {
        let Some(folder) = std::env::var_os("LIMONITER_ST_DEPACKERS").map(PathBuf::from) else {
            eprintln!("LIMONITER_ST_DEPACKERS isn't set, so the depacking calls weren't assembled.");
            return;
        };
        let dir = std::env::temp_dir().join(format!("limoniter-st-depack-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for tool in [Tool::Salvador, Tool::Lzsa, Tool::Lz4, Tool::Shrinkler] {
            let include = tool.depacker().0.split(' ').next().unwrap().to_string();
            let Ok(depacker) = std::fs::read(folder.join(&include)) else {
                eprintln!("{include} isn't in LIMONITER_ST_DEPACKERS, so {} wasn't assembled.", tool.name());
                continue;
            };
            std::fs::write(dir.join(&include), depacker).unwrap();
            if tool == Tool::Lz4
                && let Ok(block) = std::fs::read(folder.join("lz4_smallest.asm"))
            {
                std::fs::write(dir.join("lz4_smallest.asm"), block).unwrap();
            }
            std::fs::write(dir.join(tool.packed_name("pic.pi1")), [0u8; 64]).unwrap();
            let snippet = depack_snippet(tool, &tool.packed_name("pic.pi1"), 32_034);
            let source = format!("start:\t\tbsr\tdepack\n\t\trts\n{snippet}");
            let path = dir.join("main.s");
            std::fs::write(&path, &source).unwrap();
            let assembled = crate::assemblers::assemble_own(crate::assemblers::Cpu::M68k, &path, &source);
            let (mine, theirs): (Vec<_>, Vec<_>) = assembled.assembly.problems.iter().partition(|p| p.file == 0);
            for problem in theirs {
                eprintln!("In {include}, not the snippet: line {}: {}", problem.line + 1, problem.message);
            }
            assert!(mine.is_empty(), "{}: {:?}", tool.name(), mine);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Building from source, when LIMONITER_BUILD_ST_TOOLS is set (it fetches from GitHub and needs a C compiler).
    #[test]
    fn packers_build_from_their_source_when_asked() {
        if std::env::var_os("LIMONITER_BUILD_ST_TOOLS").is_none() || cfg!(windows) {
            eprintln!("LIMONITER_BUILD_ST_TOOLS isn't set, so nothing was built.");
            return;
        }
        let dir = std::env::temp_dir().join(format!("limoniter-st-build-{}", std::process::id()));
        for tool in [Tool::Zx0, Tool::Salvador, Tool::Lzsa, Tool::Lz4] {
            let program = build(tool, &dir.join(tool.id())).unwrap_or_else(|e| panic!("{}: {e}", tool.name()));
            let check: Vec<String> = if tool == Tool::Lz4 { vec!["-V".into()] } else { Vec::new() };
            let (_, printed) = run(&program, &check, &dir, &AtomicBool::new(false)).unwrap();
            assert!(!printed.is_empty(), "{} printed nothing", tool.name());
            eprintln!("{} built: {}", tool.name(), printed[0]);
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
