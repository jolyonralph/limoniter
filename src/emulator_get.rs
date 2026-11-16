//! Getting an emulator that isn't installed, the simplest way there is on this computer, and only when asked:
//!
//! - Windows: the emulator's latest release from its own GitHub page (ZEsarUX, b2, openMSX, FS-UAE, VICE), unpacked
//!   into Limoniter's emulators folder beside its settings, where Find emulators then finds it.
//! - Linux: Debian's and Ubuntu's own packages (Fuse, VICE, Hatari, openMSX, FS-UAE), installed with apt in the
//!   Terminal panel, where the command can be seen and the password typed.
//! - A Mac: Homebrew (VICE, Hatari, openMSX, FS-UAE, XRoar, and ZEsarUX's app), in the Terminal panel the same way,
//!   when Homebrew is installed.
//! - Anything else: the emulator's own download page.

use crate::emulators::Emulator;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The most an emulator's download is let grow to.
const MOST_DOWNLOAD: u64 = 400 << 20;

/// How an emulator can be had here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Way {
    /// Downloaded from its GitHub releases and unpacked by Limoniter.
    Download,
    /// A command to run in the Terminal panel.
    Command(String),
    /// Its web page, to download it from by hand.
    Page,
}

/// The emulators with a Windows release on GitHub: the repository, and what the right file's name has in it.
fn github(id: &str) -> Option<(&'static str, fn(&str) -> bool)> {
    Some(match id {
        "zesarux" => ("chernandezba/zesarux", |n| n.contains("_windows-") && !n.contains("legacy") && n.ends_with(".zip")),
        "b2" => ("tom-seddon/b2", |n| n.starts_with("b2-windows-") && n.ends_with(".zip")),
        "openmsx" => ("openMSX/openMSX", |n| n.ends_with("-windows-vc-x64-bin.zip")),
        "fs-uae" => ("FrodeSolheim/fs-uae", |n| n.ends_with("_Windows_x86-64.zip")),
        "vice" => ("VICE-Team/svn-mirror", |n| n.starts_with("GTK3VICE-") && n.ends_with("-win64.zip")),
        _ => return None,
    })
}

/// Debian's and Ubuntu's package for an emulator.
fn apt_package(id: &str) -> Option<&'static str> {
    Some(match id {
        "fuse" => "fuse-emulator-gtk",
        "vice" => "vice",
        "hatari" => "hatari",
        "openmsx" => "openmsx",
        "fs-uae" => "fs-uae",
        _ => return None,
    })
}

/// Homebrew's formula for an emulator, or its cask (an app) when the second is true.
fn brew_package(id: &str) -> Option<(&'static str, bool)> {
    Some(match id {
        "vice" => ("vice", false),
        "hatari" => ("hatari", false),
        "openmsx" => ("openmsx", false),
        "fs-uae" => ("fs-uae", false),
        "xroar" => ("xroar", false),
        "zesarux" => ("zesarux", true),
        _ => return None,
    })
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|p| p.join(program).is_file()))
}

fn brew() -> Option<PathBuf> {
    ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"].into_iter().map(PathBuf::from).find(|p| p.is_file())
}

/// The way to get an emulator on this computer.
pub fn way(emulator: &Emulator) -> Way {
    way_for(emulator.id, cfg!(windows), cfg!(target_os = "macos"), on_path("apt-get"), brew().is_some())
}

fn way_for(id: &str, windows: bool, mac: bool, apt: bool, homebrew: bool) -> Way {
    if windows && github(id).is_some() {
        return Way::Download;
    }
    if mac && homebrew && let Some((package, cask)) = brew_package(id) {
        return Way::Command(format!("brew install {}{package}", if cask { "--cask " } else { "" }));
    }
    if !windows && !mac && apt && let Some(package) = apt_package(id) {
        return Way::Command(format!("sudo apt-get install {package}"));
    }
    Way::Page
}

/// What a way says on its button.
pub fn button(way: &Way) -> &'static str {
    match way {
        Way::Download => "Download and install",
        Way::Command(command) if command.starts_with("brew") => "Install with Homebrew",
        Way::Command(_) => "Install with apt",
        Way::Page => "Open its download page",
    }
}

/// Where downloaded emulators are kept, each in a folder of its own.
pub fn folder() -> Option<PathBuf> {
    if let Some(folder) = std::env::var_os("LIMONITER_EMULATORS") {
        return Some(folder.into());
    }
    if cfg!(test) {
        return None;
    }
    crate::settings::data_folder().map(|d| d.join("emulators"))
}

