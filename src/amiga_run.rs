//! Running programs on an emulated Amiga (FS-UAE, WinUAE or Copperline): the model, the Kickstart ROM it starts
//! with, and the folder that's its hard drive. For FS-UAE and WinUAE the program goes in the folder with an
//! S/Startup-Sequence that runs it, and the emulator is started from a configuration file made for the run, so the
//! Amiga boots straight into it. PUAE, in Limoniter's window, is given the same folder (see `retro::write_game`).
//! Copperline boots straight into a program itself (its --run), so it's only given the machine, in a configuration
//! of its own beside the program. Kickstarts are told by what's in them here, for all of them, Set up the Amiga too.
//!
//! Kickstart is Commodore's (now Cloanto's and Hyperion's), so Limoniter never includes or copies one: the person
//! points at their own (Amiga Forever's, or FS-UAE's or WinUAE's ROM folder), or the emulator's own AROS, a free
//! stand-in, is used.

use crate::running::{Format, Runnable};
use eframe::egui::{self, RichText, Ui};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// An Amiga to run on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Model {
    A500,
    A500Plus,
    A1200,
}

impl Model {
    pub const ALL: [Model; 3] = [Model::A500, Model::A500Plus, Model::A1200];

    pub fn name(self) -> &'static str {
        match self {
            Model::A500 => "A500 (OCS, 1 MB, Kickstart 1.3)",
            Model::A500Plus => "A500+ (ECS, 1 MB chip, Kickstart 2.04)",
            Model::A1200 => "A1200 (AGA, 68020, 2 MB, Kickstart 3.1)",
        }
    }

    pub fn short_name(self) -> &'static str {
        match self {
            Model::A500 => "A500",
            Model::A500Plus => "A500+",
            Model::A1200 => "A1200",
        }
    }

    /// Its name in the settings.
    pub fn id(self) -> &'static str {
        match self {
            Model::A500 => "a500",
            Model::A500Plus => "a500plus",
            Model::A1200 => "a1200",
        }
    }

    /// Copperline's machine profile for it: its A500 is the common Rev 6A board (512K chip and 512K slow), its
    /// A500Plus has 1 MB of chip memory and the ECS chips, and its A1200 2 MB and AGA.
    fn copperline(self) -> &'static str {
        match self {
            Model::A500 => "A500",
            Model::A500Plus => "A500Plus",
            Model::A1200 => "A1200",
        }
    }

    fn from_id(id: &str) -> Option<Model> {
        Model::ALL.into_iter().find(|m| m.id() == id)
    }

    /// WinUAE's settings for it, as its own quick start sets them.
    fn winuae(self) -> &'static [&'static str] {
        match self {
            Model::A500 => &["cpu_model=68000", "cpu_compatible=true", "cpu_cycle_exact=true", "blitter_cycle_exact=true", "chipset=ocs", "chipmem_size=1", "bogomem_size=2", "fastmem_size=0"],
            Model::A500Plus => &["cpu_model=68000", "cpu_compatible=true", "cpu_cycle_exact=true", "blitter_cycle_exact=true", "chipset=ecs", "chipmem_size=2", "bogomem_size=0", "fastmem_size=0"],
            Model::A1200 => &["cpu_model=68020", "cpu_24bit_addressing=true", "cpu_compatible=true", "chipset=aga", "chipmem_size=4", "bogomem_size=0", "fastmem_size=0"],
        }
    }
}

/// The Amiga runs are on: the model, and the Kickstart chosen for each model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub model: Model,
    pub kickstarts: Vec<(Model, PathBuf)>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { model: Model::A500, kickstarts: Vec::new() }
    }
}

impl Settings {
    pub fn kickstart(&self, model: Model) -> Option<&Path> {
        self.kickstarts.iter().find(|(m, _)| *m == model).map(|(_, p)| p.as_path())
    }

    pub fn set_kickstart(&mut self, model: Model, path: Option<PathBuf>) {
        self.kickstarts.retain(|(m, _)| *m != model);
        if let Some(path) = path {
            self.kickstarts.push((model, path));
        }
    }

    fn to_text(&self) -> String {
        let mut text = format!("model={}\n", self.model.id());
        for (model, path) in &self.kickstarts {
            text.push_str(&format!("kickstart.{}={}\n", model.id(), path.display()));
        }
        text
    }

    fn from_text(text: &str) -> Settings {
        let mut settings = Settings::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            if key == "model" {
                settings.model = Model::from_id(value.trim()).unwrap_or(Model::A500);
            } else if let Some(model) = key.strip_prefix("kickstart.").and_then(Model::from_id)
                && !value.trim().is_empty()
            {
                settings.set_kickstart(model, Some(PathBuf::from(value.trim())));
            }
        }
        settings
    }
}

/// Where they're kept, beside Limoniter's settings.
fn settings_file() -> Option<PathBuf> {
    crate::settings::data_folder().map(|dir| dir.join("amiga.txt"))
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

/// A Kickstart ROM, as told from its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kickstart {
    pub name: String,
    /// The models it's right for, best first.
    pub suits: Vec<Model>,
    /// How well it suits each, lower being better: by the order the models prefer them.
    pub rank: usize,
}

