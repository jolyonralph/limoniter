//! Running programs on an emulated Atari ST, STE, Mega ST, Mega STE, TT or Falcon in Hatari: the machine, its
//! memory, monitor and processor, the TOS ROM it starts with, and whether the program runs from a folder made its
//! hard drive or from a floppy that boots into it. Hatari's own command line options are used for all of it, as
//! its manual gives them (the same in Hatari 2.3 to 2.6).
//!
//! TOS is Atari's, so Limoniter never includes or copies one: the person's own are found where Hatari and Steem
//! keep them and told apart by their headers (version, country and date), or one is chosen. EmuTOS, a free (GPL)
//! TOS, is downloaded from its SourceForge page when the person asks, and kept beside the settings, for when they
//! have none.
//!
//! Hatari's debugger also profiles: a run can count each instruction's cycles from the program's start until it
//! ends or for some seconds, saved by Hatari and summed here by source line and by label.

use crate::running::{Format, Runnable};
use eframe::egui::{self, RichText, Ui};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

/// An Atari to run on, by Hatari's names for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    St,
    MegaSt,
    Ste,
    MegaSte,
    Tt,
    Falcon,
}

/// A monitor, by Hatari's names for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Monitor {
    Colour,
    Mono,
    Vga,
    Tv,
}

impl Monitor {
    pub fn id(self) -> &'static str {
        match self {
            Monitor::Colour => "rgb",
            Monitor::Mono => "mono",
            Monitor::Vga => "vga",
            Monitor::Tv => "tv",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Monitor::Colour => "Colour (RGB, SC1224)",
            Monitor::Mono => "Monochrome (SM124)",
            Monitor::Vga => "VGA",
            Monitor::Tv => "TV",
        }
    }

    fn from_id(id: &str) -> Option<Monitor> {
        [Monitor::Colour, Monitor::Mono, Monitor::Vga, Monitor::Tv].into_iter().find(|m| m.id() == id)
    }

    /// Frames a second: what Hatari's VBL counter counts.
    fn rate(self) -> u32 {
        match self {
            Monitor::Mono => 71,
            Monitor::Vga => 60,
            _ => 50,
        }
    }
}

impl Model {
    pub const ALL: [Model; 6] = [Model::St, Model::MegaSt, Model::Ste, Model::MegaSte, Model::Tt, Model::Falcon];

    pub fn name(self) -> &'static str {
        match self {
            Model::St => "ST (68000, 8 MHz)",
            Model::MegaSt => "Mega ST (68000, 8 MHz, blitter)",
            Model::Ste => "STE (68000, 8 MHz, blitter, DMA sound)",
            Model::MegaSte => "Mega STE (68000, 16 MHz)",
            Model::Tt => "TT (68030, 32 MHz)",
            Model::Falcon => "Falcon (68030, 16 MHz, DSP)",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Model::St => "ST",
            Model::MegaSt => "Mega ST",
            Model::Ste => "STE",
            Model::MegaSte => "Mega STE",
            Model::Tt => "TT",
            Model::Falcon => "Falcon",
        }
    }

    /// Hatari's name for it (--machine), which is its name in the settings too.
    pub fn id(self) -> &'static str {
        match self {
            Model::St => "st",
            Model::MegaSt => "megast",
            Model::Ste => "ste",
            Model::MegaSte => "megaste",
            Model::Tt => "tt",
            Model::Falcon => "falcon",
        }
    }

    fn from_id(id: &str) -> Option<Model> {
        Model::ALL.into_iter().find(|m| m.id() == id)
    }

    /// The monitors it can have.
    pub fn monitors(self) -> &'static [Monitor] {
        match self {
            Model::Tt => &[Monitor::Vga, Monitor::Mono],
            Model::Falcon => &[Monitor::Colour, Monitor::Vga, Monitor::Tv, Monitor::Mono],
            _ => &[Monitor::Colour, Monitor::Mono, Monitor::Tv],
        }
    }

    /// Its memory as it usually came, in KiB, and its processor's speed in MHz.
    fn usual(self) -> (u32, u32) {
        match self {
            Model::St | Model::Ste => (1024, 8),
            Model::MegaSt => (2048, 8),
            Model::MegaSte => (4096, 16),
            Model::Tt => (4096, 32),
            Model::Falcon => (4096, 16),
        }
    }

    /// Whether its processor is a 68030, which takes TT-RAM.
    pub fn is_030(self) -> bool {
        matches!(self, Model::Tt | Model::Falcon)
    }

    /// The EmuTOS image made for it: 192K for the ST and Mega ST (which don't look for other hardware), 256K for
    /// the STE and Mega STE, 512K for the TT and Falcon.
    fn emutos_size(self) -> u32 {
        match self {
            Model::St | Model::MegaSt => 192,
            Model::Ste | Model::MegaSte => 256,
            Model::Tt | Model::Falcon => 512,
        }
    }
}

/// The ST RAM sizes offered, in KiB.
pub const MEMORY: [u32; 6] = [512, 1024, 2048, 4096, 8192, 14336];
/// The TT-RAM sizes offered for a TT or Falcon, in MiB.
pub const TT_RAM: [u32; 5] = [0, 16, 64, 128, 256];

fn memory_name(kib: u32) -> String {
    if kib < 1024 { format!("{kib} KB") } else { format!("{} MB", kib / 1024) }
}

/// How the program is put in the Atari.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// In a folder that's drive C: (GEMDOS hard drive), started by Hatari.
    Folder,
    /// In the AUTO folder of a floppy made for the run, which the Atari boots from.
    Floppy,
}

/// One machine's own choices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choices {
    pub tos: Option<PathBuf>,
    /// ST RAM, in KiB.
    pub memory: u32,
    /// TT-RAM, in MiB, for a TT or Falcon.
    pub tt_ram: u32,
    pub monitor: Monitor,
    /// In MHz: 8, 16 or 32.
    pub clock: u32,
}

impl Choices {
    fn usual(model: Model) -> Choices {
        let (memory, clock) = model.usual();
        Choices { tos: None, memory, tt_ram: 0, monitor: model.monitors()[0], clock }
    }
}

/// How runs in Hatari are made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub model: Model,
    pub machines: Vec<(Model, Choices)>,
    /// Hatari's more exact 68000 prefetch, and its cycle-exact processor: what demos that count cycles need.
    pub compatible: bool,
    pub cycle_exact: bool,
    /// Skips TOS's memory test.
    pub fast_boot: bool,
    pub start: Start,
    /// Code for a floppy's boot sector, put on floppies made to boot the program.
    pub boot_sector: Option<PathBuf>,
    /// The language of the EmuTOS used when there's no TOS: its two letters, as EmuTOS names its images.
    pub emutos_language: String,
    /// How long a profiled run counts for, at most, in seconds after the Atari is switched on.
    pub profile_seconds: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { model: Model::St, machines: Vec::new(), compatible: true, cycle_exact: true, fast_boot: true, start: Start::Folder, boot_sector: None, emutos_language: "uk".into(), profile_seconds: 20 }
    }
}

fn on(value: &str) -> bool {
    value.trim() == "yes"
}

fn yes(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

impl Settings {
    pub fn choices(&self, model: Model) -> Choices {
        self.machines.iter().find(|(m, _)| *m == model).map_or_else(|| Choices::usual(model), |(_, c)| c.clone())
    }

    pub fn set_choices(&mut self, model: Model, choices: Choices) {
        self.machines.retain(|(m, _)| *m != model);
        if choices != Choices::usual(model) {
            self.machines.push((model, choices));
        }
    }

    pub fn tos(&self, model: Model) -> Option<PathBuf> {
        self.choices(model).tos
    }

    pub fn set_tos(&mut self, model: Model, tos: Option<PathBuf>) {
        let mut choices = self.choices(model);
        choices.tos = tos;
        self.set_choices(model, choices);
    }

    fn to_text(&self) -> String {
        let mut text = format!("model={}\ncompatible={}\ncycle_exact={}\nfast_boot={}\nstart={}\nemutos_language={}\nprofile_seconds={}\n", self.model.id(), yes(self.compatible), yes(self.cycle_exact), yes(self.fast_boot), if self.start == Start::Floppy { "floppy" } else { "folder" }, self.emutos_language, self.profile_seconds);
        if let Some(boot) = &self.boot_sector {
            text.push_str(&format!("boot_sector={}\n", boot.display()));
        }
        for (model, choices) in &self.machines {
            let id = model.id();
            if let Some(tos) = &choices.tos {
                text.push_str(&format!("tos.{id}={}\n", tos.display()));
            }
            text.push_str(&format!("memory.{id}={}\ntt_ram.{id}={}\nmonitor.{id}={}\nclock.{id}={}\n", choices.memory, choices.tt_ram, choices.monitor.id(), choices.clock));
        }
        text
    }

    fn from_text(text: &str) -> Settings {
        let mut settings = Settings::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            match key {
                "model" => settings.model = Model::from_id(value).unwrap_or(Model::St),
                "compatible" => settings.compatible = on(value),
                "cycle_exact" => settings.cycle_exact = on(value),
                "fast_boot" => settings.fast_boot = on(value),
                "start" => settings.start = if value == "floppy" { Start::Floppy } else { Start::Folder },
                "boot_sector" if !value.is_empty() => settings.boot_sector = Some(PathBuf::from(value)),
                "emutos_language" if value.len() == 2 => settings.emutos_language = value.to_string(),
                "profile_seconds" => settings.profile_seconds = value.parse::<u32>().unwrap_or(20).clamp(1, 3600),
                _ => {
                    let Some((what, id)) = key.split_once('.') else { continue };
                    let Some(model) = Model::from_id(id) else { continue };
                    let mut choices = settings.choices(model);
                    match what {
                        "tos" if !value.is_empty() => choices.tos = Some(PathBuf::from(value)),
                        "memory" => choices.memory = value.parse::<u32>().ok().filter(|m| MEMORY.contains(m)).unwrap_or(choices.memory),
                        "tt_ram" => choices.tt_ram = value.parse::<u32>().ok().filter(|m| TT_RAM.contains(m)).unwrap_or(0),
                        "monitor" => choices.monitor = Monitor::from_id(value).filter(|m| model.monitors().contains(m)).unwrap_or(choices.monitor),
                        "clock" => choices.clock = value.parse::<u32>().ok().filter(|c| matches!(c, 8 | 16 | 32)).unwrap_or(choices.clock),
                        _ => {}
                    }
                    settings.set_choices(model, choices);
                }
            }
        }
        settings
    }
}

