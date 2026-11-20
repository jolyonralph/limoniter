//! Getting an emulator that isn't installed, the simplest way there is on this computer, and only when asked:
//!
//! - Windows: the emulator's latest release from its own GitHub page (ZEsarUX, b2, openMSX, FS-UAE, VICE, Stella,
//!   SameBoy, Mesen, FCEUX, Nestopia, Copperline), unpacked
//!   into Limoniter's emulators folder beside its settings, where Find emulators then finds it.
//! - Linux: Debian's and Ubuntu's own packages (Fuse, VICE, Hatari, openMSX, FS-UAE, Stella, MAME, mGBA, FCEUX,
//!   Nestopia), installed with apt in the
//!   Terminal panel, where the command can be seen and the password typed. Copperline, which has no package, is
//!   downloaded from GitHub as its AppImage and its command-line tools, side by side.
//! - A Mac: Homebrew (VICE, Hatari, openMSX, FS-UAE, XRoar, MAME, Copperline from its own tap, and ZEsarUX's,
//!   Stella's, SameBoy's and mGBA's apps), in the Terminal panel the same way, when Homebrew is installed.
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
        "stella" => ("stella-emu/stella", |n| n.starts_with("Stella-") && n.ends_with("-windows.zip")),
        "dosbox" => ("dosbox-staging/dosbox-staging", |n| n.starts_with("dosbox-staging-windows-x64-") && n.ends_with(".zip")),
        "sameboy" => ("LIJI32/SameBoy", |n| n.starts_with("sameboy_winsdl_") && n.ends_with(".zip")),
        "mesen" => ("SourMesen/Mesen2", |n| n.starts_with("Mesen_") && n.ends_with("_Windows.zip")),
        "fceux" => ("TASEmulators/fceux", |n| n.starts_with("fceux-") && n.ends_with("-win64.zip")),
        "nestopia" => ("0ldsk00l/nestopia", |n| n.starts_with("nestopia-") && n.ends_with("-windows-x86_64.zip")),
        // The portable zip, not the installer: copperline.exe with copperline-ctl.exe beside it.
        "copperline" => ("CopperlineHQ/Copperline", |n| n.starts_with("Copperline-") && n.ends_with("-win-x64.zip")),
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
        "stella" => "stella",
        "mame" => "mame",
        "blastem" => "blastem",
        "dosbox" => "dosbox",
        "qemu" => "qemu-system-x86",
        "mgba" => "mgba-qt",
        "fceux" => "fceux",
        "nestopia" => "nestopia",
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
        "stella" => ("stella", true),
        "mame" => ("mame", false),
        "dosbox" => ("dosbox-staging", false),
        "qemu" => ("qemu", false),
        "sameboy" => ("sameboy", true),
        "mgba" => ("mgba", true),
        _ => return None,
    })
}

/// The emulators with a Linux release on GitHub that's a program in itself (an AppImage), and the tools that go
/// beside it: the repository, and what each file's name has in it. Only for x86-64, which is all they're built for.
fn linux_github(id: &str) -> Option<(&'static str, fn(&str) -> bool, fn(&str) -> bool)> {
    Some(match id {
        "copperline" => ("CopperlineHQ/Copperline", |n| n.starts_with("Copperline-") && n.ends_with("-x86_64.AppImage"), |n| n.starts_with("Copperline-") && n.ends_with("-x86_64-tools.tar.gz")),
        _ => return None,
    })
}

