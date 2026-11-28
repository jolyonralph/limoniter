//! Real floppy disks read and written through a Greaseweazle (Keir Fraser's USB floppy controller), by running
//! its own command-line tool, `gw` (https://github.com/keirf/greaseweazle). Limoniter finds `gw` (or installs
//! it), builds its command lines, follows its progress track by track, and opens the image it makes.
//!
//! What `gw` takes and says was read from its source (version 1.23, src/greaseweazle/tools/*.py, cli.py, usb.py
//! and util.py) and its wiki's Software-Installation page; NOTES.md lists where each flag came from. In short:
//! - `gw read [--device PORT] [--drive A|B|0|1|2|3] [--format F] [--revs N] [--tracks TSPEC] [--raw] FILE`
//!   (a flux image such as .scp is written with `--raw`, which still checks the sectors against `--format`);
//! - `gw write [--device PORT] [--drive D] [--format F] [--tracks TSPEC] [--no-verify] FILE`;
//! - `gw erase`, `gw clean`, `gw rpm --nr N` and `gw info` and `gw update` (which take `--device`, the first four
//!   also `--drive`);
//! - everything is printed on stderr, one line at a time; a failure prints `** FATAL ERROR:` or `Command Failed:`
//!   and the exit code isn't always set, so the words are looked for as well.

use crate::eprom::Output;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

/// Where the tool and its wiki are.
pub const HOME: &str = "https://github.com/keirf/greaseweazle";

/// GitHub's description of the latest release of the host tools.
pub const LATEST_RELEASE: &str = "https://api.github.com/repos/keirf/greaseweazle/releases/latest";

/// What pipx and pip are given to install the tools, from the README and wiki.
pub const PIP_SOURCE: &str = "git+https://github.com/keirf/greaseweazle@latest";

/// The drives `gw --drive` knows: A and B on the PC cable, 0 to 3 on the Shugart one (util.py, `Drive`).
pub const DRIVES: [&str; 6] = ["A", "B", "0", "1", "2", "3"];

/// Every disk format `gw` knows by name, from its diskdefs files (one to a line, such as `ibm.1440`).
pub fn all_formats() -> Vec<&'static str> {
    include_str!("../data/gw_formats.txt").lines().map(str::trim).filter(|l| !l.is_empty()).collect()
}

/// A kind of disk in the short list: what it's called, its format in `gw`, and the sector image it's saved as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Disk {
    pub name: &'static str,
    /// Empty for a raw flux copy of any disk.
    pub format: &'static str,
    pub ending: &'static str,
}

pub const COMMON: [Disk; 8] = [
    Disk { name: "Amiga DD", format: "amiga.amigados", ending: "adf" },
    Disk { name: "PC 1.44 MB", format: "ibm.1440", ending: "img" },
    Disk { name: "PC 720 KB", format: "ibm.720", ending: "img" },
    Disk { name: "Atari ST", format: "atarist.720", ending: "st" },
    Disk { name: "Commodore 1541", format: "commodore.1541", ending: "d64" },
    Disk { name: "Apple II", format: "apple2.appledos.140", ending: "do" },
    Disk { name: "BBC DFS", format: "acorn.dfs.ss", ending: "ssd" },
    Disk { name: "Raw flux (any disk)", format: "", ending: "scp" },
];

/// The ending of the sector image that suits a format, for one that isn't in the short list.
pub fn ending_for(format: &str) -> &'static str {
    if let Some(disk) = COMMON.iter().find(|d| d.format == format) {
        return disk.ending;
    }
    match format {
        "" => "scp",
        "commodore.1571" => "d71",
        "commodore.1581" => "d81",
        "acorn.dfs.ds" | "acorn.dfs.ds80" => "dsd",
        "acorn.adfs.160" | "acorn.adfs.320" | "acorn.adfs.640" | "acorn.adfs.800" | "acorn.adfs.1600" => "adf",
        f if f.starts_with("amiga.") => "adf",
        f if f.starts_with("atarist.") => "st",
        f if f.starts_with("apple2.prodos") => "po",
        f if f.starts_with("apple2.") => "do",
        f if f.starts_with("acorn.dfs.ss") => "ssd",
        _ => "img",
    }
}

/// What the image is: the disk's sectors, or its flux as `gw` keeps it (.scp) or as an HxC image (.hfe).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Sectors,
    Scp,
    Hfe,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Sectors, Kind::Scp, Kind::Hfe];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Sectors => "The disk's sectors (.adf, .img, .st, .d64\u{2026})",
            Kind::Scp => "Raw flux (.scp)",
            Kind::Hfe => "HxC image (.hfe)",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Kind::Sectors => "sectors",
            Kind::Scp => "scp",
            Kind::Hfe => "hfe",
        }
    }

    /// The file's ending for a kind of disk.
    pub fn ending(self, format: &str) -> &'static str {
        match self {
            Kind::Sectors => ending_for(format),
            Kind::Scp => "scp",
            Kind::Hfe => "hfe",
        }
    }
}

/// Whether an image's ending is one that holds flux (or a bit-level copy), which `gw` writes without needing the
/// disk's format.
pub fn is_flux_ending(ending: &str) -> bool {
    matches!(ending.to_ascii_lowercase().as_str(), "scp" | "hfe" | "a2r" | "ctr" | "ipf" | "raw")
}

/// The choices, kept between runs.
#[derive(Clone, PartialEq, Debug)]
pub struct Prefs {
    /// `gw`'s drive: A, B, 0, 1, 2 or 3.
    pub drive: String,
    /// The serial port (COM3, /dev/ttyACM0), or empty to let `gw` look for the Greaseweazle.
    pub port: String,
    /// A `gw` chosen by hand.
    pub program: Option<PathBuf>,
    /// The disk's format as `gw` names it, empty for raw flux.
    pub format: String,
    pub kind: Kind,
    pub tracks: String,
    pub revs: String,
    /// Writing checks what was written, which takes a revolution a track more.
    pub verify: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { drive: "A".into(), port: String::new(), program: None, format: "ibm.1440".into(), kind: Kind::Sectors, tracks: String::new(), revs: String::new(), verify: true }
    }
}