/// Where they're kept, beside Limoniter's settings.
fn settings_file() -> Option<PathBuf> {
    crate::settings::data_folder().map(|dir| dir.join("hatari.txt"))
}

static SETTINGS: RwLock<Option<Settings>> = RwLock::new(None);

/// The settings, read the first time they're asked for (never from the file in tests).
pub fn settings() -> Settings {
    if let Some(known) = SETTINGS.read().ok().and_then(|s| s.clone()) {
        return known;
    }
    let read = if cfg!(test) { None } else { settings_file().and_then(|f| std::fs::read_to_string(f).ok()) };
    let settings = read.map(|t| Settings::from_text(&t)).unwrap_or_default();
    if let Ok(mut kept) = SETTINGS.write() {
        *kept = Some(settings.clone());
    }
    settings
}

/// Keeps new settings, and saves them.
pub fn set_settings(settings: Settings) {
    if !cfg!(test)
        && let Some(file) = settings_file()
    {
        let _ = std::fs::create_dir_all(file.parent().expect("in a folder"));
        let _ = std::fs::write(&file, settings.to_text());
    }
    if let Ok(mut kept) = SETTINGS.write() {
        *kept = Some(settings);
    }
}

// ---------- TOS ROMs ----------

/// A TOS ROM, as told from its header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tos {
    pub name: String,
    /// Its version, as the header gives it: 0x0104 for 1.04.
    pub version: u16,
    pub emutos: bool,
    /// The machines it's right for, best first.
    pub suits: Vec<Model>,
    /// How good a choice it is among others for the same machine, lower being better.
    pub rank: usize,
}

/// The countries a TOS says it's for, by the number in its header (EmuTOS numbers more than Atari did).
const COUNTRIES: [&str; 32] = [
    "USA", "Germany", "France", "UK", "Spain", "Italy", "Sweden", "Swiss French", "Swiss German", "Turkey", "Finland", "Norway", "Denmark", "Saudi Arabia", "Netherlands", "Czech", "Hungary", "Poland", "Lithuania", "Russia", "Estonia", "Belarus", "Ukraine", "Slovakia", "Romania", "Bulgaria", "Slovenia", "Croatia", "Serbia", "Montenegro", "Macedonia", "Greece",
];

fn country(code: u16) -> String {
    match code {
        127 => "all languages".to_string(),
        54 => "Catalan".to_string(),
        n => COUNTRIES.get(usize::from(n)).map_or_else(|| format!("country {n}"), |c| c.to_string()),
    }
}

/// What a file is, if it's a TOS ROM: Atari's or EmuTOS, by its header. A TOS starts with a branch, its version,
/// and where it starts (in the ROM at $FC0000 or $E00000); its date is at $18, its country at $1C and EmuTOS
/// says "ETOS" at $2C, with its own version at $3C.
pub fn identify(bytes: &[u8]) -> Option<Tos> {
    let size = bytes.len();
    if !matches!(size, 0x30000 | 0x40000 | 0x80000 | 0x100000) || bytes[0] != 0x60 {
        return None;
    }
    let long = |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let version = u16::from_be_bytes([bytes[2], bytes[3]]);
    let base = long(8);
    if !matches!(base, 0xFC_0000 | 0xE0_0000) || !(base..base + size as u32).contains(&long(4)) {
        return None;
    }
    let date = long(0x18);
    let date = format!("{:04x}-{:02x}-{:02x}", date & 0xFFFF, date >> 24, (date >> 16) & 0xFF);
    let place = country(u16::from_be_bytes([bytes[0x1C], bytes[0x1D]]) >> 1);
    use Model::*;
    if &bytes[0x2C..0x30] == b"ETOS" {
        let release = if &bytes[0x34..0x38] == b"OSXH" { format!(" {}.{}", bytes[0x3C], bytes[0x3D]) } else { String::new() };
        let suits: &[Model] = match size {
            0x30000 => &[St, MegaSt],
            0x40000 => &[Ste, MegaSte, St, MegaSt],
            0x80000 => &[Tt, Falcon, Ste, MegaSte, St, MegaSt],
            _ => &[St, Ste, MegaSt, MegaSte, Tt, Falcon],
        };
        return Some(Tos { name: format!("EmuTOS{release} ({}K, {place})", size / 1024), version, emutos: true, suits: suits.to_vec(), rank: if size == 0x100000 { 2 } else { 1 } });
    }
    let (suits, rank): (&[Model], usize) = match version {
        0x0104 => (&[St, MegaSt], 0),
        0x0102 => (&[MegaSt, St], 1),
        0x0100 => (&[St, MegaSt], 2),
        0x0162 => (&[Ste], 0),
        0x0106 => (&[Ste], 1),
        0x0206 => (&[MegaSte, Ste, St, MegaSt], 0),
        0x0205 => (&[MegaSte], 1),
        0x0200..=0x0204 => (&[MegaSte, Ste], 2),
        0x0306 => (&[Tt], 0),
        0x0300..=0x0305 => (&[Tt], 1),
        0x0404 => (&[Falcon], 0),
        0x0400..=0x0403 => (&[Falcon], 1),
        _ => (&[], 9),
    };
    Some(Tos { name: format!("TOS {}.{:02x} ({place}, {date})", version >> 8, version & 0xFF), version, emutos: false, suits: suits.to_vec(), rank })
}

/// Reads a file, if it's a TOS ROM.
pub fn identify_file(path: &Path) -> Option<Tos> {
    let size = std::fs::metadata(path).ok()?.len();
    if !matches!(size, 0x30000 | 0x40000 | 0x80000 | 0x100000) {
        return None;
    }
    identify(&std::fs::read(path).ok()?)
}

/// Where TOS ROMs are usually kept: Hatari's folders (and the folder Hatari is in), Steem's, and folders of
/// Atari things among where emulators are put.
pub fn tos_folders(hatari: Option<&Path>) -> Vec<PathBuf> {
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let mut folders = Vec::new();
    if let Some(dir) = hatari.and_then(Path::parent).filter(|d| !d.as_os_str().is_empty()) {
        folders.push(dir.to_path_buf());
        // Hatari for the Mac keeps its things in its app's Resources.
        folders.push(dir.join("..").join("Resources"));
    }
    let home = env("USERPROFILE").or_else(|| env("HOME"));
    if let Some(home) = &home {
        for inside in [".hatari", ".config/hatari", ".local/share/hatari", "Library/Application Support/Hatari", "Documents/Hatari", "Downloads", "Steem", ".steem"] {
            folders.push(home.join(inside));
        }
    }
    if !cfg!(windows) {
        folders.extend(["/usr/share/hatari", "/usr/local/share/hatari", "/usr/share/emutos", "/usr/share/atari"].map(PathBuf::from));
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    for variable in ["ProgramFiles", "ProgramFiles(x86)", "APPDATA", "LOCALAPPDATA"] {
        if let Some(root) = env(variable) {
            roots.push(root);
        }
    }
    if cfg!(windows) {
        roots.extend(["C:\\Emulators", "D:\\Emulators", "E:\\Emulators", "C:\\Games"].map(PathBuf::from));
    } else {
        // Windows' drives, seen from WSL.
        for drive in ['c', 'd', 'e', 'f'] {
            roots.push(PathBuf::from(format!("/mnt/{drive}/Emulators")));
        }
    }
    if let Some(home) = &home {
        roots.push(home.join("Emulators"));
        roots.push(home.join("emulators"));
        roots.push(home.join("Downloads"));
    }
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        let mut found: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        found.sort();
        for folder in found {
            let name = folder.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ["hatari", "steem", "atari st", "atarist", "atari_st", "tos"].iter().any(|hint| name.contains(hint)) || name == "atari" {
                folders.push(folder.clone());
                // And the folders inside: rom, roms, tos, or an emulator's own folder.
                if let Ok(inner) = std::fs::read_dir(&folder) {
                    let mut inner: Vec<PathBuf> = inner.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
                    inner.sort();
                    folders.extend(inner);
                }
            }
        }
    }
    folders.dedup();
    folders
}

