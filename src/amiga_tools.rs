//! Other people's tools for Amiga demos: crunching a finished program (Shrinkler, Cranker) and turning a
//! ProTracker module into what a fast replayer plays (LightSpeedPlayer's LSPConvert, The Player 6.1's P61Con).
//! They're found where they're installed or where the person points, the way assemblers and emulators are, and
//! run in the background. Nothing of theirs is part of Limoniter: the replay snippets written here only call the
//! players' routines and include their files, which come from the players' own packages.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Shrinkler,
    Cranker,
    LspConvert,
    P61Con,
}

impl Tool {
    pub const ALL: [Tool; 4] = [Tool::Shrinkler, Tool::Cranker, Tool::LspConvert, Tool::P61Con];
    pub const CRUNCHERS: [Tool; 2] = [Tool::Shrinkler, Tool::Cranker];

    pub fn id(self) -> &'static str {
        match self {
            Tool::Shrinkler => "shrinkler",
            Tool::Cranker => "cranker",
            Tool::LspConvert => "lspconvert",
            Tool::P61Con => "p61con",
        }
    }

    pub fn from_id(id: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|t| t.id() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            Tool::Shrinkler => "Shrinkler",
            Tool::Cranker => "Cranker",
            Tool::LspConvert => "LSPConvert (LightSpeedPlayer)",
            Tool::P61Con => "P61Con (The Player 6.1)",
        }
    }

    fn programs(self) -> &'static [&'static str] {
        match self {
            Tool::Shrinkler => &["Shrinkler", "shrinkler"],
            Tool::Cranker => &["cranker", "Cranker"],
            Tool::LspConvert => &["LSPConvert", "lspconvert"],
            Tool::P61Con => &["P61Con", "p61con"],
        }
    }

    fn folder_hints(self) -> &'static [&'static str] {
        match self {
            Tool::Shrinkler => &["shrinkler"],
            Tool::Cranker => &["cranker"],
            Tool::LspConvert => &["lspconvert", "lsplayer", "lightspeedplayer"],
            Tool::P61Con => &["p61", "player61", "p6108"],
        }
    }

    pub fn website(self) -> &'static str {
        match self {
            Tool::Shrinkler => "https://github.com/askeksa/Shrinkler",
            Tool::Cranker => "https://aminet.net/package/util/pack/cranker",
            Tool::LspConvert => "https://github.com/arnaud-carre/LSPlayer",
            Tool::P61Con => "https://aminet.net/package/mus/misc/P6108",
        }
    }

    /// Whose it is and on what terms, as its own package says.
    pub fn licence(self) -> &'static str {
        match self {
            Tool::Shrinkler => "Aske Simon Christensen (Blueberry). Its own permissive licence: free to use and pass on, with the licence kept with the source and the author credited; crunched programs are their authors' own.",
            Tool::Cranker => "Timm S. M\u{fc}ller. GNU GPL version 2; its decrunchers are free of copyright. The Aminet package has only the Amiga program, with the C source to build it elsewhere.",
            Tool::LspConvert => "Arnaud Carr\u{e9} (Leonard/Oxygene). MIT licence. Releases for Windows, macOS and Ubuntu.",
            Tool::P61Con => "Jarno Paananen (Guru/Sahara Surfers), updated by Photon/Scoopex and others. Freely distributed on Aminet, with no licence beyond that.",
        }
    }

    /// Whether it runs on this computer at all: P61Con is an Amiga program, so it runs on an Amiga or in an
    /// emulator, never directly here.
    pub fn runs_here(self) -> bool {
        self != Tool::P61Con
    }

    /// Where its release can be fetched from, for those whose licence allows it and that have one for this system.
    pub fn release(self) -> Option<(&'static str, &'static str)> {
        let os = std::env::consts::OS;
        match self {
            Tool::Shrinkler => {
                let inside = match os {
                    "windows" => "shrinkler47/Windows64/Shrinkler.exe",
                    "macos" => "shrinkler47/MacOSX/Shrinkler",
                    "linux" if cfg!(target_arch = "x86_64") => "shrinkler47/Linux64/Shrinkler",
                    _ => return None,
                };
                Some(("https://github.com/askeksa/Shrinkler/releases/download/v4.7/shrinkler47.zip", inside))
            }
            Tool::LspConvert => {
                let address = match os {
                    "windows" => "https://github.com/arnaud-carre/LSPlayer/releases/download/1.33/LSPConvert-win.zip",
                    "macos" => "https://github.com/arnaud-carre/LSPlayer/releases/download/1.33/LSPConvert-mac.zip",
                    "linux" if cfg!(target_arch = "x86_64") => "https://github.com/arnaud-carre/LSPlayer/releases/download/1.33/LSPConvert-ubuntu.zip",
                    _ => return None,
                };
                Some((address, if os == "windows" { "LSPConvert.exe" } else { "LSPConvert" }))
            }
            Tool::Cranker | Tool::P61Con => None,
        }
    }

    /// How to get it when it can't be fetched for you.
    pub fn how_to_get(self) -> &'static str {
        match self {
            Tool::Shrinkler => "Download its release from GitHub (Limoniter can do that, below), or build it from the source with make and a C++ compiler.",
            Tool::Cranker => "Download cranker.lha from Aminet (util/pack), unpack it, and in its src folder type make (any C99 compiler); point Limoniter at src/build/generic/cranker.",
            Tool::LspConvert => "Download its release from GitHub (Limoniter can do that, below), or build it from the source, with cmake or with g++ -O2 -std=c++17 -Isrc on its src folder's .cpp files.",
            Tool::P61Con => "P61Con is an Amiga program: run it on an Amiga or in an emulator (WinUAE, FS-UAE, Amiberry) and save the P61 file beside the module. Limoniter then finds that file and writes the replay calls for it.",
        }
    }
}

/// Shrinkler's choices, as its own help lists them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShrinklerOptions {
    /// -d: the file is data, not a program.
    pub data: bool,
    /// -w: with -d, a header saying the sizes, for easier loading.
    pub data_header: bool,
    /// -h: hunks of the same memory type merged into one.
    pub hunk_merge: bool,
    /// -o: crunched and uncrunched data overlap, which saves memory.
    pub overlap: bool,
    /// -m: the smaller decrunch header, with its restrictions.
    pub mini: bool,
    /// -c: the program can still be given its command line.
    pub command_line: bool,
    /// -b: no parity context, better on byte-oriented data.
    pub bytes: bool,
    /// -1 to -9: how hard it tries. 3 is its own default.
    pub preset: u8,
    /// -i and -r, if not the preset's.
    pub iterations: Option<u32>,
    pub references: Option<u32>,
    /// -t: a line printed before decrunching.
    pub text: String,
    /// -f: a register poked during decrunching, such as DFF180 to flash the background.
    pub flash: String,
}

