//! Other people's tools for C64 demos: packing a finished program or its data (Exomizer, TSCrunch, pucrunch, and
//! ZX0 with salvador or the original zx0), and playing a SID tune in VICE's vsid. They're found where they're
//! installed or where the person points, the way the Amiga's crunchers are, and run in the background. Nothing of
//! theirs is part of Limoniter: each packer's 6502 decruncher is named, with where it comes from and its licence,
//! for the person to take from the packer's own package.

use crate::amiga_tools::{Outcome, run};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Exomizer,
    TsCrunch,
    Pucrunch,
    Salvador,
    Zx0,
    Vsid,
}

impl Tool {
    pub const ALL: [Tool; 6] = [Tool::Exomizer, Tool::TsCrunch, Tool::Pucrunch, Tool::Salvador, Tool::Zx0, Tool::Vsid];
    pub const PACKERS: [Tool; 5] = [Tool::Exomizer, Tool::TsCrunch, Tool::Pucrunch, Tool::Salvador, Tool::Zx0];

    pub fn id(self) -> &'static str {
        match self {
            Tool::Exomizer => "exomizer",
            Tool::TsCrunch => "tscrunch",
            Tool::Pucrunch => "pucrunch",
            Tool::Salvador => "salvador",
            Tool::Zx0 => "zx0",
            Tool::Vsid => "vsid",
        }
    }

    pub fn from_id(id: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.id() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            Tool::Exomizer => "Exomizer",
            Tool::TsCrunch => "TSCrunch",
            Tool::Pucrunch => "pucrunch",
            Tool::Salvador => "salvador (ZX0)",
            Tool::Zx0 => "zx0 (ZX0)",
            Tool::Vsid => "vsid (VICE's SID player)",
        }
    }

    fn programs(self) -> &'static [&'static str] {
        match self {
            Tool::Exomizer => &["exomizer"],
            Tool::TsCrunch => &["tscrunch", "TSCrunch"],
            Tool::Pucrunch => &["pucrunch"],
            Tool::Salvador => &["salvador"],
            Tool::Zx0 => &["zx0"],
            Tool::Vsid => &["vsid"],
        }
    }

    fn folder_hints(self) -> &'static [&'static str] {
        match self {
            Tool::Exomizer => &["exomizer"],
            Tool::TsCrunch => &["tscrunch"],
            Tool::Pucrunch => &["pucrunch"],
            Tool::Salvador => &["salvador"],
            Tool::Zx0 => &["zx0"],
            Tool::Vsid => &["vice"],
        }
    }

    pub fn website(self) -> &'static str {
        match self {
            Tool::Exomizer => "https://bitbucket.org/magli143/exomizer/wiki/Home",
            Tool::TsCrunch => "https://github.com/tonysavon/TSCrunch",
            Tool::Pucrunch => "https://github.com/mist64/pucrunch",
            Tool::Salvador => "https://github.com/emmanuel-marty/salvador",
            Tool::Zx0 => "https://github.com/einar-saukas/ZX0",
            Tool::Vsid => "https://vice-emu.sourceforge.io",
        }
    }

    /// Whose it is and on what terms, as its own package says.
    pub fn licence(self) -> &'static str {
        match self {
            Tool::Exomizer => "Magnus Lind. The program and its decrunchers in exodecrs/ are under a zlib-style licence (use, change and pass on freely, keeping the notice and marking changed versions); the self-unpacking decruncher built into the program is free for non-commercial use.",
            Tool::TsCrunch => "Antonio Savona. Apache License 2.0, its decrunchers too.",
            Tool::Pucrunch => "Pasi Ojala. GNU LGPL; its decompression code is under the wxWindows Library Licence, so it can go with the packed data or into your own programs.",
            Tool::Salvador => "Emmanuel Marty. zlib licence; its 6502 depacker (John Brandwood's) is under the Boost Software License.",
            Tool::Zx0 => "Einar Saukas. BSD 3-clause licence.",
            Tool::Vsid => "The VICE team. GNU GPL version 2.",
        }
    }

    /// How to get it.
    pub fn how_to_get(self) -> &'static str {
        match self {
            Tool::Exomizer => "Download its source zip from its website, and in its src folder type make (any C compiler); Windows builds come with it.",
            Tool::TsCrunch => "Download its source from GitHub and build tscrunch.c with any C compiler (cc -O2 -o tscrunch tscrunch.c -lm); it has Go and Java versions too.",
            Tool::Pucrunch => "Download its source from GitHub (mist64/pucrunch) and type make (any C compiler).",
            Tool::Salvador => "Download its source from GitHub and type make CC=gcc (or any C compiler); Windows builds come with its releases.",
            Tool::Zx0 => "Download its source from GitHub, and in its src folder build zx0.c, optimize.c, compress.c and memory.c with any C compiler; Windows builds come with it.",
            Tool::Vsid => "Install VICE (on Linux, the vice package; VICE needs the C64's ROMs, which some distributions leave out). vsid comes with it.",
        }
    }

    /// Whether it can make a program that unpacks and runs itself.
    pub fn makes_runnable(self) -> bool {
        matches!(self, Tool::Exomizer | Tool::TsCrunch | Tool::Pucrunch)
    }
}

