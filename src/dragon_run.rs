//! Running a Dragon program in XRoar from a Dragon DOS disk, as it would be run on a real Dragon with a disk
//! drive, rather than loaded straight into memory.
//!
//! The program is put on a fresh disk (a VDK file) as NAME.BIN, and XRoar is started with the DragonDOS
//! cartridge, the disk in its first drive and the command to run it typed in. Each option is as XRoar 1.13's
//! manual gives it (https://www.6809.org.uk/xroar/doc/xroar.shtml):
//!
//! - `-cart dragondos` is the built-in DragonDOS cartridge profile (section 4.2.1), which takes ddos10.rom,
//!   sdose6.rom or dplus49b.rom; given after `-machine`, it's the cartridge attached to that machine (2.6).
//! - `-load-fd0 file` loads a disk image into drive 0 (6.6), which DragonDOS calls drive 1, the one it uses
//!   when no drive is named. (Given to `-run`, a disk only has BOOT typed for it (3.3), which needs a boot track.)
//! - `-type string` intercepts the ROM's calls to type the string into BASIC on startup (6.8); `\r` in it is
//!   ENTER (the escape sequences in 6), and quote marks are kept as they are.
//!
//! What's typed is DragonDOS's own: RUN"NAME" loads a BASIC program (NAME.BAS) and runs it (Introducing Dragon
//! DOS, page 7), and RUN"NAME.BIN" loads machine code and starts it at its EXEC address, as LOAD"NAME.BIN"
//! then EXEC would (pages 8 and 50; RUN taking binaries too is as Alastair Fiddes says on the World of Dragon
//! forum, 21 November 2019).

use crate::dragon_dos::{self, Disk};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// How a Dragon program goes into XRoar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// The Dragon DOS binary, loaded straight into memory and started by XRoar.
    Binary,
    /// On a Dragon DOS disk made for the run, loaded and started by DragonDOS's RUN.
    Disk,
}

/// What the source says, near its top: ; run from: disk (or binary).
pub fn said<'a>(lines: impl IntoIterator<Item = &'a str>) -> Option<Start> {
    lines.into_iter().take(50).find_map(|line| {
        let line = line.trim_start();
        let comment = line.strip_prefix(';').or_else(|| line.strip_prefix('*')).or_else(|| line.strip_prefix("//"))?;
        let comment = comment.trim_start_matches([';', '/', '*', ' ', '\t']).to_ascii_lowercase();
        let value = comment.strip_prefix("run from")?.trim_start().strip_prefix(':')?.trim();
        match value.split_whitespace().next()? {
            "disk" | "disc" | "vdk" => Some(Start::Disk),
            "binary" | "bin" | "memory" => Some(Start::Binary),
            _ => None,
        }
    })
}

/// Whether a program from this source goes on a disk: as it says, or else as chosen for XRoar.
pub fn wants_disk<'a>(lines: impl IntoIterator<Item = &'a str>) -> bool {
    said(lines).unwrap_or(settings().start) == Start::Disk
}

/// A name for a file as Dragon DOS has them: up to eight capital letters and digits, starting with a letter.
pub fn disk_name(stem: &str) -> String {
    let letters: String = stem.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_uppercase();
    if letters.is_empty() {
        return "PROGRAM".into();
    }
    let letters = if letters.starts_with(|c: char| c.is_ascii_alphabetic()) { letters } else { format!("P{letters}") };
    letters.chars().take(8).collect()
}

/// A fresh 40-track, single-sided disk (as DSKINIT makes by default) holding a Dragon DOS binary as NAME.BIN:
/// the VDK file's name (from `stem`) and its bytes.
pub fn disk_holding(stem: &str, binary: &[u8]) -> Result<(String, Vec<u8>), String> {
    let mut disk = Disk::new(40, 1);
    disk.add(&disk_name(stem), "BIN", binary)?;
    Ok((format!("{stem}.vdk"), disk.to_vdk()))
}

/// What's typed to run the program on a disk: the first binary on it (one with Dragon DOS's header) with RUN, or
/// else the first BASIC program. None if there's neither.
pub fn typed_command(disk: &Disk) -> Option<String> {
    let files = disk.files();
    if let Some(binary) = files.iter().find(|f| f.ending == "BIN" && dragon_dos::binary_header(&f.bytes).is_some()) {
        return Some(format!("RUN\"{}.BIN\"\\r", binary.name));
    }
    let basic = files.iter().find(|f| f.ending == "BAS")?;
    Some(format!("RUN\"{}\"\\r", basic.name))
}

/// XRoar's options for a run from a disk: the DragonDOS cartridge, the disk in drive 1 and the command typed.
/// A disk with nothing Limoniter can tell to run is booted, as XRoar's own -run does.
pub fn disk_arguments(file: &Path) -> Vec<String> {
    let named = file.display().to_string();
    let typed = std::fs::read(file).ok().and_then(|bytes| Disk::from_vdk(&bytes)).and_then(|disk| typed_command(&disk));
    let mut args = vec!["-cart".to_string(), "dragondos".to_string()];
    match typed {
        Some(typed) => args.extend(["-load-fd0".into(), named, "-type".into(), typed]),
        None => args.extend(["-run".into(), named]),
    }
    args
}

// ---------- The setting ----------

/// How runs in XRoar are made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub start: Start,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { start: Start::Binary }
    }
}

impl Settings {
    fn to_text(&self) -> String {
        format!("dragon_start={}\n", if self.start == Start::Disk { "disk" } else { "binary" })
    }

    fn from_text(text: &str) -> Settings {
        let mut settings = Settings::default();
        for line in text.lines() {
            if let Some(("dragon_start", value)) = line.split_once('=') {
                settings.start = if value.trim() == "disk" { Start::Disk } else { Start::Binary };
            }
        }
        settings
    }
}