/// The TOS ROMs in some folders (not inside their folders), with what each is.
pub fn find_tos_in(folders: &[PathBuf]) -> Vec<(PathBuf, Tos)> {
    let mut found = Vec::new();
    for folder in folders {
        let Ok(entries) = std::fs::read_dir(folder) else { continue };
        let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
        files.sort();
        for file in files {
            if let Some(tos) = identify_file(&file)
                && !found.iter().any(|(p, _)| *p == file)
            {
                found.push((file, tos));
            }
        }
    }
    found
}

/// The best TOS found for a machine: Atari's own before EmuTOS, then the machine's own before one it also runs.
pub fn best_for(model: Model, found: &[(PathBuf, Tos)]) -> Option<(PathBuf, Tos)> {
    found.iter().filter_map(|(path, t)| t.suits.iter().position(|m| *m == model).map(|place| ((t.emutos, place, t.rank), path, t))).min_by_key(|(key, ..)| *key).map(|(_, path, t)| (path.clone(), t.clone()))
}

// ---------- EmuTOS ----------

/// The EmuTOS release downloaded, and where SourceForge keeps it.
pub const EMUTOS_VERSION: &str = "1.4";
pub const EMUTOS_PAGE: &str = "https://sourceforge.net/projects/emutos/files/emutos/1.4/";

fn emutos_address(size: u32) -> String {
    format!("https://downloads.sourceforge.net/project/emutos/emutos/{EMUTOS_VERSION}/emutos-{size}k-{EMUTOS_VERSION}.zip")
}

/// The languages EmuTOS comes in: the two letters it names its images by, and the language.
pub const EMUTOS_LANGUAGES: [(&str, &str); 19] = [
    ("uk", "English (UK, PAL)"),
    ("us", "English (US, NTSC)"),
    ("de", "German"),
    ("fr", "French"),
    ("es", "Spanish"),
    ("it", "Italian"),
    ("nl", "Dutch"),
    ("se", "Swedish"),
    ("no", "Norwegian"),
    ("fi", "Finnish"),
    ("sg", "Swiss German"),
    ("cz", "Czech"),
    ("pl", "Polish"),
    ("hu", "Hungarian"),
    ("ru", "Russian"),
    ("gr", "Greek"),
    ("tr", "Turkish"),
    ("ro", "Romanian"),
    ("ca", "Catalan"),
];

/// Where a downloaded EmuTOS is kept: beside the settings.
pub fn emutos_folder() -> Option<PathBuf> {
    crate::settings::data_folder().map(|dir| dir.join("emutos"))
}

/// The EmuTOS image for a machine in a folder, in the language asked for, or in English if it hasn't that one.
pub fn emutos_in(folder: &Path, model: Model, language: &str) -> Option<PathBuf> {
    let size = model.emutos_size();
    [language, "uk", "us"].into_iter().map(|l| folder.join(format!("etos{size}{l}.img"))).find(|p| p.is_file())
}

/// The downloaded EmuTOS for a machine, if there is one.
pub fn emutos_for(model: Model, language: &str) -> Option<PathBuf> {
    emutos_in(&emutos_folder()?, model, language)
}

/// Downloads EmuTOS's 192K, 256K and 512K images from SourceForge, in every language, with its licence and
/// notes, into a folder (replacing what was there). Gives the number of images.
pub fn download_emutos(into: &Path) -> Result<usize, String> {
    let mut archives = Vec::new();
    for size in [192, 256, 512] {
        let address = emutos_address(size);
        let response = ureq::get(&address).header("User-Agent", "Limoniter").call().map_err(|e| format!("SourceForge couldn't be reached: {e}"))?;
        let bytes = response.into_body().with_config().limit(16 << 20).read_to_vec().map_err(|e| format!("EmuTOS couldn't be downloaded: {e}"))?;
        archives.push((format!("emutos-{size}k.zip"), bytes));
    }
    unpack_emutos(archives, into)
}

/// Takes the images, licence and notes out of EmuTOS's zips, into a new folder beside the old first, so a
/// failure leaves the old one.
pub fn unpack_emutos(archives: Vec<(String, Vec<u8>)>, into: &Path) -> Result<usize, String> {
    let fresh = into.with_extension("new");
    let _ = std::fs::remove_dir_all(&fresh);
    std::fs::create_dir_all(&fresh).map_err(|e| format!("The folder for EmuTOS couldn't be made: {e}"))?;
    let mut images = 0;
    for (name, bytes) in archives {
        let volume = crate::media::open(Arc::new(bytes), &name)?;
        let mut todo = vec![None];
        while let Some(dir) = todo.pop() {
            for entry in volume.list(dir.as_ref())? {
                let file = crate::media::tidy_name(&entry.name);
                if entry.is_dir {
                    if file != "." && file != ".." && file != "extras" {
                        todo.push(Some(entry));
                    }
                    continue;
                }
                let lower = file.to_lowercase();
                let kept = if lower.ends_with(".img") && lower.starts_with("etos") {
                    images += 1;
                    lower.clone()
                } else if lower == "license.txt" {
                    "license.txt".into()
                } else if lower == "readme.txt" {
                    format!("readme-{}", name.trim_start_matches("emutos-").replace(".zip", ".txt"))
                } else {
                    continue;
                };
                let bytes = volume.read(&entry)?;
                std::fs::write(fresh.join(&kept), bytes).map_err(|e| format!("{kept} couldn't be written: {e}"))?;
            }
        }
    }
    if images == 0 {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err("The download had no EmuTOS images in it.".into());
    }
    let _ = std::fs::remove_dir_all(into);
    std::fs::rename(&fresh, into).map_err(|e| format!("EmuTOS couldn't be put in place: {e}"))?;
    Ok(images)
}

/// A download going on, from the Devices box, and what came of the last.
static DOWNLOAD: Mutex<Option<std::sync::mpsc::Receiver<Result<usize, String>>>> = Mutex::new(None);
static DOWNLOAD_SAID: Mutex<String> = Mutex::new(String::new());

// ---------- What Hatari is given ----------

/// The TOS a run uses for a machine: the one chosen, or the best found on this computer, or the downloaded
/// EmuTOS. None leaves it to Hatari's own configuration.
pub fn tos_for_run(settings: &Settings, model: Model, hatari: Option<&Path>) -> Option<PathBuf> {
    tos_from(settings, model, &find_tos_in(&tos_folders(hatari)))
}

/// The same, from the TOS ROMs found.
fn tos_from(settings: &Settings, model: Model, found: &[(PathBuf, Tos)]) -> Option<PathBuf> {
    settings.tos(model).or_else(|| best_for(model, found).map(|(p, _)| p)).or_else(|| emutos_for(model, &settings.emutos_language))
}

/// The TOS ROMs found for the Devices box, looked for once (and again when Find is pressed), not every time it's
/// drawn.
static FOUND: Mutex<Option<Vec<(PathBuf, Tos)>>> = Mutex::new(None);

fn found_for_box(hatari: &Path, again: bool) -> Vec<(PathBuf, Tos)> {
    let Ok(mut found) = FOUND.lock() else {
        return Vec::new();
    };
    if again || found.is_none() {
        *found = Some(find_tos_in(&tos_folders(Some(hatari))));
    }
    found.clone().unwrap_or_default()
}

/// Hatari's options for the machine: put after the person's own, so these win.
pub fn options(settings: &Settings, tos: Option<&Path>) -> Vec<String> {
    let model = settings.model;
    let choices = settings.choices(model);
    let mut args: Vec<String> = Vec::new();
    let mut add = |option: &str, value: String| {
        args.push(option.to_string());
        args.push(value);
    };
    add("--machine", model.id().into());
    if let Some(tos) = tos {
        add("--tos", tos.display().to_string());
    }
    // Hatari takes 0 for 512 KB, and sizes up to 14 in MB.
    add("--memsize", if choices.memory < 1024 { "0".into() } else { (choices.memory / 1024).to_string() });
    if model.is_030() {
        add("--ttram", choices.tt_ram.to_string());
        if model == Model::Falcon && choices.tt_ram > 0 {
            // A Falcon only reaches TT-RAM with 32-bit addresses.
            add("--addr24", "off".into());
        }
    }
    let monitor = if model.monitors().contains(&choices.monitor) { choices.monitor } else { model.monitors()[0] };
    add("--monitor", monitor.id().into());
    add("--cpuclock", choices.clock.to_string());
    add("--compatible", yes(settings.compatible).into());
    add("--cpu-exact", yes(settings.cycle_exact).into());
    add("--fast-boot", yes(settings.fast_boot).into());
    if model == Model::Falcon {
        add("--dsp", "emu".into());
    }
    args
}

/// A name for the program that TOS takes: 8.3, in capitals, ending .PRG so the AUTO folder runs it.
pub fn atari_name(name: &str) -> String {
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    let safe: String = stem.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').take(8).collect::<String>().to_ascii_uppercase();
    format!("{}.PRG", if safe.is_empty() { "PROGRAM" } else { &safe })
}