/// How a self-unpacking program starts once it's unpacked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// At the address in the BASIC line's SYS.
    Sys,
    /// As a BASIC program, with RUN.
    Basic,
    /// At an address given.
    Address,
}

/// The choices for packing, shared by the packers where they mean the same.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackOptions {
    /// A program that unpacks and starts itself, rather than data for your own code to unpack.
    pub runnable: bool,
    pub start: Start,
    /// The address to start at, for Start::Address, in hex.
    pub address: String,
    /// Exomizer's data: `mem` (a .prg to unpack in place, with exodecrunch) rather than `raw`.
    pub exo_mem: bool,
    /// ZX0's older format, for the depackers made before ZX0 version 2.
    pub zx0_classic: bool,
    /// Packed to be unpacked backwards, from the end down.
    pub backwards: bool,
    /// TSCrunch's in-place data.
    pub inplace: bool,
}

impl Default for PackOptions {
    fn default() -> Self {
        Self { runnable: true, start: Start::Sys, address: String::new(), exo_mem: true, zx0_classic: false, backwards: false, inplace: false }
    }
}

/// What's remembered between runs: where each tool is, and how they were last used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefs {
    pub paths: Vec<(Tool, PathBuf)>,
    /// Pack the C64 program F9 builds, with this packer.
    pub after_build: Option<Tool>,
    pub packer: Tool,
    pub options: PackOptions,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { paths: Vec::new(), after_build: None, packer: Tool::Exomizer, options: PackOptions::default() }
    }
}

fn prefs_file() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    crate::settings::data_folder().map(|d| d.join("c64-tools.json"))
}

thread_local! {
    static TEST_PREFS: std::cell::RefCell<Prefs> = std::cell::RefCell::new(Prefs::default());
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
        let o = &self.options;
        let paths: serde_json::Map<String, serde_json::Value> = self.paths.iter().map(|(t, p)| (t.id().to_string(), json!(p.display().to_string()))).collect();
        let start = match o.start {
            Start::Sys => "sys",
            Start::Basic => "basic",
            Start::Address => "address",
        };
        let value = json!({
            "paths": paths,
            "after_build": self.after_build.map_or("", Tool::id),
            "packer": self.packer.id(),
            "options": { "runnable": o.runnable, "start": start, "address": o.address, "exo_mem": o.exo_mem, "zx0_classic": o.zx0_classic, "backwards": o.backwards, "inplace": o.inplace },
        });
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
        prefs.after_build = value["after_build"].as_str().and_then(Tool::from_id).filter(|t| t.makes_runnable());
        prefs.packer = value["packer"].as_str().and_then(Tool::from_id).filter(|t| Tool::PACKERS.contains(t)).unwrap_or(Tool::Exomizer);
        let o = &value["options"];
        let flag = |key: &str, default: bool| o[key].as_bool().unwrap_or(default);
        prefs.options = PackOptions {
            runnable: flag("runnable", true),
            start: match o["start"].as_str() {
                Some("basic") => Start::Basic,
                Some("address") => Start::Address,
                _ => Start::Sys,
            },
            address: o["address"].as_str().unwrap_or("").to_string(),
            exo_mem: flag("exo_mem", true),
            zx0_classic: flag("zx0_classic", false),
            backwards: flag("backwards", false),
            inplace: flag("inplace", false),
        };
        prefs
    }

    /// Where a tool is: where the person pointed, if it's still there; else where it's usually installed. vsid
    /// is also looked for beside the VICE that runs programs.
    pub fn find(&self, tool: Tool) -> Option<PathBuf> {
        if let Some(path) = self.path(tool).filter(|p| p.is_file()) {
            return Some(path.to_path_buf());
        }
        if let Some(found) = crate::emulators::find_program(tool.programs(), tool.folder_hints()) {
            return Some(found);
        }
        if tool == Tool::Vsid
            && let Some(x64) = crate::emulators::find_program(&["x64sc", "x64"], &["vice"])
        {
            let ending = x64.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
            let vsid = x64.with_file_name(format!("vsid{ending}"));
            if vsid.is_file() {
                return Some(vsid);
            }
        }
        None
    }
}