/// The Kickstarts known by their CRC-32: what each is, the models it's right for and how good a choice it is.
const KNOWN: &[(u32, &str, &[Model], usize)] = &[
    (0xC4F0_F55F, "Kickstart 1.3 (34.5), A500", &[Model::A500, Model::A500Plus], 0),
    (0xA6CE_1636, "Kickstart 1.2 (33.180), A500", &[Model::A500], 2),
    (0xC3BD_B240, "Kickstart 2.04 (37.175), A500+", &[Model::A500Plus, Model::A500], 0),
    (0x8302_8FB5, "Kickstart 2.05 (37.299), A600", &[Model::A500Plus, Model::A500], 1),
    (0x6446_6C2A, "Kickstart 2.05 (37.300), A600HD", &[Model::A500Plus, Model::A500], 1),
    (0x43B0_DF7B, "Kickstart 2.05 (37.350), A600HD", &[Model::A500Plus, Model::A500], 1),
    (0xFC24_AE0D, "Kickstart 3.1 (40.63), A500, A600 and A2000", &[Model::A500Plus, Model::A500], 3),
    (0x6C9B_07D2, "Kickstart 3.0 (39.106), A1200", &[Model::A1200], 1),
    (0x1483_A091, "Kickstart 3.1 (40.68), A1200", &[Model::A1200], 0),
    (0xD6BA_E334, "Kickstart 3.1 (40.68), A4000", &[], 9),
    (0x9E6A_C152, "Kickstart 3.0 (39.106), A4000", &[], 9),
    (0x1E62_D4A5, "Kickstart 3.1 (40.60), CD32", &[], 9),
];

// Telling Kickstarts by what's in them, for every emulator: WinUAE, FS-UAE and Copperline are pointed at the file
// (`identify_file`, `find_kickstarts_in`, `best_for`), and Set up the Amiga copies them into PUAE's system folder
// under its own names (amiga_setup.rs, which builds on `open_rom` and `rom_files_in`).

/// What an encrypted ROM from Amiga Forever (Cloanto's) starts with. The rest is the ROM mixed with the bytes of
/// the rom.key kept with it: the plain XOR UAE's rommgr.c undoes.
pub const ENCRYPTED_MARK: &[u8; 11] = b"AMIROMTYPE1";
pub const SIZE_256: usize = 256 * 1024;
pub const SIZE_512: usize = 512 * 1024;

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// Whether bytes start like a Kickstart: 0x1111 or 0x1114, then the 68000's JMP (4EF9) into the ROM.
pub fn has_header(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[0] == 0x11 && (bytes[1] == 0x11 || bytes[1] == 0x14) && bytes[2] == 0x4E && bytes[3] == 0xF9
}

fn xor_with(body: &[u8], key: &[u8]) -> Vec<u8> {
    body.iter().zip(key.iter().cycle()).map(|(b, k)| b ^ k).collect()
}

/// A Kickstart's bytes, opened: decrypted, a doubled 256 KB image cut back to one copy, and the version and
/// revision exec has in it at offset 12.
#[derive(Debug)]
pub struct OpenedRom {
    pub bytes: Vec<u8>,
    pub version: u16,
    pub revision: u16,
    pub decrypted: bool,
    pub doubled: bool,
}

/// Opens what may be a Kickstart, or says plainly why it isn't one. `keys` are the contents of any rom.key files
/// found for it, tried in turn if it's encrypted: the one that gives a Kickstart's header is the right one.
pub fn open_rom(bytes: &[u8], keys: &[Vec<u8>]) -> Result<OpenedRom, String> {
    let (mut plain, decrypted) = if let Some(body) = bytes.strip_prefix(ENCRYPTED_MARK.as_slice()) {
        if keys.iter().all(|k| k.is_empty()) {
            return Err(crate::i18n::tr("It's an encrypted Amiga Forever ROM, and its rom.key isn't beside it or in Amiga Forever's Shared/rom folder. Copy rom.key next to it and try again.").into());
        }
        match keys.iter().filter(|k| !k.is_empty()).map(|k| xor_with(body, k)).find(|candidate| has_header(candidate)) {
            Some(opened) => (opened, true),
            None => return Err(crate::i18n::tr("It's an encrypted Amiga Forever ROM, and the rom.key found for it doesn't open it.").into()),
        }
    } else {
        (bytes.to_vec(), false)
    };
    if plain.len() != SIZE_256 && plain.len() != SIZE_512 {
        return Err(crate::i18n::tf!("It isn't a Kickstart: those are 256 KB or 512 KB, and this is {} bytes.", plain.len()));
    }
    if !has_header(&plain) {
        return Err(crate::i18n::tr("It isn't a Kickstart: it doesn't start the way one does.").into());
    }
    // A 256 KB ROM that's been stored twice to fill 512 KB is taken once.
    let doubled = plain.len() == SIZE_512 && plain[..SIZE_256] == plain[SIZE_256..];
    if doubled {
        plain.truncate(SIZE_256);
    }
    let version = u16::from_be_bytes([plain[12], plain[13]]);
    let revision = u16::from_be_bytes([plain[14], plain[15]]);
    Ok(OpenedRom { bytes: plain, version, revision, decrypted, doubled })
}

/// Where a rom.key might be for a file: beside it, and in a Shared/rom folder next to or above it (Amiga Forever's),
/// then in any of the folders in `more`.
pub fn key_files_for(file: &Path, more: &[PathBuf]) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    let mut add = |folder: &Path| {
        let key = folder.join("rom.key");
        if key.is_file() && !found.contains(&key) {
            found.push(key);
        }
    };
    if let Some(folder) = file.parent() {
        add(folder);
        add(&folder.join("Shared").join("rom"));
        if let Some(above) = folder.parent() {
            add(above);
            add(&above.join("Shared").join("rom"));
        }
    }
    for folder in more {
        add(folder);
    }
    found
}