/// What a run in Hatari is: the file it's given last (the program, or the floppy it boots from) and its options.
pub struct Prepared {
    pub file: PathBuf,
    pub options: Vec<String>,
    /// Where a profiled run's profile will be saved.
    pub profile: Option<PathBuf>,
}

/// Makes what a run needs in `folder` (a new folder of its own, with the program already in it as `program`):
/// the floppy, when it boots from one, and the debugger's commands for a profile. `symbols`, Hatari's symbols
/// for the program, are given to its debugger when the program starts.
pub fn prepare(folder: &Path, program: &Path, run: &Runnable, settings: &Settings, hatari: Option<&Path>, profile: bool, symbols: Option<&str>) -> Result<Prepared, String> {
    let failed = |e: std::io::Error| format!("The run couldn't be made ready.\n\n{e}");
    let model = settings.model;
    let tos = tos_for_run(settings, model, hatari);
    if let Some(rom) = &tos
        && !rom.is_file()
    {
        return Err(format!("The TOS chosen for the {} isn't there: {}.\n\nChoose another in File, Devices, under Hatari.", model.short_name(), rom.display()));
    }
    let mut options = options(settings, tos.as_deref());
    let mut file = program.to_path_buf();
    if settings.start == Start::Floppy && run.format == Format::Tos {
        let boot = match &settings.boot_sector {
            Some(path) => Some(std::fs::read(path).map_err(|e| format!("The boot sector code {} couldn't be read.\n\n{e}", path.display()))?),
            None => None,
        };
        let disk = bootable_floppy(&run.bytes, &run.name, boot.as_deref())?;
        file = folder.join("boot.st");
        std::fs::write(&file, disk).map_err(failed)?;
        // Nothing else is drive C:, so the floppy's AUTO folder is what runs.
        options.extend(["--harddrive".to_string(), "none".to_string()]);
    }
    let mut saved = None;
    if profile {
        // When the program starts: its symbols, and the profiler on; then saved, and Hatari closed, when the
        // program ends (Pterm or Pterm0) or the time's up. Hatari's commands split at spaces, but it reads a file
        // of them from the file's own folder, so files beside them go by name; a breakpoint's :file takes the rest
        // of the line, spaces and all.
        let save = folder.join("profile.ini");
        let out = folder.join("profile.txt");
        std::fs::write(&save, "profile save profile.txt\nquit\n").map_err(failed)?;
        let mut started = String::from("e TEXT\n");
        if let Some(symbols) = symbols {
            std::fs::write(folder.join("program.sym"), symbols).map_err(failed)?;
            started.push_str("symbols program.sym TEXT\n");
        }
        let rate = settings.choices(model).monitor.rate();
        let frames = settings.profile_seconds.max(1) * rate;
        started.push_str(&format!("profile on\nb GemdosOpcode = $4C :once :trace :file {0}\nb GemdosOpcode = 0 :once :trace :file {0}\nb VBL = {frames} :once :trace :file {0}\n", save.display()));
        let on = folder.join("started.ini");
        std::fs::write(&on, started).map_err(failed)?;
        let start = folder.join("start.ini");
        // TEXT is the program running; the desktop's is in ROM.
        std::fs::write(&start, format!("b pc = TEXT && TEXT < $e00000 :once :trace :file {}\n", on.display())).map_err(failed)?;
        options.extend(["--parse".to_string(), start.display().to_string()]);
        saved = Some(out);
    }
    Ok(Prepared { file, options, profile: saved })
}

/// The labels of assembled source as Hatari's debugger reads symbols ("nm" style: offset, T, name), relative to
/// the start of the program's TEXT.
pub fn symbols(assembly: &crate::asm::Assembly) -> String {
    labels(assembly).iter().map(|(offset, name, _)| format!("{offset:08x} T {name}\n")).collect()
}

/// The labels that mark places in the program, in order: where, the name, and the source line.
fn labels(assembly: &crate::asm::Assembly) -> Vec<(u32, String, usize)> {
    let mut found: Vec<(u32, String, usize)> = assembly
        .definitions
        .iter()
        .filter(|d| d.file == 0 && d.written == d.name)
        .filter_map(|d| {
            let value = *assembly.symbols.get(&d.name)?;
            // A label is where its line's code is, or where the next code is; a value set with EQU isn't a place.
            let placed = assembly.lines.iter().find(|p| p.file == d.file && p.line == d.line).map(|p| p.address);
            (placed == Some(value) && value >= 0).then(|| (value as u32, d.name.clone(), d.line))
        })
        .collect();
    found.sort();
    found.dedup_by_key(|(at, ..)| *at);
    found
}

// ---------- Floppies ----------

/// The words of a boot sector add up to this when it's to be run.
const BOOT_SUM: u16 = 0x1234;

/// Makes a boot sector run: its last word set so its words add up to $1234.
pub fn fix_boot_checksum(sector: &mut [u8]) {
    let sum = (0..255).fold(0u16, |sum, i| sum.wrapping_add(u16::from_be_bytes([sector[i * 2], sector[i * 2 + 1]])));
    sector[510..512].copy_from_slice(&BOOT_SUM.wrapping_sub(sum).to_be_bytes());
}

/// Whether a boot sector is one TOS runs.
#[cfg(test)]
pub fn boot_runs(sector: &[u8]) -> bool {
    sector.len() >= 512 && (0..256).fold(0u16, |sum, i| sum.wrapping_add(u16::from_be_bytes([sector[i * 2], sector[i * 2 + 1]]))) == BOOT_SUM
}

/// Puts code in a floppy's boot sector, keeping the disk's own description of itself (bytes 2 to 29), and makes
/// it run. The code is either a whole boot sector (512 bytes, as taken off a demo's disk: its branch and code
/// are kept) or code on its own (up to 480 bytes), which goes after the description with a branch to it.
pub fn install_boot_code(disk: &mut [u8], code: &[u8]) -> Result<(), String> {
    let sector = &mut disk[..512];
    if code.len() == 512 {
        sector[..2].copy_from_slice(&code[..2]);
        sector[0x1E..510].copy_from_slice(&code[0x1E..510]);
    } else if code.len() <= 480 {
        // BRA.S to $1E, where the code starts.
        sector[..2].copy_from_slice(&[0x60, 0x1C]);
        sector[0x1E..510].fill(0);
        sector[0x1E..0x1E + code.len()].copy_from_slice(code);
    } else {
        return Err(format!("The boot sector code is {} bytes: it has to be a whole boot sector (512 bytes) or code of at most 480.", code.len()));
    }
    fix_boot_checksum(sector);
    Ok(())
}

/// A 720K floppy that boots into a program: the program in its AUTO folder (as NAME.PRG), and, if given, code
/// for its boot sector, which runs first.
pub fn bootable_floppy(program: &[u8], name: &str, boot: Option<&[u8]>) -> Result<Vec<u8>, String> {
    use crate::media::write::{Addition, Blank, writer};
    let blank = Blank::AtariSt720.make("LIMONITER");
    let mut disk = writer("boot.st", blank)?;
    disk.make_folder(None, "AUTO")?;
    let volume = crate::media::open(Arc::new(disk.bytes().to_vec()), "boot.st")?;
    let auto = volume.list(None)?.into_iter().find(|e| e.is_dir && e.name.eq_ignore_ascii_case("AUTO")).ok_or("The AUTO folder couldn't be made.")?;
    disk.add(Some(&auto), &Addition { name: atari_name(name), bytes: program.to_vec(), ..Default::default() })?;
    let mut bytes = disk.bytes().to_vec();
    if let Some(code) = boot {
        install_boot_code(&mut bytes, code)?;
    }
    Ok(bytes)
}

/// Packs a floppy as an MSA (Magic Shadow Archiver) image: track by track, runs of a byte written as $E5, the
/// byte and the count, where that's shorter.
pub fn pack_msa(disk: &[u8]) -> Result<Vec<u8>, String> {
    let word = |at: usize| usize::from(u16::from_le_bytes([disk[at], disk[at + 1]]));
    if disk.len() < 512 {
        return Err("This isn't a floppy image.".into());
    }
    let (sectors, sides, total) = (word(24), word(26), word(19));
    if sectors == 0 || sectors > 64 || !(1..=2).contains(&sides) || total == 0 || total % (sectors * sides) != 0 || total * 512 > disk.len() {
        return Err("The floppy's boot sector doesn't describe its tracks, so it can't be packed as MSA.".into());
    }
    let tracks = total / (sectors * sides);
    let track_size = sectors * 512;
    let mut out = Vec::with_capacity(disk.len() / 2);
    for value in [0x0E0F, sectors, sides - 1, 0, tracks - 1] {
        out.extend((value as u16).to_be_bytes());
    }
    for track in disk[..total * 512].chunks(track_size) {
        let mut packed = Vec::new();
        let mut i = 0;
        while i < track.len() {
            let byte = track[i];
            let run = track[i..].iter().take_while(|b| **b == byte).count();
            if run >= 4 || byte == 0xE5 {
                packed.extend([0xE5, byte]);
                packed.extend((run as u16).to_be_bytes());
            } else {
                packed.extend(&track[i..i + run]);
            }
            i += run;
        }
        if packed.len() < track_size {
            out.extend((packed.len() as u16).to_be_bytes());
            out.extend(packed);
        } else {
            out.extend((track_size as u16).to_be_bytes());
            out.extend(track);
        }
    }
    Ok(out)
}

