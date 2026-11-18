//! Keeping the OpticOS ROM up to date. Limoniter carries OpticOS's ROM (vendor/opticos); with the setting on, it
//! asks github.com/brixham99/OpticOS for its latest release at most once a day, and puts a newer ROM beside the
//! settings, where the built-in Spectrum and the emulators it starts then find it.
//!
//! A downloaded ROM is only used when it's 16K and holds exactly the OpticBASIC keyword table Limoniter knows, since
//! listing and tokenising follow that table: a release that changes OpticBASIC's keywords waits for a Limoniter that
//! knows them, and the ROM carried in Limoniter goes on being used.

use std::path::PathBuf;
use std::sync::RwLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The ROM Limoniter carries, and its version.
pub const BUILT_IN: &[u8; 16384] = include_bytes!("../vendor/opticos/opticos.rom");
pub const BUILT_IN_VERSION: &str = "1.0.2";

const LATEST: &str = "https://api.github.com/repos/brixham99/OpticOS/releases/latest";
/// Where the release's ROM may come from: only OpticOS's own releases.
const DOWNLOADS: &str = "https://github.com/brixham99/OpticOS/releases/download/";
/// How often to ask, at most, unless asked to.
const EVERY: Duration = Duration::from_secs(20 * 60 * 60);

/// A ROM, with its version.
#[derive(Clone, Debug)]
pub struct Rom {
    pub bytes: &'static [u8],
    pub version: String,
}

/// The installed ROM, once looked for: `None` inside until then.
static CURRENT: RwLock<Option<Rom>> = RwLock::new(None);

fn folder() -> Option<PathBuf> {
    if let Some(folder) = std::env::var_os("LIMONITER_OPTICOS") {
        return Some(PathBuf::from(folder));
    }
    // Tests never pick up a ROM installed on the machine running them.
    if cfg!(test) {
        return None;
    }
    crate::settings::data_folder().map(|dir| dir.join("opticos"))
}

/// The ROM to run: a newer one downloaded, if there is one, or the one Limoniter carries.
pub fn current() -> Rom {
    if let Some(rom) = CURRENT.read().ok().and_then(|c| c.clone()) {
        return rom;
    }
    let rom = installed().unwrap_or_else(|| Rom { bytes: &BUILT_IN[..], version: BUILT_IN_VERSION.into() });
    if let Ok(mut current) = CURRENT.write() {
        *current = Some(rom.clone());
    }
    rom
}

/// A ROM downloaded earlier, if it's still sound and newer than the one carried.
fn installed() -> Option<Rom> {
    let folder = folder()?;
    let version = std::fs::read_to_string(folder.join("version.txt")).ok()?.trim().to_string();
    let bytes = std::fs::read(folder.join("opticos.rom")).ok()?;
    (usable(&bytes).is_ok() && newer(&version, BUILT_IN_VERSION)).then(|| Rom { bytes: Box::leak(bytes.into_boxed_slice()), version })
}

/// OpticBASIC's keywords as the ROM keeps them: each word's last letter with its top bit set, then a zero.
fn keyword_table() -> Vec<u8> {
    let mut table = Vec::new();
    for word in crate::opticbasic::KEYWORDS {
        let bytes = word.as_bytes();
        table.extend_from_slice(&bytes[..bytes.len() - 1]);
        table.push(bytes[bytes.len() - 1] | 0x80);
    }
    table.push(0);
    table
}

/// Whether a ROM can be used in place of the one carried, and why not.
pub fn usable(rom: &[u8]) -> Result<(), String> {
    if rom.len() != 16384 {
        return Err(crate::i18n::tf!("It's {} bytes, not the 16384 of a Spectrum ROM.", rom.len()));
    }
    let table = keyword_table();
    if !rom.windows(table.len()).any(|w| w == table) {
        return Err(crate::i18n::tr("Its OpticBASIC keywords aren't the ones this Limoniter knows, so it waits for a Limoniter that does.").into());
    }
    Ok(())
}

