//! Getting an emulator that isn't installed, the simplest way there is on this computer, and only when asked:
//!
//! - Windows: the emulator's latest release, unpacked into Limoniter's emulators folder beside its settings, where
//!   Find emulators then finds it. Most come from their GitHub pages (ZEsarUX, b2, openMSX, FS-UAE, VICE, Stella,
//!   DOSBox Staging, SameBoy, Mesen, FCEUX, Nestopia, Copperline, Gearsystem, Gearcoleco, TIC-80 and MAME); Hatari
//!   from its Framagit page; XRoar, WinUAE, BlastEm and Fuse from the link on their own download pages; and BGB from
//!   the address it's always at. Each is a zip, except MAME's, which is a 7-Zip archive that unpacks itself.
//!   Installers are never run: an emulator offered only as one has its page opened instead.
//! - Linux: Debian's and Ubuntu's own packages (Fuse, VICE, Hatari, openMSX, FS-UAE, Stella, MAME, BlastEm, DOSBox,
//!   QEMU, mGBA, FCEUX, Nestopia, SameBoy), installed with apt in the Terminal panel, where the command can be seen
//!   and the password typed. Copperline, which has no package, is downloaded from GitHub as its AppImage and its
//!   command-line tools, side by side; Mesen and TIC-80, which haven't either, as the one program their Linux
//!   zips hold.
//! - A Mac: Homebrew (VICE, Hatari, openMSX, FS-UAE, XRoar, Stella, MAME, DOSBox Staging, QEMU, mGBA, FCEUX,
//!   BlastEm, Copperline from its own tap, and SameBoy's and Gearsystem's apps), in the Terminal panel the same way,
//!   when Homebrew is installed.
//! - Anything else: the emulator's own download page.

use crate::emulators::Emulator;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The most an emulator's download is let grow to.
const MOST_DOWNLOAD: u64 = 400 << 20;

/// The longest a self-unpacking archive is given to unpack itself (MAME's is 600 MB unpacked).
const MOST_UNPACKING: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// How an emulator can be had here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Way {
    /// Downloaded from its latest release and unpacked by Limoniter.
    Download,
    /// A command to run in the Terminal panel.
    Command(String),
    /// Its web page, to download it from by hand.
    Page,
}