impl Prefs {
    pub fn to_settings(&self) -> String {
        format!(
            "gw_drive={}\ngw_port={}\ngw_program={}\ngw_format={}\ngw_kind={}\ngw_tracks={}\ngw_revs={}\ngw_verify={}\n",
            self.drive,
            self.port,
            self.program.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
            self.format,
            self.kind.id(),
            self.tracks,
            self.revs,
            self.verify
        )
    }

    pub fn read_setting(&mut self, name: &str, value: &str) {
        let value = value.trim();
        match name {
            "gw_drive" => {
                if let Some(drive) = DRIVES.iter().find(|d| d.eq_ignore_ascii_case(value)) {
                    self.drive = drive.to_string();
                }
            }
            "gw_port" => self.port = value.to_string(),
            "gw_program" => self.program = (!value.is_empty()).then(|| PathBuf::from(value)),
            "gw_format" => self.format = value.to_string(),
            "gw_kind" => self.kind = Kind::ALL.into_iter().find(|k| k.id() == value).unwrap_or(Kind::Sectors),
            "gw_tracks" => self.tracks = value.to_string(),
            "gw_revs" => self.revs = value.to_string(),
            "gw_verify" => self.verify = value != "false",
            _ => {}
        }
    }

    /// `--device PORT`, when a port is set.
    fn device(&self) -> Vec<String> {
        if self.port.trim().is_empty() { Vec::new() } else { vec!["--device".into(), self.port.trim().into()] }
    }

    fn drive_option(&self) -> Vec<String> {
        vec!["--drive".into(), self.drive.clone()]
    }
}

/// What `gw` is asked to do.
#[derive(Clone, PartialEq, Debug)]
pub enum Action {
    Info,
    /// Read into a file.
    Read(PathBuf),
    /// Write a file.
    Write(PathBuf),
    Rpm,
    Clean,
    Erase,
    /// Firmware update.
    Update,
}

/// A problem with what was asked, said before `gw` is started.
pub fn check(prefs: &Prefs, action: &Action) -> Result<(), String> {
    let tracks_ok = |t: &str| t.trim().is_empty() || t.trim().chars().all(|c| c.is_ascii_alphanumeric() || ":=,-/.+".contains(c));
    if !tracks_ok(&prefs.tracks) {
        return Err(crate::i18n::tr("The tracks aren't in a form gw knows. It takes things like c=0-79:h=0-1 (cylinders 0 to 79, both sides), or leave it empty.").to_string());
    }
    if !prefs.revs.trim().is_empty() && prefs.revs.trim().parse::<u32>().ok().filter(|n| *n >= 1).is_none() {
        return Err(crate::i18n::tr("The number of revolutions must be 1 or more, or empty to let gw choose.").to_string());
    }
    if let Action::Write(path) = action {
        let ending = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        if !is_flux_ending(&ending) && prefs.format.is_empty() && !matches!(ending.as_str(), "adf" | "d64") {
            return Err(crate::i18n::tr("Choose the kind of disk to write: a sector image doesn't say how the disk is laid out.").to_string());
        }
    }
    Ok(())
}

/// The command line for an action, after the program's name.
pub fn arguments(prefs: &Prefs, action: &Action) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    match action {
        Action::Info => {
            out.push("info".into());
            out.extend(prefs.device());
        }
        Action::Update => {
            out.push("update".into());
            out.extend(prefs.device());
        }
        Action::Read(path) => {
            out.push("read".into());
            out.extend(prefs.device());
            out.extend(prefs.drive_option());
            let ending = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            // A flux file is raw; the format, when there is one, still checks the sectors as they come.
            if !prefs.format.is_empty() {
                out.extend(["--format".into(), prefs.format.clone()]);
            }
            if ending == "scp" {
                out.push("--raw".into());
            }
            if !prefs.revs.trim().is_empty() {
                out.extend(["--revs".into(), prefs.revs.trim().into()]);
            }
            if !prefs.tracks.trim().is_empty() {
                out.extend(["--tracks".into(), prefs.tracks.trim().into()]);
            }
            out.push(path.display().to_string());
        }
        Action::Write(path) => {
            out.push("write".into());
            out.extend(prefs.device());
            out.extend(prefs.drive_option());
            let ending = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            if !prefs.format.is_empty() && !is_flux_ending(&ending) {
                out.extend(["--format".into(), prefs.format.clone()]);
            }
            if !prefs.tracks.trim().is_empty() {
                out.extend(["--tracks".into(), prefs.tracks.trim().into()]);
            }
            if !prefs.verify {
                out.push("--no-verify".into());
            }
            out.push(path.display().to_string());
        }
        Action::Rpm => {
            out.push("rpm".into());
            out.extend(prefs.device());
            out.extend(prefs.drive_option());
            out.extend(["--nr".into(), "3".into()]);
        }
        Action::Clean => {
            out.push("clean".into());
            out.extend(prefs.device());
            out.extend(prefs.drive_option());
        }
        Action::Erase => {
            out.push("erase".into());
            out.extend(prefs.device());
            out.extend(prefs.drive_option());
            if !prefs.tracks.trim().is_empty() {
                out.extend(["--tracks".into(), prefs.tracks.trim().into()]);
            }
        }
    }
    out
}

// ---------- What it says ----------

/// How far a read, write or erase has got, from what `gw` printed so far.
#[derive(Default, Clone, PartialEq, Debug)]
pub struct Progress {
    /// The tracks it will do, from its "Reading c=0-79:h=0-1 revs=3" line; 0 until it's said.
    pub total: usize,
    /// The tracks it has begun, each once however many times it's retried.
    pub done: usize,
    /// The latest track, such as "T12.1".
    pub current: String,
    /// How many times a track was read again.
    pub retries: usize,
    /// Tracks it gave up on, with what it said.
    pub bad: Vec<String>,
    /// "Found 1758 sectors of 1760 (99%)", when a read is over.
    pub summary: Option<String>,
    /// Warnings, such as a track outside the format.
    pub warnings: Vec<String>,
    /// Passes of a clean.
    pub passes: usize,
}