/// Whether bytes are a C64 program: a load address, then something. A BASIC program loads at $0801.
pub fn is_program(name: &str, bytes: &[u8]) -> bool {
    name.to_ascii_lowercase().ends_with(".prg") && bytes.len() > 2
}

/// The address a BASIC line's SYS calls, in a program loading at $0801, if it has one.
pub fn sys_address(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 8 || bytes[..2] != [0x01, 0x08] {
        return None;
    }
    let line = &bytes[2..bytes.len().min(80)];
    let at = line.iter().position(|b| *b == 0x9E)?;
    let digits: String = line[at + 1..].iter().map(|b| char::from(*b)).skip_while(|c| *c == ' ' || *c == '(').take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// What a packed copy is called, beside the original: demo.prg becomes demo_packed.prg.
pub fn packed_name(name: &str, tool: Tool, options: &PackOptions) -> String {
    let stem = name.rsplit_once('.').filter(|(s, _)| !s.is_empty()).map_or(name, |(s, _)| s);
    let ending = if options.runnable && tool.makes_runnable() {
        "prg"
    } else {
        match tool {
            Tool::Exomizer if options.exo_mem => "prg",
            Tool::Exomizer => "exo",
            Tool::TsCrunch => "tsc",
            Tool::Pucrunch => "pu",
            _ => "zx0",
        }
    };
    format!("{stem}_packed.{ending}")
}

fn hex_address(text: &str) -> Option<u16> {
    let t = text.trim().trim_start_matches('$').trim_start_matches("0x").trim_start_matches("0X");
    u16::from_str_radix(t, 16).ok()
}

/// What to give a packer to pack `input` (a .prg, with its load address, or raw data when `raw_input`) into
/// `output`. An address that's needed and can't be read is an error, said plainly.
pub fn pack_arguments(tool: Tool, options: &PackOptions, input: &Path, output: &Path, raw_input: bool, sys: Option<u16>) -> Result<Vec<String>, String> {
    let input_text = input.display().to_string();
    let output_text = output.display().to_string();
    let runnable = options.runnable && tool.makes_runnable() && !raw_input;
    let address = || hex_address(&options.address).ok_or_else(|| crate::i18n::tf!("\"{}\" isn't an address in hex, such as $0810.", options.address.trim()));
    let mut args: Vec<String> = Vec::new();
    match tool {
        Tool::Exomizer => {
            if runnable {
                args.push("sfx".into());
                args.push(match options.start {
                    Start::Sys => "sys".into(),
                    Start::Basic => "basic".into(),
                    Start::Address => format!("${:04x}", address()?),
                });
            } else if options.exo_mem && !raw_input {
                args.push("mem".into());
                // Unpacked by exodecrunch from the end down, or from the start up with -f.
                if !options.backwards {
                    args.push("-f".into());
                }
            } else {
                args.push("raw".into());
                if options.backwards {
                    args.push("-b".into());
                }
            }
            args.extend(["-o".into(), output_text]);
            // A program's load address is skipped for raw data.
            args.push(if !runnable && !options.exo_mem && !raw_input { format!("{input_text},2") } else { input_text });
        }
        Tool::TsCrunch => {
            if runnable {
                // TSCrunch reads the address as hex, whatever it looks like.
                let start = match options.start {
                    Start::Address => address()?,
                    Start::Sys => sys.ok_or(crate::i18n::tr("The program has no BASIC line with a SYS for TSCrunch to start at: give an address."))?,
                    Start::Basic => return Err(crate::i18n::tr("TSCrunch's self-unpacking programs start at an address, not with RUN: choose one.").into()),
                };
                args.extend(["-x".into(), format!("${start:04x}")]);
            } else {
                if !raw_input {
                    args.push("-p".into());
                }
                if options.inplace {
                    args.push("-i".into());
                }
            }
            args.extend([input_text, output_text]);
        }
        Tool::Pucrunch => {
            if runnable {
                match options.start {
                    Start::Address => args.push(format!("-x0x{:04x}", address()?)),
                    Start::Basic => args.push("-fbasic".into()),
                    Start::Sys => {}
                }
            } else {
                args.push("-c0".into());
            }
            args.extend([input_text, output_text]);
        }
        Tool::Salvador => {
            if options.zx0_classic {
                args.push("-classic".into());
            }
            if options.backwards {
                args.push("-b".into());
            }
            args.extend([input_text, output_text]);
        }
        Tool::Zx0 => {
            args.push("-f".into());
            if options.zx0_classic {
                args.push("-c".into());
            }
            if options.backwards {
                args.push("-b".into());
            }
            args.extend([input_text, output_text]);
        }
        Tool::Vsid => return Err(crate::i18n::tr("vsid plays tunes; it doesn't pack.").into()),
    }
    Ok(args)
}

/// Whether a packer reads raw bytes only, so a program's load address has to be taken off first.
pub fn wants_raw(tool: Tool) -> bool {
    matches!(tool, Tool::Salvador | Tool::Zx0)
}


/// The 6502 code that unpacks what a packer made, from the packer's own package, with its licence. Not copied
/// into Limoniter: the person takes it from there.
pub fn decruncher(tool: Tool, options: &PackOptions) -> &'static str {
    match tool {
        Tool::Exomizer if options.runnable => "Exomizer's self-unpacking program carries its own decruncher.",
        Tool::Exomizer => "exodecrunch.s in Exomizer's exodecrs folder (ca65), or acme/exodecrunch.asm, dasm/exodecrunch.s or kick/exodecrunch.asm for those assemblers; Magnus Lind's zlib-style licence: keep its notice, mark your changes.",
        Tool::TsCrunch if options.runnable => "TSCrunch's self-unpacking program carries its own decruncher.",
        Tool::TsCrunch => "decrunch.asm, decrunch_small.asm or decrunch_extreme.asm in TSCrunch's package (KickAssembler source), Antonio Savona's, Apache License 2.0: keep the licence with it.",
        Tool::Pucrunch if options.runnable => "pucrunch's self-unpacking program carries its own decompressor.",
        Tool::Pucrunch => "sa_uncrunch.asm (or uncrunch.asm) in pucrunch's package, Pasi Ojala's, under the wxWindows Library Licence: free to use in your programs.",
        Tool::Salvador | Tool::Zx0 if options.zx0_classic => "asm/6502/zx0_6502.asm in salvador's package (John Brandwood, Boost Software License), or dzx0_v1.asm in bitfire's packer/zx0/6502 folder (Tobias Bindhammer, BSD 3-clause, ACME source).",
        Tool::Salvador | Tool::Zx0 => "dzx0_v2.asm in bitfire's packer/zx0/6502 folder (github.com/bboxy/bitfire; Tobias Bindhammer, BSD 3-clause, ACME source). Salvador's own 6502 depacker reads only the classic format. Mind which way it unpacks if you chose backwards.",
        Tool::Vsid => "",
    }
}

