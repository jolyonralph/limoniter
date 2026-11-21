//! Installing Boriel's ZX Basic (zxbc) for the person, when it isn't installed and they ask, into a folder of
//! Limoniter's own beside its settings, where F9 then finds it straight away:
//!
//! - Windows and Linux on a PC: the ready-made build from boriel.com, its author's site, as its download page
//!   (zxbasic.readthedocs.io's archive page) links it: a zip of zxbc.exe and the Python it runs on for Windows, a
//!   tar.gz of zxbc and a Python AppImage for Linux. The listing of every file on boriel.com is read too, in case
//!   it has a newer one; the newer is taken.
//! - Anywhere else (a Mac, Linux on ARM), or when the download doesn't work: a Python environment of Limoniter's
//!   own (python -m venv), made with the newest Python 3 installed, with zxbasic installed in it from PyPI by pip.
//!   pip picks the newest zxbasic that Python can run. The Mac's own tar.gz is only the Python scripts, which need
//!   a Python 3.14 a Mac rarely has, so it isn't used.
//! - With no Python either, the download page is opened.
//!
//! Boriel's ZX Basic is free software (the compiler under the AGPL 3, its library under the MIT licence), so
//! fetching it from its own site for the person is allowed; Limoniter doesn't come with it.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// The download page Boriel's ZX Basic's README points to, which links the latest build for each system.
pub const ARCHIVE_PAGE: &str = "https://zxbasic.readthedocs.io/en/latest/archive/";

/// Every file ever released, listed by boriel.com, betas among them. Its links are written wrongly (/zxb/... for
/// /files/zxb/...), so only the names are read from it.
pub const LISTING: &str = "https://www.boriel.com/files/zxb/";

/// The oldest Python whose pip finds a zxbasic it can run (1.12, for Python 3.6 and on, is the oldest there).
const OLDEST_PYTHON: (u32, u32) = (3, 8);

/// How Boriel's ZX Basic can be installed here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Way {
    /// Its ready-made build, downloaded and unpacked: how the file's name ends (-win32.zip, -linux64.tar.gz).
    Download(&'static str),
    /// A Python environment of Limoniter's own, with zxbasic installed in it by pip: the Python, and its version.
    Python(PathBuf, (u32, u32)),
    /// Its download page, as Python 3 is needed and none was found.
    Page,
}

/// The way to install it on this computer. On a Mac this asks the Pythons installed for their versions, so it's
/// worked out once, when a box offering it opens.
pub fn way() -> Way {
    let linux_pc = cfg!(target_os = "linux") && std::env::consts::ARCH == "x86_64";
    way_for(cfg!(windows), linux_pc, find_python)
}

fn way_for(windows: bool, linux_pc: bool, python: impl FnOnce() -> Option<(PathBuf, (u32, u32))>) -> Way {
    if windows {
        // The 32-bit build runs on every Windows, ARM ones too.
        return Way::Download("-win32.zip");
    }
    if linux_pc {
        return Way::Download("-linux64.tar.gz");
    }
    python().map_or(Way::Page, |(path, version)| Way::Python(path, version))
}

/// What the way is, said in a line under its button.
pub fn explain(way: &Way) -> String {
    match way {
        Way::Download(_) => crate::i18n::tr("Limoniter downloads its latest ready-made build from boriel.com, its author's site, and keeps it in a folder beside its own settings. Nothing else on the computer is changed.").to_string(),
        Way::Python(python, (major, minor)) => crate::i18n::tf!("Limoniter makes a Python environment of its own, beside its settings, with Python {major}.{minor} ({}), and pip installs zxbasic in it from PyPI, the Python Package Index. Nothing else on the computer is changed.", python.display()),
        Way::Page => crate::i18n::tr("Boriel's ZX Basic runs on Python 3, which wasn't found on this computer. Install Python from python.org (or with Homebrew on a Mac), then try again, or get Boriel's ZX Basic from its download page.").to_string(),
    }
}

/// Where Limoniter keeps the compilers it installs, each in a folder of its own.
pub fn folder() -> Option<PathBuf> {
    if let Some(folder) = std::env::var_os("LIMONITER_COMPILERS") {
        return Some(folder.into());
    }
    if cfg!(test) {
        return None;
    }
    crate::settings::data_folder().map(|d| d.join("compilers"))
}

