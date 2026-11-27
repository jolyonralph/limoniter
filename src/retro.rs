//! Emulators in Limoniter's own window: libretro cores (Fuse, VICE, PUAE, Genesis Plus GX, Stella and others)
//! run by limoniter-retro, a small helper program of its own (GPL, so the cores' GPL code never comes into
//! Limoniter). The helper runs the core with no window, writes each frame into a file both programs map into
//! memory, takes commands as JSON lines on its stdin, and plays the sound itself. Limoniter draws the frames in a
//! panel (see `retro_host`), so this works on macOS and Wayland too, where one program's window can't be put
//! inside another's.
//!
//! Here: finding the helper, the table of machines and their cores, downloading cores and the helper, the protocol
//! client, and the frame reader.

use crate::running::{Format, Machine, Runnable};
use eframe::egui;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Where the helper's releases are published, on GitHub.
pub const HELPER_REPOSITORY: &str = "jolyonralph/limoniter-retro";
/// The helper's program, without .exe.
pub const HELPER_PROGRAM: &str = "limoniter-retro";
/// The protocol version this side speaks; the helper says its own in its ready message.
pub const PROTOCOL: u64 = 1;

/// A libretro core, and the machines it's offered for.
pub struct Core {
    /// Its name on the libretro buildbot: the file is `<core>_libretro.so` (or .dll, .dylib).
    pub core: &'static str,
    /// What it's called.
    pub name: &'static str,
    pub machines: &'static [Machine],
    /// The kinds of file it's given.
    pub formats: &'static [Format],
    /// Whether the machine is a computer, given the keyboard; a console gets its joypad from the keys.
    pub keyboard: bool,
    /// Whether it takes the mouse (the Amiga's).
    pub mouse: bool,
    /// The ROMs it needs in the system folder, if any, in words.
    pub needs: Option<&'static str>,
}

use Format as F;
use Machine as M;

/// The machines Limoniter knows that have a well-known libretro core. The first core for a machine is the one
/// used, unless only a later one is installed.
pub const CORES: &[Core] = &[
    Core { core: "fuse", name: "Fuse", machines: &[M::Spectrum48, M::Spectrum128, M::SpectrumPlus3, M::Pentagon], formats: &[F::Tap, F::Tzx, F::Z80, F::Sna, F::Dsk, F::Trd, F::Scl], keyboard: true, mouse: false, needs: None },
    Core { core: "vice_x64sc", name: "VICE (x64sc)", machines: &[M::C64], formats: &[F::Prg, F::P00, F::T64, F::D64, F::D71, F::D81, F::Crt, F::Tap], keyboard: true, mouse: false, needs: None },
    Core { core: "vice_x128", name: "VICE (x128)", machines: &[M::C128], formats: &[F::Prg, F::P00, F::T64, F::D64, F::D71, F::D81, F::Crt], keyboard: true, mouse: false, needs: None },
    Core { core: "vice_xvic", name: "VICE (xvic)", machines: &[M::Vic20], formats: &[F::Prg, F::P00, F::T64, F::D64, F::Crt, F::Tap], keyboard: true, mouse: false, needs: None },
    Core { core: "vice_xplus4", name: "VICE (xplus4)", machines: &[M::Plus4], formats: &[F::Prg, F::P00, F::T64, F::D64, F::Tap], keyboard: true, mouse: false, needs: None },
    Core { core: "vice_xpet", name: "VICE (xpet)", machines: &[M::Pet], formats: &[F::Prg, F::P00, F::D64], keyboard: true, mouse: false, needs: None },
    Core { core: "puae", name: "PUAE", machines: &[M::Amiga], formats: &[F::Adf], keyboard: true, mouse: true, needs: Some("Kickstart ROMs, named as PUAE looks for them (kick34005.A500 for an A500's Kickstart 1.3, kick40068.A1200 for an A1200's 3.1)") },
    Core { core: "genesis_plus_gx", name: "Genesis Plus GX", machines: &[M::MegaDrive, M::MasterSystem, M::GameGear, M::Sg1000], formats: &[F::MdRom, F::SmsRom, F::GgRom, F::SgRom], keyboard: false, mouse: false, needs: None },
    Core { core: "picodrive", name: "PicoDrive", machines: &[M::MegaDrive, M::MasterSystem], formats: &[F::MdRom, F::SmsRom], keyboard: false, mouse: false, needs: None },
    Core { core: "stella", name: "Stella", machines: &[M::Atari2600], formats: &[F::A26], keyboard: false, mouse: false, needs: None },
    Core { core: "prosystem", name: "ProSystem", machines: &[M::Atari7800], formats: &[F::A78], keyboard: false, mouse: false, needs: None },
    Core { core: "atari800", name: "Atari800", machines: &[M::Atari8], formats: &[F::Xex, F::Atr, F::Car], keyboard: true, mouse: false, needs: Some("the Atari's ROMs (ATARIXL.ROM, ATARIBAS.ROM)") },
    Core { core: "nestopia", name: "Nestopia", machines: &[M::Nes], formats: &[F::Nes], keyboard: false, mouse: false, needs: None },
    Core { core: "gambatte", name: "Gambatte", machines: &[M::GameBoy], formats: &[F::Gb, F::Gbc], keyboard: false, mouse: false, needs: None },
    Core { core: "snes9x", name: "Snes9x", machines: &[M::Snes], formats: &[F::Sfc, F::Smc], keyboard: false, mouse: false, needs: None },
    Core { core: "bluemsx", name: "blueMSX", machines: &[M::Msx, M::ColecoVision], formats: &[F::Rom, F::Dsk, F::Cas, F::ColRom], keyboard: true, mouse: false, needs: Some("blueMSX's Machines and Databases folders") },
    Core { core: "cap32", name: "Caprice32", machines: &[M::Cpc], formats: &[F::Dsk, F::Cdt], keyboard: true, mouse: false, needs: None },
    Core { core: "hatari", name: "Hatari", machines: &[M::AtariSt], formats: &[F::St, F::Msa], keyboard: true, mouse: true, needs: Some("a TOS ROM, named tos.img") },
    Core { core: "vecx", name: "vecx", machines: &[M::Vectrex], formats: &[F::VecCart], keyboard: false, mouse: false, needs: None },
    Core { core: "dosbox_pure", name: "DOSBox Pure", machines: &[M::Pc], formats: &[F::Com, F::Img], keyboard: true, mouse: true, needs: None },
    Core { core: "tic80", name: "TIC-80", machines: &[M::Tic80], formats: &[F::Tic], keyboard: true, mouse: true, needs: None },
];

