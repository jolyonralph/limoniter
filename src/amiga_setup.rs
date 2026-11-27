//! Setting up the Amiga for F9 (File, Run on, Set up the Amiga): what PUAE, the libretro core that runs Amiga
//! programs in Limoniter's window, needs besides itself, which is a Kickstart ROM under the file name PUAE looks
//! for, or AROS, the free replacement it has built in.
//!
//! Kickstarts are recognised by what is in them, not by what they're called, as amiga_run.rs opens them for every
//! emulator: the header, the size, and the version and revision exec has in them at offset 12, Amiga Forever's
//! encrypted ROMs decrypted with its `rom.key`. Here they're matched to the names PUAE looks for. Nothing here
//! changes or moves the original files: they're only read, and a copy is written into Limoniter's own system folder
//! for the cores.

use crate::amiga_run::{SIZE_256, SIZE_512};
use std::path::{Path, PathBuf};
/// The core option that says which Kickstart PUAE uses, and its value for the built-in AROS.
pub const OPTION_KICKSTART: &str = "puae_kickstart";
pub const OPTION_MODEL: &str = "puae_model";
pub const AROS: &str = "aros";
/// The core's name in Limoniter's tables.
pub const CORE: &str = "puae";

/// A Kickstart PUAE knows, and what suits it.
#[derive(Debug)]
pub struct Known {
    pub version: u16,
    pub revision: u16,
    pub size: usize,
    /// The name PUAE looks for in the system folder (libretro-core.h, `uae_kickstarts`).
    pub file: &'static str,
    /// What it's called when listed, such as "Kickstart 1.3 (A500)".
    pub name: &'static str,
    /// The PUAE model (the `puae_model` option's value) that suits it.
    pub model: &'static str,
    /// Whether the model alone makes PUAE use it. If not, the Kickstart option must name its file.
    pub by_model: bool,
    /// Lower is liked better when more than one is installed, since most Amiga programs on disks want 1.3 or 3.1.
    pub rank: u8,
}

const fn known(version: u16, revision: u16, size: usize, file: &'static str, name: &'static str, model: &'static str, by_model: bool, rank: u8) -> Known {
    Known { version, revision, size, file, name, model, by_model, rank }
}

/// The Kickstarts PUAE looks for by name. The 3.0 and 3.1 ROMs of the A4000 have the same version as the A1200's
/// and can't be told apart by it, so those are taken for the A1200's. The CD32's extended ROM has no header and
/// is refused.
pub const KNOWN: &[Known] = &[
    known(34, 5, SIZE_256, "kick34005.A500", "Kickstart 1.3 (A500, A1000, A2000)", "A500", true, 0),
    known(40, 63, SIZE_512, "kick40063.A600", "Kickstart 3.1 (A500, A600, A2000)", "A600", true, 1),
    known(40, 68, SIZE_512, "kick40068.A1200", "Kickstart 3.1 (A1200)", "A1200", true, 2),
    known(37, 175, SIZE_512, "kick37175.A500", "Kickstart 2.04 (A500+)", "A500PLUS", true, 3),
    known(39, 106, SIZE_512, "kick39106.A1200", "Kickstart 3.0 (A1200)", "A1200", false, 4),
    known(37, 350, SIZE_512, "kick37350.A600", "Kickstart 2.05 (A600)", "A600OG", false, 5),
    known(33, 180, SIZE_256, "kick33180.A500", "Kickstart 1.2 (A500, A1000, A2000)", "A500", false, 6),
    known(32, 34, SIZE_256, "kick32034.A1000", "Kickstart 1.1 PAL (A1000)", "A500", false, 7),
    known(31, 34, SIZE_256, "kick31034.A1000", "Kickstart 1.1 NTSC (A1000)", "A500", false, 8),
    known(40, 60, SIZE_512, "kick40060.CD32", "Kickstart 3.1 (CD32)", "CD32", true, 9),
];