/// Where an emulator's Windows release comes from, and which of the files there is the one.
#[derive(Clone, Copy)]
enum Release {
    /// The latest release on GitHub: the repository, and what the file's name has in it.
    GitHub(&'static str, fn(&str) -> bool),
    /// The latest release on a GitLab server: the address of the project's latest release in its API, and what
    /// the file's name has in it.
    GitLab(&'static str, fn(&str) -> bool),
    /// A download page that links to each new release: the page, and what the link has in it.
    Linked(&'static str, fn(&str) -> bool),
    /// A file that's always at the same address.
    Fixed(&'static str),
}

/// Whether this is an ARM computer, which some emulators have Windows releases of their own for.
fn arm() -> bool {
    std::env::consts::ARCH == "aarch64"
}

/// MAME's Windows release for this kind of computer: a 7-Zip archive that unpacks itself, one for x64 and one
/// for ARM. The other files are its source (also an .exe) and its documentation.
fn mame_file(name: &str, arm: bool) -> bool {
    name.starts_with("mame") && name.ends_with(if arm { "b_arm64.exe" } else { "b_x64.exe" })
}

/// Where each emulator with a Windows release that needs no installing comes from.
fn windows_release(id: &str) -> Option<Release> {
    use Release::*;
    Some(match id {
        "zesarux" => GitHub("chernandezba/zesarux", |n| n.contains("_windows-") && !n.contains("legacy") && n.ends_with(".zip")),
        "b2" => GitHub("tom-seddon/b2", |n| n.starts_with("b2-windows-") && n.ends_with(".zip")),
        "openmsx" => GitHub("openMSX/openMSX", |n| n.ends_with("-windows-vc-x64-bin.zip")),
        "fs-uae" => GitHub("FrodeSolheim/fs-uae", |n| n.ends_with("_Windows_x86-64.zip")),
        "vice" => GitHub("VICE-Team/svn-mirror", |n| n.starts_with("GTK3VICE-") && n.ends_with("-win64.zip")),
        "stella" => GitHub("stella-emu/stella", |n| n.starts_with("Stella-") && n.ends_with("-windows.zip")),
        "dosbox" => GitHub("dosbox-staging/dosbox-staging", |n| n.starts_with("dosbox-staging-windows-x64-") && n.ends_with(".zip")),
        "sameboy" => GitHub("LIJI32/SameBoy", |n| n.starts_with("sameboy_winsdl_") && n.ends_with(".zip")),
        "mesen" => GitHub("SourMesen/Mesen2", |n| n.starts_with("Mesen_") && n.ends_with("_Windows.zip")),
        "fceux" => GitHub("TASEmulators/fceux", |n| n.starts_with("fceux-") && n.ends_with("-win64.zip")),
        "nestopia" => GitHub("0ldsk00l/nestopia", |n| n.starts_with("nestopia-") && n.ends_with("-windows-x86_64.zip")),
        // The portable zip, not the installer: copperline.exe with copperline-ctl.exe beside it.
        "copperline" => GitHub("CopperlineHQ/Copperline", |n| n.starts_with("Copperline-") && n.ends_with(if arm() { "-win-arm64.zip" } else { "-win-x64.zip" })),
        "gearsystem" => GitHub("drhelius/Gearsystem", |n| n.starts_with("Gearsystem-") && n.ends_with(if arm() { "-desktop-windows-arm64.zip" } else { "-desktop-windows-x64.zip" })),
        "gearcoleco" => GitHub("drhelius/Gearcoleco", |n| n.starts_with("Gearcoleco-") && n.ends_with(if arm() { "-desktop-windows-arm64.zip" } else { "-desktop-windows-x64.zip" })),
        "tic80" => GitHub("nesbox/TIC-80", |n| n.starts_with("tic80-") && n.ends_with("-win.zip")),
        "mame" => GitHub("mamedev/mame", |n| mame_file(n, arm())),
        // Hatari's releases are on Framagit, a GitLab, with the Windows build linked from each.
        "hatari" => GitLab("https://framagit.org/api/v4/projects/hatari%2Fhatari/releases/permalink/latest", |n| n.starts_with("hatari-") && n.ends_with("_windows64.zip")),
        // These link the latest release's zip from their download pages, beside its installer.
        "xroar" => Linked("https://www.6809.org.uk/xroar/", |link| link.contains("/xroar-") && link.ends_with("-w64.zip")),
        "winuae" => Linked("https://www.winuae.net/download/", |link| link.contains("/WinUAE") && link.ends_with("_x64.zip")),
        "blastem" => Linked("https://www.retrodev.com/blastem/downloads.html", |link| link.contains("/blastem-win64-") && link.ends_with(".zip")),
        // SourceForge's address for a file, which passes it on to a mirror.
        "fuse" => Linked("https://fuse-emulator.sourceforge.net/", |link| link.contains("/fuse-") && link.ends_with("-win32.zip/download")),
        "bgb" => Fixed("https://bgb.bircd.org/bgbw64.zip"),
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
        // From Ubuntu 26.04 and Debian 14; on an older one, apt says there's no such package.
        "sameboy" => "sameboy",
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
        // A formula now, not an app; ZEsarUX's and TIC-80's apps were switched off by Homebrew in September 2026,
        // as macOS wouldn't open them, so theirs are got from their pages.
        "stella" => ("stella", false),
        "mame" => ("mame", false),
        "dosbox" => ("dosbox-staging", false),
        "qemu" => ("qemu", false),
        "sameboy" => ("sameboy", true),
        // A formula, which Homebrew means to retire in May 2027, as it needs an old Qt.
        "mgba" => ("mgba", false),
        "fceux" => ("fceux", false),
        "blastem" => ("blastem", false),
        "gearsystem" => ("gearsystem", true),
        _ => return None,
    })
}

/// The emulators with no Linux package but a Linux release on GitHub that's a program in itself (an AppImage, or a
/// zip with only the program in it), and the tools that go beside it: the repository, and what each file's name
/// has in it. Only for x86-64, which is all some are built for.
fn linux_github(id: &str) -> Option<(&'static str, fn(&str) -> bool, fn(&str) -> bool)> {
    Some(match id {
        "copperline" => ("CopperlineHQ/Copperline", |n| n.starts_with("Copperline-") && n.ends_with("-x86_64.AppImage"), |n| n.starts_with("Copperline-") && n.ends_with("-x86_64-tools.tar.gz")),
        // Mesen needs SDL2, which a Linux desktop almost always has.
        "mesen" => ("SourMesen/Mesen2", |n| n.starts_with("Mesen_") && n.ends_with("_Linux_x64.zip"), |_| false),
        "tic80" => ("nesbox/TIC-80", |n| n.starts_with("tic80-") && n.ends_with("-linux.zip"), |_| false),
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
    if windows && windows_release(id).is_some() {
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

/// The file to download, from a GitLab server's description of the latest release: the first of the files
/// linked from it whose name is wanted.
fn pick_gitlab(release: &str, wanted: fn(&str) -> bool) -> Result<String, String> {
    let json: serde_json::Value = serde_json::from_str(release).map_err(|_| crate::i18n::tr("The answer about the latest release wasn't what was expected.").to_string())?;
    let tag = json["tag_name"].as_str().unwrap_or("").to_string();
    json["assets"]["links"]
        .as_array()
        .and_then(|links| links.iter().filter_map(|l| l["url"].as_str()).find(|url| wanted(url.rsplit('/').next().unwrap_or(url))))
        .map(str::to_string)
        .ok_or_else(|| crate::i18n::tf!("The latest release ({tag}) has no download for Windows that Limoniter knows."))
}

/// Every link on a web page, made whole: one relative to the page (dl/xroar.zip) or to its server (/files/a.zip)
/// is given the page's address before it.
fn page_links(html: &str, page: &str) -> Vec<String> {
    let server_end = page.find("://").map(|at| at + 3).and_then(|from| page[from..].find('/').map(|slash| from + slash)).unwrap_or(page.len());
    let folder = if page.len() > server_end { &page[..page.rfind('/').map_or(page.len(), |slash| slash + 1)] } else { page };
    let mut links = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find("href=") {
        rest = &rest[at + 5..];
        let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            continue;
        };
        let Some(end) = rest[1..].find(quote) else {
            break;
        };
        let link = rest[1..1 + end].replace("&amp;", "&");
        rest = &rest[1 + end..];
        links.push(if link.contains("://") {
            link
        } else if let Some(path) = link.strip_prefix('/') {
            format!("{}/{path}", &page[..server_end])
        } else if folder.ends_with('/') {
            format!("{folder}{link}")
        } else {
            format!("{folder}/{link}")
        });
    }
    links
}

/// The first link on a download page that's wanted.
fn pick_link(html: &str, page: &str, wanted: fn(&str) -> bool) -> Result<String, String> {
    page_links(html, page).into_iter().find(|link| wanted(link)).ok_or_else(|| crate::i18n::tf!("{page} has no download for Windows that Limoniter knows."))
}

fn text(bytes: Vec<u8>, from: &str) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|_| crate::i18n::tf!("{from}'s answer wasn't text."))
}

/// Where the file of an emulator's latest Windows release is.
fn release_address(release: Release) -> Result<String, String> {
    match release {
        Release::GitHub(repo, wanted) => {
            let address = format!("https://api.github.com/repos/{repo}/releases/latest");
            pick_asset(&text(get(&address)?, "GitHub")?, wanted).map(|(_, url)| url)
        }
        Release::GitLab(address, wanted) => pick_gitlab(&text(get(address)?, address)?, wanted),
        Release::Linked(page, wanted) => pick_link(&String::from_utf8_lossy(&get(page)?), page, wanted),
        Release::Fixed(address) => Ok(address.to_string()),
    }
}

/// Downloads an emulator's latest Windows release and unpacks it into its own folder, replacing any there before.
/// Gives the emulator's program. On Linux, an AppImage and its tools are downloaded instead (see `download_linux`).
pub fn download(emulator: &Emulator) -> Result<PathBuf, String> {
    if !cfg!(windows)
        && let Some(found) = linux_github(emulator.id)
    {
        return download_linux(emulator, found);
    }
    let release = windows_release(emulator.id).ok_or_else(|| crate::i18n::tf!("{} isn't downloaded by Limoniter: get it from {}.", emulator.name, emulator.website))?;
    let home = folder().ok_or(crate::i18n::tr("There's nowhere to keep it: Limoniter's settings folder couldn't be found."))?;
    let bytes = get(&release_address(release)?)?;
    let target = home.join(emulator.id);
    let _ = std::fs::remove_dir_all(&target);
    if bytes.starts_with(b"PK\x03\x04") {
        unzip(bytes, &target)?;
    } else if self_unpacking(&bytes) {
        let archive = home.join(format!("{}-release.exe", emulator.id));
        std::fs::write(&archive, &bytes).map_err(|e| crate::i18n::tf!("{} couldn't be written: {e}", archive.display()))?;
        drop(bytes);
        let unpacked = unpack_itself(&archive, &target);
        let _ = std::fs::remove_file(&archive);
        unpacked?;
    } else {
        return Err(crate::i18n::tf!("The download of {} isn't a kind Limoniter can unpack: get it from {}.", emulator.name, emulator.website));
    }
    find_program(&target, emulator.programs).ok_or_else(|| crate::i18n::tf!("{} was unpacked into {}, but its program wasn't found in it.", emulator.name, target.display()))
}

/// Whether a download is a Windows program with a 7-Zip archive after it, which is how 7-Zip makes an archive
/// that unpacks itself (MAME's release is one). An installer of any other kind isn't, and is never run.
fn self_unpacking(bytes: &[u8]) -> bool {
    const SEVEN_ZIP: &[u8] = b"7z\xBC\xAF\x27\x1C";
    // 7-Zip's unpacker is a few hundred kilobytes; the archive starts straight after it.
    bytes.starts_with(b"MZ") && bytes[..bytes.len().min(4 << 20)].windows(SEVEN_ZIP.len()).any(|w| w == SEVEN_ZIP)
}

/// What a 7-Zip archive that unpacks itself is told: -y, to ask nothing and write over anything there, and -o, the
/// folder to unpack into (7-Zip's own SFX module reads both; see SfxWin.cpp in its source).
fn unpack_arguments(target: &Path) -> Vec<std::ffi::OsString> {
    let mut folder = std::ffi::OsString::from("-o");
    folder.push(target.as_os_str());
    vec!["-y".into(), folder]
}

/// Runs a 7-Zip archive that unpacks itself, into a folder, waiting for it as long as it should take at most.
/// 7-Zip shows how far it's got in a little window of its own while it works.
fn unpack_itself(archive: &Path, target: &Path) -> Result<(), String> {
    let mut command = std::process::Command::new(archive);
    command.args(unpack_arguments(target)).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|e| crate::i18n::tf!("{} couldn't be started: {e}", archive.display()))?;
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return if status.success() { Ok(()) } else { Err(crate::i18n::tf!("{} couldn't be unpacked ({status}).", archive.display())) };
        }
        if started.elapsed() > MOST_UNPACKING {
            let _ = child.kill();
            let _ = child.wait();
            return Err(crate::i18n::tf!("{} took too long to unpack, and was stopped.", archive.display()));
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

/// Downloads an emulator's latest AppImage, kept under its own program name so Find emulators knows it, and the
/// command-line tools released with it unpacked beside it (for Copperline, copperline-ctl, which finds the
/// emulator beside itself). A zip is unpacked instead, and the program in it let run.
fn download_linux(emulator: &Emulator, (repo, program, tools): (&str, fn(&str) -> bool, fn(&str) -> bool)) -> Result<PathBuf, String> {
    let home = folder().ok_or(crate::i18n::tr("There's nowhere to keep it: Limoniter's settings folder couldn't be found."))?;
    let release = text(get(&format!("https://api.github.com/repos/{repo}/releases/latest"))?, "GitHub")?;
    let (_, image) = pick_asset(&release, program).map_err(|_| crate::i18n::tf!("The latest release of {} has no download for Linux that Limoniter knows.", emulator.name))?;
    let target = home.join(emulator.id);
    let _ = std::fs::remove_dir_all(&target);
    if image.ends_with(".zip") {
        unzip(get(&image)?, &target)?;
        let path = find_program(&target, emulator.programs).ok_or_else(|| crate::i18n::tf!("{} was unpacked into {}, but its program wasn't found in it.", emulator.name, target.display()))?;
        make_runnable(&path, 0o755);
        return Ok(path);
    }
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
        assert_eq!(way_for("hatari", true, false, false, false, false), Way::Download, "from Framagit");
        assert_eq!(way_for("cspect", true, false, false, false, false), Way::Page, "only from itch.io");
        assert_eq!(way_for("qemu", true, false, false, false, false), Way::Page, "only an installer");
        assert_eq!(way_for("mame", true, false, false, false, false), Way::Download, "from GitHub, unpacking itself");
        for id in ["bgb", "xroar", "winuae", "blastem", "fuse", "gearsystem", "gearcoleco", "tic80"] {
            assert_eq!(way_for(id, true, false, false, false, false), Way::Download, "{id}");
        }
        assert_eq!(way_for("vice", false, false, true, false, true), Way::Command("sudo apt-get install vice".into()));
        assert_eq!(way_for("fuse", false, false, true, false, true), Way::Command("sudo apt-get install fuse-emulator-gtk".into()));
        assert_eq!(way_for("b2", false, false, true, false, true), Way::Page);
        assert_eq!(way_for("zesarux", false, true, false, true, false), Way::Page, "Homebrew switched its app off");
        assert_eq!(way_for("tic80", false, true, false, true, false), Way::Page, "Homebrew switched its app off");
        assert_eq!(way_for("stella", false, true, false, true, false), Way::Command("brew install stella".into()), "a formula now");
        assert_eq!(way_for("mgba", false, true, false, true, false), Way::Command("brew install mgba".into()), "a formula, not an app");
        assert_eq!(way_for("gearsystem", false, true, false, true, false), Way::Command("brew install --cask gearsystem".into()));
        assert_eq!(way_for("mame", false, true, false, true, false), Way::Command("brew install mame".into()));
        // On Linux, Mesen and TIC-80 have no package, so their own Linux builds are downloaded.
        assert_eq!(way_for("mesen", false, false, true, false, true), Way::Download);
        assert_eq!(way_for("tic80", false, false, true, false, true), Way::Download);
        assert_eq!(way_for("mame", false, false, true, false, true), Way::Command("sudo apt-get install mame".into()));
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
        let wanted = |id: &str| match windows_release(id) {
            Some(Release::GitHub(_, wanted)) => wanted,
            _ => panic!("{id} isn't from GitHub"),
        };
        let pick = |id: &str, names: &[&str]| pick_asset(&release(names), wanted(id)).map(|(_, url)| url.rsplit('/').next().unwrap().to_string());
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
        // As their latest releases name them (20 November 2026).
        assert_eq!(pick("stella", &["Stella-7.0-macos.dmg", "stella-7.0-src.tar.xz", "Stella-7.0c-windows.zip", "Stella-7.0c-x64.exe", "stella_7.0_amd64.deb"]).unwrap(), "Stella-7.0c-windows.zip");
        assert_eq!(pick("dosbox", &["dosbox-staging-linux-x86_64-v0.83.1.tar.xz", "dosbox-staging-windows-x64-v0.83.1-setup.exe", "dosbox-staging-windows-x64-v0.83.1.zip"]).unwrap(), "dosbox-staging-windows-x64-v0.83.1.zip");
        let tic80 = ["tic80-v1.4-html.zip", "tic80-v1.4-linux-arm64.deb", "tic80-v1.4-linux-arm64.zip", "tic80-v1.4-linux.deb", "tic80-v1.4-linux.zip", "tic80-v1.4-mac-arm64.app.zip", "tic80-v1.4-mac.zip", "tic80-v1.4-switch.zip", "tic80-v1.4-win.zip"];
        assert_eq!(pick("tic80", &tic80).unwrap(), "tic80-v1.4-win.zip");
        let mame = ["mame0289b_arm64.exe", "mame0289b_x64.exe", "mame0289lx.zip", "mame0289s.exe", "SHA1SUMS", "SHA256SUMS", "whatsnew_0289.txt"];
        let pick_mame = |arm: bool| mame.iter().find(|n| mame_file(n, arm)).copied();
        assert_eq!(pick_mame(false), Some("mame0289b_x64.exe"));
        assert_eq!(pick_mame(true), Some("mame0289b_arm64.exe"), "an ARM computer's own");
        assert!(!mame_file("mame0289s.exe", false) && !mame_file("mame0289lx.zip", false), "not its source, nor its documentation");
        if !arm() {
            assert_eq!(pick("mame", &mame).unwrap(), "mame0289b_x64.exe");
            let gearsystem = ["Gearsystem-3.9.25-desktop-macos-arm64.zip", "Gearsystem-3.9.25-desktop-ubuntu24.04-x64.zip", "Gearsystem-3.9.25-desktop-windows-arm64.zip", "Gearsystem-3.9.25-desktop-windows-x64.zip", "Gearsystem-3.9.25-libretro-linux-x64.zip", "Gearsystem-3.9.25-mcpb-windows-x64.mcpb"];
            assert_eq!(pick("gearsystem", &gearsystem).unwrap(), "Gearsystem-3.9.25-desktop-windows-x64.zip");
            let gearcoleco = ["Gearcoleco-1.8.0-desktop-bsd-x64.zip", "Gearcoleco-1.8.0-desktop-windows-arm64.zip", "Gearcoleco-1.8.0-desktop-windows-x64.zip", "Gearcoleco-1.8.0-mcpb-windows-x64.mcpb"];
            assert_eq!(pick("gearcoleco", &gearcoleco).unwrap(), "Gearcoleco-1.8.0-desktop-windows-x64.zip");
        }
        let linux = |id: &str, names: &[&str]| pick_asset(&release(names), linux_github(id).unwrap().1).map(|(_, url)| url.rsplit('/').next().unwrap().to_string());
        assert_eq!(linux("tic80", &tic80).unwrap(), "tic80-v1.4-linux.zip", "not the ARM one");
        assert_eq!(linux("mesen", &["Mesen_2.1.1_Linux_ARM64.zip", "Mesen_2.1.1_Linux_x64.zip", "Mesen_2.1.1_Windows.zip"]).unwrap(), "Mesen_2.1.1_Linux_x64.zip");
        assert_eq!(way_for("mgba", false, false, true, false, true), Way::Command("sudo apt-get install mgba-qt".into()));
        assert_eq!(way_for("sameboy", false, true, false, true, false), Way::Command("brew install --cask sameboy".into()));
        // Copperline's, as its 1.1.0 release names them.
        let copperline = ["Copperline-1.1.0-libretro-Linux-X64.zip", "Copperline-1.1.0-libretro-Windows-X64.zip", "Copperline-1.1.0-macos-universal.dmg", "Copperline-1.1.0-win-arm64.zip", "Copperline-1.1.0-win-x64-setup.exe", "Copperline-1.1.0-win-x64.zip", "Copperline-1.1.0-x86_64-net-helper.tar.gz", "Copperline-1.1.0-x86_64-tools.tar.gz", "Copperline-1.1.0-x86_64.AppImage"];
        if !arm() {
            assert_eq!(pick("copperline", &copperline).unwrap(), "Copperline-1.1.0-win-x64.zip");
        }
        let (_, image, tools) = linux_github("copperline").unwrap();
        let named = |wanted: fn(&str) -> bool| pick_asset(&release(&copperline), wanted).map(|(_, url)| url.rsplit('/').next().unwrap().to_string());
        assert_eq!(named(image).unwrap(), "Copperline-1.1.0-x86_64.AppImage");
        assert_eq!(named(tools).unwrap(), "Copperline-1.1.0-x86_64-tools.tar.gz");
    }

    #[test]
    fn a_release_is_found_on_gitlab_and_on_download_pages() {
        // Hatari's latest release, as Framagit's API describes it (cut short).
        let hatari = r#"{"name":"v2.6.1","tag_name":"v2.6.1","assets":{"count":8,"sources":[{"format":"zip","url":"https://framagit.org/hatari/hatari/-/archive/v2.6.1/hatari-v2.6.1.zip"}],"links":[{"name":"Windows 64-bit binary","url":"https://framagit.org/hatari/releases/-/raw/main/v2.6.1/hatari-2.6.1_windows64.zip"},{"name":"Windows 32-bit binary","url":"https://framagit.org/hatari/releases/-/raw/main/v2.6.1/hatari-2.6.1_windows.zip"},{"name":"macOS binary","url":"https://framagit.org/hatari/releases/-/raw/main/v2.6.1/hatari-2.6.1_macos.zip"}]}}"#;
        let Some(Release::GitLab(_, wanted)) = windows_release("hatari") else { panic!("Hatari is on Framagit") };
        assert_eq!(pick_gitlab(hatari, wanted).unwrap(), "https://framagit.org/hatari/releases/-/raw/main/v2.6.1/hatari-2.6.1_windows64.zip");
        assert!(pick_gitlab(r#"{"tag_name":"v3","assets":{"links":[]}}"#, wanted).is_err());
        // Each download page, as it links its files (20 November 2026).
        let linked = |id: &str, html: &str| {
            let Some(Release::Linked(page, wanted)) = windows_release(id) else { panic!("{id} is linked from its page") };
            pick_link(html, page, wanted)
        };
        let xroar = "<li>64-bit Windows: <a href='dl/xroar-1.13.1-w64.zip'>ZIP archive</a>,\n    <a href='dl/xroar-1.13.1-w64.msi'>MSI installer</a>\n<li>32-bit Windows: <a href='dl/xroar-1.13.1-w32.zip'>ZIP archive</a>";
        assert_eq!(linked("xroar", xroar).unwrap(), "https://www.6809.org.uk/xroar/dl/xroar-1.13.1-w64.zip");
        let winuae = r#"<a href="https://download.abime.net/winuae/releases/InstallWinUAE6030_x64.msi">x</a><a href="https://download.abime.net/winuae/releases/WinUAE6030.zip">x</a><a href="https://download.abime.net/winuae/releases/WinUAE6030_x64.zip">x</a><a href="https://download.abime.net/winuae/releases/translations/WinUAE6030_translation.zip">x</a>"#;
        assert_eq!(linked("winuae", winuae).unwrap(), "https://download.abime.net/winuae/releases/WinUAE6030_x64.zip");
        let blastem = r#"<a href="blastem-win32-1.0.0.zip">32-bit</a> <a href="blastem-win64-1.0.0.zip">64-bit</a>"#;
        assert_eq!(linked("blastem", blastem).unwrap(), "https://www.retrodev.com/blastem/blastem-win64-1.0.0.zip");
        let fuse = r#"<a href="https://sourceforge.net/projects/fuse-emulator/files/fuse/1.10.1/fuse-1.10.1-win32-setup.exe/download">installer</a> <a href="https://sourceforge.net/projects/fuse-emulator/files/fuse/1.10.1/fuse-1.10.1-win32.zip/download">zip</a> <a href="https://sourceforge.net/projects/fuse-emulator/files/fuse-utils/1.5.1/fuse-utils-1.5.1-win32.zip/download">utils</a>"#;
        assert_eq!(linked("fuse", fuse).unwrap(), "https://sourceforge.net/projects/fuse-emulator/files/fuse/1.10.1/fuse-1.10.1-win32.zip/download");
        assert!(linked("xroar", "<a href='dl/xroar-1.13.1.tar.gz'>source</a>").is_err());
        // Links relative to the server, with &amp; in them, and on a page with no path.
        assert_eq!(page_links(r#"<a href="/files/a.zip?x=1&amp;y=2">a</a>"#, "https://example.org/emu/get.html"), ["https://example.org/files/a.zip?x=1&y=2"]);
        assert_eq!(page_links(r#"<a href="b.zip">b</a>"#, "https://example.org"), ["https://example.org/b.zip"]);
        assert!(matches!(windows_release("bgb"), Some(Release::Fixed("https://bgb.bircd.org/bgbw64.zip"))));
    }

    #[test]
    fn only_a_7_zip_archive_that_unpacks_itself_is_run() {
        // As MAME's begins: 7-Zip's unpacker (a Windows program), then the archive at 205,824 bytes in.
        let mut mame = b"MZ".to_vec();
        mame.resize(205_824, 0);
        mame.extend(b"7z\xBC\xAF\x27\x1C\0\x04");
        assert!(self_unpacking(&mame));
        let mut installer = b"MZ".to_vec();
        installer.resize(300_000, 0x55);
        assert!(!self_unpacking(&installer), "another kind of installer is never run");
        assert!(!self_unpacking(b"PK\x03\x04 7z\xBC\xAF\x27\x1C"), "a zip is unzipped");
        // It's told to ask nothing and where to unpack, as one word each, as 7-Zip's SFX module reads them.
        let target = std::env::temp_dir().join("Limoniter emulators").join("mame");
        let arguments = unpack_arguments(&target);
        assert_eq!(arguments[0], "-y");
        assert_eq!(arguments[1], std::ffi::OsString::from(format!("-o{}", target.display())));
        assert_eq!(arguments.len(), 2);
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

    /// Asks each emulator's release page for real: cargo test -- --ignored emulator_get::tests::every_release_has_its_windows_file
    #[test]
    #[ignore]
    fn every_release_has_its_windows_file() {
        for emulator in crate::emulators::KNOWN {
            if let Some(release) = windows_release(emulator.id) {
                let address = release_address(release);
                println!("{}: {address:?}", emulator.id);
                assert!(address.is_ok(), "{}", emulator.id);
            }
            if let Some((repo, program, _)) = linux_github(emulator.id) {
                let release = String::from_utf8(get(&format!("https://api.github.com/repos/{repo}/releases/latest")).unwrap()).unwrap();
                println!("{} on Linux: {:?}", emulator.id, pick_asset(&release, program));
                assert!(pick_asset(&release, program).is_ok(), "{} on Linux", emulator.id);
            }
        }
    }
}