// ---------- Profiles ----------

/// What Hatari's profiler counted at one address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Counted {
    pub address: u32,
    pub instructions: u64,
    pub cycles: u64,
    /// Hatari's disassembly of it.
    pub text: String,
}

/// A profile Hatari saved: the program's place, the processor's speed, and each address's counts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    pub text_start: Option<u32>,
    pub text_end: Option<u32>,
    pub cycles_per_second: u64,
    pub counted: Vec<Counted>,
}

fn hex(text: &str) -> Option<u32> {
    u32::from_str_radix(text.trim().trim_start_matches('$').trim_start_matches("0x").trim_end_matches(':'), 16).ok()
}

/// Reads a profile as Hatari's "profile save" writes it: a header ("Cycles/second:", "PROGRAM_TEXT:" with the
/// program's range), then a line per address with its disassembly and, in brackets at the end, its counts
/// (instructions, then cycles).
pub fn read_profile(text: &str) -> Profile {
    let mut profile = Profile::default();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Cycles/second:") {
            profile.cycles_per_second = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("PROGRAM_TEXT:") {
            if let Some((start, end)) = rest.trim().split_once('-') {
                profile.text_start = hex(start);
                profile.text_end = hex(end);
            }
        } else if line.ends_with(')')
            && let Some(open) = line.rfind('(')
            && let Some(address) = line.split_whitespace().next().and_then(hex)
        {
            let counts: Vec<u64> = line[open + 1..line.len() - 1].split(',').filter_map(|n| n.trim().parse().ok()).collect();
            if counts.len() >= 2 && line.starts_with(|c: char| c.is_ascii_hexdigit() || c == '$') {
                // The disassembly: after the address and its words, before the percentage.
                let body = line[..open].trim_end();
                let body = body.rsplit_once(' ').map_or(body, |(before, last)| if last.ends_with('%') { before } else { body }).trim_end();
                profile.counted.push(Counted { address, instructions: counts[0], cycles: counts[1], text: body.to_string() });
            }
        }
    }
    profile
}

/// A label's share of the time: the routine from the label to the next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Routine {
    pub name: String,
    pub cycles: u64,
    /// How many times its first instruction ran.
    pub entered: u64,
    pub line: Option<usize>,
}

/// A source line's share.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HotLine {
    pub line: usize,
    pub cycles: u64,
    pub runs: u64,
}

/// A profile summed up: the program's cycles and everyone else's (TOS, interrupts), by label, by source line,
/// and its busiest addresses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub program: u64,
    pub elsewhere: u64,
    pub routines: Vec<Routine>,
    pub lines: Vec<HotLine>,
    /// The program's busiest addresses, as offsets into it, with Hatari's disassembly.
    pub busiest: Vec<(u32, u64, String)>,
    pub cycles_per_second: u64,
}

/// Sums a profile up, by the source it was assembled from when there is some. `text` is where the program was,
/// when the profile doesn't say.
pub fn summarise(profile: &Profile, text: Option<u32>, assembly: Option<&crate::asm::Assembly>) -> Summary {
    let start = profile.text_start.or(text);
    let end = profile.text_end;
    let labels = assembly.map(labels).unwrap_or_default();
    let mut summary = Summary { cycles_per_second: profile.cycles_per_second, ..Summary::default() };
    let mut routines: std::collections::BTreeMap<String, Routine> = Default::default();
    let mut lines: std::collections::BTreeMap<usize, HotLine> = Default::default();
    for counted in &profile.counted {
        let offset = start.filter(|s| counted.address >= *s && end.is_none_or(|e| counted.address < e)).map(|s| counted.address - s);
        let Some(offset) = offset else {
            summary.elsewhere += counted.cycles;
            continue;
        };
        summary.program += counted.cycles;
        summary.busiest.push((offset, counted.cycles, counted.text.clone()));
        let Some(assembly) = assembly else { continue };
        let (name, line, first) = match labels.iter().rev().find(|(at, ..)| *at <= offset) {
            Some((at, name, line)) => (name.clone(), Some(*line), *at == offset),
            None => ("(before the first label)".to_string(), None, false),
        };
        let routine = routines.entry(name.clone()).or_insert(Routine { name, cycles: 0, entered: 0, line });
        routine.cycles += counted.cycles;
        if first {
            routine.entered += counted.instructions;
        }
        if let Some((0, line)) = assembly.line_at(i64::from(offset)) {
            let hot = lines.entry(line).or_insert(HotLine { line, cycles: 0, runs: 0 });
            hot.cycles += counted.cycles;
            hot.runs = hot.runs.max(counted.instructions);
        }
    }
    summary.routines = routines.into_values().collect();
    summary.routines.sort_by_key(|r| std::cmp::Reverse(r.cycles));
    summary.lines = lines.into_values().collect();
    summary.lines.sort_by_key(|l| std::cmp::Reverse(l.cycles));
    summary.busiest.sort_by_key(|(_, cycles, _)| std::cmp::Reverse(*cycles));
    summary.busiest.truncate(40);
    summary
}

// ---------- The Devices box ----------