/// A Kickstart that was recognised, with the bytes to write for it.
#[derive(Debug)]
pub struct Rom {
    pub known: &'static Known,
    /// The ROM as it's to be written: decrypted, and a doubled 256 KB image cut back to one copy.
    pub bytes: Vec<u8>,
    pub decrypted: bool,
    pub doubled: bool,
}

/// Works out what a file is, if it's a Kickstart PUAE looks for, or says plainly why it isn't. `keys` are the
/// contents of any `rom.key` files found for it, tried in turn if the ROM is encrypted.
pub fn recognise(bytes: &[u8], keys: &[Vec<u8>]) -> Result<Rom, String> {
    known_for(crate::amiga_run::open_rom(bytes, keys)?)
}

/// What PUAE calls an opened Kickstart, by its version and revision.
fn known_for(rom: crate::amiga_run::OpenedRom) -> Result<Rom, String> {
    let (version, revision) = (rom.version, rom.revision);
    let Some(known) = KNOWN.iter().find(|k| k.version == version && k.revision == revision) else {
        return Err(crate::i18n::tf!("It's a Kickstart, version {version}.{revision}, but not one PUAE looks for (it takes 1.1 to 3.1 for the A500, A600, A1200 and CD32)."));
    };
    if known.size != rom.bytes.len() {
        return Err(crate::i18n::tf!("It's Kickstart {version}.{revision}, but not the size that one is ({} bytes), so it's damaged or not a whole ROM.", known.size));
    }
    Ok(Rom { known, bytes: rom.bytes, decrypted: rom.decrypted, doubled: rom.doubled })
}

/// Reads a file and says what it is. Files that aren't a Kickstart's size are refused without reading them.
pub fn read_rom(file: &Path, more_keys: &[PathBuf]) -> Result<Rom, String> {
    known_for(crate::amiga_run::open_rom_file(file, more_keys)?)
}

/// Writes a recognised ROM into the system folder under PUAE's name for it. Whether anything was written: a copy
/// that's already there, byte for byte, is left alone.
pub fn write_rom(rom: &Rom, system: &Path) -> Result<bool, String> {
    let target = system.join(rom.known.file);
    if std::fs::read(&target).is_ok_and(|have| have == rom.bytes) {
        return Ok(false);
    }
    std::fs::create_dir_all(system).map_err(|e| crate::i18n::tf!("{} couldn't be made: {e}", system.display()))?;
    std::fs::write(&target, &rom.bytes).map_err(|e| crate::i18n::tf!("{} couldn't be written: {e}", target.display()))?;
    Ok(true)
}

/// A Kickstart already in the system folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub file: &'static str,
    pub name: &'static str,
    pub model: &'static str,
    pub by_model: bool,
    pub rank: u8,
}

impl From<&'static Known> for Installed {
    fn from(k: &'static Known) -> Self {
        Installed { file: k.file, name: k.name, model: k.model, by_model: k.by_model, rank: k.rank }
    }
}

/// The Kickstarts in the system folder under PUAE's names, judged by what's in them.
pub fn installed(system: &Path) -> Vec<Installed> {
    KNOWN.iter().filter(|k| std::fs::read(system.join(k.file)).ok().and_then(|b| recognise(&b, &[]).ok()).is_some_and(|r| r.known.file == k.file)).map(Installed::from).collect()
}

/// What PUAE is told so that its model suits a Kickstart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub model: String,
    /// The value for the Kickstart option: "auto" lets the model pick, or a file name, or AROS.
    pub kickstart: String,
    /// What's said about the choice.
    pub why: String,
}

/// The model and Kickstart option to use, given the Kickstarts installed: the best liked one, or AROS if there are
/// none and it's wanted. None if there's nothing to run on.
pub fn choose(have: &[Installed], aros: bool) -> Option<Choice> {
    if let Some(best) = have.iter().min_by_key(|i| i.rank) {
        let kickstart = if best.by_model { "auto".to_string() } else { best.file.to_string() };
        return Some(Choice { model: best.model.to_string(), kickstart, why: crate::i18n::tf!("{} (PUAE's {} model)", best.name, best.model) });
    }
    // AROS needs the room of a bigger Amiga.
    aros.then(|| Choice { model: "A1200".into(), kickstart: AROS.into(), why: crate::i18n::tr("AROS (PUAE's A1200 model)").into() })
}