fn get(address: &str) -> Result<Vec<u8>, String> {
    let response = ureq::get(address).header("User-Agent", "Limoniter").call().map_err(|e| format!("{address} couldn't be reached: {e}"))?;
    let mut bytes = Vec::new();
    response.into_body().into_reader().take(MOST_DOWNLOAD).read_to_end(&mut bytes).map_err(|e| format!("The download stopped: {e}"))?;
    Ok(bytes)
}

/// The release file to download, from GitHub's description of the latest release.
fn pick_asset(release: &str, wanted: fn(&str) -> bool) -> Result<(String, String), String> {
    let json: serde_json::Value = serde_json::from_str(release).map_err(|_| "GitHub's answer wasn't what was expected.".to_string())?;
    let tag = json["tag_name"].as_str().unwrap_or("").to_string();
    json["assets"]
        .as_array()
        .and_then(|assets| assets.iter().find(|a| a["name"].as_str().is_some_and(wanted)))
        .and_then(|a| a["browser_download_url"].as_str())
        .map(|url| (tag.clone(), url.to_string()))
        .ok_or_else(|| format!("The latest release ({tag}) has no download for Windows that Limoniter knows."))
}

/// Downloads an emulator's latest Windows release and unpacks it into its own folder, replacing any there before.
/// Gives the emulator's program.
pub fn download(emulator: &Emulator) -> Result<PathBuf, String> {
    let (repo, wanted) = github(emulator.id).ok_or_else(|| format!("{} isn't downloaded by Limoniter: get it from {}.", emulator.name, emulator.website))?;
    let home = folder().ok_or("There's nowhere to keep it: Limoniter's settings folder couldn't be found.")?;
    let release = String::from_utf8(get(&format!("https://api.github.com/repos/{repo}/releases/latest"))?).map_err(|_| "GitHub's answer wasn't text.".to_string())?;
    let (_, address) = pick_asset(&release, wanted)?;
    let zip = get(&address)?;
    let target = home.join(emulator.id);
    let _ = std::fs::remove_dir_all(&target);
    unzip(zip, &target)?;
    find_program(&target, emulator.programs).ok_or_else(|| format!("{} was unpacked into {}, but its program wasn't found in it.", emulator.name, target.display()))
}

/// Unpacks a zip into a folder. When everything in it is in one folder, that folder's contents go straight in.
fn unzip(zip: Vec<u8>, target: &Path) -> Result<(), String> {
    let volume = crate::media::open(std::sync::Arc::new(zip), "release.zip")?;
    let top = volume.list(None)?;
    let only = (top.len() == 1 && top[0].is_dir).then(|| top[0].clone());
    write_folder(&*volume, only.as_ref(), target)
}

fn write_folder(volume: &dyn crate::media::Volume, dir: Option<&crate::media::Entry>, target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target).map_err(|e| format!("{} couldn't be made: {e}", target.display()))?;
    for entry in volume.list(dir)? {
        // Nothing in the zip is let out of the folder.
        if entry.name.contains(['/', '\\', ':']) || entry.name == ".." || entry.name == "." {
            continue;
        }
        let path = target.join(&entry.name);
        if entry.is_dir {
            write_folder(volume, Some(&entry), &path)?;
        } else {
            std::fs::write(&path, volume.read(&entry)?).map_err(|e| format!("{} couldn't be written: {e}", path.display()))?;
        }
    }
    Ok(())
}