/// The cores for a machine, the first choice first.
pub fn cores_for(machine: Machine) -> impl Iterator<Item = &'static Core> {
    CORES.iter().filter(move |core| core.machines.contains(&machine))
}

/// Every machine with a core, in the table's order, each once.
pub fn machines() -> Vec<Machine> {
    let mut all: Vec<Machine> = Vec::new();
    for core in CORES {
        for machine in core.machines {
            if !all.contains(machine) {
                all.push(*machine);
            }
        }
    }
    all
}

/// The core to run a machine's programs with: the first that's installed, or else the first.
pub fn core_for(machine: Machine) -> Option<&'static Core> {
    cores_for(machine).find(|core| installed_core(core.core).is_some()).or_else(|| cores_for(machine).next())
}

/// What a machine's choice in Run on is called.
pub fn target_name(machine: Machine) -> String {
    crate::i18n::tf!("{}, in Limoniter's window", machine.name())
}

/// The machine a Run on choice's name is for.
pub fn machine_named(name: &str) -> Option<Machine> {
    machines().into_iter().find(|machine| target_name(*machine) == name)
}

/// The ending of a core's file on this kind of system.
fn library_ending(os: &str) -> &'static str {
    match os {
        "windows" => "dll",
        "macos" => "dylib",
        _ => "so",
    }
}

/// A core's file, as the buildbot names it.
pub fn core_file(core: &str, os: &str) -> String {
    format!("{core}_libretro.{}", library_ending(os))
}

/// Where the libretro buildbot keeps a core's latest nightly build for this system and processor, zipped.
pub fn core_address(core: &str, os: &str, arch: &str) -> Option<String> {
    let folder = match (os, arch) {
        ("linux", "x86_64") => "linux/x86_64",
        ("windows", "x86_64") => "windows/x86_64",
        ("macos", "x86_64") => "apple/osx/x86_64",
        ("macos", "aarch64") => "apple/osx/arm64",
        _ => return None,
    };
    Some(format!("https://buildbot.libretro.com/nightly/{folder}/latest/{}.zip", core_file(core, os)))
}

/// The name of the helper's release download for this system and processor.
pub fn helper_asset(os: &str, arch: &str) -> Option<&'static str> {
    Some(match (os, arch) {
        ("windows", "x86_64") => "limoniter-retro-windows-x64.zip",
        ("macos", "aarch64") => "limoniter-retro-macos-arm64.zip",
        ("macos", "x86_64") => "limoniter-retro-macos-x64.zip",
        ("linux", "x86_64") => "limoniter-retro-linux-x64.zip",
        _ => return None,
    })
}

/// Where cores are kept, beside Get an emulator's emulators.
pub fn cores_folder() -> Option<PathBuf> {
    crate::emulator_get::folder().map(|f| f.join("libretro-cores"))
}

/// The folder the cores are told is the system's, where BIOS and Kickstart files go.
pub fn system_folder() -> Option<PathBuf> {
    crate::emulator_get::folder().map(|f| f.join("libretro-system"))
}

/// Where the cores keep their saves, and where the frames file and the program being run are put.
pub fn saves_folder() -> Option<PathBuf> {
    crate::emulator_get::folder().map(|f| f.join("libretro-saves"))
}

/// A core's file, if it's been downloaded.
pub fn installed_core(core: &str) -> Option<PathBuf> {
    let path = cores_folder()?.join(core_file(core, std::env::consts::OS));
    path.is_file().then_some(path)
}

/// What's remembered: the helper chosen by hand, and the options set for each core.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prefs {
    pub helper: Option<PathBuf>,
    /// Each core's options as set from Limoniter: the core, then the option's key and value.
    pub options: Vec<(String, String, String)>,
}

thread_local! {
    /// The choices as a test leaves them, in place of the file.
    static TEST_PREFS: std::cell::RefCell<Prefs> = std::cell::RefCell::new(Prefs::default());
}

fn prefs_file() -> Option<PathBuf> {
    crate::settings::data_folder().map(|d| d.join("retro.json"))
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
            return;
        }
        if let Some(file) = prefs_file() {
            if let Some(folder) = file.parent() {
                let _ = std::fs::create_dir_all(folder);
            }
            let _ = std::fs::write(file, self.to_json());
        }
    }

    pub fn from_json(text: &str) -> Prefs {
        let json: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        let helper = json["helper"].as_str().filter(|s| !s.is_empty()).map(PathBuf::from);
        let options = json["options"]
            .as_array()
            .map(|all| all.iter().filter_map(|o| Some((o["core"].as_str()?.to_string(), o["key"].as_str()?.to_string(), o["value"].as_str()?.to_string()))).collect())
            .unwrap_or_default();
        Prefs { helper, options }
    }

    pub fn to_json(&self) -> String {
        let options: Vec<Value> = self.options.iter().map(|(core, key, value)| json!({"core": core, "key": key, "value": value})).collect();
        serde_json::to_string_pretty(&json!({"helper": self.helper.as_ref().map(|p| p.display().to_string()), "options": options})).unwrap_or_default()
    }

    /// The options set for a core.
    pub fn options_for(&self, core: &str) -> Vec<(String, String)> {
        self.options.iter().filter(|(c, _, _)| c == core).map(|(_, key, value)| (key.clone(), value.clone())).collect()
    }

    pub fn set_option(&mut self, core: &str, key: &str, value: &str) {
        self.options.retain(|(c, k, _)| !(c == core && k == key));
        self.options.push((core.to_string(), key.to_string(), value.to_string()));
    }
}

fn program_name() -> String {
    if cfg!(windows) { format!("{HELPER_PROGRAM}.exe") } else { HELPER_PROGRAM.to_string() }
}

/// The helper: the one chosen in Settings, or named by `LIMONITER_RETRO_HELPER`, or beside Limoniter's own
/// program, or in Get an emulator's folder.
pub fn find_helper() -> Option<PathBuf> {
    let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf));
    find_helper_in(Prefs::load().helper, std::env::var_os("LIMONITER_RETRO_HELPER").map(PathBuf::from), beside, crate::emulator_get::folder())
}

fn find_helper_in(chosen: Option<PathBuf>, named: Option<PathBuf>, beside: Option<PathBuf>, folder: Option<PathBuf>) -> Option<PathBuf> {
    chosen
        .filter(|p| p.is_file())
        .or_else(|| named.filter(|p| p.is_file()))
        .or_else(|| beside.map(|b| b.join(program_name())).filter(|p| p.is_file()))
        .or_else(|| folder.and_then(|f| crate::emulator_get::find_program(&f.join(HELPER_PROGRAM), &[HELPER_PROGRAM])))
}