/// Puts a choice into the options remembered for PUAE.
pub fn apply(prefs: &mut crate::retro::Prefs, choice: &Choice) {
    prefs.set_option(CORE, OPTION_MODEL, &choice.model);
    prefs.set_option(CORE, OPTION_KICKSTART, &choice.kickstart);
}

/// Whether AROS is what PUAE is set to use.
pub fn aros_chosen(prefs: &crate::retro::Prefs) -> bool {
    prefs.options_for(CORE).iter().any(|(k, v)| k == OPTION_KICKSTART && v == AROS)
}

/// Whether PUAE has a Kickstart to start with: AROS chosen, or a file in the system folder named the way PUAE
/// looks (its own names, Amiga Forever's amiga-os-*.rom or a TOSEC "Kickstart v..." one). Easy on purpose: only a
/// folder with nothing like one in it is turned away.
pub fn kickstart_ready(system: Option<&Path>, prefs: &crate::retro::Prefs) -> bool {
    let Some(system) = system else { return true };
    if aros_chosen(prefs) {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(system) else { return false };
    entries.flatten().any(|e| {
        let name = e.file_name().to_string_lossy().to_lowercase();
        name.starts_with("kick") || name.starts_with("amiga-os") || name.starts_with("ks rom")
    })
}

/// A Kickstart found on this computer, or picked by hand.
pub struct Found {
    pub path: PathBuf,
    /// What it is, or why it isn't a Kickstart.
    pub what: Result<&'static Known, String>,
    pub decrypted: bool,
    pub doubled: bool,
    pub ticked: bool,
}

impl Found {
    pub fn read(path: &Path, more_keys: &[PathBuf]) -> Found {
        match read_rom(path, more_keys) {
            Ok(rom) => Found { path: path.to_path_buf(), what: Ok(rom.known), decrypted: rom.decrypted, doubled: rom.doubled, ticked: true },
            Err(why) => Found { path: path.to_path_buf(), what: Err(why), decrypted: false, doubled: false, ticked: false },
        }
    }
}

/// The Kickstarts in the given places that aren't already in the system folder, each once, the best liked first.
/// Files that aren't Kickstarts are passed over without a word.
pub fn scan(places: &[(PathBuf, usize)], system: Option<&Path>) -> Vec<Found> {
    let key_folders = crate::amiga_run::key_folders(places);
    let mut found: Vec<Found> = Vec::new();
    for file in crate::amiga_run::rom_files_in(places) {
        if system.is_some_and(|s| file.starts_with(s)) {
            continue;
        }
        let one = Found::read(&file, &key_folders);
        let Ok(known) = one.what else { continue };
        let there = system.is_some_and(|s| s.join(known.file).exists());
        if !there && !found.iter().any(|f| f.what.as_ref().is_ok_and(|k| k.file == known.file)) {
            found.push(one);
        }
    }
    found.sort_by_key(|f| f.what.as_ref().map_or(u8::MAX, |k| k.rank));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up ROM of the given size: the header, the version and revision, and filler. Not a real Kickstart.
    fn made_up(size: usize, version: u16, revision: u16) -> Vec<u8> {
        let mut rom = vec![0xA5u8; size];
        rom[0] = 0x11;
        rom[1] = if size == SIZE_256 { 0x11 } else { 0x14 };
        rom[2] = 0x4E;
        rom[3] = 0xF9;
        rom[12..14].copy_from_slice(&version.to_be_bytes());
        rom[14..16].copy_from_slice(&revision.to_be_bytes());
        rom
    }

    fn encrypt(rom: &[u8], key: &[u8]) -> Vec<u8> {
        let mut out = crate::amiga_run::ENCRYPTED_MARK.to_vec();
        out.extend(rom.iter().zip(key.iter().cycle()).map(|(b, k)| b ^ k));
        out
    }

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("limoniter-amiga-setup-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn kickstarts_are_told_by_their_contents() {
        let one_three = recognise(&made_up(SIZE_256, 34, 5), &[]).unwrap();
        assert_eq!(one_three.known.file, "kick34005.A500");
        assert_eq!(one_three.known.model, "A500");
        assert_eq!(recognise(&made_up(SIZE_512, 40, 63), &[]).unwrap().known.file, "kick40063.A600");
        assert_eq!(recognise(&made_up(SIZE_512, 40, 68), &[]).unwrap().known.file, "kick40068.A1200");
        assert_eq!(recognise(&made_up(SIZE_512, 40, 60), &[]).unwrap().known.file, "kick40060.CD32");
        // Amiga Forever's, opened with its key (amiga_run.rs tests the opening itself).
        let key = vec![0x13, 0x37, 0xAB, 0x01, 0x77];
        let opened = recognise(&encrypt(&made_up(SIZE_512, 40, 68), &key), &[key]).unwrap();
        assert!(opened.decrypted);
        assert_eq!(opened.known.file, "kick40068.A1200");
    }

    #[test]
    fn a_256_kb_rom_stored_twice_is_written_once() {
        let single = made_up(SIZE_256, 34, 5);
        let twice = [single.clone(), single.clone()].concat();
        let rom = recognise(&twice, &[]).unwrap();
        assert!(rom.doubled);
        assert_eq!(rom.known.file, "kick34005.A500");
        assert_eq!(rom.bytes, single);
        // Two halves that differ are a 512 KB ROM, and one that has no 512 KB version is refused.
        let mut unequal = twice;
        unequal[SIZE_256 + 100] ^= 1;
        assert!(recognise(&unequal, &[]).is_err());
    }

    #[test]
    fn what_isnt_a_kickstart_is_refused_and_why() {
        assert!(recognise(&vec![0u8; 1000], &[]).unwrap_err().contains("1000 bytes"));
        assert!(recognise(&vec![0u8; SIZE_256], &[]).unwrap_err().contains("doesn't start"));
        // The right header and size but a version nothing looks for.
        assert!(recognise(&made_up(SIZE_256, 99, 9), &[]).unwrap_err().contains("99.9"));
        // A known version at the wrong size.
        let mut wrong = made_up(SIZE_512, 34, 5);
        wrong[SIZE_256] ^= 0xFF;
        assert!(recognise(&wrong, &[]).unwrap_err().contains("not the size"));
    }

    #[test]
    fn roms_are_written_under_puaes_names_and_not_twice() {
        let system = scratch("write").join("libretro-system");
        let rom = recognise(&made_up(SIZE_512, 40, 68), &[]).unwrap();
        assert_eq!(write_rom(&rom, &system), Ok(true));
        assert_eq!(std::fs::read(system.join("kick40068.A1200")).unwrap(), rom.bytes);
        assert_eq!(write_rom(&rom, &system), Ok(false));
        assert_eq!(installed(&system).iter().map(|i| i.file).collect::<Vec<_>>(), ["kick40068.A1200"]);
        // A file with the right name but other contents isn't taken for it.
        std::fs::write(system.join("kick34005.A500"), vec![1u8; 100]).unwrap();
        assert_eq!(installed(&system).len(), 1);
        let _ = std::fs::remove_dir_all(system.parent().unwrap());
    }

    #[test]
    fn the_model_follows_the_kickstart_best_liked() {
        let have = |files: &[&str]| -> Vec<Installed> { KNOWN.iter().filter(|k| files.contains(&k.file)).map(Installed::from).collect() };
        let one_three = choose(&have(&["kick34005.A500"]), false).unwrap();
        assert_eq!((one_three.model.as_str(), one_three.kickstart.as_str()), ("A500", "auto"));
        let a1200 = choose(&have(&["kick40068.A1200"]), false).unwrap();
        assert_eq!((a1200.model.as_str(), a1200.kickstart.as_str()), ("A1200", "auto"));
        // 1.3 beats 3.1 where both are there; one the model can't pick by itself is named.
        assert_eq!(choose(&have(&["kick40068.A1200", "kick34005.A500"]), false).unwrap().model, "A500");
        assert_eq!(choose(&have(&["kick33180.A500"]), false).unwrap().kickstart, "kick33180.A500");
        // AROS only when there's nothing else, and only if asked for.
        assert_eq!(choose(&[], false), None);
        let aros = choose(&[], true).unwrap();
        assert_eq!((aros.model.as_str(), aros.kickstart.as_str()), ("A1200", "aros"));
        assert_eq!(choose(&have(&["kick34005.A500"]), true).unwrap().kickstart, "auto");
    }

    #[test]
    fn the_choice_goes_into_puaes_options() {
        let mut prefs = crate::retro::Prefs::default();
        prefs.set_option("puae", "puae_model", "A1200");
        apply(&mut prefs, &choose(&[Installed::from(&KNOWN[0])], false).unwrap());
        assert_eq!(prefs.options_for("puae"), [("puae_model".to_string(), "A500".to_string()), ("puae_kickstart".to_string(), "auto".to_string())]);
        assert!(!aros_chosen(&prefs));
        apply(&mut prefs, &choose(&[], true).unwrap());
        assert!(aros_chosen(&prefs));
    }

    #[test]
    fn kickstarts_are_found_in_the_usual_places_but_not_twice() {
        let home = scratch("home");
        let system = home.join("system");
        std::fs::create_dir_all(home.join("Downloads")).unwrap();
        std::fs::create_dir_all(home.join("Documents/FS-UAE/Kickstarts")).unwrap();
        std::fs::create_dir_all(&system).unwrap();
        let (one_three, a1200) = (made_up(SIZE_256, 34, 5), made_up(SIZE_512, 40, 68));
        std::fs::write(home.join("Downloads/Kickstart 1.3.rom"), &one_three).unwrap();
        // The same one again under another name, a doubled one, and something that isn't a ROM at all.
        std::fs::write(home.join("Documents/FS-UAE/Kickstarts/copy.rom"), &one_three).unwrap();
        std::fs::write(home.join("Documents/FS-UAE/Kickstarts/a1200.rom"), &a1200).unwrap();
        std::fs::write(home.join("Downloads/notes.bin"), vec![7u8; SIZE_256]).unwrap();
        let places = crate::amiga_run::places(Some(&home), None);
        assert!(places.iter().any(|(p, _)| p.ends_with("Downloads")));
        let found = scan(&places, Some(&system));
        let names: Vec<&str> = found.iter().map(|f| f.what.as_ref().unwrap().file).collect();
        assert_eq!(names, ["kick34005.A500", "kick40068.A1200"]);
        assert!(found.iter().all(|f| f.ticked));
        // Once 1.3 is in the system folder it isn't offered again.
        std::fs::write(system.join("kick34005.A500"), &one_three).unwrap();
        assert_eq!(scan(&places, Some(&system)).len(), 1);
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_kickstart_is_ready_when_a_file_or_aros_is_there() {
        let system = scratch("ready");
        let mut prefs = crate::retro::Prefs::default();
        assert!(kickstart_ready(None, &prefs));
        assert!(!kickstart_ready(Some(&system), &prefs));
        std::fs::write(system.join("amiga-os-130.rom"), b"x").unwrap();
        assert!(kickstart_ready(Some(&system), &prefs));
        std::fs::remove_file(system.join("amiga-os-130.rom")).unwrap();
        apply(&mut prefs, &choose(&[], true).unwrap());
        assert!(kickstart_ready(Some(&system), &prefs));
        let _ = std::fs::remove_dir_all(system);
    }
}