impl Progress {
    /// The fraction done, for a progress bar; None while the number of tracks isn't known.
    pub fn fraction(&self) -> Option<f32> {
        (self.total > 0).then(|| (self.done as f32 / self.total as f32).min(1.0))
    }
}

/// How many numbers a list such as `0-79`, `0,2,4` or `0-80/2` holds.
fn count_range(list: &str) -> usize {
    list.split(',')
        .map(|part| {
            let (range, step) = part.split_once('/').map_or((part, 1), |(r, s)| (r, s.parse().unwrap_or(1).max(1)));
            match range.split_once('-') {
                Some((a, b)) => match (a.parse::<usize>(), b.parse::<usize>()) {
                    (Ok(a), Ok(b)) if b >= a => (b - a) / step + 1,
                    _ => 0,
                },
                None => usize::from(range.parse::<usize>().is_ok()),
            }
        })
        .sum()
}

/// The tracks in a track set as `gw` prints it, `c=0-79:h=0-1`: cylinders times heads.
pub fn count_tracks(spec: &str) -> usize {
    let (mut cyls, mut heads) = (0, 0);
    for part in spec.split(':') {
        if let Some(list) = part.strip_prefix("c=") {
            cyls = count_range(list);
        } else if let Some(list) = part.strip_prefix("h=") {
            heads = count_range(list);
        }
    }
    cyls * heads
}

/// A line's track, if it begins like `T12.1: `.
fn track_of(line: &str) -> Option<(u32, u32, &str)> {
    let rest = line.strip_prefix('T')?;
    let (place, said) = rest.split_once(": ")?;
    let (c, h) = place.split_once('.')?;
    Some((c.parse().ok()?, h.parse().ok()?, said))
}

/// Follows what `gw` printed so far.
pub fn progress(text: &str) -> Progress {
    let mut p = Progress::default();
    let mut seen = std::collections::HashSet::new();
    for line in text.lines() {
        let line = line.trim_end();
        for word in ["Reading ", "Writing ", "Erasing "] {
            if let Some(rest) = line.strip_prefix(word)
                && rest.starts_with("c=")
            {
                p.total = count_tracks(rest.split_whitespace().next().unwrap_or("").trim_end_matches(','));
            }
        }
        if let Some((c, h, said)) = track_of(line) {
            if seen.insert((c, h)) {
                p.done += 1;
            }
            p.current = format!("T{c}.{h}");
            if said.contains("(Retry #") {
                p.retries += 1;
            }
            if said.starts_with("Giving up") {
                p.bad.push(format!("T{c}.{h}: {}", said.trim_start_matches("Giving up: ")));
            } else if said.starts_with("WARNING") {
                p.warnings.push(format!("T{c}.{h}: {said}"));
            }
        } else if line.starts_with("Found ") && line.contains(" sectors of ") {
            p.summary = Some(line.to_string());
        } else if line.starts_with("Pass ") {
            p.passes += 1;
        }
    }
    p
}

/// What `gw info` said.
#[derive(Default, Clone, PartialEq, Debug)]
pub struct Info {
    /// The host tools' version, such as "1.23".
    pub tool: Option<String>,
    pub port: Option<String>,
    pub model: Option<String>,
    pub firmware: Option<String>,
    pub serial: Option<String>,
    /// The version `gw` says is out, when it says one.
    pub newer_firmware: Option<String>,
    /// `gw` ran but found no Greaseweazle.
    pub no_device: bool,
}

/// Reads `gw info`'s answer (info.py).
pub fn parse_info(text: &str) -> Info {
    let mut info = Info::default();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("Not found") {
            info.no_device = true;
        } else if let Some(rest) = line.strip_prefix("*** New firmware version ") {
            info.newer_firmware = rest.split_whitespace().next().map(str::to_string);
        } else if let Some((key, value)) = line.split_once(':') {
            let value = value.trim();
            let value = (!value.is_empty()).then(|| value.to_string());
            match key {
                "Host Tools" => info.tool = value,
                "Port" => info.port = value,
                "Model" => info.model = value,
                "Firmware" => info.firmware = value,
                "Serial" => info.serial = value,
                _ => {}
            }
        }
    }
    info
}

impl Info {
    /// A line for the person: the model, its firmware and port.
    pub fn describe(&self) -> String {
        match (&self.model, &self.firmware, &self.port) {
            (Some(model), Some(firmware), Some(port)) => crate::i18n::tf!("{model}, firmware {firmware}, on {port}"),
            (Some(model), Some(firmware), None) => crate::i18n::tf!("{model}, firmware {firmware}"),
            (Some(model), ..) => model.clone(),
            _ => crate::i18n::tr("No Greaseweazle found").to_string(),
        }
    }
}

/// Whether a run failed: its exit code, or the words `gw` prints when it fails while exiting with 0.
pub fn failed(exit_ok: bool, text: &str) -> bool {
    !exit_ok || text.lines().any(|l| l.starts_with("** FATAL ERROR") || l.starts_with("Command Failed") || l.starts_with("ERROR:") || l.starts_with("** UPDATE FAILED"))
}

/// What to do about a Linux account that may not use the USB serial device.
fn permission_advice() -> String {
    if cfg!(target_os = "linux") {
        crate::i18n::tr("The USB serial port was refused. Your account probably isn't allowed to use it. Add yourself to the group that owns it (dialout on Debian and Ubuntu, uucp on Arch: sudo usermod -aG dialout $USER, then log out and in again), or install Greaseweazle's udev rule, scripts/49-greaseweazle.rules from its release download, into /etc/udev/rules.d and plug the Greaseweazle in again.").to_string()
    } else {
        crate::i18n::tr("The port was refused: another program (a terminal, or another copy of gw) may be using it.").to_string()
    }
}