/// The zxbc Limoniter installed, if it did.
pub fn installed() -> Option<PathBuf> {
    folder().and_then(|home| installed_in(&home))
}

/// The zxbc installed in a folder: the ready-made build's, or the Python environment's.
fn installed_in(home: &Path) -> Option<PathBuf> {
    let build = home.join("zxbasic");
    if build.is_dir()
        && let Some(found) = crate::emulator_get::find_program(&build, &["zxbc"])
    {
        return Some(found);
    }
    Some(venv_program(&home.join("zxbasic-python"), "zxbc")).filter(|p| p.is_file())
}

/// A program in a Python environment: in Scripts, as an .exe, on Windows; in bin elsewhere.
fn venv_program(venv: &Path, name: &str) -> PathBuf {
    if cfg!(windows) { venv.join("Scripts").join(format!("{name}.exe")) } else { venv.join("bin").join(name) }
}

/// What zxbc is run with. Linux's build runs on a Python AppImage, which mounts itself with FUSE; where there's
/// no FUSE (a container, some virtual machines), the AppImage is told to unpack itself and run instead.
pub fn environment() -> Vec<(String, String)> {
    let fuse = Path::new("/dev/fuse").exists() && ["/bin/fusermount", "/usr/bin/fusermount", "/bin/fusermount3", "/usr/bin/fusermount3"].iter().any(|p| Path::new(p).is_file());
    if cfg!(target_os = "linux") && !fuse { vec![("APPIMAGE_EXTRACT_AND_RUN".into(), "1".into())] } else { Vec::new() }
}

/// A version from a file's name, zxbasic-1.19.0-win32.zip or zxbasic-v1.19.0-win32.zip, with the ending given:
/// only a release's, so betas and builds for one Python (-py314-) are left out.
fn version_in(name: &str, ending: &str) -> Option<Vec<u32>> {
    let middle = name.strip_prefix("zxbasic-")?.strip_suffix(ending)?;
    let numbers: Option<Vec<u32>> = middle.strip_prefix('v').unwrap_or(middle).split('.').map(|n| n.parse().ok()).collect();
    numbers.filter(|n| n.len() >= 2)
}

fn file_name(link: &str) -> &str {
    link.rsplit('/').next().unwrap_or(link)
}

/// The newest build with this ending linked from the download page: its version and address.
fn pick_from_page(html: &str, ending: &str) -> Option<(Vec<u32>, String)> {
    crate::emulator_get::page_links(html, ARCHIVE_PAGE).into_iter().filter_map(|link| version_in(file_name(&link), ending).map(|v| (v, link))).max_by(|a, b| a.0.cmp(&b.0))
}

/// The newest build with this ending in boriel.com's listing, at its address in /files/zxb/.
fn pick_from_listing(html: &str, ending: &str) -> Option<(Vec<u32>, String)> {
    crate::emulator_get::page_links(html, LISTING).iter().map(|link| file_name(link)).filter_map(|name| version_in(name, ending).map(|v| (v, format!("{LISTING}{name}")))).max_by(|a, b| a.0.cmp(&b.0))
}

/// The newer of the two, the download page's when they're the same.
fn newest(page: Option<(Vec<u32>, String)>, listing: Option<(Vec<u32>, String)>) -> Option<(String, String)> {
    let best = match (page, listing) {
        (Some(page), Some(listing)) => Some(if listing.0 > page.0 { listing } else { page }),
        (page, listing) => page.or(listing),
    };
    best.map(|(version, address)| (version.iter().map(u32::to_string).collect::<Vec<_>>().join("."), address))
}

/// The latest build with this ending: its version and where it is.
fn latest(ending: &str) -> Result<(String, String), String> {
    let read = |address: &str| crate::emulator_get::get(address).map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    let page = read(ARCHIVE_PAGE);
    let listing = read(LISTING);
    let found = newest(page.as_deref().ok().and_then(|html| pick_from_page(html, ending)), listing.as_deref().ok().and_then(|html| pick_from_listing(html, ending)));
    found.ok_or_else(|| match page.err().or(listing.err()) {
        Some(problem) => problem,
        None => crate::i18n::tf!("{ARCHIVE_PAGE} has no build of Boriel's ZX Basic for this computer that Limoniter knows."),
    })
}