/// Reads a file and opens it, if it's a Kickstart. A file that isn't one of a Kickstart's sizes (plain or
/// encrypted) is refused without reading it.
pub fn open_rom_file(file: &Path, more_keys: &[PathBuf]) -> Result<OpenedRom, String> {
    let length = std::fs::metadata(file).map_err(|e| crate::i18n::tf!("It couldn't be read: {e}"))?.len();
    if !rom_sized(length) {
        return Err(crate::i18n::tf!("It isn't a Kickstart: those are 256 KB or 512 KB, and this is {length} bytes."));
    }
    let bytes = std::fs::read(file).map_err(|e| crate::i18n::tf!("It couldn't be read: {e}"))?;
    let keys: Vec<Vec<u8>> = if bytes.starts_with(ENCRYPTED_MARK) { key_files_for(file, more_keys).iter().filter_map(|f| std::fs::read(f).ok()).collect() } else { Vec::new() };
    open_rom(&bytes, &keys)
}

/// Whether a file of this many bytes could be a Kickstart, plain or encrypted.
fn rom_sized(length: u64) -> bool {
    let mark = ENCRYPTED_MARK.len();
    [SIZE_256, SIZE_512, SIZE_256 + mark, SIZE_512 + mark].iter().any(|size| *size as u64 == length)
}

/// What an opened Kickstart is: a known one by its CRC, or any other by its version.
fn describe(rom: &OpenedRom) -> Kickstart {
    let crc = crc32(&rom.bytes);
    if let Some((_, name, suits, rank)) = KNOWN.iter().find(|(c, ..)| *c == crc) {
        return Kickstart { name: name.to_string(), suits: suits.to_vec(), rank: *rank };
    }
    let (version, revision) = (rom.version, rom.revision);
    let (release, suits): (&str, &[Model]) = match version {
        33 => ("1.2", &[Model::A500]),
        34 => ("1.3", &[Model::A500]),
        36 | 37 => ("2.0", &[Model::A500Plus, Model::A500]),
        39 => ("3.0", &[]),
        40 => ("3.1", &[]),
        45..=47 => ("3.1.4 or later", &[]),
        _ => ("of a version not known here", &[]),
    };
    Kickstart { name: format!("Kickstart {release} ({version}.{revision})"), suits: suits.to_vec(), rank: 5 }
}

/// What some bytes are, if they're a Kickstart. An encrypted one of Amiga Forever's is read with the keys given.
#[cfg(test)]
fn identify(bytes: &[u8], keys: &[Vec<u8>]) -> Option<Kickstart> {
    open_rom(bytes, keys).ok().map(|rom| describe(&rom))
}

/// Reads a file, if it's a Kickstart: Amiga Forever's rom.key, beside it or in a Shared/rom near it, unlocks one of
/// its own.
pub fn identify_file(path: &Path) -> Option<Kickstart> {
    open_rom_file(path, &[]).ok().map(|rom| describe(&rom))
}

/// The usual places for Kickstarts on this computer, each with how deep to look in it.
pub fn usual_places() -> Vec<(PathBuf, usize)> {
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    places(env("USERPROFILE").or_else(|| env("HOME")).as_deref(), env("PUBLIC").as_deref())
}

/// The usual places for Kickstarts, given the home folder and Windows' Public one, each with how deep to look in
/// it: Amiga Forever's shared ROM folder, FS-UAE's and WinUAE's, folders of Amiga things among where emulators are
/// put, and Downloads, Documents and the Desktop. Those last are looked at shallowly since they hold much else.
/// Only folders that are there are given, each once.
pub fn places(home: Option<&Path>, public: Option<&Path>) -> Vec<(PathBuf, usize)> {
    let mut out: Vec<(PathBuf, usize)> = Vec::new();
    for base in [home, public].into_iter().flatten() {
        for shallow in ["Downloads", "Documents", "Desktop", "Public/Documents"] {
            out.push((base.join(shallow), 1));
        }
        for deep in [
            "Documents/FS-UAE/Kickstarts",
            "FS-UAE/Kickstarts",
            ".local/share/fs-uae/Kickstarts",
            "Documents/WinUAE/Roms",
            "Documents/WinUAE/ROMs",
            "WinUAE/Roms",
            "Documents/Amiga Files/Shared/rom",
            "Documents/Amiga Files/WinUAE/Roms",
            "Amiga Files/Shared/rom",
            "Amiga Forever/Shared/rom",
            "Documents/Amiga Forever/Shared/rom",
        ] {
            out.push((base.join(deep), 2));
        }
    }
    // Amiga Forever's own, for every user of this computer, and WinUAE's beside its program.
    for fixed in [
        "C:/Users/Public/Documents/Amiga Files/Shared/rom",
        "C:/Users/Public/Documents/Amiga Files/WinUAE/Roms",
        "C:/Program Files/Cloanto/Amiga Forever/Shared/rom",
        "C:/Program Files (x86)/Cloanto/Amiga Forever/Shared/rom",
        "C:/Program Files/WinUAE/Roms",
        "C:/Program Files (x86)/WinUAE/Roms",
    ] {
        out.push((PathBuf::from(fixed), 2));
    }
    // A folder of emulators often has one for the Amiga, with its ROMs in it or in a folder of their own.
    let mut roots: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        roots.extend(["C:\\Emulators", "D:\\Emulators", "E:\\Emulators", "C:\\Games"].map(PathBuf::from));
    }
    if let Some(home) = home {
        roots.push(home.join("Emulators"));
        roots.push(home.join("emulators"));
    }
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        let mut folders: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        folders.sort();
        for folder in folders {
            let name = folder.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if ["amiga", "uae", "kick"].iter().any(|hint| name.contains(hint)) {
                out.push((folder, 2));
            }
        }
    }
    // Each once, even where the same folder is reached by two names (Emulators and emulators, where case is ignored).
    let mut seen: Vec<PathBuf> = Vec::new();
    out.retain(|(p, _)| {
        let Ok(real) = std::fs::canonicalize(p) else { return false };
        let new = real.is_dir() && !seen.contains(&real);
        seen.push(real);
        new
    });
    out
}