/// A plain explanation of what went wrong, from what `gw` printed, when it's one of the known problems.
pub fn explain(text: &str) -> Option<String> {
    let has = |s: &str| text.contains(s);
    Some(if has("Permission denied") || has("PermissionError") || has("could not open port") && has("Errno 13") {
        permission_advice()
    } else if has("Cannot find the Greaseweazle device") {
        crate::i18n::tr("No Greaseweazle was found. Is it plugged in? A port can be given in Set up Greaseweazle if it's not found by itself.").to_string()
    } else if has("Device is in Firmware Update Mode") || has("firmware version") && has("unsupported") {
        crate::i18n::tr("The Greaseweazle's firmware needs updating: Set up Greaseweazle has the button.").to_string()
    } else if has("Disk is Write Protected") {
        crate::i18n::tr("The disk is write protected: slide its tab to make it writable.").to_string()
    } else if has("No Index") {
        crate::i18n::tr("The drive gave no index pulses, so it isn't turning a disk. Is a disk in it, and the drive's cable and power connected?").to_string()
    } else if has("Track 0 not found") {
        crate::i18n::tr("The drive's head didn't find track 0. Is the right drive chosen, and the cable on the right way round?").to_string()
    } else if has("Unknown format") {
        crate::i18n::tr("gw doesn't know that format. Another can be chosen under More.").to_string()
    } else {
        return None;
    })
}

/// The one line that says how a run ended, for under the progress bar.
pub fn summary(action: &Action, ok: bool, text: &str) -> String {
    if !ok {
        if let Some(advice) = explain(text) {
            return advice;
        }
        let last = text.lines().rev().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('$') && !l.starts_with("** FATAL")).unwrap_or("");
        return if last.is_empty() { crate::i18n::tr("gw stopped before it was done.").to_string() } else { crate::i18n::tf!("gw failed: {last}") };
    }
    let p = progress(text);
    match action {
        Action::Read(_) => {
            let mut said = p.summary.clone().unwrap_or_else(|| crate::i18n::tf!("Read {} tracks.", p.done));
            if !p.bad.is_empty() {
                let count = p.bad.len();
                said += &crate::i18n::tf!(" {count} track(s) have sectors that couldn't be read.");
            }
            if p.retries > 0 {
                let retries = p.retries;
                said += &crate::i18n::tf!(" {retries} read(s) were retried.");
            }
            said
        }
        Action::Write(_) => text.lines().rev().find(|l| l.contains("verified")).map(str::to_string).unwrap_or_else(|| crate::i18n::tf!("Wrote {} tracks.", p.done)),
        Action::Erase => crate::i18n::tf!("Erased {} tracks.", p.done),
        Action::Clean => crate::i18n::tr("The heads were cleaned.").to_string(),
        Action::Rpm => text.lines().rev().find(|l| l.starts_with("Median:") || l.starts_with("Rate:")).map(|l| l.trim_start_matches("Median:").trim().to_string()).unwrap_or_default(),
        Action::Info => parse_info(text).describe(),
        Action::Update => crate::i18n::tr("The firmware was updated.").to_string(),
    }
}

// ---------- Finding it ----------

/// Where Limoniter keeps what it installs.
pub fn home() -> Option<PathBuf> {
    if let Some(folder) = std::env::var_os("LIMONITER_GREASEWEAZLE") {
        return Some(folder.into());
    }
    if cfg!(test) {
        return None;
    }
    crate::settings::data_folder().map(|d| d.join("greaseweazle"))
}

/// The `gw` Limoniter installed in a home, if it did: gw.exe from the release on Windows, or the one in its own
/// Python environment.
pub fn own_in(home: &Path) -> Option<PathBuf> {
    if cfg!(windows) {
        crate::emulator_get::find_program(&home.join("tools"), &["gw"])
    } else {
        Some(home.join("venv/bin/gw")).filter(|p| p.is_file())
    }
}

/// `gw` in some folders.
pub fn find_in(folders: &[PathBuf]) -> Option<PathBuf> {
    let name = if cfg!(windows) { "gw.exe" } else { "gw" };
    folders.iter().map(|f| f.join(name)).find(|p| p.is_file())
}

/// Where `gw` is: the one chosen by hand, then one on the PATH (or where pipx puts it), then Limoniter's own.
pub fn find(prefs: &Prefs) -> Option<PathBuf> {
    if let Some(chosen) = prefs.program.as_ref().filter(|p| p.is_file()) {
        return Some(chosen.clone());
    }
    let mut folders: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        folders.push(home.join(".local/bin"));
    }
    folders.extend(["/usr/local/bin", "/opt/homebrew/bin"].map(PathBuf::from));
    find_in(&folders).or_else(|| home().and_then(|h| own_in(&h)))
}

/// Whether the `gw` found is the one Limoniter installed, which Update can replace.
pub fn is_own(program: &Path) -> bool {
    home().is_some_and(|h| program.starts_with(h))
}

/// A found `gw`, looked for again at most every few seconds, so menus can ask each frame whether it's there.
pub fn found_cached(prefs: &Prefs) -> Option<PathBuf> {
    static CACHE: Mutex<Option<(std::time::Instant, Option<PathBuf>, Option<PathBuf>)>> = Mutex::new(None);
    let mut cache = lock(&CACHE);
    if let Some((when, chosen, found)) = cache.as_ref()
        && when.elapsed() < std::time::Duration::from_secs(3)
        && *chosen == prefs.program
    {
        return found.clone();
    }
    let found = find(prefs);
    *cache = Some((std::time::Instant::now(), prefs.program.clone(), found.clone()));
    found
}

/// Why the menu items are greyed, when there's no `gw`.
pub fn missing_reason() -> String {
    crate::i18n::tr("Greaseweazle's gw program isn't installed. Set up Greaseweazle installs it.").to_string()
}

// ---------- Installing it ----------