/// Unpacks a downloaded build into a folder: a zip, or a tar.gz.
fn unpack(bytes: Vec<u8>, target: &Path) -> Result<(), String> {
    if bytes.starts_with(b"PK\x03\x04") {
        crate::emulator_get::unzip(bytes, target)
    } else if bytes.starts_with(&[0x1F, 0x8B]) {
        crate::emulator_get::untar(&crate::media::gunzip_bytes(&bytes)?, target)
    } else {
        Err(crate::i18n::tr("The download isn't a zip or a tar.gz, so it couldn't be unpacked.").into())
    }
}

/// Downloads the latest build and unpacks it into the zxbasic folder, in place of any there before (which is
/// kept until the new one's unpacked). Gives its zxbc.
fn download(ending: &str, home: &Path, say: &dyn Fn(String)) -> Result<PathBuf, String> {
    say(crate::i18n::tr("Looking for the latest build of Boriel's ZX Basic\u{2026}").into());
    let (version, address) = latest(ending)?;
    say(crate::i18n::tf!("Downloading Boriel's ZX Basic {version} from {address}\u{2026}"));
    let bytes = crate::emulator_get::get(&address)?;
    say(crate::i18n::tf!("Unpacking it ({} MB)\u{2026}", bytes.len().div_ceil(1 << 20)));
    unpack_into(bytes, home)
}

/// Unpacks a build beside the zxbasic folder, then puts it in that folder's place. Gives its zxbc.
fn unpack_into(bytes: Vec<u8>, home: &Path) -> Result<PathBuf, String> {
    let fresh = home.join("zxbasic-new");
    let _ = std::fs::remove_dir_all(&fresh);
    if let Err(problem) = unpack(bytes, &fresh) {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(problem);
    }
    if crate::emulator_get::find_program(&fresh, &["zxbc"]).is_none() {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(crate::i18n::tr("The download was unpacked, but zxbc wasn't in it.").into());
    }
    let target = home.join("zxbasic");
    let _ = std::fs::remove_dir_all(&target);
    std::fs::rename(&fresh, &target).map_err(|e| crate::i18n::tf!("{} couldn't be renamed: {e}", fresh.display()))?;
    crate::emulator_get::find_program(&target, &["zxbc"]).ok_or_else(|| crate::i18n::tr("The download was unpacked, but zxbc wasn't in it.").into())
}

/// A Python's version, from what python --version prints ("Python 3.12.1").
fn python_version(said: &str) -> Option<(u32, u32)> {
    let numbers = said.trim().strip_prefix("Python ")?;
    let mut parts = numbers.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.trim_end_matches(|c: char| !c.is_ascii_digit()).parse().ok()?;
    Some((major, minor))
}

/// The Pythons there might be, newest names first: on the PATH, and where installers put Python when a program
/// started from the desktop may not have it on its path.
fn python_candidates() -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = std::env::var_os("PATH").map(|paths| std::env::split_paths(&paths).collect()).unwrap_or_default();
    if cfg!(target_os = "macos") {
        folders.extend(["/opt/homebrew/bin", "/usr/local/bin", "/Library/Frameworks/Python.framework/Versions/Current/bin"].map(PathBuf::from));
    } else if cfg!(windows) {
        // python.org's installer puts each version in a folder of its own, for the person or for everyone.
        for root in [std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join("Programs").join("Python")), std::env::var_os("ProgramFiles").map(PathBuf::from)].into_iter().flatten() {
            if let Ok(entries) = std::fs::read_dir(&root) {
                let mut found: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().starts_with("python3"))).collect();
                found.sort();
                folders.extend(found.into_iter().rev());
            }
        }
    } else {
        folders.extend(["/usr/bin", "/usr/local/bin"].map(PathBuf::from));
    }
    let names: Vec<String> = (8..=20).rev().map(|minor| format!("python3.{minor}")).chain(["python3".to_string(), "python".to_string()]).collect();
    let mut candidates: Vec<PathBuf> = Vec::new();
    for folder in &folders {
        // Windows' python.exe in WindowsApps only offers to install Python from the Store.
        if folder.to_string_lossy().contains("WindowsApps") {
            continue;
        }
        // A Mac's /usr/bin/python3 asks to install Apple's developer tools when they aren't, rather than running.
        if cfg!(target_os = "macos") && folder == Path::new("/usr/bin") && !Path::new("/Library/Developer/CommandLineTools/usr/bin/python3").is_file() && !Path::new("/Applications/Xcode.app").is_dir() {
            continue;
        }
        for name in &names {
            let path = folder.join(if cfg!(windows) { format!("{name}.exe") } else { name.clone() });
            if path.is_file() && !candidates.contains(&path) {
                candidates.push(path);
            }
        }
    }
    candidates
}