/// The command that installs an emulator from a Homebrew tap of its own: Copperline's repository is its tap,
/// which a recent Homebrew must be told to trust (an older one hasn't the word, and needn't).
fn brew_tap(id: &str) -> Option<&'static str> {
    Some(match id {
        "copperline" => "brew tap copperlinehq/copperline https://github.com/CopperlineHQ/Copperline && (brew trust copperlinehq/copperline || true) && brew install copperline",
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
    let linux = cfg!(target_os = "linux") && std::env::consts::ARCH == "x86_64";
    way_for(emulator.id, cfg!(windows), cfg!(target_os = "macos"), on_path("apt-get"), brew().is_some(), linux)
}

fn way_for(id: &str, windows: bool, mac: bool, apt: bool, homebrew: bool, linux_x86: bool) -> Way {
    if windows && github(id).is_some() {
        return Way::Download;
    }
    if linux_x86 && linux_github(id).is_some() {
        return Way::Download;
    }
    if mac && homebrew && let Some(command) = brew_tap(id) {
        return Way::Command(command.to_string());
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
    let response = ureq::get(address).header("User-Agent", "Limoniter").call().map_err(|e| crate::i18n::tf!("{address} couldn't be reached: {e}"))?;
    let mut bytes = Vec::new();
    response.into_body().into_reader().take(MOST_DOWNLOAD).read_to_end(&mut bytes).map_err(|e| crate::i18n::tf!("The download stopped: {e}"))?;
    Ok(bytes)
}

/// The release file to download, from GitHub's description of the latest release.
fn pick_asset(release: &str, wanted: fn(&str) -> bool) -> Result<(String, String), String> {
    let json: serde_json::Value = serde_json::from_str(release).map_err(|_| crate::i18n::tr("GitHub's answer wasn't what was expected.").to_string())?;
    let tag = json["tag_name"].as_str().unwrap_or("").to_string();
    json["assets"]
        .as_array()
        .and_then(|assets| assets.iter().find(|a| a["name"].as_str().is_some_and(wanted)))
        .and_then(|a| a["browser_download_url"].as_str())
        .map(|url| (tag.clone(), url.to_string()))
        .ok_or_else(|| crate::i18n::tf!("The latest release ({tag}) has no download for Windows that Limoniter knows."))
}

/// Downloads an emulator's latest Windows release and unpacks it into its own folder, replacing any there before.
/// Gives the emulator's program. On Linux, an AppImage and its tools are downloaded instead (see `download_linux`).
pub fn download(emulator: &Emulator) -> Result<PathBuf, String> {
    if !cfg!(windows)
        && let Some(found) = linux_github(emulator.id)
    {
        return download_linux(emulator, found);
    }
    let (repo, wanted) = github(emulator.id).ok_or_else(|| crate::i18n::tf!("{} isn't downloaded by Limoniter: get it from {}.", emulator.name, emulator.website))?;
    let home = folder().ok_or(crate::i18n::tr("There's nowhere to keep it: Limoniter's settings folder couldn't be found."))?;
    let release = String::from_utf8(get(&format!("https://api.github.com/repos/{repo}/releases/latest"))?).map_err(|_| crate::i18n::tr("GitHub's answer wasn't text.").to_string())?;
    let (_, address) = pick_asset(&release, wanted)?;
    let zip = get(&address)?;
    let target = home.join(emulator.id);
    let _ = std::fs::remove_dir_all(&target);
    unzip(zip, &target)?;
    find_program(&target, emulator.programs).ok_or_else(|| crate::i18n::tf!("{} was unpacked into {}, but its program wasn't found in it.", emulator.name, target.display()))
}

/// Downloads an emulator's latest AppImage, kept under its own program name so Find emulators knows it, and the
/// command-line tools released with it unpacked beside it (for Copperline, copperline-ctl, which finds the
/// emulator beside itself).
fn download_linux(emulator: &Emulator, (repo, program, tools): (&str, fn(&str) -> bool, fn(&str) -> bool)) -> Result<PathBuf, String> {
    let home = folder().ok_or(crate::i18n::tr("There's nowhere to keep it: Limoniter's settings folder couldn't be found."))?;
    let release = String::from_utf8(get(&format!("https://api.github.com/repos/{repo}/releases/latest"))?).map_err(|_| crate::i18n::tr("GitHub's answer wasn't text.").to_string())?;
    let (_, image) = pick_asset(&release, program).map_err(|_| crate::i18n::tf!("The latest release of {} has no download for Linux that Limoniter knows.", emulator.name))?;
    let target = home.join(emulator.id);
    let _ = std::fs::remove_dir_all(&target);
    if let Ok((_, address)) = pick_asset(&release, tools) {
        untar(&crate::media::gunzip_bytes(&get(&address)?)?, &target)?;
    }
    std::fs::create_dir_all(&target).map_err(|e| crate::i18n::tf!("{} couldn't be made: {e}", target.display()))?;
    let path = target.join(emulator.programs.first().copied().unwrap_or(emulator.id));
    std::fs::write(&path, get(&image)?).map_err(|e| crate::i18n::tf!("{} couldn't be written: {e}", path.display()))?;
    make_runnable(&path, 0o755);
    Ok(path)
}

/// Lets a file be run, as a tar file says it can be (on Windows, everything can).
fn make_runnable(path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode & 0o777));
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
}