/// The folders to look for a rom.key in, for ROMs found in some places: the places, and a Shared/rom in each.
pub fn key_folders(places: &[(PathBuf, usize)]) -> Vec<PathBuf> {
    places.iter().map(|(p, _)| p.clone()).chain(places.iter().map(|(p, _)| p.join("Shared").join("rom"))).collect()
}

fn files_in(folder: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    let mut entries: Vec<std::fs::DirEntry> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            if depth > 1 {
                files_in(&path, depth - 1, out);
            }
        } else if kind.is_file()
            && entry.metadata().is_ok_and(|meta| rom_sized(meta.len()))
            && !out.contains(&path)
        {
            out.push(path);
        }
    }
}

/// The files in some places that are a Kickstart's size, plain or encrypted, each once. Depth 1 is the files in a
/// place; 2 those in its folders too.
pub fn rom_files_in(places: &[(PathBuf, usize)]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for (folder, depth) in places {
        files_in(folder, *depth, &mut files);
    }
    files
}

/// The Kickstarts in some places, with what each is.
pub fn find_kickstarts_in(places: &[(PathBuf, usize)]) -> Vec<(PathBuf, Kickstart)> {
    let keys = key_folders(places);
    rom_files_in(places).into_iter().filter_map(|file| open_rom_file(&file, &keys).ok().map(|rom| (file, describe(&rom)))).collect()
}

/// The best Kickstart found for a model.
pub fn best_for(model: Model, found: &[(PathBuf, Kickstart)]) -> Option<(PathBuf, Kickstart)> {
    found.iter().filter_map(|(path, k)| k.suits.iter().position(|m| *m == model).map(|place| (place, k.rank, path, k))).min_by_key(|(place, rank, ..)| (*place, *rank)).map(|(_, _, path, k)| (path.clone(), k.clone()))
}

/// The name a program goes by on the Amiga's drive: what's safe in AmigaDOS and in a startup-sequence.
fn amiga_name(name: &str) -> String {
    let stem = name.strip_suffix(".exe").unwrap_or(name);
    let safe: String = stem.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')).take(30).collect();
    if safe.is_empty() { "program".to_string() } else { safe }
}

/// Makes the Amiga's hard drive for a program in `folder`: a folder of its own, `dh0`, holding the program under a
/// name AmigaDOS takes and an S/Startup-Sequence that runs it, so the Amiga boots straight into it. WinUAE and
/// FS-UAE are given it as DH0:, and so is PUAE, in Limoniter's window, which takes a folder as a hard drive.
pub fn hard_drive(folder: &Path, run: &Runnable) -> std::io::Result<PathBuf> {
    let drive = folder.join("dh0");
    std::fs::create_dir_all(drive.join("s"))?;
    let name = amiga_name(&run.name);
    std::fs::write(drive.join(&name), run.bytes.as_slice())?;
    std::fs::write(drive.join("s").join("startup-sequence"), format!("{name}\n"))?;
    Ok(drive)
}

/// The name of the configuration Copperline is given for a run, beside the program or disk.
pub const COPPERLINE_CONFIG: &str = "copperline.toml";

/// Makes what a run needs in `folder` (a new folder of its own): the hard drive holding the program and the
/// startup-sequence that runs it (or the disk, for a disk), and the emulator's configuration. Gives the file to
/// start the emulator with. For Copperline, that's the program (or the disk) itself, with its configuration
/// beside it, named `COPPERLINE_CONFIG`.
pub fn prepare(emulator: &str, folder: &Path, run: &Runnable, settings: &Settings) -> Result<PathBuf, String> {
    let failed = |e: std::io::Error| crate::i18n::tf!("The Amiga's drive couldn't be made.\n\n{e}");
    let model = settings.model;
    let kickstart = settings.kickstart(model).map(Path::to_path_buf).or_else(|| best_for(model, &find_kickstarts_in(&usual_places())).map(|(p, _)| p));
    if let Some(rom) = &kickstart
        && !rom.is_file()
    {
        return Err(crate::i18n::tf!("The Kickstart chosen for the {} isn't there: {}.\n\nChoose another in File, Devices, under the emulator.", model.short_name(), rom.display()));
    }
    if emulator == "copperline" {
        return prepare_copperline(folder, run, model, kickstart.as_deref()).map_err(failed);
    }
    let (drive, disk) = match run.format {
        Format::Adf => {
            let disk = folder.join(&run.name);
            std::fs::write(&disk, run.bytes.as_slice()).map_err(failed)?;
            (None, Some(disk))
        }
        _ => (Some(hard_drive(folder, run).map_err(failed)?), None),
    };
    let (file, text) = match emulator {
        "winuae" => {
            let mut lines: Vec<String> = vec!["config_description=Made by Limoniter to run a program".into(), "use_gui=no".into()];
            lines.extend(model.winuae().iter().map(|s| s.to_string()));
            // Without a Kickstart, WinUAE's own AROS.
            lines.push(format!("kickstart_rom_file={}", kickstart.as_ref().map_or_else(|| ":AROS".to_string(), |k| k.display().to_string())));
            if let Some(drive) = &drive {
                lines.push(format!("filesystem2=rw,DH0:Limoniter:{},0", drive.display()));
            }
            if let Some(disk) = &disk {
                lines.push(format!("floppy0={}", disk.display()));
            }
            (folder.join("amiga.uae"), lines.join("\r\n") + "\r\n")
        }
        _ => {
            let mut lines = vec!["[fs-uae]".to_string(), format!("amiga_model = {}", model.short_name())];
            // Without a Kickstart, FS-UAE looks in its own Kickstarts folder, and then uses its own AROS.
            if let Some(rom) = &kickstart {
                lines.push(format!("kickstart_file = {}", rom.display()));
            }
            if let Some(drive) = &drive {
                lines.push(format!("hard_drive_0 = {}", drive.display()));
                lines.push("hard_drive_0_label = Limoniter".into());
            }
            if let Some(disk) = &disk {
                lines.push(format!("floppy_drive_0 = {}", disk.display()));
            }
            (folder.join("amiga.fs-uae"), lines.join("\n") + "\n")
        }
    };
    std::fs::write(&file, text).map_err(failed)?;
    Ok(file)
}