/// Hatari's own rows in its fields, in the Devices box: the machine and its TOS, memory, monitor and processor,
/// how the program goes in, and EmuTOS.
pub fn settings_rows(ui: &mut Ui, hint: &dyn Fn(&str) -> RichText, hatari: &Path) {
    let mut settings = settings();
    let before = settings.clone();
    let note = |ui: &mut Ui, text: String| {
        ui.scope(|ui| {
            ui.set_max_width(330.0);
            ui.add(egui::Label::new(hint(&text)).wrap());
        });
    };
    ui.label("Atari:");
    egui::ComboBox::from_id_salt("st-model").width(320.0).selected_text(settings.model.name()).show_ui(ui, |ui| {
        for model in Model::ALL {
            ui.selectable_value(&mut settings.model, model, model.name());
        }
    });
    ui.end_row();
    let model = settings.model;
    let mut choices = settings.choices(model);
    ui.label("Memory:");
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("st-memory").width(90.0).selected_text(memory_name(choices.memory)).show_ui(ui, |ui| {
            for size in MEMORY {
                ui.selectable_value(&mut choices.memory, size, memory_name(size));
            }
        });
        if model.is_030() {
            ui.label("TT-RAM:");
            egui::ComboBox::from_id_salt("st-ttram").width(80.0).selected_text(if choices.tt_ram == 0 { "None".to_string() } else { format!("{} MB", choices.tt_ram) }).show_ui(ui, |ui| {
                for size in TT_RAM {
                    ui.selectable_value(&mut choices.tt_ram, size, if size == 0 { "None".to_string() } else { format!("{size} MB") });
                }
            });
        }
    });
    ui.end_row();
    ui.label("Monitor:");
    ui.horizontal(|ui| {
        if !model.monitors().contains(&choices.monitor) {
            choices.monitor = model.monitors()[0];
        }
        egui::ComboBox::from_id_salt("st-monitor").width(150.0).selected_text(choices.monitor.name()).show_ui(ui, |ui| {
            for monitor in model.monitors() {
                ui.selectable_value(&mut choices.monitor, *monitor, monitor.name());
            }
        });
        ui.label("Speed:");
        egui::ComboBox::from_id_salt("st-clock").width(70.0).selected_text(format!("{} MHz", choices.clock)).show_ui(ui, |ui| {
            for clock in [8, 16, 32] {
                ui.selectable_value(&mut choices.clock, clock, format!("{clock} MHz"));
            }
        });
    });
    ui.end_row();
    ui.label("Processor:");
    ui.horizontal(|ui| {
        ui.checkbox(&mut settings.cycle_exact, "Cycle-exact").on_hover_text("Hatari's --cpu-exact: every instruction takes the cycles it does on the real chip, as raster and sync effects need. Slower.");
        ui.checkbox(&mut settings.compatible, "Exact prefetch").on_hover_text("Hatari's --compatible: the 68000's prefetch as it really is, for self-modifying code");
        ui.checkbox(&mut settings.fast_boot, "Fast boot").on_hover_text("Hatari's --fast-boot: skips TOS's memory test");
    });
    ui.end_row();
    ui.label("TOS:");
    let mut found_note = None;
    ui.horizontal(|ui| {
        let mut text = choices.tos.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        if ui.add(egui::TextEdit::singleline(&mut text).hint_text("Found, or EmuTOS").desired_width(170.0)).changed() {
            choices.tos = (!text.trim().is_empty()).then(|| PathBuf::from(text.trim()));
        }
        if ui.button("Choose\u{2026}").on_hover_text(format!("The TOS ROM for the {}", model.short_name())).clicked()
            && let Some(chosen) = rfd::FileDialog::new().set_title(format!("TOS ROM for the {}", model.short_name())).add_filter("TOS ROM", &["img", "rom", "bin", "tos"]).pick_file()
        {
            choices.tos = Some(chosen);
        }
        if ui.button("Find").on_hover_text("Looks for TOS ROMs where Hatari and Steem keep them, and in folders of emulators, and chooses the best for each machine").clicked() {
            let found = found_for_box(hatari, true);
            let mut chosen = 0;
            for each in Model::ALL {
                let best = best_for(each, &found).filter(|(_, t)| !t.emutos);
                if let Some((path, _)) = best {
                    if each == model {
                        choices.tos = Some(path);
                    } else {
                        settings.set_tos(each, Some(path));
                    }
                    chosen += 1;
                }
            }
            let real = found.iter().filter(|(_, t)| !t.emutos).count();
            found_note = Some(match (real, chosen) {
                (0, _) => "No TOS ROMs were found.".to_string(),
                (n, 0) => format!("{n} found, but none for these machines."),
                (n, _) => format!("{n} found."),
            });
        }
    });
    ui.end_row();
    ui.label("");
    let said = match &choices.tos {
        Some(path) => match identify_file(path) {
            Some(t) if t.suits.contains(&model) => format!("{}.", t.name),
            Some(t) => format!("{}: not made for the {}, so Hatari may change machine or not start.", t.name, model.short_name()),
            None if path.is_file() => "That isn't a TOS Limoniter knows; Hatari may still take it.".to_string(),
            None => "That file isn't there.".to_string(),
        },
        None => match tos_from(&settings, model, &found_for_box(hatari, false)) {
            Some(path) => format!("None chosen: {} is used.", identify_file(&path).map_or_else(|| path.display().to_string(), |t| t.name)),
            None => "None chosen and none found: Hatari uses the TOS its own settings name. EmuTOS, below, is a free one.".to_string(),
        },
    };
    note(ui, found_note.map_or(said.clone(), |n| format!("{n} {said}")));
    ui.end_row();
    ui.label("EmuTOS:");
    ui.horizontal(|ui| {
        let busy = DOWNLOAD.lock().is_ok_and(|d| d.is_some());
        let have = emutos_folder().is_some_and(|f| f.join("license.txt").is_file());
        let language = EMUTOS_LANGUAGES.iter().find(|(id, _)| *id == settings.emutos_language).map_or("English (UK, PAL)", |(_, name)| name);
        egui::ComboBox::from_id_salt("st-emutos-language").width(130.0).selected_text(language).show_ui(ui, |ui| {
            for (id, name) in EMUTOS_LANGUAGES {
                ui.selectable_value(&mut settings.emutos_language, id.to_string(), name);
            }
        });
        if busy {
            ui.spinner();
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
        } else if ui.button(if have { "Download again" } else { "Download" }).on_hover_text(EMUTOS_PAGE).clicked()
            && let Some(folder) = emutos_folder()
        {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(download_emutos(&folder));
            });
            if let Ok(mut d) = DOWNLOAD.lock() {
                *d = Some(rx);
            }
        }
        if have && !busy && ui.button("Remove").on_hover_text("Deletes the downloaded EmuTOS").clicked()
            && let Some(folder) = emutos_folder()
        {
            let _ = std::fs::remove_dir_all(folder);
        }
    });
    ui.end_row();
    if let Ok(mut download) = DOWNLOAD.lock()
        && let Some(rx) = download.as_ref()
        && let Ok(result) = rx.try_recv()
    {
        if let Ok(mut said) = DOWNLOAD_SAID.lock() {
            *said = match result {
                Ok(n) => format!("Downloaded: {n} images."),
                Err(problem) => problem,
            };
        }
        *download = None;
    }
    ui.label("");
    let have = emutos_folder().and_then(|f| emutos_in(&f, model, &settings.emutos_language));
    let said = DOWNLOAD_SAID.lock().map(|s| s.clone()).unwrap_or_default();
    let state = match have {
        Some(image) => format!("In use when there's no TOS: {}.", image.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
        None => "Not downloaded.".to_string(),
    };
    note(ui, format!("{said} EmuTOS is a free TOS (GPL) from the EmuTOS team, used when there's no TOS of Atari's. Limoniter only fetches it from SourceForge when you ask (version {EMUTOS_VERSION}, about 9 MB, kept beside the settings), with the right image for each machine. {state}").trim().to_string());
    ui.end_row();
    ui.label("Start from:");
    ui.horizontal(|ui| {
        ui.radio_value(&mut settings.start, Start::Folder, "A folder as drive C:");
        ui.radio_value(&mut settings.start, Start::Floppy, "A bootable floppy");
    });
    ui.end_row();
    if settings.start == Start::Floppy {
        ui.label("Boot sector:");
        ui.horizontal(|ui| {
            let mut text = settings.boot_sector.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
            if ui.add(egui::TextEdit::singleline(&mut text).hint_text("None").desired_width(170.0)).changed() {
                settings.boot_sector = (!text.trim().is_empty()).then(|| PathBuf::from(text.trim()));
            }
            if ui.button("Choose\u{2026}").on_hover_text("Code for the floppy's boot sector: a whole boot sector (512 bytes) or code of up to 480 bytes").clicked()
                && let Some(chosen) = rfd::FileDialog::new().set_title("Boot sector code").pick_file()
            {
                settings.boot_sector = Some(chosen);
            }
        });
        ui.end_row();
    }
    ui.label("");
    note(ui, match settings.start {
        Start::Folder => "The program is put in a folder that's drive C: (Hatari's GEMDOS hard drive), which it can write files to, and started straight away.".to_string(),
        Start::Floppy => "The program is put in the AUTO folder of a 720K floppy made for the run, which the Atari boots from, as a demo on a disk does. Boot sector code, if chosen, goes on it with its checksum made $1234, so TOS runs it first.".to_string(),
    });
    ui.end_row();
    ui.label("Profile for:");
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut settings.profile_seconds).range(1..=3600).suffix(" s"));
        ui.label(hint("at most, with File, Run on, Profile in Hatari"));
    });
    ui.end_row();
    settings.set_choices(model, choices);
    if settings != before {
        set_settings(settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ROM with a TOS header: its version, where it is, its date and its country.
    fn rom(size: usize, version: u16, base: u32, country: u16, emutos: bool) -> Vec<u8> {
        let mut bytes = vec![0u8; size];
        bytes[..2].copy_from_slice(&[0x60, 0x2E]);
        bytes[2..4].copy_from_slice(&version.to_be_bytes());
        bytes[4..8].copy_from_slice(&(base + 0x30).to_be_bytes());
        bytes[8..12].copy_from_slice(&base.to_be_bytes());
        bytes[0x18..0x1C].copy_from_slice(&0x0406_1989u32.to_be_bytes());
        bytes[0x1C..0x1E].copy_from_slice(&(country << 1 | 1).to_be_bytes());
        if emutos {
            bytes[0x2C..0x30].copy_from_slice(b"ETOS");
            bytes[0x34..0x38].copy_from_slice(b"OSXH");
            bytes[0x3C..0x3E].copy_from_slice(&[1, 4]);
        }
        bytes
    }

    #[test]
    fn tos_roms_are_told_apart_by_their_headers() {
        let tos104 = identify(&rom(0x30000, 0x0104, 0xFC_0000, 3, false)).unwrap();
        assert_eq!(tos104.name, "TOS 1.04 (UK, 1989-04-06)");
        assert_eq!(tos104.suits, [Model::St, Model::MegaSt]);
        let tos206 = identify(&rom(0x40000, 0x0206, 0xE0_0000, 1, false)).unwrap();
        assert_eq!(tos206.name, "TOS 2.06 (Germany, 1989-04-06)");
        assert_eq!(tos206.suits[0], Model::MegaSte);
        assert_eq!(identify(&rom(0x80000, 0x0404, 0xE0_0000, 2, false)).unwrap().suits, [Model::Falcon]);
        assert_eq!(identify(&rom(0x80000, 0x0306, 0xE0_0000, 0, false)).unwrap().suits, [Model::Tt]);
        let emutos = identify(&rom(0x30000, 0x0104, 0xFC_0000, 3, true)).unwrap();
        assert_eq!((emutos.name.as_str(), emutos.emutos), ("EmuTOS 1.4 (192K, UK)", true));
        assert_eq!(identify(&rom(0x40000, 0x0206, 0xE0_0000, 127, true)).unwrap().name, "EmuTOS 1.4 (256K, all languages)");
        assert_eq!(identify(&vec![0; 0x30000]), None, "not a ROM");
        assert_eq!(identify(&rom(1000, 0x0104, 0xFC_0000, 3, false)), None, "not a ROM's size");
        assert_eq!(identify(&rom(0x30000, 0x0104, 0x12_0000, 3, false)), None, "not where a ROM is");
    }

    #[test]
    fn the_best_tos_is_chosen_for_each_machine_atari_s_own_first() {
        let named = |name: &str, bytes: Vec<u8>| (PathBuf::from(name), identify(&bytes).unwrap());
        let found = vec![
            named("etos512uk.img", rom(0x80000, 0x0206, 0xE0_0000, 3, true)),
            named("tos100.img", rom(0x30000, 0x0100, 0xFC_0000, 3, false)),
            named("tos104.img", rom(0x30000, 0x0104, 0xFC_0000, 3, false)),
            named("tos162.img", rom(0x40000, 0x0162, 0xE0_0000, 3, false)),
            named("tos206.img", rom(0x40000, 0x0206, 0xE0_0000, 3, false)),
        ];
        let best = |model| best_for(model, &found).map(|(p, _)| p.display().to_string());
        assert_eq!(best(Model::St).as_deref(), Some("tos104.img"));
        assert_eq!(best(Model::Ste).as_deref(), Some("tos162.img"));
        assert_eq!(best(Model::MegaSte).as_deref(), Some("tos206.img"));
        assert_eq!(best(Model::Falcon).as_deref(), Some("etos512uk.img"), "EmuTOS when there's no TOS 4");
    }

    #[test]
    fn settings_are_kept_as_text() {
        let mut settings = Settings { model: Model::Falcon, cycle_exact: false, start: Start::Floppy, boot_sector: Some(PathBuf::from("/demos/boot.bin")), emutos_language: "de".into(), profile_seconds: 7, ..Settings::default() };
        settings.set_tos(Model::Ste, Some(PathBuf::from("C:\\Atari\\tos 162.img")));
        settings.set_choices(Model::Falcon, Choices { tos: None, memory: 14336, tt_ram: 64, monitor: Monitor::Vga, clock: 32 });
        assert_eq!(Settings::from_text(&settings.to_text()), settings);
        assert_eq!(Settings::from_text("model=nonsense\nmemory.st=3\n"), Settings::default());
    }

    #[test]
    fn hatari_is_given_the_machine_as_its_manual_says() {
        let mut settings = Settings { model: Model::Ste, ..Settings::default() };
        assert_eq!(options(&settings, Some(Path::new("/roms/tos162.img"))), ["--machine", "ste", "--tos", "/roms/tos162.img", "--memsize", "1", "--monitor", "rgb", "--cpuclock", "8", "--compatible", "yes", "--cpu-exact", "yes", "--fast-boot", "yes"]);
        settings.model = Model::Falcon;
        settings.set_choices(Model::Falcon, Choices { tos: None, memory: 512, tt_ram: 16, monitor: Monitor::Vga, clock: 16 });
        let args = options(&settings, None);
        assert_eq!(&args[..10], ["--machine", "falcon", "--memsize", "0", "--ttram", "16", "--addr24", "off", "--monitor", "vga"]);
        assert!(args.ends_with(&["--dsp".to_string(), "emu".to_string()]));
    }

    #[test]
    fn the_devices_box_rows_draw_for_every_machine() {
        let ctx = egui::Context::default();
        for model in Model::ALL {
            set_settings(Settings { model, start: Start::Floppy, ..settings() });
            ctx.run_ui(Default::default(), |ui| {
                egui::Grid::new("st").show(ui, |ui| settings_rows(ui, &|t| RichText::new(t), Path::new("/nowhere/hatari")));
            })
            .textures_delta
            .clear();
        }
        set_settings(Settings::default());
    }

    #[test]
    fn a_bootable_floppy_has_the_program_in_auto_and_a_boot_sector_that_runs() {
        let program = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/src/asm/testdata/st_hello.tos")).unwrap();
        let disk = bootable_floppy(&program, "my demo.tos", None).unwrap();
        assert_eq!(disk.len(), 737_280);
        assert!(!boot_runs(&disk[..512]), "a floppy with no boot code isn't run");
        let volume = crate::media::open(Arc::new(disk.clone()), "boot.st").unwrap();
        let auto = volume.list(None).unwrap().into_iter().find(|e| e.name == "AUTO").unwrap();
        let files = volume.list(Some(&auto)).unwrap();
        assert_eq!(files.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["MYDEMO.PRG"]);
        assert_eq!(volume.read(&files[0]).unwrap(), program);
        // Boot code on its own goes after the disk's description, with a branch to it.
        let code = [0x4E, 0x75];
        let booting = bootable_floppy(&program, "x.tos", Some(&code)).unwrap();
        assert!(boot_runs(&booting[..512]));
        assert_eq!(&booting[..2], [0x60, 0x1C]);
        assert_eq!(&booting[0x1E..0x20], code);
        assert_eq!(&booting[11..30], &disk[11..30], "the description of the disk is kept");
        // A whole boot sector keeps its branch and code.
        let mut whole = vec![0u8; 512];
        whole[..2].copy_from_slice(&[0x60, 0x38]);
        whole[0x3A..0x3C].copy_from_slice(&[0x4E, 0x75]);
        let mut again = disk.clone();
        install_boot_code(&mut again, &whole).unwrap();
        assert!(boot_runs(&again[..512]) && again[0x3A] == 0x4E && again[..2] == [0x60, 0x38]);
        assert!(install_boot_code(&mut again, &[0; 600]).is_err());
        assert_eq!(atari_name("a very long name.tos"), "AVERYLON.PRG");
    }

    #[test]
    fn floppies_pack_as_msa_and_unpack_as_they_were() {
        let mut disk = bootable_floppy(&[0x60, 0x1A, 0xE5, 0xE5, 1, 2, 3], "x.tos", Some(&[0xE5; 20])).unwrap();
        disk[100_000..100_010].copy_from_slice(&[0xE5, 1, 0xE5, 0xE5, 0xE5, 0xE5, 0xE5, 7, 7, 7]);
        let msa = pack_msa(&disk).unwrap();
        assert!(msa.len() < disk.len() / 4, "{}", msa.len());
        assert_eq!(&msa[..10], [0x0E, 0x0F, 0, 9, 0, 1, 0, 0, 0, 79]);
        // Unpacked as the MSA reader does, it's the floppy again, byte for byte.
        let mut unpacked = Vec::new();
        let mut at = 10;
        while at < msa.len() {
            let length = usize::from(u16::from_be_bytes([msa[at], msa[at + 1]]));
            let track = &msa[at + 2..at + 2 + length];
            at += 2 + length;
            if length == 9 * 512 {
                unpacked.extend(track);
                continue;
            }
            let mut i = 0;
            while i < track.len() {
                if track[i] == 0xE5 {
                    unpacked.extend(std::iter::repeat_n(track[i + 1], usize::from(u16::from_be_bytes([track[i + 2], track[i + 3]]))));
                    i += 4;
                } else {
                    unpacked.push(track[i]);
                    i += 1;
                }
            }
        }
        assert_eq!(unpacked, disk);
        let volume = crate::media::open(Arc::new(msa), "x.msa").unwrap();
        assert!(volume.list(None).unwrap().iter().any(|e| e.name == "AUTO"));
    }

    #[test]
    fn a_profile_is_read_and_summed_by_label_and_line() {
        let text = "Hatari CPU profile (Hatari v2.3.1, WinUAE CPU core)\nCycles/second:\t8021247\nField names:\tExecuted instructions, Used cycles, Instruction cache misses, Data cache hits\nROM_TOS:\t0xfc0000-0xff0000\nPROGRAM_TEXT:\t0x00e81c-0x00e866\n# disassembly with profile data\n0000E81C 4879 0000 e842           PEA.L $0000e842                    0.00% (1, 20, 0, 0)\n0000E822 3f3c 0009                MOVE.W #$0009,-(A7) [6a90]         0.00% (3, 36, 0, 0)\n[...]\n00FC0448 52b8 0466                ADDQ.L #$01,$0466 [000002b9]       0.31% (622, 14984, 0, 0)\n";
        let profile = read_profile(text);
        assert_eq!((profile.text_start, profile.text_end, profile.cycles_per_second), (Some(0xE81C), Some(0xE866), 8_021_247));
        assert_eq!(profile.counted.len(), 3);
        assert_eq!(profile.counted[1], Counted { address: 0xE822, instructions: 3, cycles: 36, text: "0000E822 3f3c 0009                MOVE.W #$0009,-(A7) [6a90]".into() });
        let source = "start:\tpea\tmessage\nloop:\tmove.w\t#9,-(sp)\n\ttrap\t#1\nmessage:\tdc.b\t\"Hi\",0\n";
        let assembly = crate::asm::assemble(&crate::asm::m68k::M68k, Path::new("t.s"), source, &|_| Err("none".into()));
        assert!(assembly.ok(), "{:?}", assembly.problems);
        let summary = summarise(&profile, None, Some(&assembly));
        assert_eq!((summary.program, summary.elsewhere), (56, 14984));
        assert_eq!(summary.routines.iter().map(|r| (r.name.as_str(), r.cycles, r.entered, r.line)).collect::<Vec<_>>(), [("loop", 36, 3, Some(1)), ("start", 20, 1, Some(0))]);
        assert_eq!(summary.lines[0], HotLine { line: 1, cycles: 36, runs: 3 });
        assert_eq!(symbols(&assembly), "00000000 T start\n00000006 T loop\n0000000c T message\n");
    }

    /// A program that writes RESULT.TXT to its current drive, then ends.
    const WRITES_A_FILE: &str = "start:\tclr.w\t-(sp)\n\tpea\tname\n\tmove.w\t#$3c,-(sp)\n\ttrap\t#1\n\taddq.l\t#8,sp\n\tmove.w\td0,d7\nbusy:\tmove.w\t#5000,d1\nspin:\tdbra\td1,spin\n\tpea\ttext\n\tmove.l\t#16,-(sp)\n\tmove.w\td7,-(sp)\n\tmove.w\t#$40,-(sp)\n\ttrap\t#1\n\tlea\t12(sp),sp\n\tmove.w\td7,-(sp)\n\tmove.w\t#$3e,-(sp)\n\ttrap\t#1\n\taddq.l\t#4,sp\n\tclr.w\t-(sp)\n\ttrap\t#1\nname:\tdc.b\t\"RESULT.TXT\",0\ntext:\tdc.b\t\"Limoniter ran it\"\n\teven\n";

    fn st_program(source: &str) -> (crate::asm::Assembly, Vec<u8>) {
        let none = |_: &Path| Err("none".to_string());
        let assembly = crate::asm::assemble_at(&crate::asm::m68k::M68k, Path::new("t.s"), source, &none, 0);
        assert!(assembly.ok(), "{:?}", assembly.problems);
        let moved = crate::asm::assemble_at(&crate::asm::m68k::M68k, Path::new("t.s"), source, &none, 0x10000);
        let bytes = crate::asm::output::atari_program(&assembly, &moved, 0x10000).unwrap();
        (assembly, bytes)
    }

    /// Runs Hatari as Limoniter would, with no window or sound, until it quits or `frames` have gone by. Unless
    /// it's profiling (which quits by itself), it's told to quit as the program ends: --run-vbls leaves without
    /// writing a floppy back.
    fn run_hatari(hatari: &Path, folder: &Path, prepared: &Prepared, frames: u32) -> String {
        let mut args = prepared.options.clone();
        if prepared.profile.is_none() {
            let (start, ended, quit) = (folder.join("test-start.ini"), folder.join("test-ended.ini"), folder.join("test-quit.ini"));
            std::fs::write(&quit, "quit\n").unwrap();
            std::fs::write(&ended, format!("b GemdosOpcode = 0 :once :trace :file {}\n", quit.display())).unwrap();
            std::fs::write(&start, format!("b pc = TEXT && TEXT < $e00000 :once :trace :file {}\n", ended.display())).unwrap();
            args.extend(["--parse".into(), start.display().to_string()]);
        }
        args.extend(["--sound".into(), "off".into(), "--fast-forward".into(), "on".into(), "--confirm-quit".into(), "off".into(), "--run-vbls".into(), frames.to_string()]);
        args.push(prepared.file.display().to_string());
        let out = std::process::Command::new(hatari).args(&args).current_dir(folder).env("SDL_VIDEODRIVER", "dummy").env("SDL_AUDIODRIVER", "dummy").output().unwrap();
        format!("hatari {}\n{}{}", args.join(" "), String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
    }

    /// With LIMONITER_DOWNLOAD_EMUTOS set: EmuTOS is downloaded from SourceForge and unpacked, an image for each
    /// machine in each language.
    #[test]
    fn emutos_is_downloaded_and_unpacked() {
        if std::env::var_os("LIMONITER_DOWNLOAD_EMUTOS").is_none() {
            return;
        }
        let folder = std::env::temp_dir().join(format!("limoniter-emutos-{}", std::process::id()));
        let images = download_emutos(&folder).unwrap();
        assert!(images >= 50, "{images}");
        assert!(folder.join("license.txt").is_file());
        for model in Model::ALL {
            let image = emutos_in(&folder, model, "de").unwrap();
            let tos = identify_file(&image).unwrap();
            assert!(tos.emutos && tos.suits.contains(&model) && tos.name.contains("Germany"), "{model:?}: {tos:?}");
        }
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// With LIMONITER_HATARI naming Hatari and LIMONITER_EMUTOS a folder of EmuTOS images: a program assembled
    /// here runs on each machine (ST, Mega ST, STE, Mega STE, TT, Falcon), from a folder as drive C: and from a
    /// bootable floppy, writing a file each time; and a profiled run is saved and summed by label.
    #[test]
    fn programs_run_in_a_real_hatari_on_several_machines() {
        let (Some(hatari), Some(emutos)) = (std::env::var_os("LIMONITER_HATARI"), std::env::var_os("LIMONITER_EMUTOS")) else {
            return;
        };
        let (hatari, emutos) = (PathBuf::from(hatari), PathBuf::from(emutos));
        // The real images are told apart by their headers.
        let named = |file: &str| identify_file(&emutos.join(file)).map(|t| (t.name, t.suits[0]));
        assert_eq!(named("etos192uk.img"), Some(("EmuTOS 1.4 (192K, UK)".to_string(), Model::St)));
        assert_eq!(named("etos256de.img"), Some(("EmuTOS 1.4 (256K, Germany)".to_string(), Model::Ste)));
        assert_eq!(named("etos512fr.img"), Some(("EmuTOS 1.4 (512K, France)".to_string(), Model::Tt)));
        assert_eq!(named("etos192us.img"), Some(("EmuTOS 1.4 (192K, USA)".to_string(), Model::St)));
        let (assembly, bytes) = st_program(WRITES_A_FILE);
        let run = Runnable { machine: crate::running::Machine::AtariSt, format: Format::Tos, name: "writes.tos".into(), bytes: Arc::new(bytes) };
        // A folder with a space in its name, as Windows user folders often have.
        let base = std::env::temp_dir().join(format!("limoniter hatari {}", std::process::id()));
        for model in Model::ALL {
            for start in [Start::Folder, Start::Floppy] {
                let folder = base.join(format!("{}-{start:?}", model.id()));
                let _ = std::fs::remove_dir_all(&folder);
                std::fs::create_dir_all(&folder).unwrap();
                let program = folder.join(&run.name);
                std::fs::write(&program, run.bytes.as_slice()).unwrap();
                let mut settings = Settings { model, start, ..Settings::default() };
                settings.set_tos(model, emutos_in(&emutos, model, "uk"));
                let prepared = prepare(&folder, &program, &run, &settings, None, false, None).unwrap();
                let said = run_hatari(&hatari, &folder, &prepared, 1500);
                let written = match start {
                    Start::Folder => std::fs::read(folder.join("RESULT.TXT")).ok(),
                    // Hatari writes what the program wrote back into the floppy's image.
                    Start::Floppy => {
                        let volume = crate::media::open(Arc::new(std::fs::read(&prepared.file).unwrap()), "boot.st").unwrap();
                        volume.list(None).unwrap().into_iter().find(|e| e.name == "RESULT.TXT").map(|e| volume.read(&e).unwrap())
                    }
                };
                assert_eq!(written.as_deref(), Some(&b"Limoniter ran it"[..]), "{model:?} from a {start:?}:\n{said}");
                eprintln!("{model:?} from a {start:?}: RESULT.TXT written");
            }
        }
        // Boot sector code runs as the floppy boots: here, setting the first colour to $123, which Hatari watches for.
        let folder = base.join("boot-sector");
        std::fs::create_dir_all(&folder).unwrap();
        let program = folder.join(&run.name);
        std::fs::write(&program, run.bytes.as_slice()).unwrap();
        let code = folder.join("boot.bin");
        std::fs::write(&code, [0x31, 0xFC, 0x01, 0x23, 0x82, 0x40, 0x4E, 0x75]).unwrap();
        let watch = folder.join("watch.ini");
        std::fs::write(&watch, "b ($ff8240).w & $fff = $123 :once :trace\n").unwrap();
        for boot in [None, Some(code)] {
            let mut settings = Settings { start: Start::Floppy, boot_sector: boot.clone(), ..Settings::default() };
            settings.set_tos(Model::St, emutos_in(&emutos, Model::St, "uk"));
            let mut prepared = prepare(&folder, &program, &run, &settings, None, false, None).unwrap();
            prepared.options.extend(["--parse".into(), watch.display().to_string()]);
            let said = run_hatari(&hatari, &folder, &prepared, 1500);
            // Hatari spaces the condition out as it prints it.
            assert_eq!(said.replace(' ', "").contains("matched1times.\n\t($ff8240).w&$fff=$123"), boot.is_some(), "{said}");
        }
        eprintln!("The boot sector's code ran");
        // Profiled: from the program's start until it ends, when Hatari saves the profile and quits.
        let folder = base.join("profile");
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let program = folder.join(&run.name);
        std::fs::write(&program, run.bytes.as_slice()).unwrap();
        let mut settings = Settings::default();
        settings.set_tos(Model::St, emutos_in(&emutos, Model::St, "uk"));
        let prepared = prepare(&folder, &program, &run, &settings, None, true, Some(&symbols(&assembly))).unwrap();
        let said = run_hatari(&hatari, &folder, &prepared, 3000);
        let text = std::fs::read_to_string(prepared.profile.as_ref().unwrap()).unwrap_or_else(|_| panic!("no profile:\n{said}"));
        let summary = summarise(&read_profile(&text), None, Some(&assembly));
        assert!(said.contains("Loaded 5 symbols"), "{said}");
        let spin = summary.routines.iter().find(|r| r.name == "spin").unwrap_or_else(|| panic!("{summary:?}"));
        assert_eq!(spin.entered, 5001, "the DBRA loop ran 5001 times");
        assert_eq!(summary.routines[0].name, "spin", "{summary:?}");
        assert!(summary.lines[0].line == 7 && summary.program > 50_000, "{summary:?}");
        eprintln!("Profile: {} cycles in the program, {} elsewhere; {:?}", summary.program, summary.elsewhere, summary.routines);
        let _ = std::fs::remove_dir_all(&base);
    }
}