/// The release file for Windows from GitHub's description of the latest release, and its tag. The 64-bit one,
/// unless this is a 32-bit build.
pub fn pick_windows_asset(release: &str, x86: bool) -> Result<(String, String), String> {
    let json: serde_json::Value = serde_json::from_str(release).map_err(|_| crate::i18n::tr("GitHub's answer wasn't what was expected.").to_string())?;
    let tag = json["tag_name"].as_str().unwrap_or("").to_string();
    let ending = if x86 { "-win32.zip" } else { "-win64.zip" };
    json["assets"]
        .as_array()
        .and_then(|assets| assets.iter().find(|a| a["name"].as_str().is_some_and(|n| n.starts_with("greaseweazle-") && n.ends_with(ending))))
        .and_then(|a| a["browser_download_url"].as_str())
        .map(|url| (tag.clone(), url.to_string()))
        .ok_or_else(|| crate::i18n::tf!("The latest release ({tag}) has no download for Windows that Limoniter knows."))
}

/// The numbers in a version such as "v1.23" or "1.23.1", for comparing.
pub fn version_numbers(version: &str) -> Vec<u32> {
    version.trim().trim_start_matches('v').split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect()
}

/// Whether the version out is newer than the one installed.
pub fn is_newer(out: &str, installed: &str) -> bool {
    version_numbers(out) > version_numbers(installed)
}

/// The commands that install the tools into a home with Python: a virtual environment of Limoniter's own, then
/// pip into it from GitHub, as the README says for pipx. Each is a program and its arguments.
pub fn pip_steps(home: &Path, python: &str) -> Vec<(String, Vec<String>)> {
    let venv = home.join("venv");
    let own_python = venv.join(if cfg!(windows) { "Scripts/python.exe" } else { "bin/python" });
    vec![
        (python.to_string(), vec!["-m".into(), "venv".into(), venv.display().to_string()]),
        (own_python.display().to_string(), vec!["-m".into(), "pip".into(), "install".into(), "--upgrade".into(), PIP_SOURCE.into()]),
    ]
}

/// What's needed, said plainly, when this computer can't install the tools by itself.
fn needs(what: &str) -> String {
    crate::i18n::tf!("{what} Then try again, or follow {HOME}/wiki/Software-Installation.")
}