/// The numbers in a version, "v1.0.2" being [1, 0, 2].
fn numbers(version: &str) -> Vec<u32> {
    version.trim().trim_start_matches(['v', 'V']).split(['.', '-']).map_while(|part| part.parse().ok()).collect()
}

/// Whether one version comes after another.
pub fn newer(version: &str, than: &str) -> bool {
    let (a, b) = (numbers(version), numbers(than));
    !a.is_empty() && a > b
}

/// What a check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Checked too lately to ask again.
    NotDue,
    UpToDate(String),
    Installed(String),
    /// A newer release whose ROM can't be used, and why.
    Unusable(String, String),
}

impl Outcome {
    /// What to tell the person, if anything.
    pub fn message(&self) -> Option<String> {
        match self {
            Outcome::NotDue => None,
            Outcome::UpToDate(version) => Some(crate::i18n::tf!("OpticOS {version} is the latest, and it's the one the built-in Spectrum runs.")),
            Outcome::Installed(version) => Some(crate::i18n::tf!("OpticOS {version} is installed. The built-in Spectrum runs it from now on.")),
            Outcome::Unusable(version, why) => Some(crate::i18n::tf!("OpticOS {version} is out, but its ROM isn't used: {why}")),
        }
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The latest release's version and its ROM's address.
fn latest() -> Result<(String, String), String> {
    let response = ureq::get(LATEST)
        .header("User-Agent", "Limoniter")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| crate::i18n::tf!("GitHub couldn't be asked: {e}"))?;
    let text = response.into_body().with_config().limit(1 << 20).read_to_string().map_err(|e| crate::i18n::tf!("GitHub's answer couldn't be read: {e}"))?;
    release_from(&text)
}

/// The version and ROM address in GitHub's description of a release.
fn release_from(text: &str) -> Result<(String, String), String> {
    let json: serde_json::Value = serde_json::from_str(text).map_err(|_| crate::i18n::tr("GitHub's answer wasn't what was expected.").to_string())?;
    let tag = json["tag_name"].as_str().ok_or(crate::i18n::tr("The release has no version."))?.to_string();
    let address = json["assets"]
        .as_array()
        .and_then(|assets| assets.iter().find(|a| a["name"] == "opticos.rom"))
        .and_then(|a| a["browser_download_url"].as_str())
        .ok_or_else(|| crate::i18n::tf!("OpticOS {tag} has no opticos.rom."))?;
    if !address.starts_with(DOWNLOADS) {
        return Err(crate::i18n::tr("The release's ROM isn't among OpticOS's own downloads.").into());
    }
    Ok((tag.trim_start_matches(['v', 'V']).to_string(), address.to_string()))
}

/// Asks GitHub for a newer OpticOS and installs its ROM. Unless `now_anyway`, it doesn't ask again within a day.
pub fn check(now_anyway: bool) -> Result<Outcome, String> {
    let folder = folder().ok_or(crate::i18n::tr("There's nowhere to keep a downloaded ROM."))?;
    let stamp = folder.join("checked.txt");
    let last = std::fs::read_to_string(&stamp).ok().and_then(|t| t.trim().parse::<u64>().ok()).unwrap_or(0);
    if !now_anyway && now().saturating_sub(last) < EVERY.as_secs() {
        return Ok(Outcome::NotDue);
    }
    let (version, address) = latest()?;
    std::fs::create_dir_all(&folder).map_err(|e| crate::i18n::tf!("The folder for OpticOS couldn't be made: {e}"))?;
    let _ = std::fs::write(&stamp, now().to_string());
    let running = current().version;
    if !newer(&version, &running) {
        return Ok(Outcome::UpToDate(running));
    }
    let response = ureq::get(&address).header("User-Agent", "Limoniter").call().map_err(|e| crate::i18n::tf!("The ROM couldn't be downloaded: {e}"))?;
    let bytes = response.into_body().with_config().limit(65536).read_to_vec().map_err(|e| crate::i18n::tf!("The ROM couldn't be downloaded: {e}"))?;
    let outcome = install(&folder, &version, bytes)?;
    if let (Outcome::Installed(_), Some(rom)) = (&outcome, installed())
        && let Ok(mut current) = CURRENT.write()
    {
        *current = Some(rom);
    }
    Ok(outcome)
}

/// Puts a downloaded ROM in place, if it can be used.
fn install(folder: &std::path::Path, version: &str, bytes: Vec<u8>) -> Result<Outcome, String> {
    if let Err(why) = usable(&bytes) {
        return Ok(Outcome::Unusable(version.into(), why));
    }
    // The ROM first, under another name, so a half-written one is never used.
    let part = folder.join("opticos.rom.part");
    std::fs::write(&part, &bytes).map_err(|e| crate::i18n::tf!("The ROM couldn't be saved: {e}"))?;
    std::fs::rename(&part, folder.join("opticos.rom")).map_err(|e| crate::i18n::tf!("The ROM couldn't be saved: {e}"))?;
    std::fs::write(folder.join("version.txt"), version).map_err(|e| crate::i18n::tf!("The ROM's version couldn't be saved: {e}"))?;
    Ok(Outcome::Installed(version.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rom_carried_is_usable_and_others_are_checked() {
        assert!(usable(BUILT_IN).is_ok(), "the carried ROM has the keyword table");
        assert!(usable(&BUILT_IN[..8192]).is_err());
        let mut changed = BUILT_IN.to_vec();
        let at = changed.windows(4).position(|w| w == b"PRIN").unwrap();
        changed[at] = b'Q';
        assert!(usable(&changed).unwrap_err().contains("keywords"));
        assert!(usable(&[0u8; 16384]).is_err());
    }

    #[test]
    fn versions_are_compared_by_their_numbers() {
        assert!(newer("1.0.2", "1.0.1"));
        assert!(newer("v1.0.10", "1.0.9"));
        assert!(newer("1.1", "1.0.9"));
        assert!(!newer("1.0.1", "1.0.1"));
        assert!(!newer("1.0", "1.0.1"));
        assert!(!newer("beta", "1.0.1"));
    }

    #[test]
    fn a_release_gives_its_version_and_rom_only_from_opticos_own_downloads() {
        let release = r#"{"tag_name":"v1.0.2","assets":[{"name":"spec.pdf","browser_download_url":"https://github.com/brixham99/OpticOS/releases/download/v1.0.2/spec.pdf"},{"name":"opticos.rom","browser_download_url":"https://github.com/brixham99/OpticOS/releases/download/v1.0.2/opticos.rom"}]}"#;
        assert_eq!(release_from(release).unwrap(), ("1.0.2".to_string(), "https://github.com/brixham99/OpticOS/releases/download/v1.0.2/opticos.rom".to_string()));
        assert!(release_from(r#"{"tag_name":"v2","assets":[]}"#).is_err());
        assert!(release_from(r#"{"tag_name":"v2","assets":[{"name":"opticos.rom","browser_download_url":"https://example.com/opticos.rom"}]}"#).is_err());
    }

    #[test]
    fn a_usable_rom_is_installed_and_run_and_an_unusable_one_is_not() {
        let folder = std::env::temp_dir().join(format!("limoniter-opticos-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        assert!(matches!(install(&folder, "9.9.9", vec![0; 100]).unwrap(), Outcome::Unusable(..)));
        assert!(!folder.join("opticos.rom").exists(), "nothing is written for a ROM that can't be used");
        let mut newer_rom = BUILT_IN.to_vec();
        newer_rom[16383] ^= 0xFF;
        assert_eq!(install(&folder, "9.9.9", newer_rom.clone()).unwrap(), Outcome::Installed("9.9.9".into()));
        assert_eq!(std::fs::read(folder.join("opticos.rom")).unwrap(), newer_rom);
        assert_eq!(std::fs::read_to_string(folder.join("version.txt")).unwrap(), "9.9.9");
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Asks GitHub for real: LIMONITER_OPTICOS=<folder> cargo test -- --ignored opticos_rom::tests::asks_github
    #[test]
    #[ignore]
    fn asks_github() {
        let outcome = check(true).unwrap();
        println!("{outcome:?}");
        assert!(!matches!(outcome, Outcome::NotDue));
    }
}