/// The emulator's program in what was unpacked, a few folders down at most, the first of its names found.
fn find_program(folder: &Path, programs: &[&str]) -> Option<PathBuf> {
    let exe = |name: &str| if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    let mut level = vec![folder.to_path_buf()];
    for _ in 0..4 {
        for program in programs {
            if let Some(found) = level.iter().map(|f| f.join(exe(program))).find(|p| p.is_file()) {
                return Some(found);
            }
        }
        let mut next = Vec::new();
        for f in &level {
            if let Ok(entries) = std::fs::read_dir(f) {
                let mut folders: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
                folders.sort();
                next.extend(folders);
            }
        }
        level = next;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_computer_gets_its_own_way() {
        assert_eq!(way_for("vice", true, false, false, false), Way::Download);
        assert_eq!(way_for("hatari", true, false, false, false), Way::Page, "no Windows release on GitHub");
        assert_eq!(way_for("vice", false, false, true, false), Way::Command("sudo apt-get install vice".into()));
        assert_eq!(way_for("fuse", false, false, true, false), Way::Command("sudo apt-get install fuse-emulator-gtk".into()));
        assert_eq!(way_for("b2", false, false, true, false), Way::Page);
        assert_eq!(way_for("zesarux", false, true, false, true), Way::Command("brew install --cask zesarux".into()));
        assert_eq!(way_for("fuse", false, true, false, true), Way::Page, "Homebrew's fuse is something else");
        assert_eq!(way_for("hatari", false, true, false, false), Way::Page, "no Homebrew");
        assert_eq!(button(&Way::Command("brew install vice".into())), "Install with Homebrew");
    }

    #[test]
    fn the_windows_file_is_picked_from_each_release() {
        let release = |names: &[&str]| serde_json::json!({"tag_name": "v1", "assets": names.iter().map(|n| serde_json::json!({"name": n, "browser_download_url": format!("https://github.com/x/{n}")})).collect::<Vec<_>>()}).to_string();
        let pick = |id: &str, names: &[&str]| pick_asset(&release(names), github(id).unwrap().1).map(|(_, url)| url.rsplit('/').next().unwrap().to_string());
        assert_eq!(pick("zesarux", &["ZEsarUX_windows-13.0-legacy.zip", "ZEsarUX_windows-13.0.zip", "ZEsarUX_src-13.0.tar.gz"]).unwrap(), "ZEsarUX_windows-13.0.zip");
        assert_eq!(pick("openmsx", &["openmsx-22.0-windows-vc-x64-bin-msi.zip", "openmsx-22.0-windows-vc-x64-bin.zip", "openmsx-22.0-windows-vc-x64-pdb.zip"]).unwrap(), "openmsx-22.0-windows-vc-x64-bin.zip");
        assert_eq!(pick("vice", &["GTK3VICE-3.10-win64.7z", "GTK3VICE-3.10-win64.zip", "SDL2VICE-3.10-win64.zip"]).unwrap(), "GTK3VICE-3.10-win64.zip");
        assert_eq!(pick("fs-uae", &["FS-UAE_3.2.35_Windows_x86-64.exe", "FS-UAE_3.2.35_Windows_x86-64.zip"]).unwrap(), "FS-UAE_3.2.35_Windows_x86-64.zip");
        assert_eq!(pick("b2", &["symbols.b2-windows-1.7z", "b2-windows-1.zip"]).unwrap(), "b2-windows-1.zip");
        assert!(pick("b2", &["b2-macos.dmg"]).is_err());
    }

    /// A zip with its files stored, as the KickAssembler tests make one.
    fn stored_zip(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let (mut out, mut directory) = (Vec::new(), Vec::new());
        for (name, bytes) in files {
            let at = out.len() as u32;
            let sizes = [(bytes.len() as u32).to_le_bytes(), (bytes.len() as u32).to_le_bytes()].concat();
            out.extend(b"PK\x03\x04\x14\0\0\0\0\0\0\0\0\0\0\0\0\0");
            out.extend(&sizes);
            out.extend((name.len() as u16).to_le_bytes());
            out.extend([0, 0]);
            out.extend(name.as_bytes());
            out.extend(bytes);
            directory.extend(b"PK\x01\x02\x14\0\x14\0\0\0\0\0\0\0\0\0\0\0\0\0");
            directory.extend(&sizes);
            directory.extend((name.len() as u16).to_le_bytes());
            directory.extend([0u8; 12]);
            directory.extend(at.to_le_bytes());
            directory.extend(name.as_bytes());
        }
        let start = out.len() as u32;
        out.extend(&directory);
        out.extend(b"PK\x05\x06\0\0\0\0");
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((files.len() as u16).to_le_bytes());
        out.extend((directory.len() as u32).to_le_bytes());
        out.extend(start.to_le_bytes());
        out.extend([0, 0]);
        out
    }

    #[test]
    fn a_release_is_unpacked_and_its_program_found() {
        let folder = std::env::temp_dir().join(format!("limoniter-emulator-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let program = if cfg!(windows) { "x64sc.exe" } else { "x64sc" };
        let zip = stored_zip(&[(&format!("GTK3VICE-3.10-win64/bin/{program}"), b"MZ".to_vec()), ("GTK3VICE-3.10-win64/README.txt", b"read me".to_vec())]);
        unzip(zip, &folder).unwrap();
        assert!(folder.join("README.txt").is_file(), "the one top folder's contents go straight in");
        assert_eq!(find_program(&folder, &["x64sc"]), Some(folder.join("bin").join(program)));
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Asks GitHub for real: cargo test -- --ignored emulator_get::tests::every_release_has_its_windows_file
    #[test]
    #[ignore]
    fn every_release_has_its_windows_file() {
        for id in ["zesarux", "b2", "openmsx", "fs-uae", "vice"] {
            let (repo, wanted) = github(id).unwrap();
            let release = String::from_utf8(get(&format!("https://api.github.com/repos/{repo}/releases/latest")).unwrap()).unwrap();
            println!("{id}: {:?}", pick_asset(&release, wanted));
            assert!(pick_asset(&release, wanted).is_ok(), "{id}");
        }
    }
}