/// Packs `input` into `output`. A program for a packer that wants raw bytes is given without its load address,
/// through a copy in `scratch`.
pub fn pack(tool: Tool, program: &Path, options: &PackOptions, input: &Path, output: &Path, stop: &AtomicBool) -> Result<Outcome, String> {
    let before = std::fs::metadata(input).map_err(|e| crate::i18n::tf!("{} couldn't be read: {e}", input.display()))?.len();
    let _ = std::fs::remove_file(output);
    let started = Instant::now();
    let folder = output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let raw = wants_raw(tool) && input.extension().is_some_and(|e| e.eq_ignore_ascii_case("prg"));
    let (used, stripped) = if raw {
        let bytes = std::fs::read(input).map_err(|e| crate::i18n::tf!("{} couldn't be read: {e}", input.display()))?;
        let copy = folder.join(format!(".{}.raw", output.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        std::fs::write(&copy, bytes.get(2..).unwrap_or(&[])).map_err(|e| crate::i18n::tf!("{} couldn't be written: {e}", copy.display()))?;
        (copy.clone(), Some(copy))
    } else {
        (input.to_path_buf(), None)
    };
    let sys = std::fs::read(input).ok().and_then(|bytes| sys_address(&bytes));
    let args = pack_arguments(tool, options, &used, output, raw, sys);
    let result = args.and_then(|args| run(program, &args, folder, stop));
    if let Some(copy) = stripped {
        let _ = std::fs::remove_file(copy);
    }
    let (ok, mut printed) = result?;
    if raw {
        printed.push("The program's load address was taken off first: put it back, or load the packed data where you like.".into());
    }
    let made: Vec<(PathBuf, u64)> = std::fs::metadata(output).ok().map(|m| (output.to_path_buf(), m.len())).into_iter().collect();
    Ok(Outcome { ok: ok && !made.is_empty(), printed, before, made, seconds: started.elapsed().as_secs_f64() })
}

/// Starts vsid playing a tune at a song (from 1), and leaves it playing in its own window.
pub fn play_in_vsid(vsid: &Path, tune: &Path, song: u16) -> Result<(), String> {
    let mut command = std::process::Command::new(vsid);
    command.args(["-tune".to_string(), song.to_string(), tune.display().to_string()]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    command.spawn().map(|_| ()).map_err(|e| crate::i18n::tf!("{} couldn't be started: {e}", vsid.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packers_are_given_the_options_chosen() {
        let (input, output) = (Path::new("in.prg"), Path::new("out.prg"));
        let mut options = PackOptions::default();
        assert_eq!(pack_arguments(Tool::Exomizer, &options, input, output, false, None).unwrap(), ["sfx", "sys", "-o", "out.prg", "in.prg"]);
        assert_eq!(pack_arguments(Tool::TsCrunch, &options, input, output, false, Some(2061)).unwrap(), ["-x", "$080d", "in.prg", "out.prg"]);
        assert!(pack_arguments(Tool::TsCrunch, &options, input, output, false, None).is_err());
        options.start = Start::Basic;
        assert_eq!(pack_arguments(Tool::Pucrunch, &options, input, output, false, None).unwrap(), ["-fbasic", "in.prg", "out.prg"]);
        options.start = Start::Address;
        options.address = "$0810".into();
        assert_eq!(pack_arguments(Tool::Exomizer, &options, input, output, false, None).unwrap(), ["sfx", "$0810", "-o", "out.prg", "in.prg"]);
        assert_eq!(pack_arguments(Tool::TsCrunch, &options, input, output, false, None).unwrap(), ["-x", "$0810", "in.prg", "out.prg"]);
        assert_eq!(pack_arguments(Tool::Pucrunch, &options, input, output, false, None).unwrap(), ["-x0x0810", "in.prg", "out.prg"]);
        options.address = "nowhere".into();
        assert!(pack_arguments(Tool::Exomizer, &options, input, output, false, None).unwrap_err().contains("nowhere"));
        options.runnable = false;
        assert_eq!(pack_arguments(Tool::Exomizer, &options, input, output, false, None).unwrap(), ["mem", "-f", "-o", "out.prg", "in.prg"]);
        options.exo_mem = false;
        options.backwards = true;
        assert_eq!(pack_arguments(Tool::Exomizer, &options, input, output, false, None).unwrap(), ["raw", "-b", "-o", "out.prg", "in.prg,2"]);
        assert_eq!(pack_arguments(Tool::TsCrunch, &options, input, output, false, None).unwrap(), ["-p", "in.prg", "out.prg"]);
        assert_eq!(pack_arguments(Tool::Pucrunch, &options, input, output, false, None).unwrap(), ["-c0", "in.prg", "out.prg"]);
        options.zx0_classic = true;
        assert_eq!(pack_arguments(Tool::Zx0, &options, input, output, true, None).unwrap(), ["-f", "-c", "-b", "in.prg", "out.prg"]);
        assert_eq!(pack_arguments(Tool::Salvador, &options, input, output, true, None).unwrap(), ["-classic", "-b", "in.prg", "out.prg"]);
    }

    #[test]
    fn names_and_sys_lines_are_worked_out() {
        let options = PackOptions::default();
        assert_eq!(packed_name("demo.prg", Tool::Exomizer, &options), "demo_packed.prg");
        assert_eq!(packed_name("pic.bin", Tool::Zx0, &options), "pic_packed.zx0");
        let basic = [0x01, 0x08, 0x0C, 0x08, 0x0A, 0x00, 0x9E, b' ', b'2', b'0', b'6', b'4', 0, 0, 0];
        assert_eq!(sys_address(&basic), Some(2064));
        assert_eq!(sys_address(&[0x00, 0xC0, 0x60]), None);
        assert!(is_program("x.PRG", &basic));
    }

    #[test]
    fn the_choices_are_remembered() {
        let mut prefs = Prefs { after_build: Some(Tool::Exomizer), packer: Tool::Salvador, ..Prefs::default() };
        prefs.set_path(Tool::Pucrunch, Some(PathBuf::from("/opt/pucrunch")));
        prefs.options = PackOptions { runnable: false, start: Start::Address, address: "$1000".into(), exo_mem: false, zx0_classic: true, backwards: true, inplace: true };
        assert_eq!(Prefs::from_json(&prefs.to_json()), prefs);
        assert_eq!(Prefs::from_json("nonsense"), Prefs::default());
        assert_eq!(Prefs::from_json(r#"{"after_build":"zx0"}"#).after_build, None, "only a packer that makes programs can pack after building");
    }

    /// A program of a BASIC line's SYS 2061 and code that clears the screen, turns the border green and the
    /// background red and stays there, followed by 4,000 bytes of table for a packer to find repeats in.
    fn test_program() -> Vec<u8> {
        let mut prg = vec![0x01, 0x08, 0x0B, 0x08, 0x0A, 0x00, 0x9E, b'2', b'0', b'6', b'1', 0, 0, 0];
        prg.extend([0xA9, 0x20, 0xA2, 0x00, 0x9D, 0x00, 0x04, 0x9D, 0x00, 0x05, 0x9D, 0x00, 0x06, 0x9D, 0xE8, 0x06, 0xE8, 0xD0, 0xF1]);
        prg.extend([0xA9, 0x05, 0x8D, 0x20, 0xD0, 0xA9, 0x02, 0x8D, 0x21, 0xD0, 0x78, 0x4C, 0x2B, 0x08]);
        prg.extend((0..4000u32).map(|i| (i * 7 % 13) as u8));
        prg
    }

    fn tool_for_test(tool: Tool) -> Option<PathBuf> {
        let variable = format!("LIMONITER_{}", tool.id().to_ascii_uppercase());
        std::env::var_os(&variable).map(PathBuf::from).filter(|p| p.is_file()).or_else(|| Prefs::default().find(tool)).or_else(|| {
            eprintln!("{} isn't installed (or named by {variable}), so it wasn't tried.", tool.name());
            None
        })
    }

    /// Runs a program in x64sc, headless, and says whether the border ended green and the screen red, as the test
    /// program leaves them. None if VICE can't be run (LIMONITER_VICE_DATA names the folder of its ROMs).
    fn runs_in_vice(prg: &Path) -> Option<bool> {
        let data = std::env::var_os("LIMONITER_VICE_DATA")?;
        let shot = prg.with_extension("png");
        let _ = std::fs::remove_file(&shot);
        let mut args: Vec<String> = vec!["-a".into(), "x64sc".into(), "-default".into(), "-directory".into(), PathBuf::from(data).display().to_string()];
        args.extend(["-autostartprgmode", "1", "-warp", "-limitcycles", "40000000", "-exitscreenshot"].map(String::from));
        args.push(shot.display().to_string());
        args.extend(["-autostart".to_string(), prg.display().to_string()]);
        // xvfb-run now and then fails to start its display straight after another: tried again then.
        for _ in 0..3 {
            std::process::Command::new("xvfb-run").args(&args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().ok()?;
            if shot.is_file() {
                break;
            }
        }
        let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&shot).ok()?));
        let mut reader = decoder.read_info().ok()?;
        let mut pixels = vec![0u8; reader.output_buffer_size()?];
        let frame = reader.next_frame(&mut pixels).ok()?;
        let channels = frame.line_size / frame.width as usize;
        let at = |x: u32, y: u32| {
            let i = y as usize * frame.line_size + x as usize * channels;
            (pixels[i], pixels[i + 1], pixels[i + 2])
        };
        let (border, middle) = (at(4, 4), at(frame.width / 2, frame.height / 2));
        Some(border.1 > border.0 && border.1 > border.2 && middle.0 > middle.1 && middle.0 > middle.2)
    }

    /// Each packer that makes programs packs the test program, smaller, into one that still runs in VICE; ZX0's
    /// packers' data comes back as it was through salvador's own unpacker.
    #[test]
    fn packers_pack_and_what_they_make_still_runs() {
        let dir = std::env::temp_dir().join(format!("limoniter-c64-pack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("demo.prg");
        let program = test_program();
        std::fs::write(&input, &program).unwrap();
        match runs_in_vice(&input) {
            Some(ran) => assert!(ran, "the program as it is runs as it should"),
            None => eprintln!("VICE couldn't be run (LIMONITER_VICE_DATA names its ROMs), so nothing packed was run."),
        }
        for tool in [Tool::Exomizer, Tool::Pucrunch, Tool::TsCrunch] {
            let Some(packer) = tool_for_test(tool) else {
                continue;
            };
            let output = dir.join(format!("{}.prg", tool.id()));
            let outcome = pack(tool, &packer, &PackOptions::default(), &input, &output, &AtomicBool::new(false)).unwrap();
            assert!(outcome.ok, "{}: {:?}", tool.name(), outcome.printed);
            assert!(outcome.made[0].1 < outcome.before, "{}: {}", tool.name(), outcome.sizes());
            eprintln!("{}: {}", tool.name(), outcome.sizes());
            if let Some(ran) = runs_in_vice(&output) {
                assert!(ran, "{}'s program didn't run as it should", tool.name());
                eprintln!("{}: its program runs in VICE", tool.name());
            }
            // And as data.
            let options = PackOptions { runnable: false, ..PackOptions::default() };
            let data = dir.join(packed_name("demo.prg", tool, &options));
            let outcome = pack(tool, &packer, &options, &input, &data, &AtomicBool::new(false)).unwrap();
            assert!(outcome.ok && outcome.made[0].1 < outcome.before, "{} as data: {:?}", tool.name(), outcome.printed);
        }
        for tool in [Tool::Salvador, Tool::Zx0] {
            let Some(packer) = tool_for_test(tool) else {
                continue;
            };
            for classic in [false, true] {
                let options = PackOptions { runnable: false, zx0_classic: classic, ..PackOptions::default() };
                let output = dir.join(format!("{}-{classic}.zx0", tool.id()));
                let outcome = pack(tool, &packer, &options, &input, &output, &AtomicBool::new(false)).unwrap();
                assert!(outcome.ok && outcome.made[0].1 < 300, "{}: {:?}", tool.name(), outcome.printed);
                if let Some(salvador) = tool_for_test(Tool::Salvador) {
                    let back = dir.join("back.bin");
                    let mut args = vec!["-d".to_string()];
                    if classic {
                        args.push("-classic".into());
                    }
                    args.extend([output.display().to_string(), back.display().to_string()]);
                    let _ = run(&salvador, &args, &dir, &AtomicBool::new(false)).unwrap();
                    assert_eq!(std::fs::read(&back).unwrap(), &program[2..], "{} (classic {classic}) unpacks to the program without its load address", tool.name());
                }
            }
            eprintln!("{}: packed and unpacked again", tool.name());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