/// What's said when the helper has no release to download yet.
pub fn no_release_words() -> String {
    crate::i18n::tf!("No release of the helper ({HELPER_REPOSITORY}) has been published on GitHub yet, so there's nothing to download. If you've built it yourself, choose it with Choose helper\u{2026}.")
}

/// The helper's download in GitHub's description of its latest release.
fn pick_helper_asset(release: &str, wanted: &str) -> Result<String, String> {
    let json: Value = serde_json::from_str(release).map_err(|_| crate::i18n::tr("GitHub's answer wasn't what was expected.").to_string())?;
    let tag = json["tag_name"].as_str().unwrap_or("").to_string();
    json["assets"]
        .as_array()
        .and_then(|assets| assets.iter().find(|a| a["name"].as_str() == Some(wanted)))
        .and_then(|a| a["browser_download_url"].as_str())
        .map(str::to_string)
        .ok_or_else(|| crate::i18n::tf!("The helper's latest release ({tag}) has no {wanted} for this computer."))
}

fn fetch(address: &str) -> Result<Result<Vec<u8>, u16>, String> {
    use std::io::Read;
    match ureq::get(address).header("User-Agent", "Limoniter").call() {
        Ok(response) => {
            let mut bytes = Vec::new();
            response.into_body().into_reader().take(200 << 20).read_to_end(&mut bytes).map_err(|e| crate::i18n::tf!("The download stopped: {e}"))?;
            Ok(Ok(bytes))
        }
        Err(ureq::Error::StatusCode(code)) => Ok(Err(code)),
        Err(e) => Err(crate::i18n::tf!("{address} couldn't be reached: {e}")),
    }
}

/// Downloads the helper's latest release for this computer into Get an emulator's folder, and gives its program.
pub fn download_helper() -> Result<PathBuf, String> {
    let wanted = helper_asset(std::env::consts::OS, std::env::consts::ARCH).ok_or(crate::i18n::tr("The helper isn't built for this kind of computer: build it from its source, and choose it by hand."))?;
    let home = crate::emulator_get::folder().ok_or(crate::i18n::tr("There's nowhere to keep it: Limoniter's settings folder couldn't be found."))?;
    let release = match fetch(&format!("https://api.github.com/repos/{HELPER_REPOSITORY}/releases/latest"))? {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        // GitHub says 404 for a repository with no releases.
        Err(404) => return Err(no_release_words()),
        Err(code) => return Err(crate::i18n::tf!("GitHub answered {code} when asked for the helper's latest release.")),
    };
    let address = pick_helper_asset(&release, wanted)?;
    let bytes = fetch(&address)?.map_err(|code| crate::i18n::tf!("The helper's download answered {code}."))?;
    let target = home.join(HELPER_PROGRAM);
    let _ = std::fs::remove_dir_all(&target);
    crate::emulator_get::unzip(bytes, &target)?;
    let program = crate::emulator_get::find_program(&target, &[HELPER_PROGRAM]).ok_or_else(|| crate::i18n::tf!("The helper was unpacked into {}, but its program wasn't found in it.", target.display()))?;
    crate::emulator_get::make_runnable(&program, 0o755);
    Ok(program)
}

/// Downloads a core from the libretro buildbot into the cores folder, and gives its file.
pub fn download_core(core: &str) -> Result<PathBuf, String> {
    let address = core_address(core, std::env::consts::OS, std::env::consts::ARCH).ok_or(crate::i18n::tr("The libretro buildbot has no cores for this kind of computer."))?;
    let folder = cores_folder().ok_or(crate::i18n::tr("There's nowhere to keep it: Limoniter's settings folder couldn't be found."))?;
    let bytes = fetch(&address)?.map_err(|code| crate::i18n::tf!("The buildbot answered {code} for {address}."))?;
    crate::emulator_get::unzip(bytes, &folder)?;
    let _ = std::fs::create_dir_all(system_folder().unwrap_or_default());
    installed_core(core).ok_or_else(|| crate::i18n::tf!("The download from {address} didn't have {} in it.", core_file(core, std::env::consts::OS)))
}

/// The program as a core can take it: a Spectrum BASIC program in a +3DOS file (as the disk browser gives one
/// from a tape or disk) goes on a tape, as SAVE would write it, since Fuse loads tapes.
pub fn prepared(run: &Runnable) -> Runnable {
    let spectrum = matches!(run.machine, Machine::Spectrum48 | Machine::Spectrum128 | Machine::SpectrumPlus3);
    let bytes = run.bytes.as_slice();
    if spectrum && run.format == Format::Plus3dos && bytes.len() >= 128 && bytes.starts_with(b"PLUS3DOS") && bytes[15] == 0 {
        let length = usize::from(u16::from_le_bytes([bytes[16], bytes[17]]));
        let start = u16::from_le_bytes([bytes[18], bytes[19]]);
        let program = &bytes[128..(128 + length).min(bytes.len())];
        let stem = Path::new(&run.name).file_stem().map_or_else(|| run.name.clone(), |s| s.to_string_lossy().into_owned());
        let tape = crate::basic::spectrum_tape(program, &stem, (start < 0x8000).then_some(start));
        return Runnable { machine: run.machine, format: Format::Tap, name: format!("{stem}.tap"), bytes: std::sync::Arc::new(tape) };
    }
    run.clone()
}

/// Whether a machine's programs can be run in Limoniter's window; if not, why not, in words that say what to do.
pub fn takes(machine: Machine, run: &Runnable) -> Result<&'static Core, String> {
    let run = &prepared(run);
    let core = core_for(machine).ok_or(crate::i18n::tr("No libretro core is known for it."))?;
    if run.machine != machine {
        return Err(crate::i18n::tf!("It runs {} programs.", machine.name()));
    }
    if !cores_for(machine).any(|c| c.formats.contains(&run.format)) {
        return Err(crate::i18n::tf!("{} isn't given .{} files.", core.name, run.format.ending()));
    }
    if find_helper().is_none() {
        return Err(crate::i18n::tr("It needs the helper, limoniter-retro: Get an emulator, under Devices and emulators, gets it, or choose it by hand.").into());
    }
    let core = cores_for(machine).find(|c| c.formats.contains(&run.format) && installed_core(c.core).is_some()).ok_or_else(|| crate::i18n::tf!("It needs the {} core: Get an emulator, under Devices and emulators, downloads it.", core.name))?;
    Ok(core)
}