/// Copperline's run: the program (named as AmigaDOS can take it) or the disk in the folder, and beside it a
/// configuration with the machine and its Kickstart. With no Kickstart, it has none, and Copperline boots its own
/// AROS. Being named with --config, the configuration is all there is: Copperline's own saved settings aren't
/// mixed in, so every run is on the same machine.
fn prepare_copperline(folder: &Path, run: &Runnable, model: Model, kickstart: Option<&Path>) -> std::io::Result<PathBuf> {
    let file = match run.format {
        Format::Adf if run.name.to_ascii_lowercase().ends_with(".adf") => folder.join(&run.name),
        Format::Adf => folder.join(format!("{}.adf", amiga_name(&run.name))),
        _ => folder.join(amiga_name(&run.name)),
    };
    std::fs::write(&file, run.bytes.as_slice())?;
    let mut lines = vec![format!("# {}", crate::i18n::tr("Made by Limoniter to run a program"))];
    if let Some(rom) = kickstart {
        lines.push(format!("rom = {}", toml_string(&rom.display().to_string())));
    }
    lines.extend([String::new(), "[machine]".into(), format!("profile = \"{}\"", model.copperline())]);
    std::fs::write(folder.join(COPPERLINE_CONFIG), lines.join("\n") + "\n")?;
    Ok(file)
}

/// Arms Copperline's checks in the configuration made for a run being debugged: its validator of the custom chips'
/// use (writes the chips ignore, read-only registers written, pointers past chip memory, blits started over
/// others) and its detector of code written after it ran, which the Debug panel's warnings read.
pub fn arm_copperline_checks(folder: &Path) -> std::io::Result<()> {
    let file = folder.join(COPPERLINE_CONFIG);
    let mut text = std::fs::read_to_string(&file)?;
    text.push_str("\n[debug]\nvalidate_chipset = true\ndetect_smc = true\n");
    std::fs::write(file, text)
}

/// A string as TOML writes it: in single quotes, where nothing in it is changed (so a Windows path's backslashes
/// are kept), unless it has a single quote of its own, when it's in double quotes with its backslashes and double
/// quotes escaped.
fn toml_string(text: &str) -> String {
    if !text.contains('\'') && !text.chars().any(char::is_control) {
        return format!("'{text}'");
    }
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out + "\""
}