/// Unpacks a tar file into a folder, the folder everything's in (as a release's is) left out: its files and
/// folders, with the permission to run where a file has it. Nothing is let out of the folder.
fn untar(tar: &[u8], target: &Path) -> Result<(), String> {
    let mut at = 0;
    while let Some(header) = tar.get(at..at + 512) {
        if header.iter().all(|b| *b == 0) {
            break;
        }
        let text = |range: std::ops::Range<usize>| String::from_utf8_lossy(&header[range]).trim_end_matches('\0').to_string();
        let octal = |range: std::ops::Range<usize>| u64::from_str_radix(text(range).trim(), 8).unwrap_or(0);
        let size = octal(124..136) as usize;
        let mode = octal(100..108) as u32;
        // ustar puts the start of a long name in a prefix of its own.
        let prefix = if &header[257..262] == b"ustar" { text(345..500) } else { String::new() };
        let name = if prefix.is_empty() { text(0..100) } else { format!("{prefix}/{}", text(0..100)) };
        let kind = header[156];
        let body = tar.get(at + 512..at + 512 + size).ok_or(crate::i18n::tr("The tar file ends too soon."))?;
        at += 512 + size.div_ceil(512) * 512;
        let parts: Vec<&str> = name.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
        if parts.iter().any(|p| *p == ".." || p.contains(['\\', ':'])) || parts.len() < 2 {
            continue;
        }
        let path = parts[1..].iter().fold(target.to_path_buf(), |path, part| path.join(part));
        match kind {
            b'5' => std::fs::create_dir_all(&path).map_err(|e| crate::i18n::tf!("{} couldn't be made: {e}", path.display()))?,
            b'0' | 0 => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| crate::i18n::tf!("{} couldn't be made: {e}", parent.display()))?;
                }
                std::fs::write(&path, body).map_err(|e| crate::i18n::tf!("{} couldn't be written: {e}", path.display()))?;
                make_runnable(&path, mode);
            }
            _ => {}
        }
    }
    Ok(())
}

/// Unpacks a zip into a folder. When everything in it is in one folder, that folder's contents go straight in.
fn unzip(zip: Vec<u8>, target: &Path) -> Result<(), String> {
    let volume = crate::media::open(std::sync::Arc::new(zip), "release.zip")?;
    let top = volume.list(None)?;
    let only = (top.len() == 1 && top[0].is_dir).then(|| top[0].clone());
    write_folder(&*volume, only.as_ref(), target)
}