// The frames file, which the helper writes and Limoniter reads.
//
// Layout (protocol version 1). Every number is little-endian; the same description is in limoniter-retro's
// src/frames.rs, and the two must be changed together.
//
// | offset | size | what                                                                    |
// |--------|------|-------------------------------------------------------------------------|
// | 0      | 8    | the magic, `LIMRETRO`                                                   |
// | 8      | 4    | the protocol version, 1                                                 |
// | 12     | 4    | each buffer's size in bytes (its capacity)                              |
// | 16     | 8    | the number of the newest whole frame, 0 before the first; written last |
// | 24     | 4    | buffer 0's width in pixels                                              |
// | 28     | 4    | buffer 0's height in pixels                                             |
// | 32     | 4    | buffer 1's width in pixels                                              |
// | 36     | 4    | buffer 1's height in pixels                                             |
// | 40     | 24   | kept for later, zero                                                    |
// | 64     | cap  | buffer 0: RGBA, four bytes a pixel, rows one after another, no padding  |
// | 64+cap | cap  | buffer 1, the same                                                      |
//
// Frame number n (from 1) is in buffer n % 2. The helper writes the buffer, then its width and height, and only
// then the frame number. So the frame number is read, that buffer copied, and the frame number read again: if
// it's moved on by two or more, the buffer may have been written over while it was copied, and it's read again.
// When the picture outgrows the buffers the file is made bigger, and the capacity says so.

pub const FRAME_MAGIC: &[u8; 8] = b"LIMRETRO";
pub const FRAME_HEADER: usize = 64;

/// A picture from the core, as RGBA.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub number: u64,
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

/// Reads frames from the file the helper writes them to.
pub struct FrameReader {
    file: std::fs::File,
    map: Option<memmap2::Mmap>,
}

fn u32_at(bytes: &[u8], at: usize) -> usize {
    bytes.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
}

impl FrameReader {
    pub fn open(path: &Path) -> Result<FrameReader, String> {
        let file = std::fs::File::open(path).map_err(|e| crate::i18n::tf!("The helper's frames ({}) couldn't be opened: {e}", path.display()))?;
        Ok(FrameReader { file, map: None })
    }

    fn frame_number(map: &memmap2::Mmap) -> u64 {
        // SAFETY: offset 16 of a page-aligned mapping at least 64 bytes long is an aligned u64; the helper writes it
        // with one store, after the picture.
        let number = unsafe { std::ptr::read_volatile(map.as_ptr().add(16) as *const u64) };
        std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
        u64::from_le(number)
    }

    /// The newest frame, if it's newer than `after`.
    pub fn read(&mut self, after: u64) -> Option<Frame> {
        for _ in 0..4 {
            let length = self.file.metadata().ok()?.len() as usize;
            if self.map.as_ref().is_none_or(|m| m.len() != length) {
                if length < FRAME_HEADER {
                    return None;
                }
                // SAFETY: the helper makes the file once, at its full size, and never resizes it while it runs (Windows
                // won't resize a mapped file); a different size, as while it's first being made, is noticed
                // here before the mapping is read.
                self.map = Some(unsafe { memmap2::Mmap::map(&self.file) }.ok()?);
            }
            let map = self.map.as_ref()?;
            if &map[..8] != FRAME_MAGIC || u32_at(map, 8) != 1 {
                return None;
            }
            let capacity = u32_at(map, 12);
            if FRAME_HEADER + capacity * 2 > map.len() {
                // The file is growing: map it again.
                self.map = None;
                continue;
            }
            let number = Self::frame_number(map);
            if number <= after {
                return None;
            }
            let which = (number % 2) as usize;
            let (width, height) = (u32_at(map, 24 + which * 8), u32_at(map, 28 + which * 8));
            if width * height * 4 > capacity || width == 0 || height == 0 {
                return None;
            }
            let at = FRAME_HEADER + capacity * which;
            let pixels = map[at..at + width * height * 4].to_vec();
            std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
            if Self::frame_number(map) < number + 2 {
                return Some(Frame { number, width, height, pixels });
            }
        }
        None
    }
}

/// What the helper said when it was ready.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ready {
    pub core: String,
    pub version: String,
    pub loaded: bool,
    pub width: u32,
    pub height: u32,
    pub aspect: f32,
    pub fps: f64,
    /// Memory regions, by name, with their sizes.
    pub regions: Vec<(String, usize)>,
    /// The core's options: key, what it's called, its values, and the one it has.
    pub options: Vec<CoreOption>,
    /// Why the program couldn't be loaded, if it couldn't.
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoreOption {
    pub key: String,
    pub description: String,
    pub values: Vec<String>,
    pub value: String,
}

impl Ready {
    pub fn from_json(value: &Value) -> Ready {
        let regions = value["regions"].as_array().map(|all| all.iter().filter_map(|r| Some((r["name"].as_str()?.to_string(), r["size"].as_u64()? as usize))).collect()).unwrap_or_default();
        let mut ready = Ready { core: value["core"].as_str().unwrap_or("").into(), version: value["version"].as_str().unwrap_or("").into(), regions, error: value["error"].as_str().map(str::to_string), ..Ready::default() };
        ready.loaded = value["loaded"].as_bool().unwrap_or(false);
        ready.update(value);
        ready
    }

    /// Takes in a geometry event, or the reply to load, which describe the machine as ready does.
    pub fn update(&mut self, value: &Value) {
        if let Some(width) = value["width"].as_u64() {
            self.width = width as u32;
        }
        if let Some(height) = value["height"].as_u64() {
            self.height = height as u32;
        }
        if let Some(aspect) = value["aspect"].as_f64() {
            self.aspect = aspect as f32;
        }
        if let Some(fps) = value["fps"].as_f64() {
            self.fps = fps;
        }
        if let Some(options) = value["options"].as_array() {
            self.options = options
                .iter()
                .filter_map(|o| {
                    Some(CoreOption {
                        key: o["key"].as_str()?.into(),
                        description: o["description"].as_str().unwrap_or("").into(),
                        values: o["values"].as_array().map(|v| v.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default(),
                        value: o["value"].as_str().unwrap_or("").into(),
                    })
                })
                .collect();
        }
    }

    /// The machine's system RAM, if the core lets it be seen.
    pub fn system_ram(&self) -> Option<usize> {
        self.regions.iter().find(|(name, _)| name == "system_ram").map(|(_, size)| *size)
    }

    /// The memory to show, and how big it is: the core's memory map, which has the machine's own addresses,
    /// when it gives one, or else its system RAM.
    pub fn memory(&self) -> Option<(&'static str, usize)> {
        let size = |wanted: &str| self.regions.iter().find(|(name, _)| name == wanted).map(|(_, size)| *size);
        size("memory_map").map(|s| ("memory_map", s)).or_else(|| self.system_ram().map(|s| ("system_ram", s)))
    }
}

/// A line from the helper, made sense of.
#[derive(Clone, Debug, PartialEq)]
pub enum Incoming {
    Ready(Value),
    /// A reply to a command: its id, and what it gave or why it failed.
    Reply(u64, Result<Value, String>),
    /// Anything else the helper says of itself: geometry, message, shutdown, error.
    Event(String, Value),
}

/// Makes sense of one line from the helper. Anything that isn't JSON, or isn't one of these, is let go (a core
/// may print to stdout on systems where the helper can't stop it).
pub fn parse_line(line: &str) -> Option<Incoming> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;
    if let Some(event) = value["event"].as_str() {
        return Some(if event == "ready" { Incoming::Ready(value) } else { Incoming::Event(event.to_string(), value) });
    }
    let id = value["id"].as_u64()?;
    match value["ok"].as_bool() {
        Some(true) => Some(Incoming::Reply(id, Ok(value))),
        Some(false) => Some(Incoming::Reply(id, Err(value["error"].as_str().unwrap_or("The helper didn't say what went wrong.").to_string()))),
        None => None,
    }
}