/// The Amiga's own rows in an Amiga emulator's fields, in the Devices box: the model, and its Kickstart.
pub fn settings_rows(ui: &mut Ui, hint: &dyn Fn(&str) -> RichText, emulator: &str) {
    let mut settings = settings();
    let before = settings.clone();
    ui.label(crate::i18n::tr("Amiga:"));
    egui::ComboBox::from_id_salt("amiga-model").width(320.0).selected_text(crate::i18n::tr(settings.model.name())).show_ui(ui, |ui| {
        for model in Model::ALL {
            ui.selectable_value(&mut settings.model, model, crate::i18n::tr(model.name()));
        }
    });
    ui.end_row();
    let model = settings.model;
    ui.label(crate::i18n::tr("Kickstart:"));
    let mut found_note = None;
    ui.horizontal(|ui| {
        let mut text = settings.kickstart(model).map(|p| p.display().to_string()).unwrap_or_default();
        if ui.add(egui::TextEdit::singleline(&mut text).hint_text(crate::i18n::tr("None: the emulator's own")).desired_width(170.0)).changed() {
            settings.set_kickstart(model, (!text.trim().is_empty()).then(|| PathBuf::from(text.trim())));
        }
        if ui.button(crate::i18n::tr("Choose\u{2026}")).on_hover_text(crate::i18n::tf!("The Kickstart ROM for the {}", model.short_name())).clicked()
            && let Some(chosen) = crate::sys_dialogs::file_dialog().set_title(crate::i18n::tf!("Kickstart ROM for the {}", model.short_name())).add_filter(crate::i18n::tr("Kickstart ROM"), &["rom", "bin", "a500", "a1200"]).pick_file()
        {
            settings.set_kickstart(model, Some(chosen));
        }
        if ui.button(crate::i18n::tr("Find")).on_hover_text(crate::i18n::tr("Looks for Kickstarts where Amiga Forever, FS-UAE and WinUAE keep them, and chooses the best for each model")).clicked() {
            let found = find_kickstarts_in(&usual_places());
            let mut chosen = 0;
            for each in Model::ALL {
                if let Some((path, _)) = best_for(each, &found) {
                    settings.set_kickstart(each, Some(path));
                    chosen += 1;
                }
            }
            found_note = Some(match (found.len(), chosen) {
                (0, _) => crate::i18n::tr("No Kickstarts were found. Choose one, or leave it to the emulator.").to_string(),
                (n, 0) => crate::i18n::tf!("{n} found, but none for these models."),
                (n, _) => format!("{n} found."),
            });
        }
    });
    ui.end_row();
    ui.label(crate::i18n::tr(""));
    let rom = settings.kickstart(model);
    let said = match rom {
        Some(path) => match identify_file(path) {
            Some(k) if k.suits.contains(&model) => format!("{}.", k.name),
            Some(k) => crate::i18n::tf!("{}: not made for the {}, so it may not start.", k.name, model.short_name()),
            None if path.is_file() => crate::i18n::tr("That isn't a Kickstart Limoniter knows; the emulator may still take it.").to_string(),
            None => crate::i18n::tr("That file isn't there.").to_string(),
        },
        None if emulator == "winuae" => crate::i18n::tr("None chosen: WinUAE uses its own AROS, a free stand-in for Kickstart that runs system-friendly programs, though not every demo.").to_string(),
        None if emulator == "copperline" => crate::i18n::tr("None chosen: Copperline uses its own AROS, a free stand-in for Kickstart that runs system-friendly programs, though not every demo.").to_string(),
        None => crate::i18n::tr("None chosen: FS-UAE uses one from its Kickstarts folder, or its own AROS, a free stand-in for Kickstart that runs system-friendly programs, though not every demo.").to_string(),
    };
    let said = match found_note {
        Some(note) => format!("{note} {said}"),
        None => said,
    };
    ui.scope(|ui| {
        ui.set_max_width(330.0);
        let drive = if emulator == "copperline" {
            crate::i18n::tf!("{said} The program is started straight from its folder, which the Amiga sees as RunProg: and can write files to. With copperline-ctl beside Copperline, it's debugged there: breakpoints, stepping (backwards too) and the registers, in the Debug panel.")
        } else {
            crate::i18n::tf!("{said} The program runs from a folder that's the Amiga's hard drive, DH0:, which it can write files to.")
        };
        ui.add(egui::Label::new(hint(&drive)).wrap());
    });
    ui.end_row();
    if settings != before {
        set_settings(settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn rom(version: u16, revision: u16, size: usize) -> Vec<u8> {
        let mut bytes = vec![0u8; size];
        bytes[..8].copy_from_slice(&[0x11, 0x11, 0x4E, 0xF9, 0x00, 0xFC, 0x00, 0xD2]);
        bytes[12..14].copy_from_slice(&version.to_be_bytes());
        bytes[14..16].copy_from_slice(&revision.to_be_bytes());
        bytes
    }

    fn encrypt(plain: &[u8], key: &[u8]) -> Vec<u8> {
        let mut locked = ENCRYPTED_MARK.to_vec();
        locked.extend(plain.iter().enumerate().map(|(i, b)| b ^ key[i % key.len()]));
        locked
    }

    #[test]
    fn kickstarts_are_told_by_their_headers() {
        let k13 = identify(&rom(34, 5, 0x40000), &[]).unwrap();
        assert_eq!((k13.name.as_str(), k13.suits.as_slice()), ("Kickstart 1.3 (34.5)", &[Model::A500][..]));
        assert_eq!(identify(&rom(37, 175, 0x80000), &[]).unwrap().suits, [Model::A500Plus, Model::A500]);
        assert!(identify(&rom(40, 68, 0x80000), &[]).unwrap().suits.is_empty(), "a 3.1 not known by its CRC isn't taken for an A1200's");
        assert_eq!(identify(&vec![0; 0x40000], &[]), None, "not a ROM");
        assert_eq!(identify(&rom(34, 5, 1000), &[]), None, "not a ROM's size");
        // A 256 KB ROM stored twice is opened as one, and told the same.
        let doubled = open_rom(&rom(34, 5, 0x40000).repeat(2), &[]).unwrap();
        assert!(doubled.doubled && doubled.bytes.len() == SIZE_256 && (doubled.version, doubled.revision) == (34, 5));
        assert_eq!(identify(&rom(34, 5, 0x40000).repeat(2), &[]), Some(k13));
        // Why something isn't one is said.
        assert!(open_rom(&[0u8; 1000], &[]).unwrap_err().contains("1000 bytes"));
        assert!(open_rom(&vec![0u8; SIZE_256], &[]).unwrap_err().contains("doesn't start"));
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn an_encrypted_rom_is_opened_with_its_key() {
        let plain = rom(34, 5, 0x40000);
        let key = b"not really the key".to_vec();
        let locked = encrypt(&plain, &key);
        // The right key among others opens it; it's then told as the plain one is.
        let opened = open_rom(&locked, &[vec![9, 9, 9], key.clone()]).unwrap();
        assert!(opened.decrypted && opened.bytes == plain);
        assert_eq!(identify(&locked, std::slice::from_ref(&key)), identify(&plain, &[]));
        // No key, or a wrong one, is said plainly.
        assert!(open_rom(&locked, &[]).unwrap_err().contains("rom.key isn't beside"));
        assert!(open_rom(&locked, &[vec![1, 2, 3]]).unwrap_err().contains("doesn't open it"));
        assert_eq!(identify(&locked, &[]), None, "without its key it can't be read");
    }

    #[test]
    fn the_key_is_found_beside_the_rom_or_in_shared_rom() {
        let folder = std::env::temp_dir().join(format!("limoniter-kickstart-keys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let rom = rom(34, 5, SIZE_256);
        let key = vec![0x55, 0xAA, 0x0F];
        std::fs::create_dir_all(folder.join("Shared").join("rom")).unwrap();
        std::fs::create_dir_all(folder.join("Next")).unwrap();
        std::fs::create_dir_all(folder.join("far").join("away").join("Elsewhere")).unwrap();
        std::fs::write(folder.join("Shared").join("rom").join("rom.key"), &key).unwrap();
        std::fs::write(folder.join("Shared").join("rom").join("amiga-os-130.rom"), encrypt(&rom, &key)).unwrap();
        std::fs::write(folder.join("Next").join("kick.rom"), encrypt(&rom, &key)).unwrap();
        let far = folder.join("far").join("away").join("Elsewhere").join("kick.rom");
        std::fs::write(&far, encrypt(&rom, &key)).unwrap();
        // Beside it.
        assert!(open_rom_file(&folder.join("Shared").join("rom").join("amiga-os-130.rom"), &[]).unwrap().decrypted);
        assert_eq!(identify_file(&folder.join("Shared").join("rom").join("amiga-os-130.rom")).unwrap().name, "Kickstart 1.3 (34.5)");
        // In a Shared/rom next to its folder's parent.
        assert!(open_rom_file(&folder.join("Next").join("kick.rom"), &[]).unwrap().decrypted);
        // Nowhere near one, until it's told where to look.
        assert!(open_rom_file(&far, &[]).is_err());
        assert!(open_rom_file(&far, &[folder.join("Shared").join("rom")]).unwrap().decrypted);
        // A file that isn't a Kickstart's size isn't read, and says so.
        std::fs::write(folder.join("small.rom"), [0u8; 1000]).unwrap();
        assert!(open_rom_file(&folder.join("small.rom"), &[]).unwrap_err().contains("1000 bytes"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_best_kickstart_is_chosen_for_each_model() {
        let named = |name: &str, suits: &[Model], rank| (PathBuf::from(name), Kickstart { name: name.into(), suits: suits.to_vec(), rank });
        let found = vec![named("kick31.rom", &[Model::A500Plus, Model::A500], 3), named("kick13.rom", &[Model::A500, Model::A500Plus], 0), named("kick204.rom", &[Model::A500Plus, Model::A500], 0), named("cd32.rom", &[], 9)];
        assert_eq!(best_for(Model::A500, &found).unwrap().0, PathBuf::from("kick13.rom"));
        assert_eq!(best_for(Model::A500Plus, &found).unwrap().0, PathBuf::from("kick204.rom"));
        assert_eq!(best_for(Model::A1200, &found), None);
    }

    #[test]
    fn settings_are_kept_as_text() {
        let mut settings = Settings { model: Model::A1200, kickstarts: Vec::new() };
        settings.set_kickstart(Model::A1200, Some(PathBuf::from("C:\\Amiga Files\\kick 31.rom")));
        settings.set_kickstart(Model::A500, Some(PathBuf::from("/roms/kick13.rom")));
        assert_eq!(Settings::from_text(&settings.to_text()), settings);
        assert_eq!(Settings::from_text("model=nonsense\n"), Settings::default());
    }

    #[test]
    fn a_run_is_a_hard_drive_that_starts_the_program_and_a_configuration() {
        let folder = std::env::temp_dir().join(format!("limoniter-amiga-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let program = Runnable { machine: crate::running::Machine::Amiga, format: Format::Hunk, name: "my demo!.exe".into(), bytes: Arc::new(vec![0, 0, 3, 0xF3]) };
        let mut settings = Settings { model: Model::A1200, kickstarts: Vec::new() };
        let rom = folder.join("kick.rom");
        std::fs::write(&rom, rom_bytes()).unwrap();
        settings.set_kickstart(Model::A1200, Some(rom.clone()));
        let config = prepare("fs-uae", &folder, &program, &settings).unwrap();
        let text = std::fs::read_to_string(&config).unwrap();
        assert!(text.starts_with("[fs-uae]\namiga_model = A1200\n"), "{text}");
        assert!(text.contains(&format!("kickstart_file = {}", rom.display())) && text.contains(&format!("hard_drive_0 = {}", folder.join("dh0").display())), "{text}");
        assert_eq!(std::fs::read_to_string(folder.join("dh0/s/startup-sequence")).unwrap(), "mydemo\n");
        assert_eq!(std::fs::read(folder.join("dh0/mydemo")).unwrap(), [0, 0, 3, 0xF3]);
        // WinUAE's, with no Kickstart chosen for the A500: its own AROS (unless one is found on this computer).
        settings.model = Model::A500;
        let config = prepare("winuae", &folder, &program, &settings).unwrap();
        let text = std::fs::read_to_string(config).unwrap();
        assert!(text.contains("chipset=ocs\r\n") && text.contains(&format!("filesystem2=rw,DH0:Limoniter:{},0", folder.join("dh0").display())), "{text}");
        assert!(text.contains("kickstart_rom_file="));
        // A disk goes in the drive instead.
        let disk = Runnable { machine: crate::running::Machine::Amiga, format: Format::Adf, name: "game.adf".into(), bytes: Arc::new(vec![0; 901_120]) };
        let text = std::fs::read_to_string(prepare("fs-uae", &folder, &disk, &settings).unwrap()).unwrap();
        assert!(text.contains(&format!("floppy_drive_0 = {}", folder.join("game.adf").display())) && !text.contains("hard_drive_0"), "{text}");
        // Copperline's: the program in the folder, named as AmigaDOS takes it, and the machine beside it.
        settings.model = Model::A500Plus;
        settings.set_kickstart(Model::A500Plus, Some(rom.clone()));
        let program_file = prepare("copperline", &folder, &program, &settings).unwrap();
        assert_eq!(program_file, folder.join("mydemo"));
        assert_eq!(std::fs::read(&program_file).unwrap(), [0, 0, 3, 0xF3]);
        let text = std::fs::read_to_string(folder.join(COPPERLINE_CONFIG)).unwrap();
        assert!(text.contains(&format!("rom = '{}'\n", rom.display())) && text.ends_with("\n[machine]\nprofile = \"A500Plus\"\n"), "{text}");
        let disk_file = prepare("copperline", &folder, &disk, &settings).unwrap();
        assert_eq!(disk_file, folder.join("game.adf"));
        // A Kickstart that's gone is said to be.
        settings.model = Model::A500;
        settings.set_kickstart(Model::A500, Some(folder.join("gone.rom")));
        assert!(prepare("fs-uae", &folder, &program, &settings).unwrap_err().contains("isn't there"));
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn copperlines_configuration_is_toml() {
        assert_eq!(toml_string("C:\\Amiga Files\\kick13.rom"), "'C:\\Amiga Files\\kick13.rom'", "a Windows path as it is");
        assert_eq!(toml_string("/roms/Jo's \"kick\" \\1.3.rom"), "\"/roms/Jo's \\\"kick\\\" \\\\1.3.rom\"");
        // With no Kickstart, there's no rom line: Copperline boots its own AROS.
        let folder = std::env::temp_dir().join(format!("limoniter-copperline-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let program = Runnable { machine: crate::running::Machine::Amiga, format: Format::Hunk, name: "demo".into(), bytes: Arc::new(vec![0, 0, 3, 0xF3]) };
        prepare_copperline(&folder, &program, Model::A1200, None).unwrap();
        let text = std::fs::read_to_string(folder.join(COPPERLINE_CONFIG)).unwrap();
        assert!(!text.contains("rom") && text.contains("profile = \"A1200\""), "{text}");
        // Debugged, its checks are armed.
        arm_copperline_checks(&folder).unwrap();
        let text = std::fs::read_to_string(folder.join(COPPERLINE_CONFIG)).unwrap();
        assert!(text.ends_with("profile = \"A1200\"\n\n[debug]\nvalidate_chipset = true\ndetect_smc = true\n"), "{text}");
        let _ = std::fs::remove_dir_all(&folder);
    }

    fn rom_bytes() -> Vec<u8> {
        rom(40, 68, 0x80000)
    }

    #[test]
    fn kickstarts_are_found_in_the_usual_places() {
        let home = std::env::temp_dir().join(format!("limoniter-kickstarts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let roms = home.join("Documents/FS-UAE/Kickstarts");
        std::fs::create_dir_all(roms.join("old")).unwrap();
        std::fs::create_dir_all(home.join("Downloads/deeper")).unwrap();
        std::fs::create_dir_all(home.join("Emulators/WinUAE 5/Roms")).unwrap();
        std::fs::write(roms.join("amiga-os-130.rom"), rom(34, 5, 0x40000)).unwrap();
        std::fs::write(roms.join("notes.txt"), "not a ROM").unwrap();
        std::fs::write(roms.join("blank.rom"), vec![0u8; 0x40000]).unwrap();
        // A folder of ROMs is looked in a folder deep; Downloads only at the top, since it holds much else.
        std::fs::write(roms.join("old/kick204.rom"), rom(37, 175, 0x80000)).unwrap();
        std::fs::write(home.join("Downloads/deeper/kick31.rom"), rom(40, 68, 0x80000)).unwrap();
        // An Amiga emulator's folder among the emulators.
        std::fs::write(home.join("Emulators/WinUAE 5/Roms/kick12.rom"), rom(33, 180, 0x40000)).unwrap();
        let places = places(Some(&home), None);
        assert!(places.contains(&(home.join("Downloads"), 1)) && places.contains(&(roms.clone(), 2)) && places.contains(&(home.join("Emulators/WinUAE 5"), 2)), "{places:?}");
        assert!(!places.iter().any(|(p, _)| p.ends_with("Desktop")), "only folders that are there");
        let found = find_kickstarts_in(&places);
        let files: Vec<&Path> = found.iter().map(|(p, _)| p.as_path()).collect();
        assert_eq!(files, [roms.join("amiga-os-130.rom").as_path(), roms.join("old/kick204.rom").as_path(), home.join("Emulators/WinUAE 5/Roms/kick12.rom").as_path()]);
        assert_eq!(best_for(Model::A500, &found).map(|(p, _)| p), Some(roms.join("amiga-os-130.rom")));
        assert_eq!(best_for(Model::A500Plus, &found).map(|(p, _)| p), Some(roms.join("old/kick204.rom")));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_programs_hard_drive_starts_it() {
        let folder = std::env::temp_dir().join(format!("limoniter-amiga-drive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let program = Runnable { machine: crate::running::Machine::Amiga, format: Format::Hunk, name: "Copper bars (v2).exe".into(), bytes: Arc::new(vec![0, 0, 3, 0xF3]) };
        let drive = hard_drive(&folder, &program).unwrap();
        assert_eq!(drive, folder.join("dh0"));
        assert_eq!(std::fs::read_to_string(drive.join("s/startup-sequence")).unwrap(), "Copperbarsv2\n");
        assert_eq!(std::fs::read(drive.join("Copperbarsv2")).unwrap(), [0, 0, 3, 0xF3]);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