/// The newest Python 3 installed that's new enough, and its version.
fn find_python() -> Option<(PathBuf, (u32, u32))> {
    let mut best: Option<(PathBuf, (u32, u32))> = None;
    // Several names are usually the same Python; a few answers are enough.
    for candidate in python_candidates().into_iter().take(8) {
        let folder = candidate.parent().map(Path::to_path_buf).unwrap_or_default();
        let Ok((true, said)) = crate::compilers::run(&candidate, &["--version".to_string()], &folder, &[], Duration::from_secs(10)) else {
            continue;
        };
        if let Some(version) = said.iter().find_map(|line| python_version(line))
            && version.0 == 3
            && version >= OLDEST_PYTHON
            && best.as_ref().is_none_or(|(_, b)| version > *b)
        {
            best = Some((candidate, version));
        }
    }
    best
}

/// The commands that make the Python environment and install zxbasic in it: python -m venv, then the
/// environment's own pip.
fn python_commands(python: &Path, venv: &Path) -> Vec<(PathBuf, Vec<String>)> {
    let shown = |p: &Path| p.display().to_string();
    vec![
        (python.to_path_buf(), vec!["-m".into(), "venv".into(), shown(venv)]),
        (venv_program(venv, "python"), vec!["-m".into(), "pip".into(), "install".into(), "--disable-pip-version-check".into(), "--no-input".into(), "--upgrade".into(), "zxbasic".into()]),
    ]
}

/// Makes the Python environment, in place of any there before, and installs zxbasic in it. Gives its zxbc.
fn python_install(python: &Path, home: &Path, say: &dyn Fn(String)) -> Result<PathBuf, String> {
    let venv = home.join("zxbasic-python");
    let _ = std::fs::remove_dir_all(&venv);
    let steps = python_commands(python, &venv);
    let said = [crate::i18n::tf!("Making a Python environment in {}\u{2026}", venv.display()), crate::i18n::tr("Installing zxbasic from PyPI with pip\u{2026}").to_string()];
    for ((program, args), words) in steps.iter().zip(said) {
        say(words);
        let (worked, printed) = crate::compilers::run(program, args, home, &[], Duration::from_secs(600))?;
        for line in printed.iter().filter(|l| !l.trim().is_empty()) {
            say(line.clone());
        }
        if !worked {
            // Debian and Ubuntu keep venv's pip in a package of its own.
            if printed.iter().any(|l| l.contains("ensurepip")) {
                return Err(crate::i18n::tr("Python couldn't make an environment, as its venv module isn't installed. On Debian or Ubuntu, install it with: sudo apt install python3-venv").into());
            }
            return Err(crate::i18n::tf!("{} {} didn't work.", program.display(), args.join(" ")));
        }
    }
    Some(venv_program(&venv, "zxbc")).filter(|p| p.is_file()).ok_or_else(|| crate::i18n::tr("pip installed zxbasic, but its zxbc wasn't found.").into())
}

/// Runs zxbc --version, to be sure it works: what it says.
fn check(program: &Path) -> Result<String, String> {
    let folder = program.parent().map(Path::to_path_buf).unwrap_or_default();
    let (worked, printed) = crate::compilers::run(program, &["--version".to_string()], &folder, &environment(), Duration::from_secs(60))?;
    let said = printed.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n");
    if worked { Ok(said) } else { Err(crate::i18n::tf!("{} was installed, but doesn't run:\n{said}", program.display())) }
}