/// A command as the line written to the helper.
pub fn command_line(id: u64, mut command: Value) -> String {
    command["id"] = json!(id);
    let mut line = command.to_string();
    line.push('\n');
    line
}

/// What comes from the helper's stdout: a line made sense of, or the helper having gone.
enum Heard {
    Line(Incoming),
    Gone,
}

/// The helper running one core, and its protocol.
pub struct Helper {
    child: Child,
    stdin: ChildStdin,
    heard: mpsc::Receiver<Heard>,
    next_id: u64,
    pub ready: Option<Ready>,
    /// Replies not yet taken, by id.
    replies: Vec<(u64, Result<Value, String>)>,
    /// Messages from the core, and other events, newest last.
    pub messages: Vec<String>,
    /// Why it stopped, once it has.
    pub gone: Option<String>,
    pub frames_path: PathBuf,
    pub log_path: PathBuf,
    frames: Option<FrameReader>,
}

/// What the helper's started with.
pub struct Start<'a> {
    pub helper: &'a Path,
    pub core: &'a Path,
    /// Where the frames file goes; its log goes beside it.
    pub frames: &'a Path,
    pub system: &'a Path,
    pub saves: &'a Path,
    pub game: Option<&'a Path>,
    pub options: &'a [(String, String)],
    /// No sound, for tests.
    pub silent: bool,
}