/// Where they're kept, beside Limoniter's settings.
fn settings_file() -> Option<PathBuf> {
    crate::settings::data_folder().map(|dir| dir.join("xroar.txt"))
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

/// XRoar's own rows in its fields, in the Devices box: how a Dragon program goes in.
pub fn settings_rows(ui: &mut egui::Ui, hint: &dyn Fn(&str) -> egui::RichText) {
    let mut settings = settings();
    let before = settings.clone();
    ui.label(crate::i18n::tr("Dragon programs:"));
    ui.horizontal(|ui| {
        ui.radio_value(&mut settings.start, Start::Binary, crate::i18n::tr("Loaded into memory"));
        ui.radio_value(&mut settings.start, Start::Disk, crate::i18n::tr("From a Dragon DOS disk"));
    });
    ui.end_row();
    ui.label(crate::i18n::tr(""));
    let words = match settings.start {
        Start::Binary => crate::i18n::tr("The Dragon DOS binary F9 makes is loaded straight into memory by XRoar and started. Source can say otherwise near its top, with ; run from: disk."),
        Start::Disk => crate::i18n::tr("The program is put on a Dragon DOS disk made for the run, as NAME.BIN, and XRoar starts with the DragonDOS cartridge (it needs its ROM) and types RUN\"NAME.BIN\" for it. Source can say otherwise near its top, with ; run from: binary."),
    };
    ui.scope(|ui| {
        ui.set_max_width(330.0);
        ui.add(egui::Label::new(hint(words)).wrap());
    });
    ui.end_row();
    if settings != before {
        set_settings(settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_says_where_it_runs_from() {
        assert_eq!(said(["; machine: dragon32", "; run from: disk", "\torg $3000"]), Some(Start::Disk));
        assert_eq!(said(["* Run From: Binary"]), Some(Start::Binary));
        assert_eq!(said(["\t; run from:disc"]), Some(Start::Disk));
        assert_eq!(said(["; run from: somewhere"]), None);
        assert_eq!(said(["\tlda #1 ; run from: disk"]), None, "only a comment on its own line");
        // Said, it's what happens whatever's chosen; unsaid, it's as chosen (in tests, the binary).
        assert!(wants_disk(["; run from: disk"]));
        assert!(!wants_disk(["\torg $3000"]));
    }

    #[test]
    fn names_are_dragon_doss() {
        assert_eq!(disk_name("hello"), "HELLO");
        assert_eq!(disk_name("my-game_v2.final"), "MYGAMEV2");
        assert_eq!(disk_name("3d maze"), "P3DMAZE");
        assert_eq!(disk_name("éé"), "PROGRAM");
        assert_eq!(disk_name("p"), "P");
    }

    #[test]
    fn the_disk_holds_the_binary_and_says_how_to_run_it() {
        let binary = dragon_dos::binary(0x3000, 0x3004, &[0x86, 0x41, 0x39, 0x00, 0x12, 0x39]);
        let (name, vdk) = disk_holding("demo", &binary).unwrap();
        assert_eq!(name, "demo.vdk");
        // Read back with Limoniter's own reader: one file, the binary as it was.
        let disk = Disk::from_vdk(&vdk).expect("a Dragon DOS disk");
        assert_eq!((disk.tracks, disk.sides), (40, 1));
        assert_eq!(vdk.len(), 12 + 40 * 18 * 256);
        let files = disk.files();
        assert_eq!(files.len(), 1);
        assert_eq!((files[0].name.as_str(), files[0].ending.as_str()), ("DEMO", "BIN"));
        assert_eq!(files[0].bytes, binary);
        assert_eq!(typed_command(&disk).as_deref(), Some("RUN\"DEMO.BIN\"\\r"));
        // A BASIC program is run by its name alone; a binary comes first; with neither, nothing's typed.
        let mut basic = Disk::new(40, 1);
        basic.add("notes", "TXT", b"hello").unwrap();
        assert_eq!(typed_command(&basic), None);
        basic.add("game", "BAS", &[0x55, 0x01, 0x1E, 0x01, 0, 4, 0x8B, 0x8D, 0xAA, 0, 0, 0, 0]).unwrap();
        assert_eq!(typed_command(&basic).as_deref(), Some("RUN\"GAME\"\\r"));
        basic.add("loader", "BIN", &binary).unwrap();
        assert_eq!(typed_command(&basic).as_deref(), Some("RUN\"LOADER.BIN\"\\r"));
    }

    #[test]
    fn xroar_is_given_the_cartridge_the_disk_and_the_command() {
        let folder = std::env::temp_dir().join(format!("limoniter-dragon-run-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let (name, vdk) = disk_holding("demo", &dragon_dos::binary(0x3000, 0x3000, &[0x39])).unwrap();
        let file = folder.join(name);
        std::fs::write(&file, vdk).unwrap();
        let named = file.display().to_string();
        assert_eq!(disk_arguments(&file), ["-cart", "dragondos", "-load-fd0", named.as_str(), "-type", "RUN\"DEMO.BIN\"\\r"]);
        // An empty disk is booted, as XRoar does with a disk it's asked to run.
        std::fs::write(&file, Disk::new(40, 1).to_vdk()).unwrap();
        assert_eq!(disk_arguments(&file), ["-cart", "dragondos", "-run", named.as_str()]);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_setting_is_kept_as_text() {
        let disk = Settings { start: Start::Disk };
        assert_eq!(Settings::from_text(&disk.to_text()), disk);
        assert_eq!(Settings::from_text(&Settings::default().to_text()), Settings::default());
        assert_eq!(Settings::from_text("something=else\n"), Settings::default());
    }
}