impl Default for ShrinklerOptions {
    fn default() -> Self {
        Self { data: false, data_header: false, hunk_merge: false, overlap: false, mini: false, command_line: false, bytes: false, preset: 3, iterations: None, references: None, text: String::new(), flash: String::new() }
    }
}

/// Cranker's choices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrankerOptions {
    /// -cd: the file is data, not a program.
    pub data: bool,
    /// -1: faster, crunching less.
    pub fast: bool,
    /// -m: hunks merged (Cranker's own help calls it dangerous).
    pub merge: bool,
    /// -d: the decranker to load with: default, progress, minimal, cinema or black.
    pub decranker: String,
    /// -t: a line shown while loading.
    pub text: String,
}

impl Default for CrankerOptions {
    fn default() -> Self {
        Self { data: false, fast: false, merge: false, decranker: "default".into(), text: String::new() }
    }
}

pub const DECRANKERS: [&str; 5] = ["default", "progress", "minimal", "cinema", "black"];

/// LSPConvert's choices.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LspOptions {
    pub micro: bool,
    pub insane: bool,
    pub getpos: bool,
    pub setpos: bool,
    pub shrink: bool,
    pub adpcm: bool,
    pub fixed50hz: bool,
    pub nosettempo: bool,
    pub nosampleoptim: bool,
    pub preview_wav: bool,
}

/// Which way the replay calls are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Replay {
    /// LSP's own CIA toolkit, which sets DMACON itself.
    LspCia,
    /// LSP from your own interrupt, with DMACON set by the copper.
    LspCopper,
    LspInsane,
    LspMicro,
    /// The Player 6.1A.
    P61,
}

impl Replay {
    pub const ALL: [Replay; 5] = [Replay::LspCia, Replay::LspCopper, Replay::LspInsane, Replay::LspMicro, Replay::P61];

    pub fn name(self) -> &'static str {
        match self {
            Replay::LspCia => "LSP, its CIA toolkit (easiest)",
            Replay::LspCopper => "LSP, from your interrupt, DMACON by copper",
            Replay::LspInsane => "LSP insane (generated player)",
            Replay::LspMicro => "LSP micro (for small intros)",
            Replay::P61 => "The Player 6.1A",
        }
    }
}

/// What's remembered between runs: where each tool is, and how it was last used.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prefs {
    pub paths: Vec<(Tool, PathBuf)>,
    /// Crunch the Amiga program F9 builds, with this cruncher.
    pub after_build: Option<Tool>,
    pub shrinkler: ShrinklerOptions,
    pub cranker: CrankerOptions,
    pub lsp: LspOptions,
}

fn prefs_file() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    crate::settings::data_folder().map(|d| d.join("amiga-tools.json"))
}

thread_local! {
    /// The choices as a test leaves them, in place of the file.
    static TEST_PREFS: std::cell::RefCell<Prefs> = std::cell::RefCell::new(Prefs::default());
}

/// Where downloaded tools are kept.
pub fn tools_folder() -> Option<PathBuf> {
    crate::settings::data_folder().map(|d| d.join("amiga-tools"))
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
        let s = &self.shrinkler;
        let c = &self.cranker;
        let l = &self.lsp;
        let paths: serde_json::Map<String, serde_json::Value> = self.paths.iter().map(|(t, p)| (t.id().to_string(), json!(p.display().to_string()))).collect();
        let value = json!({
            "paths": paths,
            "after_build": self.after_build.map_or("", Tool::id),
            "shrinkler": { "data": s.data, "data_header": s.data_header, "hunk_merge": s.hunk_merge, "overlap": s.overlap, "mini": s.mini, "command_line": s.command_line, "bytes": s.bytes, "preset": s.preset, "iterations": s.iterations, "references": s.references, "text": s.text, "flash": s.flash },
            "cranker": { "data": c.data, "fast": c.fast, "merge": c.merge, "decranker": c.decranker, "text": c.text },
            "lsp": { "micro": l.micro, "insane": l.insane, "getpos": l.getpos, "setpos": l.setpos, "shrink": l.shrink, "adpcm": l.adpcm, "fixed50hz": l.fixed50hz, "nosettempo": l.nosettempo, "nosampleoptim": l.nosampleoptim, "preview_wav": l.preview_wav },
        });
        serde_json::to_string_pretty(&value).unwrap_or_default()
    }

    pub fn from_json(text: &str) -> Prefs {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            return Prefs::default();
        };
        let flag = |section: &str, key: &str| value[section][key].as_bool().unwrap_or(false);
        let words = |section: &str, key: &str| value[section][key].as_str().unwrap_or("").to_string();
        let number = |section: &str, key: &str| value[section][key].as_u64().and_then(|n| u32::try_from(n).ok());
        let mut prefs = Prefs::default();
        if let Some(paths) = value["paths"].as_object() {
            for (id, path) in paths {
                if let (Some(tool), Some(path)) = (Tool::from_id(id), path.as_str()) {
                    prefs.paths.push((tool, PathBuf::from(path)));
                }
            }
        }
        prefs.after_build = value["after_build"].as_str().and_then(Tool::from_id).filter(|t| Tool::CRUNCHERS.contains(t));
        prefs.shrinkler = ShrinklerOptions {
            data: flag("shrinkler", "data"),
            data_header: flag("shrinkler", "data_header"),
            hunk_merge: flag("shrinkler", "hunk_merge"),
            overlap: flag("shrinkler", "overlap"),
            mini: flag("shrinkler", "mini"),
            command_line: flag("shrinkler", "command_line"),
            bytes: flag("shrinkler", "bytes"),
            preset: number("shrinkler", "preset").map_or(3, |n| n.clamp(1, 9) as u8),
            iterations: number("shrinkler", "iterations"),
            references: number("shrinkler", "references"),
            text: words("shrinkler", "text"),
            flash: words("shrinkler", "flash"),
        };
        let decranker = words("cranker", "decranker");
        prefs.cranker = CrankerOptions { data: flag("cranker", "data"), fast: flag("cranker", "fast"), merge: flag("cranker", "merge"), decranker: if DECRANKERS.contains(&decranker.as_str()) { decranker } else { "default".into() }, text: words("cranker", "text") };
        prefs.lsp = LspOptions {
            micro: flag("lsp", "micro"),
            insane: flag("lsp", "insane"),
            getpos: flag("lsp", "getpos"),
            setpos: flag("lsp", "setpos"),
            shrink: flag("lsp", "shrink"),
            adpcm: flag("lsp", "adpcm"),
            fixed50hz: flag("lsp", "fixed50hz"),
            nosettempo: flag("lsp", "nosettempo"),
            nosampleoptim: flag("lsp", "nosampleoptim"),
            preview_wav: flag("lsp", "preview_wav"),
        };
        prefs
    }

    /// Where a tool is: where the person pointed, if it's still there; else one downloaded here; else where it's
    /// usually installed.
    pub fn find(&self, tool: Tool) -> Option<PathBuf> {
        if let Some(path) = self.path(tool).filter(|p| p.is_file()) {
            return Some(path.to_path_buf());
        }
        if let Some(downloaded) = tools_folder().map(|f| f.join(tool.id()).join(exe_name(tool))).filter(|p| p.is_file()) {
            return Some(downloaded);
        }
        crate::emulators::find_program(tool.programs(), tool.folder_hints())
    }
}