/// Runs a command to its end, giving what it printed (the end of it, on failure).
fn run_step(program: &str, args: &[String]) -> Result<String, String> {
    let out = crate::platform::command(program).args(args).stdin(Stdio::null()).output().map_err(|e| crate::i18n::tf!("{program} couldn't be started: {e}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    if out.status.success() {
        return Ok(text);
    }
    let tail: Vec<&str> = text.lines().rev().take(6).collect();
    let tail = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
    Err(crate::i18n::tf!("{program} failed:\n{tail}"))
}

/// Installs (or updates) the tools into Limoniter's folder: on Windows the release's zip, unpacked; elsewhere,
/// pip into a virtual environment. Gives the `gw` and the version installed.
pub fn install() -> Result<(PathBuf, String), String> {
    let home = home().ok_or(crate::i18n::tr("There's nowhere to keep it: Limoniter's settings folder couldn't be found."))?;
    if cfg!(windows) {
        let release = crate::emulator_get::text(crate::emulator_get::get(LATEST_RELEASE)?, "GitHub")?;
        let (tag, url) = pick_windows_asset(&release, std::env::consts::ARCH == "x86")?;
        let bytes = crate::emulator_get::get(&url)?;
        let target = home.join("tools");
        let _ = std::fs::remove_dir_all(&target);
        crate::emulator_get::unzip(bytes, &target)?;
        let program = own_in(&home).ok_or_else(|| crate::i18n::tf!("Greaseweazle was unpacked into {}, but gw.exe wasn't found in it.", target.display()))?;
        return Ok((program, tag.trim_start_matches('v').to_string()));
    }
    let python = ["python3", "python"].into_iter().find(|p| crate::platform::command(p).arg("--version").output().is_ok_and(|o| o.status.success()));
    let Some(python) = python else {
        return Err(needs(crate::i18n::tr("Python 3 isn't installed, which the Greaseweazle tools are written in. On Debian and Ubuntu: sudo apt install python3 python3-venv python3-pip git gcc python3-dev; on a Mac: brew install python git.")));
    };
    if !crate::platform::command("git").arg("--version").output().is_ok_and(|o| o.status.success()) {
        return Err(needs(crate::i18n::tr("Git isn't installed, which pip needs to fetch the tools from GitHub (sudo apt install git, or brew install git).")));
    }
    std::fs::create_dir_all(&home).map_err(|e| crate::i18n::tf!("{} couldn't be made: {e}", home.display()))?;
    for (program, args) in pip_steps(&home, python) {
        run_step(&program, &args).map_err(|e| if args.get(1).is_some_and(|a| a == "venv") { needs(&crate::i18n::tf!("Python couldn't make an environment (on Debian and Ubuntu: sudo apt install python3-venv). {e}")) } else { e })?;
    }
    let program = own_in(&home).ok_or_else(|| crate::i18n::tf!("The tools were installed into {}, but gw wasn't found in it.", home.display()))?;
    let version = parse_info(&run_step(&program.display().to_string(), &["info".into()]).unwrap_or_default()).tool.unwrap_or_default();
    Ok((program, version))
}

/// The version of the latest release on GitHub, such as "1.23".
pub fn latest_version() -> Result<String, String> {
    let release = crate::emulator_get::text(crate::emulator_get::get(LATEST_RELEASE)?, "GitHub")?;
    let json: serde_json::Value = serde_json::from_str(&release).map_err(|_| crate::i18n::tr("GitHub's answer wasn't what was expected.").to_string())?;
    json["tag_name"].as_str().map(|t| t.trim_start_matches('v').to_string()).ok_or_else(|| crate::i18n::tr("GitHub's answer wasn't what was expected.").to_string())
}

// ---------- Running it ----------

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

/// `gw` running in the background.
pub struct Job {
    pub action: Action,
    pub output: Arc<Mutex<Output>>,
    /// Whether it exited well, once it's finished; or why it couldn't be started.
    pub finished: Arc<Mutex<Option<Result<bool, String>>>>,
    child: Arc<Mutex<Option<std::process::Child>>>,
}

impl Job {
    pub fn start(program: &Path, prefs: &Prefs, action: Action) -> Job {
        let output = Arc::new(Mutex::new(Output::default()));
        let finished = Arc::new(Mutex::new(None));
        let child_slot = Arc::new(Mutex::new(None));
        let arguments = arguments(prefs, &action);
        lock(&output).push(format!("$ gw {}\n", arguments.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() }).collect::<Vec<_>>().join(" ")).as_bytes());
        // Nothing to type into: a gw that stops to ask a question ends instead of waiting for ever. Python is
        // told not to hold its words back, so the progress comes as it happens.
        // A program only just written (the stand-in in a test, or a tool just installed) can be refused for a moment
        // as "text file busy" while another thread's fork still holds it open; it's asked again.
        let mut spawned = None;
        for attempt in 0..20 {
            let tried = crate::platform::command(program).args(&arguments).env("PYTHONUNBUFFERED", "1").env("PYTHONIOENCODING", "utf-8").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
            let busy = tried.as_ref().err().is_some_and(|e| e.raw_os_error() == Some(26));
            spawned = Some(tried);
            if !busy || attempt == 19 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let spawned = spawned.expect("tried at least once");
        match spawned {
            Err(e) => *lock(&finished) = Some(Err(crate::i18n::tf!("gw couldn't be started: {e}"))),
            Ok(mut child) => {
                let readers: Vec<_> = [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)]
                    .into_iter()
                    .flatten()
                    .map(|mut stream| {
                        let output = output.clone();
                        std::thread::spawn(move || {
                            let mut buffer = [0u8; 1024];
                            while let Ok(n) = stream.read(&mut buffer) {
                                if n == 0 {
                                    break;
                                }
                                lock(&output).push(&buffer[..n]);
                            }
                        })
                    })
                    .collect();
                *lock(&child_slot) = Some(child);
                let (slot, done) = (child_slot.clone(), finished.clone());
                std::thread::spawn(move || {
                    for reader in readers {
                        let _ = reader.join();
                    }
                    // Waited for a little at a time, so that Stop can take the child to end it meanwhile.
                    let result = loop {
                        let status = lock(&slot).as_mut().map(|c| c.try_wait());
                        match status {
                            Some(Ok(Some(status))) => break Ok(status.success()),
                            Some(Ok(None)) => std::thread::sleep(std::time::Duration::from_millis(20)),
                            Some(Err(e)) => break Err(e.to_string()),
                            None => break Ok(false),
                        }
                    };
                    *lock(&done) = Some(result);
                });
            }
        }
        Job { action, output, finished, child: child_slot }
    }

    pub fn done(&self) -> Option<Result<bool, String>> {
        lock(&self.finished).clone()
    }

    pub fn text(&self) -> String {
        lock(&self.output).text.clone()
    }

    /// Stops `gw`, which turns the drive's motor off as it ends.
    pub fn stop(&self) {
        if let Some(child) = lock(&self.child).as_mut() {
            let _ = child.kill();
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A stand-in for `gw`, since no Greaseweazle is attached here: a Python script (src/gw_testdata/fake_gw.py)
    /// that prints what the real one does, as read from its source, started through a small wrapper that gives it
    /// a scenario, a log and the image to make. Gives the wrapper.
    #[cfg(unix)]
    pub(crate) fn fake_gw(dir: &Path, scenario: &str, image: Option<&Path>) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/gw_testdata/fake_gw.py");
        let image = image.map(|i| format!(" --image \"{}\"", i.display())).unwrap_or_default();
        let log = dir.join("log");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let wrapper = dir.join("gw");
            std::fs::write(&wrapper, format!("#!/bin/sh\nexec python3 \"{}\" --log \"{}\" --scenario {scenario}{image} \"$@\"\n", script.display(), log.display())).unwrap();
            std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
            wrapper
        }
        #[cfg(not(unix))]
        {
            let wrapper = dir.join("gw.cmd");
            std::fs::write(&wrapper, format!("@python \"{}\" --log \"{}\" --scenario {scenario}{image} %*\r\n", script.display(), log.display())).unwrap();
            wrapper
        }
    }

    /// Runs an action with a stand-in to its end, giving whether it succeeded and what it printed.
    #[cfg(unix)]
    pub(crate) fn run(program: &Path, prefs: &Prefs, action: Action) -> (bool, String) {
        let job = Job::start(program, prefs, action);
        let began = std::time::Instant::now();
        loop {
            if let Some(done) = job.done() {
                return (done.unwrap_or(false), job.text());
            }
            assert!(began.elapsed() < std::time::Duration::from_secs(30), "the stand-in for gw never finished");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    fn prefs_with(format: &str) -> Prefs {
        Prefs { format: format.into(), ..Prefs::default() }
    }

    #[test]
    fn the_formats_list_has_the_ones_in_the_short_list() {
        let all = all_formats();
        assert!(all.len() > 100);
        for disk in COMMON.iter().filter(|d| !d.format.is_empty()) {
            assert!(all.contains(&disk.format), "{} isn't a gw format", disk.format);
        }
    }

    #[test]
    fn a_read_of_each_kind_of_disk_builds_the_command_gw_takes() {
        let read = |prefs: &Prefs, file: &str| arguments(prefs, &Action::Read(PathBuf::from(file)));
        // The sector image: the format, no --raw.
        assert_eq!(read(&prefs_with("amiga.amigados"), "disk.adf"), ["read", "--drive", "A", "--format", "amiga.amigados", "disk.adf"]);
        // Flux: --raw, still checked against the format; with revolutions, tracks and a port and drive set.
        let mut prefs = prefs_with("ibm.1440");
        prefs.drive = "1".into();
        prefs.port = "/dev/ttyACM0".into();
        prefs.revs = " 5 ".into();
        prefs.tracks = "c=0-39:h=0".into();
        assert_eq!(read(&prefs, "d.scp"), ["read", "--device", "/dev/ttyACM0", "--drive", "1", "--format", "ibm.1440", "--raw", "--revs", "5", "--tracks", "c=0-39:h=0", "d.scp"]);
        // Raw flux of any disk: no format at all.
        assert_eq!(read(&prefs_with(""), "any.scp"), ["read", "--drive", "A", "--raw", "any.scp"]);
        // An HFE is made from the decoded sectors, as gw's HFE writer takes decoded tracks.
        assert_eq!(read(&prefs_with("atarist.720"), "d.hfe"), ["read", "--drive", "A", "--format", "atarist.720", "d.hfe"]);
    }

    #[test]
    fn a_write_verifies_unless_told_not_to_and_names_the_format_only_for_sector_images() {
        let write = |prefs: &Prefs, file: &str| arguments(prefs, &Action::Write(PathBuf::from(file)));
        let mut prefs = prefs_with("ibm.720");
        assert_eq!(write(&prefs, "a.img"), ["write", "--drive", "A", "--format", "ibm.720", "a.img"]);
        assert_eq!(write(&prefs, "a.scp"), ["write", "--drive", "A", "a.scp"]);
        prefs.verify = false;
        assert_eq!(write(&prefs, "a.img"), ["write", "--drive", "A", "--format", "ibm.720", "--no-verify", "a.img"]);
    }

    #[test]
    fn the_other_commands_and_what_is_checked_first() {
        let prefs = Prefs::default();
        assert_eq!(arguments(&prefs, &Action::Info), ["info"]);
        assert_eq!(arguments(&prefs, &Action::Rpm), ["rpm", "--drive", "A", "--nr", "3"]);
        assert_eq!(arguments(&prefs, &Action::Clean), ["clean", "--drive", "A"]);
        assert_eq!(arguments(&prefs, &Action::Erase), ["erase", "--drive", "A"]);
        assert_eq!(arguments(&Prefs { port: "COM4".into(), ..Prefs::default() }, &Action::Update), ["update", "--device", "COM4"]);
        assert!(check(&Prefs { revs: "0".into(), ..Prefs::default() }, &Action::Rpm).is_err());
        assert!(check(&Prefs { tracks: "c=0-79; rm".into(), ..Prefs::default() }, &Action::Rpm).is_err());
        assert!(check(&Prefs { format: String::new(), ..Prefs::default() }, &Action::Write("x.img".into())).is_err());
        assert!(check(&Prefs { format: String::new(), ..Prefs::default() }, &Action::Write("x.adf".into())).is_ok());
        assert!(check(&Prefs { format: String::new(), ..Prefs::default() }, &Action::Write("x.scp".into())).is_ok());
    }

    #[test]
    fn endings_follow_the_disk() {
        assert_eq!(ending_for("amiga.amigados_hd"), "adf");
        assert_eq!(ending_for("commodore.1581"), "d81");
        assert_eq!(ending_for("apple2.prodos.140"), "po");
        assert_eq!(ending_for("ibm.dmf"), "img");
        assert_eq!(Kind::Hfe.ending("ibm.1440"), "hfe");
        assert_eq!(Kind::Sectors.ending("acorn.dfs.ds"), "dsd");
    }

    #[test]
    fn settings_are_kept() {
        let prefs = Prefs { drive: "B".into(), port: "COM3".into(), program: Some("C:\\gw\\gw.exe".into()), format: "amiga.amigados".into(), kind: Kind::Hfe, tracks: "c=0-79".into(), revs: "2".into(), verify: false };
        let mut read = Prefs::default();
        for line in prefs.to_settings().lines() {
            let (name, value) = line.split_once('=').unwrap();
            read.read_setting(name, value);
        }
        assert_eq!(read, prefs);
    }

    #[test]
    fn progress_follows_gw_s_lines() {
        let text = "$ gw read x\nReading c=0-79:h=0-1 revs=3\nFormat amiga.amigados\nT0.0: Amiga (11/11 sectors) from Raw Flux\nT0.1: Amiga (11/11 sectors) from Raw Flux\nT1.0: Amiga (10/11 sectors) from Raw Flux\nT1.0: Amiga (10/11 sectors) from Raw Flux (Retry #0.1)\nT1.0: Giving up: 1 sectors missing\n";
        let p = progress(text);
        assert_eq!((p.total, p.done, p.retries), (160, 3, 1));
        assert_eq!(p.current, "T1.0");
        assert_eq!(p.bad, ["T1.0: 1 sectors missing"]);
        assert_eq!(p.fraction(), Some(3.0 / 160.0));
        assert_eq!(count_tracks("c=0-79/2:h=1"), 40);
        assert_eq!(count_tracks("c=0,2,4-6:h=0-1"), 10);
        let text = "Reading c=0-0:h=0 revs=2\nT0.0: ok\nFound 10 sectors of 11 (90%)\n";
        assert_eq!(progress(text).summary.as_deref(), Some("Found 10 sectors of 11 (90%)"));
        assert_eq!(progress("Writing c=0-1:h=0\nT0.0: Writing Track (x)\nT0.0: Writing Track (Verify Failure: Retry #1)\n").done, 1);
    }

    #[test]
    fn info_is_read() {
        let text = "Host Tools: 1.23\nDevice:\n  Port:     /dev/ttyACM0\n  Model:    Greaseweazle F7 Lightning\n  MCU:      AT32F403A, 240MHz, 96kB SRAM\n  Firmware: 1.6\n  Serial:   GW1234\n  USB:      Full Speed (12 Mbit/s), 64kB Buffer\n\n*** New firmware version 1.7 is available\nTo perform an Update:\n";
        let info = parse_info(text);
        assert_eq!(info.tool.as_deref(), Some("1.23"));
        assert_eq!(info.model.as_deref(), Some("Greaseweazle F7 Lightning"));
        assert_eq!((info.firmware.as_deref(), info.port.as_deref(), info.serial.as_deref()), (Some("1.6"), Some("/dev/ttyACM0"), Some("GW1234")));
        assert_eq!(info.newer_firmware.as_deref(), Some("1.7"));
        assert!(!info.no_device);
        let none = parse_info("Host Tools: 1.23\nDevice:\n  Not found\n");
        assert!(none.no_device && none.model.is_none() && none.tool.as_deref() == Some("1.23"));
    }

    #[test]
    fn failures_are_told_apart_and_explained() {
        assert!(failed(true, "Reading c=0-79\nCommand Failed: Seek: Track 0 not found\n"));
        assert!(failed(true, "** FATAL ERROR:\nCannot find the Greaseweazle device\n"));
        assert!(failed(false, "nothing odd"));
        assert!(!failed(true, "T0.0: ok\nFound 1 sectors of 1 (100%)"));
        assert!(explain("** FATAL ERROR:\nCannot find the Greaseweazle device").unwrap().contains("plugged in"));
        assert!(explain("** FATAL ERROR:\n[Errno 13] could not open port /dev/ttyACM0: [Errno 13] Permission denied: '/dev/ttyACM0'").unwrap().contains(if cfg!(target_os = "linux") { "dialout" } else { "port" }));
        assert!(explain("Command Failed: Write: Disk is Write Protected").unwrap().contains("write protected"));
        assert!(explain("Command Failed: Read: No Index").unwrap().contains("index"));
        assert!(explain("T0.0: ok").is_none());
    }

    #[test]
    fn the_windows_download_is_picked_from_a_saved_release() {
        let release = include_str!("gw_testdata/releases_latest.json");
        let (tag, url) = pick_windows_asset(release, false).unwrap();
        assert_eq!(tag, "v1.23");
        assert!(url.ends_with("/v1.23/greaseweazle-1.23-win64.zip"));
        // Not the source zip, and the 32-bit build gets the 32-bit zip.
        assert!(pick_windows_asset(release, true).unwrap().1.ends_with("-win32.zip"));
        assert!(pick_windows_asset("{\"tag_name\":\"v2\",\"assets\":[]}", false).is_err());
        assert!(pick_windows_asset("not json", false).is_err());
    }

    #[test]
    fn versions_compare_by_their_numbers() {
        assert!(is_newer("v1.24", "1.23"));
        assert!(is_newer("1.23.1", "1.23"));
        assert!(is_newer("1.100", "1.99"));
        assert!(!is_newer("v1.23", "1.23"));
        assert!(!is_newer("1.9", "1.10"));
    }

    #[test]
    fn the_pip_install_is_a_virtual_environment_then_pip_from_github() {
        let steps = pip_steps(Path::new("/h/greaseweazle"), "python3");
        assert_eq!(steps[0], ("python3".to_string(), vec!["-m".to_string(), "venv".to_string(), Path::new("/h/greaseweazle").join("venv").display().to_string()]));
        assert_eq!(steps[1].1[..4], ["-m", "pip", "install", "--upgrade"]);
        assert_eq!(steps[1].1[4], "git+https://github.com/keirf/greaseweazle@latest");
        assert!(steps[1].0.contains("venv"));
    }

    #[test]
    fn gw_is_found_where_it_is_and_not_where_it_isnt() {
        let dir = std::env::temp_dir().join(format!("limoniter-gw-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(find_in(std::slice::from_ref(&dir)), None);
        let name = if cfg!(windows) { "gw.exe" } else { "gw" };
        std::fs::write(dir.join(name), "").unwrap();
        assert_eq!(find_in(std::slice::from_ref(&dir)), Some(dir.join(name)));
        // One chosen by hand wins.
        let chosen = dir.join("mine");
        std::fs::write(&chosen, "").unwrap();
        assert_eq!(find(&Prefs { program: Some(chosen.clone()), ..Prefs::default() }), Some(chosen));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn the_stand_in_reads_writes_and_fails_as_gw_does() {
        let dir = std::env::temp_dir().join(format!("limoniter-gw-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let out = dir.join("out.adf");
        let gw = fake_gw(&dir.join("bin"), "good", None);
        let prefs = prefs_with("amiga.amigados");
        std::fs::create_dir_all(&dir).unwrap();
        // Info.
        let (ok, text) = run(&gw, &prefs, Action::Info);
        assert!(ok && !failed(ok, &text));
        assert_eq!(parse_info(&text).model.as_deref(), Some("Greaseweazle F7 Lightning"));
        // A read with a bad track: progress, the sector summary, and an image made.
        let gw_bad = fake_gw(&dir.join("bin-bad"), "badsector", None);
        let (ok, text) = run(&gw_bad, &prefs, Action::Read(out.clone()));
        assert!(ok && !failed(ok, &text), "{text}");
        let p = progress(&text);
        assert_eq!((p.total, p.done), (4, 4));
        assert_eq!(p.bad.len(), 1);
        assert!(p.retries >= 1);
        assert!(p.summary.as_deref().unwrap().contains("Found 43 sectors of 44"));
        assert!(summary(&Action::Read(out.clone()), true, &text).contains("couldn't be read"));
        assert!(out.is_file());
        // The arguments reached it as built.
        let log = std::fs::read_to_string(dir.join("bin-bad/log")).unwrap();
        assert!(log.contains("read --drive A --format amiga.amigados"), "{log}");
        // Errors.
        for (scenario, expect) in [("nodevice", "plugged in"), ("denied", ""), ("nodisk", "index"), ("protected", "write protected")] {
            let gw = fake_gw(&dir.join(format!("bin-{scenario}")), scenario, None);
            let (ok, text) = run(&gw, &prefs, Action::Write(out.clone()));
            assert!(failed(ok, &text), "{scenario}: {text}");
            assert!(explain(&text).is_some(), "{scenario}: {text}");
            assert!(summary(&Action::Write(out.clone()), false, &text).contains(expect));
        }
        // Rpm.
        let (ok, text) = run(&gw, &prefs, Action::Rpm);
        assert!(ok);
        assert!(summary(&Action::Rpm, true, &text).contains("rpm"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