/// Installs Boriel's ZX Basic the way given, into a folder of Limoniter's own, saying how it's going. A download
/// that doesn't work is followed by the Python way, when there's a Python. Gives its zxbc and what zxbc --version
/// says.
pub fn install(way: &Way, home: &Path, say: &dyn Fn(String)) -> Result<(PathBuf, String), String> {
    std::fs::create_dir_all(home).map_err(|e| crate::i18n::tf!("{} couldn't be made: {e}", home.display()))?;
    let program = match way {
        Way::Download(ending) => match download(ending, home, say) {
            Ok(program) => program,
            Err(problem) => match find_python() {
                Some((python, (major, minor))) => {
                    say(crate::i18n::tf!("The download didn't work ({problem}), so Python {major}.{minor} is tried instead."));
                    python_install(&python, home, say)?
                }
                None => return Err(problem),
            },
        },
        Way::Python(python, _) => python_install(python, home, say)?,
        Way::Page => return Err(explain(way)),
    };
    say(crate::i18n::tf!("Checking {} runs\u{2026}", program.display()));
    let version = check(&program)?;
    Ok((program, version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_computer_gets_its_own_way() {
        let python = || Some((PathBuf::from("/opt/homebrew/bin/python3.13"), (3, 13)));
        assert_eq!(way_for(true, false, || None), Way::Download("-win32.zip"));
        assert_eq!(way_for(false, true, || None), Way::Download("-linux64.tar.gz"));
        assert_eq!(way_for(false, false, python), Way::Python(PathBuf::from("/opt/homebrew/bin/python3.13"), (3, 13)), "a Mac, or Linux on ARM");
        assert_eq!(way_for(false, false, || None), Way::Page);
        assert!(explain(&Way::Python(PathBuf::from("/usr/bin/python3"), (3, 12))).contains("Python 3.12"));
    }

    #[test]
    fn a_python_s_version_is_read() {
        assert_eq!(python_version("Python 3.12.1"), Some((3, 12)));
        assert_eq!(python_version("Python 3.14.0rc2\n"), Some((3, 14)));
        assert_eq!(python_version("Python 3.13t"), Some((3, 13)));
        assert_eq!(python_version("Python 2.7.18"), Some((2, 7)));
        assert_eq!(python_version("Python was not found; run without arguments to install from the Microsoft Store"), None);
    }

    /// The download page and boriel.com's listing, as they were on 20 November 2026 (cut short).
    const PAGE: &str = r#"<a href="http://github.com/boriel-basic/zxbasic">GitHub</a>
<a href="https://www.boriel.com/files/zxb/zxbasic-1.19.0-win32.zip"><img src="win.png"></a>
<a href="https://www.boriel.com/files/zxb/zxbasic-1.19.0-macos.tar.gz"><img src="mac.png"></a>
<a href="https://www.boriel.com/files/zxb/zxbasic-1.19.0-linux64.tar.gz"><img src="linux.png"></a>
<a href="https://www.boriel.com/files/zxb/zxbasic-1.19.0.zip">zip</a>
<a href="https://www.boriel.com/files/zxb/zxbasic-1.19.0.tar.gz">tar.gz</a>
<a href="https://www.boriel.com/files/zxb/">archive</a>"#;
    const LISTED: &str = r#"<h1>ZX BASIC version archive</h1>
<table><tr><td><a href="/zxb/zxbasic-1.10.0-win32.zip">zxbasic-1.10.0-win32.zip</a></td>
<tr><td><a href="/zxb/zxbasic-1.9.9-win32.zip">zxbasic-1.9.9-win32.zip</a></td>
<tr><td><a href="/zxb/zxbasic-1.18.7-linux64.tar.gz">zxbasic-1.18.7-linux64.tar.gz</a></td>
<tr><td><a href="/zxb/zxbasic-pera-win32.zip">zxbasic-pera-win32.zip</a></td>
<tr><td><a href="/zxb/zxbasic-v1.19.0-linux64.tar.gz">zxbasic-v1.19.0-linux64.tar.gz</a></td>
<tr><td><a href="/zxb/zxbasic-v1.19.0-py314-win32.zip">zxbasic-v1.19.0-py314-win32.zip</a></td>
<tr><td><a href="/zxb/zxbasic-v1.19.0-win32.zip">zxbasic-v1.19.0-win32.zip</a></td>
<tr><td><a href="/zxb/zxbasic-v1.19.1-beta2-win32.zip">zxbasic-v1.19.1-beta2-win32.zip</a></td>"#;

    #[test]
    fn the_latest_build_is_picked_from_the_page_and_the_listing() {
        let page = pick_from_page(PAGE, "-win32.zip");
        assert_eq!(page, Some((vec![1, 19, 0], "https://www.boriel.com/files/zxb/zxbasic-1.19.0-win32.zip".to_string())));
        assert_eq!(pick_from_page(PAGE, "-linux64.tar.gz").map(|p| p.1).as_deref(), Some("https://www.boriel.com/files/zxb/zxbasic-1.19.0-linux64.tar.gz"));
        // 1.10 is newer than 1.9; a beta, a build for one Python and a named one aren't releases; the listing's
        // links are put right.
        let listing = pick_from_listing(LISTED, "-win32.zip");
        assert_eq!(listing, Some((vec![1, 19, 0], "https://www.boriel.com/files/zxb/zxbasic-v1.19.0-win32.zip".to_string())));
        assert_eq!(pick_from_listing(&LISTED.replace("v1.19.0", "v1.0"), "-win32.zip").map(|p| p.0), Some(vec![1, 10, 0]));
        // The same version: the download page's. A newer one in the listing: that.
        assert_eq!(newest(page.clone(), listing).map(|n| n.1).as_deref(), Some("https://www.boriel.com/files/zxb/zxbasic-1.19.0-win32.zip"));
        let newer = pick_from_listing(&LISTED.replace("v1.19.0-win32", "v1.20.0-win32"), "-win32.zip");
        assert_eq!(newest(page, newer), Some(("1.20.0".to_string(), "https://www.boriel.com/files/zxb/zxbasic-v1.20.0-win32.zip".to_string())));
        assert_eq!(newest(None, pick_from_listing(LISTED, "-linux64.tar.gz")).map(|n| n.0).as_deref(), Some("1.19.0"));
        assert_eq!(newest(None, None), None);
        assert_eq!(version_in("zxbasic-1.19.0.zip", "-win32.zip"), None, "the Python scripts alone aren't a build");
    }

    /// A tar file of a build as GNU tar writes it: the zxbasic folder, its launcher and a file below.
    fn tar_gz(files: &[(&str, u32, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, mode, bytes) in files {
            let mut header = [0u8; 512];
            header[..name.len()].copy_from_slice(name.as_bytes());
            header[100..107].copy_from_slice(format!("{mode:07o}").as_bytes());
            header[124..135].copy_from_slice(format!("{:011o}", bytes.len()).as_bytes());
            header[156] = if name.ends_with('/') { b'5' } else { b'0' };
            header[257..265].copy_from_slice(b"ustar  \0");
            out.extend(header);
            out.extend(*bytes);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        out.extend([0u8; 1024]);
        // Stored gzip: deflate's blocks kept as they are, each at most 65,535 bytes.
        let mut gz = vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 3];
        let chunks: Vec<&[u8]> = out.chunks(65_535).collect();
        for (n, chunk) in chunks.iter().enumerate() {
            gz.push(u8::from(n + 1 == chunks.len()));
            gz.extend((chunk.len() as u16).to_le_bytes());
            gz.extend((!(chunk.len() as u16)).to_le_bytes());
            gz.extend(*chunk);
        }
        gz.extend([0u8; 4]);
        gz.extend((out.len() as u32).to_le_bytes());
        gz
    }

    #[test]
    fn a_build_is_unpacked_in_place_of_the_last_and_found() {
        let home = std::env::temp_dir().join(format!("limoniter-zxbasic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("zxbasic")).unwrap();
        std::fs::write(home.join("zxbasic").join("old.txt"), "the last one").unwrap();
        assert_eq!(installed_in(&home), None, "nothing to run yet");
        let launcher: &[u8] = b"#!/bin/sh\necho zxbc.py 1.19.0\n";
        let program = if cfg!(windows) { "zxbasic/zxbc.exe" } else { "zxbasic/zxbc" };
        let gz = tar_gz(&[("zxbasic/", 0o775, b""), (program, 0o775, launcher), ("zxbasic/src/zxbc/__init__.py", 0o664, b""), ("zxbasic/LICENSE.txt", 0o664, b"AGPL")]);
        let found = unpack_into(gz, &home).unwrap();
        assert_eq!(found, home.join("zxbasic").join(if cfg!(windows) { "zxbc.exe" } else { "zxbc" }), "the launcher, not the src/zxbc folder");
        assert!(!home.join("zxbasic").join("old.txt").exists(), "the last one replaced");
        assert!(!home.join("zxbasic-new").exists());
        assert_eq!(installed_in(&home), Some(found.clone()), "F9 finds it straight away");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&found).unwrap().permissions().mode() & 0o777, 0o775);
            assert_eq!(check(&found).unwrap(), "zxbc.py 1.19.0", "zxbc --version is asked");
        }
        // Something that isn't a build leaves the one there alone.
        assert!(unpack_into(b"<html>not found</html>".to_vec(), &home).is_err());
        assert!(unpack_into(tar_gz(&[("zxbasic/README.md", 0o664, b"read me")]), &home).is_err(), "no zxbc in it");
        assert_eq!(installed_in(&home), Some(found));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_python_way_makes_an_environment_and_pip_installs_zxbasic() {
        let venv = PathBuf::from("/home/me/.config/limoniter/compilers/zxbasic-python");
        let steps = python_commands(Path::new("/usr/bin/python3.12"), &venv);
        assert_eq!(steps[0], (PathBuf::from("/usr/bin/python3.12"), vec!["-m".to_string(), "venv".into(), venv.display().to_string()]));
        assert_eq!(steps[1].0, venv_program(&venv, "python"), "the environment's own Python runs its pip");
        assert_eq!(steps[1].1[..3], ["-m", "pip", "install"]);
        assert_eq!(steps[1].1.last().map(String::as_str), Some("zxbasic"));
        // Its zxbc is found where pip puts it.
        let home = std::env::temp_dir().join(format!("limoniter-zxbasic-venv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let zxbc = venv_program(&home.join("zxbasic-python"), "zxbc");
        std::fs::create_dir_all(zxbc.parent().unwrap()).unwrap();
        std::fs::write(&zxbc, "").unwrap();
        assert_eq!(installed_in(&home), Some(zxbc));
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Unpacks builds downloaded already (the Windows zip among them, which can't be run here):
    /// LIMONITER_TEST_ZXBASIC=a.zip:b.tar.gz cargo test -- --ignored boriel_get::tests::real_builds_unpack
    #[test]
    #[ignore]
    fn real_builds_unpack() {
        let archives = std::env::var_os("LIMONITER_TEST_ZXBASIC").expect("LIMONITER_TEST_ZXBASIC names the builds");
        for (n, archive) in std::env::split_paths(&archives).enumerate() {
            let target = std::env::temp_dir().join(format!("limoniter-zxbasic-unpack-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&target);
            unpack(std::fs::read(&archive).unwrap(), &target).unwrap();
            let program = ["zxbc", "zxbc.exe"].iter().map(|p| target.join(p)).find(|p| p.is_file());
            println!("{}: {program:?}", archive.display());
            assert!(program.is_some(), "{}", archive.display());
            assert!(target.join("src").join("zxbc").is_dir() && target.join("LICENSE.txt").is_file());
            let _ = std::fs::remove_dir_all(&target);
        }
    }

    /// Installs it for real, from boriel.com, into a folder of its own, and runs it:
    /// LIMONITER_TEST_COMPILERS=/some/folder cargo test -- --ignored boriel_get::tests::it_installs_for_real
    #[test]
    #[ignore]
    fn it_installs_for_real() {
        let home = std::env::var_os("LIMONITER_TEST_COMPILERS").map_or_else(|| std::env::temp_dir().join("limoniter-zxbasic-real"), PathBuf::from);
        for ending in ["-win32.zip", "-linux64.tar.gz"] {
            let found = latest(ending);
            println!("{ending}: {found:?}");
            assert!(found.is_ok_and(|(_, address)| address.ends_with(ending)));
        }
        let way = way();
        println!("{way:?}");
        let (program, version) = install(&way, &home, &|line| println!("{line}")).unwrap();
        println!("{} says {version}", program.display());
        assert!(version.contains("zxbc"), "{version}");
        assert_eq!(installed_in(&home), Some(program.clone()));
        // F9 finds it, with no restart. Run alone, as the ignored tests are, nothing else minds the variable.
        unsafe { std::env::set_var("LIMONITER_COMPILERS", &home) };
        let found = crate::compilers::by_id("zxbc").unwrap().find(crate::running::Machine::Spectrum48);
        unsafe { std::env::remove_var("LIMONITER_COMPILERS") };
        assert_eq!(found, Some(program));
    }
}