impl Helper {
    pub fn start(start: &Start) -> Result<Helper, String> {
        let log_path = start.frames.with_extension("log");
        let log = std::fs::File::create(&log_path).map_err(|e| crate::i18n::tf!("{} couldn't be made: {e}", log_path.display()))?;
        let mut command = crate::platform::command(start.helper);
        command.arg("--core").arg(start.core).arg("--frames").arg(start.frames).arg("--system").arg(start.system).arg("--saves").arg(start.saves);
        if let Some(game) = start.game {
            command.arg("--game").arg(game);
        }
        for (key, value) in start.options {
            command.arg("--option").arg(format!("{key}={value}"));
        }
        if start.silent {
            command.arg("--silent");
        }
        // The helper has no window, and mustn't open one: it's started with no display to open one on.
        command.env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(log);
        let mut child = command.spawn().map_err(|e| crate::i18n::tf!("The helper ({}) couldn't be started: {e}", start.helper.display()))?;
        let stdin = child.stdin.take().ok_or(crate::i18n::tr("The helper's input couldn't be reached."))?;
        let stdout = child.stdout.take().ok_or(crate::i18n::tr("The helper's output couldn't be reached."))?;
        let (send, heard) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Some(incoming) = parse_line(&line)
                    && send.send(Heard::Line(incoming)).is_err()
                {
                    return;
                }
            }
            let _ = send.send(Heard::Gone);
        });
        Ok(Helper { child, stdin, heard, next_id: 1, ready: None, replies: Vec::new(), messages: Vec::new(), gone: None, frames_path: start.frames.to_path_buf(), log_path, frames: None })
    }

    fn take_in(&mut self, heard: Heard) {
        match heard {
            Heard::Line(Incoming::Ready(value)) => {
                if value["protocol"].as_u64().is_some_and(|p| p != PROTOCOL) {
                    self.gone = Some(crate::i18n::tf!("The helper speaks protocol {}, and this Limoniter speaks {PROTOCOL}: get the helper again.", value["protocol"]));
                }
                self.ready = Some(Ready::from_json(&value));
            }
            Heard::Line(Incoming::Reply(id, result)) => {
                if let (Some(ready), Ok(value)) = (self.ready.as_mut(), &result)
                    && value.get("loaded").is_some()
                {
                    *ready = Ready { core: ready.core.clone(), version: ready.version.clone(), ..Ready::from_json(value) };
                }
                self.replies.push((id, result));
                // Replies nobody took are let go once there are many.
                if self.replies.len() > 64 {
                    drop(self.replies.remove(0));
                }
            }
            Heard::Line(Incoming::Event(name, value)) => match name.as_str() {
                "geometry" => {
                    if let Some(ready) = self.ready.as_mut() {
                        ready.update(&value);
                    }
                }
                "message" => self.messages.push(value["text"].as_str().unwrap_or("").to_string()),
                "shutdown" => self.gone = Some(crate::i18n::tr("The machine switched itself off.").into()),
                "error" => self.gone = Some(value["error"].as_str().unwrap_or("").to_string()),
                _ => {}
            },
            Heard::Gone => {
                if self.gone.is_none() {
                    self.gone = Some(match self.log_tail() {
                        tail if tail.is_empty() => crate::i18n::tr("The helper stopped.").into(),
                        tail => crate::i18n::tf!("The helper stopped. It said: {tail}"),
                    });
                }
            }
        }
    }

    /// The last few lines of the helper's log.
    pub fn log_tail(&self) -> String {
        let log = std::fs::read_to_string(&self.log_path).unwrap_or_default();
        let lines: Vec<&str> = log.lines().filter(|l| !l.trim().is_empty()).collect();
        lines[lines.len().saturating_sub(3)..].join(" ")
    }

    /// Takes in whatever the helper has said since last time.
    pub fn pump(&mut self) {
        while let Ok(heard) = self.heard.try_recv() {
            self.take_in(heard);
        }
    }

    /// Sends a command, giving its id.
    pub fn send(&mut self, command: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        if self.stdin.write_all(command_line(id, command).as_bytes()).and_then(|()| self.stdin.flush()).is_err() && self.gone.is_none() {
            self.gone = Some(crate::i18n::tr("The helper can't be reached: it may have stopped.").into());
        }
        id
    }

    /// The reply to a command, if it's come.
    pub fn take_reply(&mut self, id: u64) -> Option<Result<Value, String>> {
        self.pump();
        let at = self.replies.iter().position(|(i, _)| *i == id)?;
        Some(self.replies.remove(at).1)
    }

    /// Waits for something to be heard, at most this long.
    #[cfg(test)]
    fn hear(&mut self, until: Instant) -> bool {
        let wait = until.saturating_duration_since(Instant::now());
        match self.heard.recv_timeout(wait) {
            Ok(heard) => {
                self.take_in(heard);
                true
            }
            Err(_) => false,
        }
    }

    /// Waits for the helper to be ready.
    #[cfg(test)]
    pub fn wait_ready(&mut self, timeout: Duration) -> Result<&Ready, String> {
        let until = Instant::now() + timeout;
        while self.ready.is_none() {
            if let Some(gone) = &self.gone {
                return Err(gone.clone());
            }
            if !self.hear(until) && Instant::now() >= until {
                return Err(crate::i18n::tr("The helper didn't say it was ready.").into());
            }
        }
        self.ready.as_ref().ok_or_else(String::new)
    }

    /// Sends a command and waits for its reply.
    #[cfg(test)]
    pub fn ask(&mut self, command: Value, timeout: Duration) -> Result<Value, String> {
        let id = self.send(command);
        let until = Instant::now() + timeout;
        loop {
            if let Some(reply) = self.take_reply(id) {
                return reply;
            }
            if let Some(gone) = &self.gone {
                return Err(gone.clone());
            }
            if !self.hear(until) && Instant::now() >= until {
                return Err(crate::i18n::tr("The helper didn't answer.").into());
            }
        }
    }

    /// The newest frame, if there's one newer than `after`.
    pub fn frame(&mut self, after: u64) -> Option<Frame> {
        if self.frames.is_none() {
            self.frames = FrameReader::open(&self.frames_path).ok();
        }
        self.frames.as_mut()?.read(after)
    }

    pub fn running(&mut self) -> bool {
        self.pump();
        self.gone.is_none() && matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for Helper {
    /// Asks the helper to quit, and makes sure it has.
    fn drop(&mut self) {
        let _ = self.stdin.write_all(command_line(0, json!({"cmd": "quit"})).as_bytes());
        let _ = self.stdin.flush();
        let until = Instant::now() + Duration::from_millis(500);
        while Instant::now() < until {
            if !matches!(self.child.try_wait(), Ok(None)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.frames_path);
    }
}

/// Libretro's key code for a key, as the keyboard callback and the keyboard device take them.
pub fn retro_key(key: egui::Key) -> Option<u32> {
    use egui::Key as K;
    let name = key.name();
    if let [c] = name.as_bytes()
        && c.is_ascii_alphanumeric()
    {
        return Some(u32::from(c.to_ascii_lowercase()));
    }
    Some(match key {
        K::Backspace => 8,
        K::Tab => 9,
        K::Enter => 13,
        K::Escape => 27,
        K::Space => 32,
        K::Exclamationmark => 33,
        K::Quote => 39,
        K::Plus => 43,
        K::Comma => 44,
        K::Minus => 45,
        K::Period => 46,
        K::Slash => 47,
        K::Colon => 58,
        K::Semicolon => 59,
        K::Equals => 61,
        K::Questionmark => 63,
        K::OpenBracket => 91,
        K::Backslash => 92,
        K::CloseBracket => 93,
        K::Backtick => 96,
        K::Delete => 127,
        K::ArrowUp => 273,
        K::ArrowDown => 274,
        K::ArrowRight => 275,
        K::ArrowLeft => 276,
        K::Insert => 277,
        K::Home => 278,
        K::End => 279,
        K::PageUp => 280,
        K::PageDown => 281,
        K::F1 => 282,
        K::F2 => 283,
        K::F3 => 284,
        K::F4 => 285,
        K::F5 => 286,
        K::F6 => 287,
        K::F7 => 288,
        K::F8 => 289,
        K::F9 => 290,
        K::F10 => 291,
        K::F11 => 292,
        K::F12 => 293,
        _ => return None,
    })
}

/// Libretro's key codes for the modifier keys, which egui reports as modifiers rather than keys.
pub const KEY_LEFT_SHIFT: u32 = 304;
pub const KEY_LEFT_CTRL: u32 = 306;
pub const KEY_LEFT_ALT: u32 = 308;

/// The joypad button a key is on a console: the arrows the D-pad, Z and X its B and A, A and S its Y and X,
/// Enter Start and Shift Select. Bit numbers are libretro's.
pub fn joypad_bit(key: egui::Key) -> Option<u16> {
    use egui::Key as K;
    Some(match key {
        K::Z => 0,
        K::A => 1,
        K::Enter => 3,
        K::ArrowUp => 4,
        K::ArrowDown => 5,
        K::ArrowLeft => 6,
        K::ArrowRight => 7,
        K::X => 8,
        K::S => 9,
        _ => return None,
    })
}
/// Select's bit, which Shift holds.
pub const JOYPAD_SELECT: u16 = 2;

/// Puts the program to run where the core can read it, under its own name (whose ending tells the core what
/// it is), in a folder of its own for this run.
pub fn write_program(run: &Runnable, folder: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(folder).map_err(|e| crate::i18n::tf!("{} couldn't be made: {e}", folder.display()))?;
    let name: String = run.name.chars().map(|c| if c == '/' || c == '\\' || c == ':' { '_' } else { c }).collect();
    let name = if name.contains('.') { name } else { format!("{name}.{}", run.format.ending()) };
    let path = folder.join(name);
    std::fs::write(&path, run.bytes.as_slice()).map_err(|e| crate::i18n::tf!("{} couldn't be written: {e}", path.display()))?;
    Ok(path)
}

/// Bytes from hex, as the helper sends memory.
pub fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("limoniter-retro-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Writes a frames file by the layout, as the helper does.
    fn write_frames(path: &Path, capacity: usize, number: u64, sizes: [(u32, u32); 2], buffers: [&[u8]; 2]) {
        let mut bytes = vec![0u8; FRAME_HEADER + capacity * 2];
        bytes[..8].copy_from_slice(b"LIMRETRO");
        bytes[8..12].copy_from_slice(&1u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&(capacity as u32).to_le_bytes());
        bytes[16..24].copy_from_slice(&number.to_le_bytes());
        for (i, (w, h)) in sizes.iter().enumerate() {
            bytes[24 + i * 8..28 + i * 8].copy_from_slice(&w.to_le_bytes());
            bytes[28 + i * 8..32 + i * 8].copy_from_slice(&h.to_le_bytes());
            bytes[FRAME_HEADER + capacity * i..][..buffers[i].len()].copy_from_slice(buffers[i]);
        }
        // Written over in place, as the helper writes through its mapping: a mapped file can't be truncated on
        // Windows, which std::fs::write would do.
        let mut file = std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(path).unwrap();
        std::io::Write::write_all(&mut file, &bytes).unwrap();
    }

    #[test]
    fn frames_are_read_as_written() {
        let dir = scratch("frames");
        let path = dir.join("frames");
        let red = [255, 0, 0, 255].repeat(4);
        let blue = [0, 0, 255, 255].repeat(6);
        write_frames(&path, 64, 7, [(3, 2), (2, 2)], [&blue, &red]);
        let mut reader = FrameReader::open(&path).unwrap();
        // Frame 7 is odd, so it's buffer 1's: 2 × 2 and red.
        let frame = reader.read(0).unwrap();
        assert_eq!((frame.number, frame.width, frame.height), (7, 2, 2));
        assert_eq!(frame.pixels, red);
        assert!(reader.read(7).is_none(), "nothing newer");
        // Frame 8 comes, in buffer 0.
        write_frames(&path, 64, 8, [(3, 2), (2, 2)], [&blue, &red]);
        let frame = reader.read(7).unwrap();
        assert_eq!((frame.number, frame.width, frame.height), (8, 3, 2));
        assert_eq!(frame.pixels, blue);
        // Not a frames file at all.
        let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        std::io::Write::write_all(&mut file, &[0u8; 8]).unwrap();
        drop(file);
        assert!(reader.read(0).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lines_from_the_helper_are_made_sense_of() {
        assert!(matches!(parse_line(r#"{"event":"ready","core":"Fuse"}"#), Some(Incoming::Ready(v)) if v["core"] == "Fuse"));
        assert_eq!(parse_line(r#"{"id":3,"ok":false,"error":"No."}"#), Some(Incoming::Reply(3, Err("No.".into()))));
        assert!(matches!(parse_line(r#"{"id":4,"ok":true,"data":"00"}"#), Some(Incoming::Reply(4, Ok(v))) if v["data"] == "00"));
        assert!(matches!(parse_line(r#"{"event":"geometry","width":320}"#), Some(Incoming::Event(name, _)) if name == "geometry"));
        // What a core prints, or a line cut short, is let go.
        assert_eq!(parse_line("Fuse 1.6.0 starting"), None);
        assert_eq!(parse_line(r#"{"id":5,"ok":tr"#), None);
        assert_eq!(parse_line(r#"{"id":5}"#), None);
        assert_eq!(command_line(9, json!({"cmd": "reset"})), "{\"cmd\":\"reset\",\"id\":9}\n");
    }

    #[test]
    fn ready_is_read() {
        let ready = Ready::from_json(&json!({"event": "ready", "core": "Fuse", "version": "1.6", "loaded": true, "width": 320, "height": 240, "aspect": 1.333, "fps": 50.08, "regions": [{"name": "system_ram", "size": 49152}], "options": [{"key": "fuse_machine", "description": "Model", "values": ["Spectrum 48K", "Spectrum 128K"], "value": "Spectrum 48K"}]}));
        assert_eq!((ready.width, ready.height, ready.loaded), (320, 240, true));
        assert_eq!(ready.system_ram(), Some(49152));
        assert_eq!(ready.memory(), Some(("system_ram", 49152)));
        let mapped = Ready::from_json(&json!({"regions": [{"name": "system_ram", "size": 49152}, {"name": "memory_map", "size": 65536}]}));
        assert_eq!(mapped.memory(), Some(("memory_map", 65536)));
        assert_eq!(ready.options[0].values.len(), 2);
        assert_eq!(ready.error, None);
    }

    #[test]
    fn the_core_table_and_its_addresses() {
        assert_eq!(core_for(Machine::Spectrum48).unwrap().core, "fuse");
        assert_eq!(core_for(Machine::C64).unwrap().core, "vice_x64sc");
        assert_eq!(core_for(Machine::Amiga).unwrap().core, "puae");
        assert!(core_for(Machine::Amiga).unwrap().needs.is_some(), "PUAE says it needs a Kickstart");
        assert_eq!(core_for(Machine::MegaDrive).unwrap().core, "genesis_plus_gx");
        assert_eq!(cores_for(Machine::MegaDrive).nth(1).unwrap().core, "picodrive");
        assert_eq!(core_for(Machine::Atari2600).unwrap().core, "stella");
        assert!(core_for(Machine::Ssem).is_none());
        assert_eq!(core_address("fuse", "linux", "x86_64").unwrap(), "https://buildbot.libretro.com/nightly/linux/x86_64/latest/fuse_libretro.so.zip");
        assert_eq!(core_address("stella", "windows", "x86_64").unwrap(), "https://buildbot.libretro.com/nightly/windows/x86_64/latest/stella_libretro.dll.zip");
        assert_eq!(core_address("puae", "macos", "aarch64").unwrap(), "https://buildbot.libretro.com/nightly/apple/osx/arm64/latest/puae_libretro.dylib.zip");
        assert_eq!(core_address("puae", "macos", "x86_64").unwrap(), "https://buildbot.libretro.com/nightly/apple/osx/x86_64/latest/puae_libretro.dylib.zip");
        assert_eq!(core_address("fuse", "freebsd", "x86_64"), None);
        assert_eq!(helper_asset("windows", "x86_64"), Some("limoniter-retro-windows-x64.zip"));
        assert_eq!(helper_asset("linux", "aarch64"), None);
        // Each machine's Run on choice is found again by its name.
        for machine in machines() {
            assert_eq!(machine_named(&target_name(machine)), Some(machine));
        }
    }

    #[test]
    fn the_helper_is_found_where_it_should_be() {
        let dir = scratch("find");
        let exe = if cfg!(windows) { "limoniter-retro.exe" } else { "limoniter-retro" };
        let (chosen, beside, folder) = (dir.join("chosen/mine"), dir.join("beside"), dir.join("emulators"));
        for d in [chosen.parent().unwrap(), &beside, &folder.join("limoniter-retro/inner")] {
            std::fs::create_dir_all(d).unwrap();
        }
        assert_eq!(find_helper_in(Some(chosen.clone()), None, Some(beside.clone()), Some(folder.clone())), None);
        std::fs::write(folder.join("limoniter-retro/inner").join(exe), "").unwrap();
        assert_eq!(find_helper_in(Some(chosen.clone()), None, Some(beside.clone()), Some(folder.clone())), Some(folder.join("limoniter-retro/inner").join(exe)));
        std::fs::write(beside.join(exe), "").unwrap();
        assert_eq!(find_helper_in(Some(chosen.clone()), None, Some(beside.clone()), Some(folder.clone())), Some(beside.join(exe)));
        std::fs::write(&chosen, "").unwrap();
        assert_eq!(find_helper_in(Some(chosen.clone()), None, Some(beside.clone()), Some(folder.clone())), Some(chosen.clone()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_release_without_this_computers_helper_says_so() {
        let release = r#"{"tag_name":"v0.1.0","assets":[{"name":"limoniter-retro-linux-x64.zip","browser_download_url":"https://example.com/l.zip"}]}"#;
        assert_eq!(pick_helper_asset(release, "limoniter-retro-linux-x64.zip").unwrap(), "https://example.com/l.zip");
        assert!(pick_helper_asset(release, "limoniter-retro-windows-x64.zip").unwrap_err().contains("v0.1.0"));
        assert!(pick_helper_asset("not json", "x").is_err());
        assert!(no_release_words().contains(HELPER_REPOSITORY));
    }

    #[test]
    fn prefs_are_kept() {
        let mut prefs = Prefs { helper: Some(PathBuf::from("/opt/limoniter-retro")), options: Vec::new() };
        prefs.set_option("fuse", "fuse_machine", "Spectrum 48K");
        prefs.set_option("fuse", "fuse_machine", "Spectrum 128K");
        assert_eq!(Prefs::from_json(&prefs.to_json()), prefs);
        assert_eq!(prefs.options_for("fuse"), [("fuse_machine".to_string(), "Spectrum 128K".to_string())]);
        assert_eq!(Prefs::from_json("rubbish"), Prefs::default());
    }

    #[test]
    fn keys_become_libretro_codes() {
        assert_eq!(retro_key(egui::Key::A), Some(97));
        assert_eq!(retro_key(egui::Key::Num0), Some(48));
        assert_eq!(retro_key(egui::Key::Enter), Some(13));
        assert_eq!(retro_key(egui::Key::ArrowLeft), Some(276));
        assert_eq!(retro_key(egui::Key::F1), Some(282));
        assert_eq!(joypad_bit(egui::Key::X), Some(8));
        assert_eq!(joypad_bit(egui::Key::Q), None);
    }

    #[test]
    fn a_plus3dos_basic_program_goes_on_a_tape_for_fuse() {
        let program = [0u8, 10, 5, 0, 0xF5, b'"', b'H', b'"', 13];
        let file = crate::basic::plus3dos_file(&program, Some(10));
        let run = Runnable { machine: Machine::SpectrumPlus3, format: Format::Plus3dos, name: "hello.bas".into(), bytes: std::sync::Arc::new(file) };
        let tape = prepared(&run);
        assert_eq!((tape.format, tape.name.as_str()), (Format::Tap, "hello.tap"));
        assert_eq!(*tape.bytes, crate::basic::spectrum_tape(&program, "hello", Some(10)));
        assert!(cores_for(Machine::SpectrumPlus3).any(|c| c.formats.contains(&tape.format)));
    }

    #[test]
    fn the_program_is_written_under_a_name_the_core_knows() {
        let dir = scratch("program");
        let run = Runnable { machine: Machine::Spectrum48, format: Format::Tap, name: "game".into(), bytes: std::sync::Arc::new(vec![1, 2, 3]) };
        let path = write_program(&run, &dir).unwrap();
        assert_eq!(path.file_name().unwrap(), "game.tap");
        assert_eq!(std::fs::read(&path).unwrap(), [1, 2, 3]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_helper_that_says_nothing_or_nonsense_is_handled() {
        let dir = scratch("silent");
        let frames = dir.join("frames");
        let script = dir.join("fake");
        // Says something that isn't JSON, then that it's ready, then never answers.
        std::fs::write(&script, "#!/bin/sh\necho 'a core printing'\necho '{\"event\":\"ready\",\"core\":\"Fake\",\"loaded\":true,\"protocol\":1}'\ncat > /dev/null\n").unwrap();
        crate::emulator_get::make_runnable(&script, 0o755);
        let start = Start { helper: &script, core: &dir, frames: &frames, system: &dir, saves: &dir, game: None, options: &[], silent: true };
        let mut helper = Helper::start(&start).unwrap();
        assert_eq!(helper.wait_ready(Duration::from_secs(5)).unwrap().core, "Fake");
        assert_eq!(helper.ask(json!({"cmd": "reset"}), Duration::from_millis(300)).unwrap_err(), "The helper didn't answer.");
        drop(helper);
        // One that stops at once says what it said.
        std::fs::write(&script, "#!/bin/sh\necho 'no core there' >&2\nexit 1\n").unwrap();
        let mut helper = Helper::start(&start).unwrap();
        let error = helper.wait_ready(Duration::from_secs(5)).unwrap_err();
        assert!(error.contains("no core there"), "{error}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The real helper with its test core, when both are named: it starts, frames come, a key reaches the core,
    /// and memory is read.
    #[test]
    fn the_real_helper_with_its_test_core() {
        let (Some(helper), Some(core)) = (std::env::var_os("LIMONITER_RETRO_HELPER"), std::env::var_os("LIMONITER_RETRO_TESTCORE")) else {
            return;
        };
        let dir = scratch("end-to-end");
        let frames = dir.join("frames");
        let start = Start { helper: Path::new(&helper), core: Path::new(&core), frames: &frames, system: &dir, saves: &dir, game: None, options: &[], silent: true };
        let mut helper = Helper::start(&start).unwrap();
        let ready = helper.wait_ready(Duration::from_secs(10)).unwrap().clone();
        assert_eq!(ready.core, "Test core");
        assert_eq!(ready.system_ram(), Some(65536));
        let until = Instant::now() + Duration::from_secs(10);
        let first = loop {
            if let Some(frame) = helper.frame(0) {
                break frame;
            }
            assert!(Instant::now() < until, "no frame came");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!((first.width, first.height), (160, 120));
        let later = loop {
            if let Some(frame) = helper.frame(first.number) {
                break frame;
            }
            assert!(Instant::now() < until, "frames stopped");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(later.number > first.number);
        helper.send(json!({"cmd": "key", "code": retro_key(egui::Key::Q).unwrap(), "down": true}));
        let memory = helper.ask(json!({"cmd": "read_memory", "region": "system_ram", "offset": 8, "length": 2}), Duration::from_secs(5)).unwrap();
        assert_eq!(unhex(memory["data"].as_str().unwrap()).unwrap(), [113, 0]);
        drop(helper);
        assert!(!frames.exists(), "the frames file is tidied away");
        std::fs::remove_dir_all(&dir).ok();
    }
}