fn exe_name(tool: Tool) -> String {
    let name = tool.programs()[0];
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// Whether bytes are an AmigaDOS program: a hunk file starts with HUNK_HEADER ($3F3).
pub fn is_hunk_executable(bytes: &[u8]) -> bool {
    bytes.len() >= 24 && bytes[..4] == [0, 0, 3, 0xF3]
}

/// What a crunched copy is called, beside the original: demo becomes demo_crunched, intro.exe intro_crunched.exe.
pub fn crunched_name(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, ending)) if !stem.is_empty() && !ending.contains(['/', '\\']) => format!("{stem}_crunched.{ending}"),
        _ => format!("{name}_crunched"),
    }
}

/// What to give the cruncher to crunch `input` into `output`.
pub fn crunch_arguments(tool: Tool, prefs: &Prefs, input: &Path, output: &Path) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    match tool {
        Tool::Shrinkler => {
            let s = &prefs.shrinkler;
            if s.data {
                args.push("-d".into());
                if s.data_header {
                    args.push("-w".into());
                }
            } else {
                for (on, flag) in [(s.hunk_merge, "-h"), (s.overlap, "-o"), (s.mini, "-m"), (s.command_line, "-c")] {
                    if on {
                        args.push(flag.into());
                    }
                }
            }
            if s.bytes {
                args.push("-b".into());
            }
            args.push(format!("-{}", s.preset.clamp(1, 9)));
            if let Some(n) = s.iterations {
                args.extend(["-i".into(), n.to_string()]);
            }
            if let Some(n) = s.references {
                args.extend(["-r".into(), n.to_string()]);
            }
            if !s.data && !s.text.trim().is_empty() {
                args.extend(["-t".into(), s.text.clone()]);
            }
            if !s.data && !s.flash.trim().is_empty() {
                args.extend(["-f".into(), s.flash.trim().trim_start_matches('$').to_string()]);
            }
            // Progress is drawn with the terminal's codes, which the Output panel can't show.
            args.push("-p".into());
            args.push(input.display().to_string());
            args.push(output.display().to_string());
        }
        Tool::Cranker => {
            let c = &prefs.cranker;
            args.extend(["-f".into(), input.display().to_string(), "-o".into(), output.display().to_string()]);
            if c.fast {
                args.push("-1".into());
            }
            if c.data {
                args.push("-cd".into());
            } else {
                if c.merge {
                    args.push("-m".into());
                }
                if c.decranker != "default" && DECRANKERS.contains(&c.decranker.as_str()) {
                    args.extend(["-d".into(), c.decranker.clone()]);
                }
                if !c.text.trim().is_empty() {
                    args.extend(["-t".into(), c.text.clone()]);
                }
            }
        }
        Tool::LspConvert | Tool::P61Con => {}
    }
    args
}

/// What to give LSPConvert to convert a module.
pub fn lsp_arguments(options: &LspOptions, module: &Path) -> Vec<String> {
    let mut args = vec![module.display().to_string()];
    for (on, flag) in [
        (options.micro, "-micro"),
        (options.insane && !options.micro, "-insane"),
        (options.getpos && !options.micro, "-getpos"),
        (options.setpos && !options.micro && !options.insane, "-setpos"),
        (options.shrink, "-shrink"),
        (options.adpcm, "-adpcm"),
        (options.fixed50hz, "-fixed50hz"),
        (options.nosettempo, "-nosettempo"),
        (options.nosampleoptim, "-nosampleoptim"),
        (options.preview_wav, "-amigapreview"),
    ] {
        if on {
            args.push(flag.into());
        }
    }
    // The files are named for it here, as LSPConvert would call mod.title's files mod.lsmusic and so on.
    let outputs = lsp_outputs(module, options);
    let mut named = |flag: &str, file: Option<&PathBuf>| {
        if let Some(file) = file {
            args.extend([flag.to_string(), file.display().to_string()]);
        }
    };
    named("-lsmusic", outputs.first());
    named("-lsbank", outputs.get(1));
    named("-insanefile", outputs.iter().find(|f| f.to_string_lossy().ends_with("_insane.asm")));
    named("-wav", outputs.iter().find(|f| f.extension().is_some_and(|e| e == "wav")));
    args
}

/// A module's title as its file names it: tune for tune.mod or the Amiga's mod.tune.
pub fn module_title(module: &Path) -> String {
    let name = module.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let lower = name.to_ascii_lowercase();
    let title = if lower.starts_with("mod.") && name.len() > 4 {
        &name[4..]
    } else {
        module.file_stem().and_then(|s| s.to_str()).unwrap_or(&name)
    };
    if title.is_empty() { "music".into() } else { title.to_string() }
}

/// The files LSPConvert writes beside a module.
pub fn lsp_outputs(module: &Path, options: &LspOptions) -> Vec<PathBuf> {
    let folder = module.parent().unwrap_or(Path::new("."));
    let stem = module_title(module);
    let mut files = vec![folder.join(format!("{stem}.lsmusic")), folder.join(format!("{stem}.lsbank"))];
    if options.insane && !options.micro {
        files.push(folder.join(format!("{stem}_insane.asm")));
    }
    if options.preview_wav {
        files.push(folder.join(format!("{stem}.wav")));
    }
    files
}

/// The P61 file P61Con saves for a module: P61.name, beside it, whether the module is mod.name or name.mod.
pub fn p61_file(module: &Path) -> PathBuf {
    module.parent().unwrap_or(Path::new(".")).join(format!("P61.{}", module_title(module)))
}

/// What running a tool came to.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub ok: bool,
    pub printed: Vec<String>,
    /// The file's size before, and each file made with its size.
    pub before: u64,
    pub made: Vec<(PathBuf, u64)>,
    pub seconds: f64,
}