fn write_folder(volume: &dyn crate::media::Volume, dir: Option<&crate::media::Entry>, target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(target).map_err(|e| crate::i18n::tf!("{} couldn't be made: {e}", target.display()))?;
    for entry in volume.list(dir)? {
        // Nothing in the zip is let out of the folder.
        if entry.name.contains(['/', '\\', ':']) || entry.name == ".." || entry.name == "." {
            continue;
        }
        let path = target.join(&entry.name);
        if entry.is_dir {
            write_folder(volume, Some(&entry), &path)?;
        } else {
            std::fs::write(&path, volume.read(&entry)?).map_err(|e| crate::i18n::tf!("{} couldn't be written: {e}", path.display()))?;
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
        assert_eq!(way_for("vice", true, false, false, false, false), Way::Download);
        assert_eq!(way_for("hatari", true, false, false, false, false), Way::Page, "no Windows release on GitHub");
        assert_eq!(way_for("vice", false, false, true, false, true), Way::Command("sudo apt-get install vice".into()));
        assert_eq!(way_for("fuse", false, false, true, false, true), Way::Command("sudo apt-get install fuse-emulator-gtk".into()));
        assert_eq!(way_for("b2", false, false, true, false, true), Way::Page);
        assert_eq!(way_for("zesarux", false, true, false, true, false), Way::Command("brew install --cask zesarux".into()));
        assert_eq!(way_for("fuse", false, true, false, true, false), Way::Page, "Homebrew's fuse is something else");
        assert_eq!(way_for("hatari", false, true, false, false, false), Way::Page, "no Homebrew");
        // Copperline: its zip on Windows, its AppImage on a PC's Linux (not an ARM one's), and its own tap on a Mac.
        assert_eq!(way_for("copperline", true, false, false, false, false), Way::Download);
        assert_eq!(way_for("copperline", false, false, true, false, true), Way::Download);
        assert_eq!(way_for("copperline", false, false, true, false, false), Way::Page);
        let Way::Command(tap) = way_for("copperline", false, true, false, true, false) else { panic!("Homebrew") };
        assert!(tap.starts_with("brew tap copperlinehq/copperline https://github.com/CopperlineHQ/Copperline") && tap.ends_with("brew install copperline"), "{tap}");
        assert_eq!(button(&Way::Command(tap)), "Install with Homebrew");
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
        // As their latest releases name them (November 2026).
        assert_eq!(pick("sameboy", &["sameboy_cocoa_v1.0.5.zip", "sameboy_winsdl_v1.0.5.zip", "SHA2-256SUMS"]).unwrap(), "sameboy_winsdl_v1.0.5.zip");
        assert_eq!(pick("mesen", &["Mesen_2.1.1_Linux_x64.zip", "Mesen_2.1.1_Windows.zip"]).unwrap(), "Mesen_2.1.1_Windows.zip");
        assert_eq!(pick("fceux", &["fceux-2.7.0-win32.zip", "fceux-2.7.0-win64-QtSDL.zip", "fceux-2.7.0-win64.zip"]).unwrap(), "fceux-2.7.0-win64.zip");
        assert_eq!(pick("nestopia", &["nestopia-2.0.0-macos-arm64.dmg", "nestopia-2.0.0-windows-x86_64.zip"]).unwrap(), "nestopia-2.0.0-windows-x86_64.zip");
        assert_eq!(way_for("mgba", false, false, true, false, true), Way::Command("sudo apt-get install mgba-qt".into()));
        assert_eq!(way_for("sameboy", false, true, false, true, false), Way::Command("brew install --cask sameboy".into()));
        // Copperline's, as its 1.1.0 release names them.
        let copperline = ["Copperline-1.1.0-libretro-Linux-X64.zip", "Copperline-1.1.0-libretro-Windows-X64.zip", "Copperline-1.1.0-macos-universal.dmg", "Copperline-1.1.0-win-arm64.zip", "Copperline-1.1.0-win-x64-setup.exe", "Copperline-1.1.0-win-x64.zip", "Copperline-1.1.0-x86_64-net-helper.tar.gz", "Copperline-1.1.0-x86_64-tools.tar.gz", "Copperline-1.1.0-x86_64.AppImage"];
        assert_eq!(pick("copperline", &copperline).unwrap(), "Copperline-1.1.0-win-x64.zip");
        let (_, image, tools) = linux_github("copperline").unwrap();
        let named = |wanted: fn(&str) -> bool| pick_asset(&release(&copperline), wanted).map(|(_, url)| url.rsplit('/').next().unwrap().to_string());
        assert_eq!(named(image).unwrap(), "Copperline-1.1.0-x86_64.AppImage");
        assert_eq!(named(tools).unwrap(), "Copperline-1.1.0-x86_64-tools.tar.gz");
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

    /// A tar file, as GNU tar writes a release's: a header for each file (its name, mode and size in octal), its
    /// bytes padded to 512, and two empty blocks to end.
    fn tar(files: &[(&str, u32, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, mode, bytes) in files {
            let mut header = [0u8; 512];
            header[..name.len()].copy_from_slice(name.as_bytes());
            header[100..107].copy_from_slice(format!("{mode:07o}").as_bytes());
            header[124..135].copy_from_slice(format!("{:011o}", bytes.len()).as_bytes());
            header[156] = if name.ends_with('/') { b'5' } else { b'0' };
            header[257..263].copy_from_slice(b"ustar\0");
            out.extend(header);
            out.extend(*bytes);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        out.extend([0u8; 1024]);
        out
    }

    #[test]
    fn a_linux_release_s_tools_are_unpacked_beside_it() {
        let folder = std::env::temp_dir().join(format!("limoniter-untar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let file = tar(&[("Copperline-1.1.0-x86_64-tools/", 0o755, b""), ("Copperline-1.1.0-x86_64-tools/copperline-ctl", 0o755, b"\x7fELF and more"), ("Copperline-1.1.0-x86_64-tools/README.txt", 0o644, b"read me"), ("Copperline-1.1.0-x86_64-tools/../../escaped", 0o644, b"no")]);
        untar(&file, &folder).unwrap();
        assert_eq!(std::fs::read(folder.join("copperline-ctl")).unwrap(), b"\x7fELF and more", "the one top folder left out");
        assert_eq!(std::fs::read_to_string(folder.join("README.txt")).unwrap(), "read me");
        assert!(!folder.parent().unwrap().join("escaped").exists(), "nothing gets out of the folder");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(folder.join("copperline-ctl")).unwrap().permissions().mode() & 0o777, 0o755);
        }
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Asks GitHub for real: cargo test -- --ignored emulator_get::tests::every_release_has_its_windows_file
    #[test]
    #[ignore]
    fn every_release_has_its_windows_file() {
        for id in ["zesarux", "b2", "openmsx", "fs-uae", "vice", "sameboy", "mesen", "fceux", "nestopia", "copperline"] {
            let (repo, wanted) = github(id).unwrap();
            let release = String::from_utf8(get(&format!("https://api.github.com/repos/{repo}/releases/latest")).unwrap()).unwrap();
            println!("{id}: {:?}", pick_asset(&release, wanted));
            assert!(pick_asset(&release, wanted).is_ok(), "{id}");
        }
    }
}