impl Outcome {
    /// The sizes in a sentence: "12,345 bytes to 6,789 (55% of it, 5,556 saved)".
    pub fn sizes(&self) -> String {
        match self.made.first() {
            Some((_, after)) if self.before > 0 => {
                let percent = *after as f64 * 100.0 / self.before as f64;
                let saved = self.before as i64 - *after as i64;
                crate::i18n::tf!("{} bytes to {} ({percent:.1}% of it, {} saved)", group(self.before), group(*after), if saved >= 0 { group(saved as u64) } else { format!("{} more", group(saved.unsigned_abs())) })
            }
            _ => String::new(),
        }
    }
}

pub fn group(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Runs a tool in a folder until it finishes or `stop` is set, collecting what it printed. It's meant for a
/// thread of its own: Shrinkler's highest presets can take minutes on a big program.
pub fn run(program: &Path, args: &[String], folder: &Path, stop: &AtomicBool) -> Result<(bool, Vec<String>), String> {
    let mut command = std::process::Command::new(program);
    command.args(args).current_dir(folder).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|e| crate::i18n::tf!("{} couldn't be started: {e}", program.display()))?;
    // Read as it goes, so a tool that prints a lot never stops waiting for room to print.
    let reader = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut all = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut all);
            }
            all
        })
    };
    let out = reader(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err = reader(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if stop.load(Ordering::Relaxed) || started.elapsed() > LONGEST {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if stop.load(Ordering::Relaxed) { "Stopped.".into() } else { crate::i18n::tf!("{} took over an hour, so it was stopped.", program.display()) });
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut printed = Vec::new();
    for bytes in [out.join().unwrap_or_default(), err.join().unwrap_or_default()] {
        // Progress drawn with carriage returns keeps only its last state.
        printed.extend(String::from_utf8_lossy(&bytes).lines().map(|l| l.rsplit('\r').next().unwrap_or(l).trim_end().to_string()).filter(|l| !l.is_empty()));
    }
    Ok((status.success(), printed))
}

/// Crunches `input` into `output` with a cruncher.
pub fn crunch(tool: Tool, program: &Path, prefs: &Prefs, input: &Path, output: &Path, stop: &AtomicBool) -> Result<Outcome, String> {
    let before = std::fs::metadata(input).map_err(|e| crate::i18n::tf!("{} couldn't be read: {e}", input.display()))?.len();
    let _ = std::fs::remove_file(output);
    let started = Instant::now();
    let folder = output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let (ok, printed) = run(program, &crunch_arguments(tool, prefs, input, output), folder, stop)?;
    let made: Vec<(PathBuf, u64)> = std::fs::metadata(output).ok().map(|m| (output.to_path_buf(), m.len())).into_iter().collect();
    Ok(Outcome { ok: ok && !made.is_empty(), printed, before, made, seconds: started.elapsed().as_secs_f64() })
}

/// Converts a module with LSPConvert, which writes its files beside it.
pub fn convert_lsp(program: &Path, options: &LspOptions, module: &Path, stop: &AtomicBool) -> Result<Outcome, String> {
    let before = std::fs::metadata(module).map_err(|e| crate::i18n::tf!("{} couldn't be read: {e}", module.display()))?.len();
    let started = Instant::now();
    let since = SystemTime::now() - Duration::from_secs(2);
    let folder = module.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let (ok, printed) = run(program, &lsp_arguments(options, module), folder, stop)?;
    let made: Vec<(PathBuf, u64)> = lsp_outputs(module, options)
        .into_iter()
        .filter_map(|p| std::fs::metadata(&p).ok().filter(|m| m.modified().is_ok_and(|t| t >= since)).map(|m| (p, m.len())))
        .collect();
    Ok(Outcome { ok: ok && made.len() >= 2, printed, before, made, seconds: started.elapsed().as_secs_f64() })
}

/// Fetches a tool's release and keeps its program in Limoniter's folder for tools. Only ever when asked.
pub fn download(tool: Tool) -> Result<PathBuf, String> {
    let folder = tools_folder().ok_or(crate::i18n::tr("There's nowhere to keep it: Limoniter's settings folder couldn't be found."))?.join(tool.id());
    download_into(tool, &folder)
}

fn download_into(tool: Tool, folder: &Path) -> Result<PathBuf, String> {
    let (address, inside) = tool.release().ok_or_else(|| crate::i18n::tf!("There's no {} release for this computer to download.", tool.name()))?;
    let response = ureq::get(address).header("User-Agent", "Limoniter").call().map_err(|e| crate::i18n::tf!("{address} couldn't be reached: {e}"))?;
    let mut zip = Vec::new();
    response.into_body().into_reader().take(64 << 20).read_to_end(&mut zip).map_err(|e| crate::i18n::tf!("The download stopped: {e}"))?;
    let program = from_zip(zip, inside)?;
    std::fs::create_dir_all(folder).map_err(|e| crate::i18n::tf!("{} couldn't be made: {e}", folder.display()))?;
    let path = folder.join(exe_name(tool));
    std::fs::write(&path, program).map_err(|e| crate::i18n::tf!("{} couldn't be saved: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
    }
    // A release built on a newer Linux than this one can't start here: better said now than at the first convert.
    if let Ok((_, printed)) = run(&path, &[], folder, &AtomicBool::new(false))
        && printed.iter().any(|l| l.contains("GLIBC_"))
    {
        let _ = std::fs::remove_file(&path);
        return Err(crate::i18n::tf!("{}'s release needs a newer C library (glibc) than this computer has, so it can't run here. {}", tool.name(), tool.how_to_get().replace("Download its release from GitHub (Limoniter can do that, below), or b", "B")));
    }
    // The licence travels with the program, as Shrinkler's asks.
    if tool == Tool::Shrinkler {
        let _ = std::fs::write(folder.join("README.txt"), crate::i18n::tf!("Shrinkler by Aske Simon Christensen (Blueberry), from {address}\n{}\n", tool.website()));
    }
    Ok(path)
}

/// One file out of a zip, found by its path inside.
fn from_zip(zip: Vec<u8>, inside: &str) -> Result<Vec<u8>, String> {
    let volume = crate::media::open(std::sync::Arc::new(zip), "release.zip")?;
    let mut dir: Option<crate::media::Entry> = None;
    let parts: Vec<&str> = inside.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        let entries = volume.list(dir.as_ref())?;
        let found = entries.into_iter().find(|e| e.name.eq_ignore_ascii_case(part)).ok_or_else(|| crate::i18n::tf!("The release has no {inside} in it."))?;
        if i + 1 == parts.len() {
            return volume.read(&found);
        }
        dir = Some(found);
    }
    Err(crate::i18n::tf!("The release has no {inside} in it."))
}

/// The replay calls for a module converted for a player, as commented 68000 source to adapt. They call the
/// player's own routines and include its files, which come from its package; none of its code is copied here.
pub fn replay_snippet(replay: Replay, module: &Path) -> String {
    let stem = module_title(module);
    let p61 = p61_file(module).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut s = String::new();
    let mut line = |text: &str| {
        s.push_str(text);
        s.push('\n');
    };
    match replay {
        Replay::LspCia => {
            line(&crate::i18n::tf!("; --- LightSpeedPlayer: {stem}, played by LSP's CIA toolkit ---"));
            line("; Needs LightSpeedPlayer.asm and LightSpeedPlayer_cia.asm from github.com/arnaud-carre/LSPlayer,");
            line(&crate::i18n::tf!("; and {stem}.lsmusic and {stem}.lsbank from LSPConvert. The toolkit takes the CIA-B timer and its"));
            line("; level 6 interrupt, and sets DMACON itself; it doesn't hand anything back to the system.");
            line("");
            line("music_start:");
            line("\t\tlea\tlsp_music,a0\t\t; the score: any memory");
            line("\t\tlea\tlsp_bank,a1\t\t; the samples: chip memory");
            line("\t\tsuba.l\ta2,a2\t\t\t; the VBR: 0 on a 68000 (read it with movec on a 68010 or later)");
            line("\t\tmoveq\t#0,d0\t\t\t; 0 for PAL, 1 for NTSC");
            line("\t\tbsr\tLSP_MusicDriver_CIA_Start");
            line("\t\tmove.w\t#$e000,$dff09a\t\t; INTENA: master and level 6 (CIA-B) interrupts on");
            line("\t\trts");
            line("");
            line("music_stop:");
            line("\t\tbra\tLSP_MusicDriver_CIA_Stop\t; stops the timer and the four audio channels' DMA");
            line("");
            line("\t\tinclude\t\"LightSpeedPlayer_cia.asm\"");
            line("\t\tinclude\t\"LightSpeedPlayer.asm\"");
            lsp_data(&mut line, &stem);
        }
        Replay::LspCopper | Replay::LspInsane | Replay::LspMicro => {
            let (init, tick, files) = match replay {
                Replay::LspInsane => ("LSP_MusicInitInsane", "LSP_MusicPlayTickInsane", crate::i18n::tf!("{stem}_insane.asm (written by LSPConvert -insane, for this module only)")),
                Replay::LspMicro => ("LSP_MusicInitMicro", "LSP_MusicPlayTickMicro", crate::i18n::tr("LightSpeedPlayer_Micro.asm (and convert with LSPConvert -micro)").to_string()),
                _ => ("LSP_MusicInit", "LSP_MusicPlayTick", "LightSpeedPlayer.asm".to_string()),
            };
            line(&crate::i18n::tf!("; --- LightSpeedPlayer: {stem}, played from your own interrupt once a frame ---"));
            line(&crate::i18n::tf!("; Needs {files}, from github.com/arnaud-carre/LSPlayer,"));
            line(&crate::i18n::tf!("; and {stem}.lsmusic and {stem}.lsbank from LSPConvert."));
            line("; LSP doesn't write DMACON itself: it pokes the channels to start into the low byte of a copper");
            line("; move to DMACON, which must come about 11 raster lines after the line the tick runs on, so");
            line("; Paula has stopped the old samples by then. Keep that copper move in every copper list you use.");
            line("");
            line("music_init:");
            line("\t\tlea\tlsp_music,a0\t\t; the score: any memory");
            line("\t\tlea\tlsp_bank,a1\t\t; the samples: chip memory");
            line("\t\tlea\tcopper_dmacon+3,a2\t; the low byte of the DMACON value in the copper list (an odd address)");
            line(&format!("\t\tbsr\t{init}\t; a0 comes back pointing at the music's BPM (a word)"));
            line("\t\trts");
            line("");
            line("; Once a frame, from your copper or vertical blank interrupt, before the copper reaches copper_dmacon:");
            line("music_tick:");
            line("\t\tmovem.l\td0-d2/a0-a6,-(sp)\t; the tick uses d0-d2 and a0-a5 (a list for the insane player is in its file)");
            line("\t\tlea\t$dff0a0,a6\t\t; LSP wants a6 at the first audio channel");
            line(&format!("\t\tbsr\t{tick}"));
            line("\t\tmovem.l\t(sp)+,d0-d2/a0-a6");
            line("\t\trts");
            line("");
            line("; To stop: stop calling music_tick, then turn the four audio channels off.");
            line("music_stop:");
            line("\t\tmove.w\t#$000f,$dff096\t\t; DMACON: AUD0-AUD3 off");
            line("\t\tmove.w\t#$0000,$dff0a8\t\t; and their volumes down");
            line("\t\tmove.w\t#$0000,$dff0b8");
            line("\t\tmove.w\t#$0000,$dff0c8");
            line("\t\tmove.w\t#$0000,$dff0d8");
            line("\t\trts");
            line("");
            line(&format!("\t\tinclude\t\"{}\"", files.split(' ').next().unwrap_or_default()));
            line("");
            line("\t\tsection\tcopper,data_c");
            line("copper_list:");
            line("\t\tdc.w\t$3009,$fffe\t\t; wait for line $30 (or where your interrupt fires)");
            line("\t\tdc.w\t$009c,$8010\t\t; INTREQ: a copper interrupt, which calls music_tick");
            line("\t\tdc.w\t$3b09,$fffe\t\t; 11 lines later");
            line("copper_dmacon:");
            line("\t\tdc.w\t$0096,$8000\t\t; DMACON: set; LSP fills in the channels' bits");
            line("\t\tdc.w\t$ffff,$fffe\t\t; the end of the list");
            lsp_data(&mut line, &stem);
        }
        Replay::P61 => {
            line(&crate::i18n::tf!("; --- The Player 6.1A: {p61}, made from {stem} by P61Con ---"));
            line("; Needs P6108-Play.S and P6108-StandardOptions.S from Aminet (mus/misc/P6108), and the P61 file,");
            line("; which P61Con (an Amiga program) saves. P61Con shows a USE code for the module, and keeps it in");
            line("; the P61 file's comment: set use to it and the player leaves out what the module doesn't need.");
            line("");
            line("use\t\t= -1\t\t\t; replace with the USE code P61Con gave, such as $9410");
            line("C\t\t= 0\t\t\t; your custom base is $dff000+C");
            line("\t\tinclude\t\"P6108-StandardOptions.S\"\t; cia, lev6, split4 and the rest are set in here");
            line("");
            line("music_init:");
            line("\t\tlea\t$dff000+C,a6");
            line("\t\tlea\tp61_module,a0");
            line("\t\tsub.l\ta1,a1\t\t\t; 0: the samples are in the module (else the address of the samples file)");
            line("\t\tsub.l\ta2,a2\t\t\t; only for packed samples: a buffer to unpack them into");
            line(";\t\tlea\tcopper_dmacon+3,a4\t; only with lev6=0: where P61 pokes the DMACON byte, a copper move 11 lines on, as for LSP");
            line("\t\tbsr\tP61_Init\t\t; d0 is 0 if it's a P61 module");
            line("\t\trts");
            line("");
            line("; Once a frame, from your vertical blank interrupt, unless cia=1 plays it from a timer:");
            line("music_tick:");
            line("\t\tmovem.l\td0-a6,-(sp)");
            line("\t\tlea\t$dff000+C,a6");
            line("\t\tbsr\tP61_Music\t\t; a6 is $dff000+C again afterwards");
            line("\t\tmovem.l\t(sp)+,d0-a6");
            line("\t\trts");
            line("");
            line("music_stop:");
            line("\t\tlea\t$dff000+C,a6");
            line("\t\tbra\tP61_End\t\t\t; channels off, and its interrupts given back");
            line("");
            line("\t\tinclude\t\"P6108-Play.S\"");
            line("");
            line("\t\tsection\tmusic,data_c\t\t; the whole module goes in chip memory");
            line(&format!("p61_module:\tincbin\t\"{p61}\""));
            line("\t\teven");
        }
    }
    s
}

fn lsp_data(line: &mut impl FnMut(&str), stem: &str) {
    line("");
    line("\t\tsection\tlsp_samples,data_c\t; the bank must be in chip memory");
    line(&format!("lsp_bank:\tincbin\t\"{stem}.lsbank\""));
    line("\t\teven");
    line("\t\tsection\tlsp_score,data");
    line(&format!("lsp_music:\tincbin\t\"{stem}.lsmusic\""));
    line("\t\teven");
}

/// Where a tool stops being worth waiting for, so a stuck one doesn't stay for ever: an hour, far longer than
/// Shrinkler's slowest preset takes on a whole demo.
const LONGEST: Duration = Duration::from_secs(3600);

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest AmigaDOS program: one code hunk holding moveq #0,d0 and rts.
    pub fn tiny_program() -> Vec<u8> {
        let words: [u32; 10] = [0x3F3, 0, 1, 0, 0, 1, 0x3E9, 1, 0x7000_4E75, 0x3F2];
        words.iter().flat_map(|w| w.to_be_bytes()).collect()
    }

    /// A bigger program, with enough repetition for a cruncher to find.
    fn bigger_program() -> Vec<u8> {
        let mut code = Vec::new();
        for i in 0..2000u32 {
            code.extend_from_slice(&[0x30, 0x3C]);
            code.extend_from_slice(&((i % 37) as u16).to_be_bytes());
            code.extend_from_slice(&[0x4E, 0x71]);
        }
        code.extend_from_slice(&[0x4E, 0x75]);
        while code.len() % 4 != 0 {
            code.push(0);
        }
        let longs = (code.len() / 4) as u32;
        let mut bytes: Vec<u8> = [0x3F3, 0, 1, 0, 0, longs, 0x3E9, longs].iter().flat_map(|w: &u32| w.to_be_bytes()).collect();
        bytes.extend(code);
        bytes.extend(0x3F2u32.to_be_bytes());
        bytes
    }

    #[test]
    fn programs_are_told_from_data() {
        assert!(is_hunk_executable(&tiny_program()));
        assert!(!is_hunk_executable(b"FORM\0\0\0\x04ILBM and so on, long enough"));
        assert_eq!(crunched_name("demo"), "demo_crunched");
        assert_eq!(crunched_name("intro.exe"), "intro_crunched.exe");
        assert_eq!(crunched_name(".hidden"), ".hidden_crunched");
    }

    #[test]
    fn shrinkler_is_given_the_options_chosen() {
        let mut prefs = Prefs::default();
        let (input, output) = (Path::new("in"), Path::new("out"));
        assert_eq!(crunch_arguments(Tool::Shrinkler, &prefs, input, output), ["-3", "-p", "in", "out"]);
        prefs.shrinkler = ShrinklerOptions { hunk_merge: true, overlap: true, preset: 9, references: Some(200_000), text: "Loading...".into(), flash: "$DFF180".into(), data_header: true, ..ShrinklerOptions::default() };
        assert_eq!(crunch_arguments(Tool::Shrinkler, &prefs, input, output), ["-h", "-o", "-9", "-r", "200000", "-t", "Loading...", "-f", "DFF180", "-p", "in", "out"]);
        // Data takes none of a program's options, but can have its header.
        prefs.shrinkler.data = true;
        assert_eq!(crunch_arguments(Tool::Shrinkler, &prefs, input, output), ["-d", "-w", "-9", "-r", "200000", "-p", "in", "out"]);
    }

    #[test]
    fn cranker_is_given_the_options_chosen() {
        let mut prefs = Prefs::default();
        let (input, output) = (Path::new("in"), Path::new("out"));
        assert_eq!(crunch_arguments(Tool::Cranker, &prefs, input, output), ["-f", "in", "-o", "out"]);
        prefs.cranker = CrankerOptions { fast: true, merge: true, decranker: "progress".into(), text: "hello".into(), data: false };
        assert_eq!(crunch_arguments(Tool::Cranker, &prefs, input, output), ["-f", "in", "-o", "out", "-1", "-m", "-d", "progress", "-t", "hello"]);
        prefs.cranker.data = true;
        assert_eq!(crunch_arguments(Tool::Cranker, &prefs, input, output), ["-f", "in", "-o", "out", "-1", "-cd"]);
    }

    #[test]
    fn lspconvert_is_given_the_options_chosen_and_its_files_are_known() {
        let module = Path::new("/music/rink-a-dink.mod");
        let mut options = LspOptions { insane: true, getpos: true, setpos: true, shrink: true, ..LspOptions::default() };
        // SetPos isn't in the insane player.
        // The files go beside the module, with this system's own separator.
        let beside = |name: &str| module.with_file_name(name).to_string_lossy().into_owned();
        let (music, bank, insane) = (beside("rink-a-dink.lsmusic"), beside("rink-a-dink.lsbank"), beside("rink-a-dink_insane.asm"));
        assert_eq!(lsp_arguments(&options, module), ["/music/rink-a-dink.mod", "-insane", "-getpos", "-shrink", "-lsmusic", &music, "-lsbank", &bank, "-insanefile", &insane]);
        assert_eq!(lsp_outputs(module, &options), [PathBuf::from("/music/rink-a-dink.lsmusic"), PathBuf::from("/music/rink-a-dink.lsbank"), PathBuf::from("/music/rink-a-dink_insane.asm")]);
        options.micro = true;
        assert_eq!(lsp_arguments(&options, module), ["/music/rink-a-dink.mod", "-micro", "-shrink", "-lsmusic", &music, "-lsbank", &bank]);
        // The Amiga's mod.title gets title's files, not mod's.
        assert_eq!(lsp_outputs(Path::new("/m/mod.sowhat"), &options)[0], PathBuf::from("/m/sowhat.lsmusic"));
        assert_eq!(module_title(Path::new("/m/MOD.Tune")), "Tune");
        assert_eq!(p61_file(Path::new("/m/mod.sowhat")), PathBuf::from("/m/P61.sowhat"));
        assert_eq!(p61_file(Path::new("/m/sowhat-intro.mod")), PathBuf::from("/m/P61.sowhat-intro"));
    }

    #[test]
    fn the_choices_are_remembered() {
        let mut prefs = Prefs { after_build: Some(Tool::Shrinkler), ..Prefs::default() };
        prefs.set_path(Tool::Cranker, Some(PathBuf::from("/opt/cranker/cranker")));
        prefs.shrinkler.preset = 7;
        prefs.shrinkler.iterations = Some(5);
        prefs.cranker.decranker = "cinema".into();
        prefs.lsp.micro = true;
        assert_eq!(Prefs::from_json(&prefs.to_json()), prefs);
        assert_eq!(Prefs::from_json("not json"), Prefs::default());
        // A cruncher that isn't one can't be chosen for after building.
        assert_eq!(Prefs::from_json(r#"{"after_build":"lspconvert"}"#).after_build, None);
    }

    #[test]
    fn replay_snippets_call_the_players_routines_and_include_their_files() {
        let module = Path::new("/m/rink-a-dink.mod");
        let cia = replay_snippet(Replay::LspCia, module);
        assert!(cia.contains("bsr\tLSP_MusicDriver_CIA_Start") && cia.contains("LSP_MusicDriver_CIA_Stop") && cia.contains("incbin\t\"rink-a-dink.lsbank\""));
        let copper = replay_snippet(Replay::LspCopper, module);
        assert!(copper.contains("copper_dmacon+3,a2") && copper.contains("dc.w\t$0096,$8000") && copper.contains("bsr\tLSP_MusicPlayTick") && copper.contains("include\t\"LightSpeedPlayer.asm\""));
        assert!(copper.contains("11 raster lines"), "the DMACON-in-copper note");
        let insane = replay_snippet(Replay::LspInsane, module);
        assert!(insane.contains("LSP_MusicInitInsane") && insane.contains("include\t\"rink-a-dink_insane.asm\""));
        assert!(replay_snippet(Replay::LspMicro, module).contains("include\t\"LightSpeedPlayer_Micro.asm\""));
        let p61 = replay_snippet(Replay::P61, module);
        assert!(p61.contains("bsr\tP61_Init") && p61.contains("bsr\tP61_Music") && p61.contains("P61_End") && p61.contains("incbin\t\"P61.rink-a-dink\""));
        // Every line is a comment, a label, or an instruction indented by tabs: something to paste into source.
        for snippet in [cia, copper, insane, p61] {
            assert!(snippet.lines().all(|l| l.is_empty() || l.starts_with(';') || l.starts_with('\t') || l.contains(':') || l.contains('=')), "{snippet}");
        }
    }

    #[test]
    fn sizes_are_said_plainly() {
        let outcome = Outcome { ok: true, before: 12_345, made: vec![(PathBuf::from("x"), 6_789)], ..Outcome::default() };
        assert_eq!(outcome.sizes(), "12,345 bytes to 6,789 (55.0% of it, 5,556 saved)");
    }

    /// Where a tool is for these tests: named by an environment variable (LIMONITER_SHRINKLER and so on), or
    /// found as Limoniter would find it. Without one, the test says so and passes.
    fn tool_for_test(tool: Tool) -> Option<PathBuf> {
        let variable = format!("LIMONITER_{}", tool.id().to_ascii_uppercase());
        std::env::var_os(&variable).map(PathBuf::from).filter(|p| p.is_file()).or_else(|| Prefs::default().find(tool)).or_else(|| {
            eprintln!("{} isn't installed (or named by {variable}), so it wasn't tried.", tool.name());
            None
        })
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-amiga-tools-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn shrinkler_crunches_a_program_when_it_is_installed() {
        let Some(program) = tool_for_test(Tool::Shrinkler) else {
            return;
        };
        let dir = scratch("shrinkler");
        let input = dir.join("demo");
        std::fs::write(&input, bigger_program()).unwrap();
        let output = dir.join(crunched_name("demo"));
        let prefs = Prefs { shrinkler: ShrinklerOptions { preset: 1, ..ShrinklerOptions::default() }, ..Prefs::default() };
        let outcome = crunch(Tool::Shrinkler, &program, &prefs, &input, &output, &AtomicBool::new(false)).unwrap();
        assert!(outcome.ok, "{:?}", outcome.printed);
        let crunched = std::fs::read(&output).unwrap();
        assert!(is_hunk_executable(&crunched), "a crunched program is still a program");
        assert!(outcome.made[0].1 < outcome.before, "{}", outcome.sizes());
        // And data, with its header.
        let data_prefs = Prefs { shrinkler: ShrinklerOptions { data: true, data_header: true, preset: 1, ..ShrinklerOptions::default() }, ..Prefs::default() };
        let data = dir.join("table.bin");
        std::fs::write(&data, b"sine table ".repeat(500)).unwrap();
        let outcome = crunch(Tool::Shrinkler, &program, &data_prefs, &data, &dir.join("table.shr"), &AtomicBool::new(false)).unwrap();
        assert!(outcome.ok && outcome.made[0].1 < 5500, "{:?}", outcome.printed);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cranker_crunches_a_program_when_it_is_installed() {
        let Some(program) = tool_for_test(Tool::Cranker) else {
            return;
        };
        let dir = scratch("cranker");
        let input = dir.join("demo");
        std::fs::write(&input, bigger_program()).unwrap();
        let output = dir.join("demo_crunched");
        let outcome = crunch(Tool::Cranker, &program, &Prefs::default(), &input, &output, &AtomicBool::new(false)).unwrap();
        assert!(outcome.ok, "{:?}", outcome.printed);
        assert!(is_hunk_executable(&std::fs::read(&output).unwrap()));
        assert!(outcome.made[0].1 < outcome.before, "{}", outcome.sizes());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lspconvert_converts_a_module_when_it_is_installed() {
        let Some(program) = tool_for_test(Tool::LspConvert) else {
            return;
        };
        let dir = scratch("lsp");
        // A module made here: one short looped square wave played on each row of one pattern.
        let mut bytes = crate::protracker::tests::synthetic("square", &[("square", 16, 64, 0, 0, 16)], &[0], b"M.K.", 4);
        let pattern = 1084;
        for row in 0..64 {
            let at = pattern + row * 16;
            // Sample 1, period 428 (C-2), no effect.
            bytes[at..at + 4].copy_from_slice(&[0x01, 0xAC, 0x10, 0x00]);
        }
        let sample = bytes.len() - 32;
        for (i, b) in bytes[sample..].iter_mut().enumerate() {
            *b = if i < 16 { 0x40 } else { 0xC0 };
        }
        // Named as the Amiga names modules, so LSPConvert has to be told what to call its files.
        let module = dir.join("mod.square");
        std::fs::write(&module, &bytes).unwrap();
        let options = LspOptions { insane: true, ..LspOptions::default() };
        let outcome = convert_lsp(&program, &options, &module, &AtomicBool::new(false)).unwrap();
        assert!(outcome.ok, "{:?}", outcome.printed);
        assert_eq!(outcome.made.len(), 3, "{:?}", outcome.made);
        assert_eq!(&std::fs::read(dir.join("square.lsmusic")).unwrap()[..4], b"LSP1");
        assert!(std::fs::read_to_string(dir.join("square_insane.asm")).unwrap().contains("LSP_MusicPlayTickInsane"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The replay calls assemble, with Limoniter's own 68000 assembler, against the players' real files: from a
    /// copy of LSPlayer's repository named by LIMONITER_LSPLAYER, and P61's package by LIMONITER_P61.
    #[test]
    fn replay_snippets_assemble_with_the_players_files() {
        let lsp = std::env::var_os("LIMONITER_LSPLAYER").map(PathBuf::from).filter(|p| p.join("LightSpeedPlayer.asm").is_file());
        let p61 = std::env::var_os("LIMONITER_P61").map(PathBuf::from).filter(|p| p.join("P6108-Play.S").is_file());
        if lsp.is_none() && p61.is_none() {
            eprintln!("Neither LIMONITER_LSPLAYER nor LIMONITER_P61 is set, so the snippets weren't assembled.");
            return;
        }
        let dir = scratch("snippets");
        let module = dir.join("tune.mod");
        let mut replays = Vec::new();
        if let Some(lsp) = &lsp {
            for file in ["LightSpeedPlayer.asm", "LightSpeedPlayer_cia.asm", "LightSpeedPlayer_Micro.asm"] {
                std::fs::copy(lsp.join(file), dir.join(file)).unwrap();
            }
            std::fs::write(dir.join("tune.lsmusic"), [0u8; 64]).unwrap();
            std::fs::write(dir.join("tune.lsbank"), [0u8; 64]).unwrap();
            replays.extend([Replay::LspCia, Replay::LspCopper, Replay::LspMicro]);
        }
        if let Some(p61) = &p61 {
            for file in ["P6108-Play.S", "P6108-StandardOptions.S", "periods", "vibtab"] {
                if p61.join(file).is_file() {
                    std::fs::copy(p61.join(file), dir.join(file)).unwrap();
                }
            }
            std::fs::write(dir.join("P61.tune"), [0u8; 64]).unwrap();
            replays.push(Replay::P61);
        }
        for replay in replays {
            let source = format!("\t\tsection\tcode,code\nstart:\t\tbsr\tmusic_{}\n\t\trts\n{}", if replay == Replay::LspCia { "start" } else { "init" }, replay_snippet(replay, &module));
            let path = dir.join("main.s");
            std::fs::write(&path, &source).unwrap();
            let assembled = crate::assemblers::assemble_own(crate::assemblers::Cpu::M68k, &path, &source);
            let say = |p: &crate::asm::Problem| format!("{}:{}: {}", assembled.assembly.files.get(p.file).map(|f| f.display().to_string()).unwrap_or_default(), p.line + 1, p.message);
            // The players' own files use directives of other assemblers (rsreset, rs.l) that Limoniter's doesn't
            // know; what matters here is that the calls written for them, in main.s, are right.
            let (mine, theirs): (Vec<_>, Vec<_>) = assembled.assembly.problems.iter().partition(|p| p.file == 0);
            for problem in theirs {
                eprintln!("In the player's file, not the snippet: {}", say(problem));
            }
            assert!(mine.is_empty(), "{}: {:#?}", replay.name(), mine.iter().map(|p| say(p)).collect::<Vec<_>>());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Fetching the releases from GitHub, only when LIMONITER_TEST_DOWNLOADS is set: the programs must come out of
    /// their zips and run.
    #[test]
    fn releases_download_and_run_when_asked() {
        if std::env::var_os("LIMONITER_TEST_DOWNLOADS").is_none() {
            eprintln!("LIMONITER_TEST_DOWNLOADS isn't set, so nothing was downloaded.");
            return;
        }
        for tool in [Tool::Shrinkler, Tool::LspConvert] {
            if tool.release().is_none() {
                continue;
            }
            let dir = scratch(&format!("download-{}", tool.id()));
            let program = match download_into(tool, &dir) {
                Ok(program) => program,
                // Ubuntu's LSPConvert is built against a newer glibc than some systems have, and says so.
                Err(problem) if problem.contains("glibc") => {
                    eprintln!("{problem}");
                    continue;
                }
                Err(problem) => panic!("{problem}"),
            };
            let (_, printed) = run(&program, &[], &dir, &AtomicBool::new(false)).unwrap();
            let expected = if tool == Tool::Shrinkler { "Shrinkler executable file compressor" } else { "Light Speed Player" };
            assert!(printed.iter().any(|l| l.contains(expected)), "{printed:?}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn a_stop_ends_a_tool_at_once() {
        let stop = AtomicBool::new(true);
        let shell = if cfg!(windows) { ("cmd", vec!["/C".to_string(), "ping -n 30 127.0.0.1".into()]) } else { ("sh", vec!["-c".to_string(), "sleep 30".into()]) };
        let started = Instant::now();
        assert_eq!(run(Path::new(shell.0), &shell.1, Path::new("."), &stop), Err(crate::i18n::tr("Stopped.").into()));
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
