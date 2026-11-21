//! Limoniter updating itself: the new version's download fetched, checked against the release's SHA-256 list and
//! that list's signature, then put in place of this copy, the one it replaces kept so it can be gone back to.
//!
//! - Windows: the release's Setup is run with no questions (/VERYSILENT). It installs for this Windows user, so it
//!   needs no administrator. A copy of Limoniter, started from the download folder, waits for this one to close,
//!   runs it, and starts the new version.
//! - macOS: the new Limoniter.app is unpacked beside this one and the two swapped, then the new one started.
//! - Linux: the AppImage is replaced. A copy installed from the .deb is only told that there's a new one, since apt
//!   looks after it.
//!
//! The list of fingerprints (SHA256SUMS.txt) is signed with the maker's own key, kept offline, in minisign's form:
//! tools/make-update-key.py makes the key once and signs each release in the Release workflow. A download whose
//! signature or fingerprint doesn't match is thrown away and nothing is changed.

use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The public half of the key releases are signed with, as tools/make-update-key.py printed it (made by Limoniter's
/// maker on 21 November 2026; key id cb042a62375b5466). Its secret half is kept offline and in the repository's
/// LIMONITER_UPDATE_KEY secret, from which the Release workflow signs each release's SHA256SUMS.txt. A key that's
/// replaced here only works for updates from the release that carries it onwards.
pub const PUBLIC_KEY: &str = "RWTLBCpiN1tUZmN/cf0yGhIS6tK4U8/sREDB9Yso7RPB2+PKW6LNcleY";

/// Where a release's files are downloaded from: the tag's folder on GitHub's Releases page.
const RELEASES: &str = "https://github.com/jolyonralph/limoniter/releases/download";
/// The list of every download's SHA-256, and its signature beside it.
pub const SUMS: &str = "SHA256SUMS.txt";
/// The most a download may be, so a wrong answer can't fill the disk.
const MOST_DOWNLOAD: u64 = 512 * 1024 * 1024;
/// minisign's name for a signature of the file itself with Ed25519.
const ALGORITHM: &[u8; 2] = b"Ed";
/// Set for the Limoniter started after an update: the version it was updated from, and to.
pub const UPDATED_FROM: &str = "LIMONITER_UPDATED_FROM";
pub const UPDATED_TO: &str = "LIMONITER_UPDATED_TO";
/// What the copy that finishes an update on Windows is started with, and the plan it follows.
pub const FINISH: &str = "--finish-update";

/// The versions this Limoniter was updated from and to, if an update (or going back) just started it.
static UPDATED: std::sync::OnceLock<Option<(String, String)>> = std::sync::OnceLock::new();

/// Takes the versions an update started this Limoniter with from its environment, so that the programs it runs
/// later (a terminal, Git) don't inherit them. Called first thing in main.
pub fn take_updated_from_environment() {
    let from = std::env::var(UPDATED_FROM).ok();
    let to = std::env::var(UPDATED_TO).ok();
    // SAFETY: this is the first thing main does, before any other thread is started that could read the
    // environment at the same time.
    unsafe {
        std::env::remove_var(UPDATED_FROM);
        std::env::remove_var(UPDATED_TO);
    }
    let _ = UPDATED.set(from.zip(to));
}

/// The versions this Limoniter was just updated from and to, if it was.
pub fn updated() -> Option<&'static (String, String)> {
    UPDATED.get().and_then(Option::as_ref)
}

/// Whether the key is still the placeholder, so no update can be checked.
pub fn key_is_placeholder() -> bool {
    key_is_placeholder_in(PUBLIC_KEY)
}

fn key_is_placeholder_in(key: &str) -> bool {
    key.starts_with("PLACEHOLDER") || parse_public_key(key).is_err()
}

/// A minisign public key: its id and its Ed25519 key.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PublicKey {
    id: [u8; 8],
    key: [u8; 32],
}

/// The text's last line that isn't minisign's untrusted comment: so a whole .pub file works as well as its key.
fn key_line(text: &str) -> &str {
    text.lines().map(str::trim).rfind(|l| !l.is_empty() && !l.starts_with("untrusted comment:")).unwrap_or("")
}

fn parse_public_key(text: &str) -> Result<PublicKey, String> {
    let bytes = crate::conversions::base64_decode(key_line(text)).map_err(|_| crate::i18n::tr("The update key isn't Base64.").to_string())?;
    if bytes.len() != 42 || &bytes[..2] != ALGORITHM {
        return Err(crate::i18n::tr("The update key isn't a minisign Ed25519 key.").into());
    }
    let mut key = PublicKey { id: [0; 8], key: [0; 32] };
    key.id.copy_from_slice(&bytes[2..10]);
    key.key.copy_from_slice(&bytes[10..]);
    Ok(key)
}

/// A minisign signature file: the signature, by which key, and the trusted comment with its own signature.
struct Signature {
    id: [u8; 8],
    signature: Vec<u8>,
    trusted: String,
    global: Vec<u8>,
}

fn parse_signature(text: &str) -> Result<Signature, String> {
    let wrong = || crate::i18n::tr("The signature file isn't minisign's.").to_string();
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
    let [_, signature, trusted, global, ..] = lines.as_slice() else { return Err(wrong()) };
    let trusted = trusted.strip_prefix("trusted comment: ").ok_or_else(wrong)?;
    let signature = crate::conversions::base64_decode(signature).map_err(|_| wrong())?;
    let global = crate::conversions::base64_decode(global).map_err(|_| wrong())?;
    if signature.len() != 74 || global.len() != 64 {
        return Err(wrong());
    }
    if &signature[..2] != ALGORITHM {
        return Err(crate::i18n::tr("The signature isn't one Limoniter checks: it should be made by tools/make-update-key.py.").into());
    }
    let mut id = [0; 8];
    id.copy_from_slice(&signature[2..10]);
    Ok(Signature { id, signature: signature[10..].to_vec(), trusted: trusted.to_string(), global })
}

/// Checks a minisign signature of `data` against a public key: the signature itself, and its trusted comment,
/// which is signed too so that it can't be swapped.
pub fn verify(data: &[u8], signature: &str, public_key: &str) -> Result<(), String> {
    if key_is_placeholder_in(public_key) {
        return Err(crate::i18n::tr("This Limoniter has no key to check updates with yet, so it doesn't update itself.").into());
    }
    let key = parse_public_key(public_key)?;
    let signature = parse_signature(signature)?;
    if signature.id != key.id {
        return Err(crate::i18n::tr("The update is signed with a different key from Limoniter's, so it wasn't installed.").into());
    }
    let ed25519 = ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key.key);
    ed25519.verify(data, &signature.signature).map_err(|_| crate::i18n::tr("The update's signature doesn't match it, so it wasn't installed.").to_string())?;
    let mut signed = signature.signature.clone();
    signed.extend_from_slice(signature.trusted.as_bytes());
    ed25519.verify(&signed, &signature.global).map_err(|_| crate::i18n::tr("The signature's trusted comment has been changed, so the update wasn't installed.").to_string())?;
    Ok(())
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// The SHA-256 a list gives for a file, in sha256sum's form: the hash, a space, then a space or *, then the name.
pub fn listed_hash(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, rest) = line.trim().split_once(' ')?;
        let file = rest.trim_start_matches([' ', '*']);
        (file == name && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hash.to_ascii_lowercase())
    })
}

/// How this copy of Limoniter can be updated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// Installed by Limoniter's Setup on Windows: the program it installed.
    WindowsSetup { exe: PathBuf },
    /// The Mac app.
    MacApp { app: PathBuf },
    /// An AppImage on Linux: the file itself.
    AppImage { file: PathBuf },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Mac,
    Linux,
    Other,
}

impl Os {
    pub fn this() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else {
            Os::Other
        }
    }
}

impl Install {
    /// The release's file that updates it.
    pub fn download_name(&self, version: &str) -> String {
        match self {
            Install::WindowsSetup { .. } => format!("Limoniter-Setup-{version}.exe"),
            Install::MacApp { .. } => format!("Limoniter-{version}-macOS.zip"),
            Install::AppImage { .. } => format!("Limoniter-{version}-x86_64.AppImage"),
        }
    }

    /// What this copy is, here: Limoniter.app, the AppImage, or the installed program.
    fn current(&self) -> &Path {
        match self {
            Install::WindowsSetup { exe } => exe,
            Install::MacApp { app } => app,
            Install::AppImage { file } => file,
        }
    }

    /// The name the previous version is kept under, in the updates folder's `previous`.
    fn kept_name(&self) -> &'static str {
        match self {
            Install::WindowsSetup { .. } => "limoniter.exe",
            Install::MacApp { .. } => "Limoniter.app",
            Install::AppImage { .. } => "Limoniter.AppImage",
        }
    }

    /// The program to start, once it's in place.
    fn program(&self, exe_name: &std::ffi::OsStr) -> PathBuf {
        match self {
            Install::MacApp { app } => app.join("Contents").join("MacOS").join(exe_name),
            other => other.current().to_path_buf(),
        }
    }
}

/// How this copy can update itself, or why it can't (and is only told about a new version).
pub fn this_install() -> Result<Install, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
    install_for(Os::this(), &exe, appimage.as_deref(), crate::settings::portable_folder().is_some())
}

/// How a copy at `exe` updates itself on `os`: `appimage` is where the AppImage is, when it is one, and
/// `portable` whether a portable file is beside it.
pub fn install_for(os: Os, exe: &Path, appimage: Option<&Path>, portable: bool) -> Result<Install, String> {
    let folder = exe.parent().unwrap_or(Path::new(""));
    match os {
        _ if portable => Err(crate::i18n::tr("This is the portable Limoniter, which doesn't change itself: download the new portable .zip and unpack it beside this one (its settings folder can be moved across).").into()),
        Os::Windows if folder.join("unins000.exe").is_file() => Ok(Install::WindowsSetup { exe: exe.to_path_buf() }),
        Os::Windows => Err(crate::i18n::tr("This Limoniter wasn't installed by its Setup, so it can't update itself.").into()),
        Os::Mac => match exe.ancestors().find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"))) {
            Some(app) => Ok(Install::MacApp { app: app.to_path_buf() }),
            None => Err(crate::i18n::tr("This Limoniter isn't the Mac app, so it can't update itself.").into()),
        },
        Os::Linux => match appimage {
            Some(file) if file.is_file() => Ok(Install::AppImage { file: file.to_path_buf() }),
            _ if exe.starts_with("/usr") => Err(crate::i18n::tr("This Limoniter was installed from the .deb, so it's updated as other software is: download the new .deb, or use your software centre.").into()),
            _ => Err(crate::i18n::tr("Only the AppImage updates itself on Linux, and this Limoniter isn't one.").into()),
        },
        Os::Other => Err(crate::i18n::tr("Limoniter can't update itself on this system.").into()),
    }
}

/// How a download is going, shared with the window that shows it.
#[derive(Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
}

/// Where updates are downloaded to and the previous version kept: `updates` in Limoniter's folder.
pub fn updates_folder() -> Option<PathBuf> {
    if let Some(folder) = std::env::var_os("LIMONITER_UPDATES") {
        return Some(PathBuf::from(folder).join("updates"));
    }
    if cfg!(test) {
        return None;
    }
    crate::settings::data_folder().map(|d| d.join("updates"))
}

/// Fetches a file into `out`, saying how far it's got.
pub type Fetch<'a> = &'a dyn Fn(&str, &mut dyn Write, &Progress) -> Result<(), String>;

/// Fetches from the web, at most MOST_DOWNLOAD bytes, stopping if asked to.
pub fn fetch_from_web(address: &str, out: &mut dyn Write, progress: &Progress) -> Result<(), String> {
    let response = ureq::get(address).header("User-Agent", "Limoniter").call().map_err(|e| crate::i18n::tf!("{address} couldn't be reached: {e}"))?;
    if let Some(length) = response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok()) {
        progress.total.store(length, Ordering::Relaxed);
    }
    let mut body = response.into_body();
    let mut reader = body.with_config().limit(MOST_DOWNLOAD).reader();
    copy_with_progress(&mut reader, out, progress)
}

fn copy_with_progress(reader: &mut dyn Read, out: &mut dyn Write, progress: &Progress) -> Result<(), String> {
    let mut buffer = vec![0; 64 * 1024];
    loop {
        if progress.cancel.load(Ordering::Relaxed) {
            return Err(crate::i18n::tr("The download was stopped.").into());
        }
        let got = reader.read(&mut buffer).map_err(|e| crate::i18n::tf!("The download stopped: {e}"))?;
        if got == 0 {
            return Ok(());
        }
        out.write_all(&buffer[..got]).map_err(|e| crate::i18n::tf!("The download couldn't be saved: {e}"))?;
        progress.done.fetch_add(got as u64, Ordering::Relaxed);
    }
}

/// Downloads a release's update for this kind of copy into `folder` (made if need be), checks the signed list of
/// fingerprints and the file against it, and gives where it is. A file already there that checks out isn't
/// downloaded again; one that doesn't is thrown away.
pub fn download(install: &Install, version: &str, folder: &Path, public_key: &str, fetch: Fetch, progress: &Progress) -> Result<PathBuf, String> {
    // Refused before anything is fetched, while there's no key to check with.
    if key_is_placeholder_in(public_key) {
        return Err(crate::i18n::tr("This Limoniter has no key to check updates with yet, so it doesn't update itself.").into());
    }
    let base = format!("{RELEASES}/v{version}");
    let quiet = Progress::default();
    let mut sums = Vec::new();
    fetch(&format!("{base}/{SUMS}"), &mut sums, &quiet)?;
    let mut signature = Vec::new();
    fetch(&format!("{base}/{SUMS}.minisig"), &mut signature, &quiet).map_err(|e| crate::i18n::tf!("The release has no signature, so it can't be checked: {e}"))?;
    verify(&sums, &String::from_utf8_lossy(&signature), public_key)?;
    let name = install.download_name(version);
    let sums = String::from_utf8_lossy(&sums);
    let Some(wanted) = listed_hash(&sums, &name) else {
        return Err(crate::i18n::tf!("The release's list of fingerprints doesn't have {name}."));
    };
    std::fs::create_dir_all(folder).map_err(|e| crate::i18n::tf!("{} couldn't be made: {e}", folder.display()))?;
    let file = folder.join(&name);
    if std::fs::read(&file).is_ok_and(|bytes| sha256_hex(&bytes) == wanted) {
        return Ok(file);
    }
    let part = folder.join(format!("{name}.part"));
    let fetched = (|| {
        let mut out = std::fs::File::create(&part).map_err(|e| crate::i18n::tf!("{} couldn't be made: {e}", part.display()))?;
        fetch(&format!("{base}/{name}"), &mut out, progress)?;
        out.flush().map_err(|e| e.to_string())?;
        drop(out);
        let bytes = std::fs::read(&part).map_err(|e| e.to_string())?;
        if sha256_hex(&bytes) != wanted {
            return Err(crate::i18n::tf!("{name} isn't the file the release signed (its SHA-256 is different), so it wasn't installed."));
        }
        std::fs::rename(&part, &file).map_err(|e| e.to_string())
    })();
    if fetched.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    fetched.map(|()| file)
}

/// One step of finishing an update once this window has closed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// Waits, a minute at most, until the program can be written to, as it can once it's no longer running.
    Wait(PathBuf),
    /// Runs a program and waits for it.
    Run(PathBuf, Vec<String>),
    /// Copies a file over another.
    Copy(PathBuf, PathBuf),
    /// Starts a program, and doesn't wait.
    Start(PathBuf, Vec<String>),
    /// A setting for the program started.
    Env(String, String),
}

/// A plan as text, a step a line, its parts apart by tabs, which no path has.
pub fn plan_text(steps: &[Step]) -> String {
    let line = |verb: &str, parts: Vec<String>| std::iter::once(verb.to_string()).chain(parts).collect::<Vec<_>>().join("\t");
    let path = |p: &Path| p.display().to_string();
    steps
        .iter()
        .map(|step| match step {
            Step::Wait(p) => line("wait", vec![path(p)]),
            Step::Run(p, args) => line("run", std::iter::once(path(p)).chain(args.iter().cloned()).collect()),
            Step::Copy(a, b) => line("copy", vec![path(a), path(b)]),
            Step::Start(p, args) => line("start", std::iter::once(path(p)).chain(args.iter().cloned()).collect()),
            Step::Env(name, value) => line("env", vec![name.clone(), value.clone()]),
        })
        .map(|l| l + "\n")
        .collect()
}

pub fn read_plan(text: &str) -> Vec<Step> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split('\t');
            let verb = parts.next()?;
            let first = parts.next()?.to_string();
            let rest: Vec<String> = parts.map(str::to_string).collect();
            Some(match verb {
                "wait" => Step::Wait(first.into()),
                "run" => Step::Run(first.into(), rest),
                "copy" => Step::Copy(first.into(), rest.first()?.into()),
                "start" => Step::Start(first.into(), rest),
                "env" => Step::Env(first, rest.first()?.clone()),
                _ => return None,
            })
        })
        .collect()
}

/// Follows a plan: the copy of Limoniter started with --finish-update does this with no window. If a program run
/// fails, the rest is still done, so that Limoniter is started again either way; it says what happened.
pub fn follow(steps: &[Step]) -> Vec<String> {
    let mut env: Vec<(String, String)> = Vec::new();
    let mut problems = Vec::new();
    for step in steps {
        match step {
            Step::Wait(program) => {
                let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
                while std::fs::OpenOptions::new().append(true).open(program).is_err() && program.exists() && std::time::Instant::now() < until {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                // A moment more, for Windows to let go of it altogether.
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            Step::Run(program, args) => match std::process::Command::new(program).args(args).status() {
                Ok(status) if status.success() => {}
                Ok(status) => problems.push(format!("{} {status}", program.display())),
                Err(e) => problems.push(format!("{}: {e}", program.display())),
            },
            Step::Copy(from, to) => {
                if let Err(e) = std::fs::copy(from, to) {
                    problems.push(format!("{} -> {}: {e}", from.display(), to.display()));
                }
            }
            Step::Env(name, value) => env.push((name.clone(), value.clone())),
            Step::Start(program, args) => {
                if let Err(e) = std::process::Command::new(program).args(args).envs(env.iter().map(|(n, v)| (n, v))).spawn() {
                    problems.push(format!("{}: {e}", program.display()));
                }
            }
        }
    }
    problems
}

/// For Windows: the copy that finishes the update, started with the plan. Its problems are written beside the
/// plan, for the next Limoniter to find.
pub fn finish(plan: &Path) -> i32 {
    let Ok(text) = std::fs::read_to_string(plan) else { return 1 };
    let problems = follow(&read_plan(&text));
    if !problems.is_empty() {
        let _ = std::fs::write(plan.with_extension("problems.txt"), problems.join("\n"));
    }
    i32::from(!problems.is_empty())
}

/// What to do, once the update is in place: start this program (with these settings), and close.
#[derive(Debug, PartialEq, Eq)]
pub struct Relaunch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(String, String)>,
}

impl Relaunch {
    pub fn start(&self) -> Result<(), String> {
        std::process::Command::new(&self.program).args(&self.args).envs(self.env.iter().map(|(n, v)| (n, v))).spawn().map(|_| ()).map_err(|e| crate::i18n::tf!("The new Limoniter couldn't be started: {e}"))
    }
}

/// The files to open again, after "--".
fn file_args(files: &[PathBuf]) -> Vec<OsString> {
    if files.is_empty() {
        return Vec::new();
    }
    std::iter::once(OsString::from("--")).chain(files.iter().map(|f| f.clone().into_os_string())).collect()
}

/// Moves a file or folder, by renaming it where that can be done, else by copying it and taking the first away.
fn move_path(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    copy_all(from, to)?;
    if from.is_dir() { std::fs::remove_dir_all(from) } else { std::fs::remove_file(from) }
}

/// Copies a file, or a folder and all in it, keeping links as links (a Mac app has them) and permissions.
fn copy_all(from: &Path, to: &Path) -> std::io::Result<()> {
    let kind = std::fs::symlink_metadata(from)?.file_type();
    if kind.is_symlink() {
        #[cfg(unix)]
        return std::os::unix::fs::symlink(std::fs::read_link(from)?, to);
        #[cfg(not(unix))]
        return std::fs::copy(from, to).map(|_| ());
    }
    if kind.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_all(&entry.path(), &to.join(entry.file_name()))?;
        }
        return Ok(());
    }
    std::fs::copy(from, to).map(|_| ())
}

fn remove_any(path: &Path) {
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
        let _ = std::fs::remove_dir_all(path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}

/// Where the previous version is kept, and the version it was.
pub fn kept_version(updates: &Path) -> Option<String> {
    let version = std::fs::read_to_string(updates.join("previous").join("version.txt")).ok()?;
    let version = version.trim().to_string();
    (!version.is_empty()).then_some(version)
}

/// The settings that tell the new Limoniter it was just updated, and from what.
fn updated_env(from: &str, to: &str) -> Vec<(String, String)> {
    vec![(UPDATED_FROM.to_string(), from.to_string()), (UPDATED_TO.to_string(), to.to_string())]
}

/// Puts a downloaded update in place of this copy, keeping this one (version `running`) in the updates folder's
/// `previous`, and says what to start. On Windows, what's started is the copy that runs the Setup once this window
/// has closed; elsewhere it's the new Limoniter itself. `files` are opened again.
pub fn install(install: &Install, download: &Path, version: &str, running: &str, updates: &Path, files: &[PathBuf]) -> Result<Relaunch, String> {
    let previous = updates.join("previous");
    let failed = |e: std::io::Error| crate::i18n::tf!("The update couldn't be put in place: {e}");
    let exe_name = std::env::current_exe().ok().and_then(|e| e.file_name().map(OsString::from)).unwrap_or_else(|| "limoniter".into());
    remove_any(&previous);
    std::fs::create_dir_all(&previous).map_err(failed)?;
    let kept = previous.join(install.kept_name());
    let env = updated_env(running, version);
    match install {
        Install::WindowsSetup { exe } => {
            std::fs::copy(exe, &kept).map_err(failed)?;
            let folder = download.parent().unwrap_or(updates);
            let helper = folder.join("limoniter-update-helper.exe");
            std::fs::copy(exe, &helper).map_err(failed)?;
            let log = folder.join("setup.log");
            let mut steps = vec![Step::Wait(exe.clone()), Step::Run(download.to_path_buf(), ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"].iter().map(|a| a.to_string()).chain([format!("/LOG={}", log.display())]).collect())];
            steps.extend(env.into_iter().map(|(n, v)| Step::Env(n, v)));
            steps.push(Step::Start(exe.clone(), file_args(files).iter().map(|a| a.to_string_lossy().into_owned()).collect()));
            let plan = folder.join("finish-update.txt");
            std::fs::write(&plan, plan_text(&steps)).map_err(failed)?;
            write_kept_version(&previous, running).map_err(failed)?;
            Ok(Relaunch { program: helper, args: vec![FINISH.into(), plan.into_os_string()], env: Vec::new() })
        }
        Install::MacApp { app } => {
            let parent = app.parent().ok_or_else(|| failed(std::io::ErrorKind::NotFound.into()))?;
            let unpack = parent.join(format!(".Limoniter-{version}-update"));
            remove_any(&unpack);
            std::fs::create_dir_all(&unpack).map_err(failed)?;
            let unpacked = unpack_mac_app(download, &unpack, &exe_name);
            let new_app = match unpacked {
                Ok(new_app) => new_app,
                Err(problem) => {
                    remove_any(&unpack);
                    return Err(problem);
                }
            };
            let swapped = swap_in(app, &new_app, &kept);
            remove_any(&unpack);
            swapped.map_err(failed)?;
            write_kept_version(&previous, running).map_err(failed)?;
            Ok(Relaunch { program: install.program(&exe_name), args: file_args(files), env })
        }
        Install::AppImage { file } => {
            let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let new = file.with_file_name(format!(".{name}.update"));
            let put = (|| {
                std::fs::copy(download, &new)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755))?;
                }
                swap_in(file, &new, &kept)
            })();
            if put.is_err() {
                let _ = std::fs::remove_file(&new);
            }
            put.map_err(failed)?;
            write_kept_version(&previous, running).map_err(failed)?;
            Ok(Relaunch { program: file.clone(), args: file_args(files), env })
        }
    }
}

fn write_kept_version(previous: &Path, version: &str) -> std::io::Result<()> {
    std::fs::write(previous.join("version.txt"), format!("{version}\n"))
}

/// Puts `new` where `current` is, keeping `current` as `kept`; if the new one can't go in, the old one goes back.
fn swap_in(current: &Path, new: &Path, kept: &Path) -> std::io::Result<()> {
    move_path(current, kept)?;
    if let Err(e) = std::fs::rename(new, current) {
        let _ = move_path(kept, current);
        return Err(e);
    }
    Ok(())
}

/// Unpacks the Mac app's zip with ditto, as Finder would, and finds the app in it, checking it has its program.
fn unpack_mac_app(zip: &Path, into: &Path, exe_name: &std::ffi::OsStr) -> Result<PathBuf, String> {
    let status = std::process::Command::new("/usr/bin/ditto").arg("-x").arg("-k").arg(zip).arg(into).status().map_err(|e| crate::i18n::tf!("The update couldn't be unpacked: {e}"))?;
    if !status.success() {
        return Err(crate::i18n::tr("The update couldn't be unpacked.").into());
    }
    find_app(into, exe_name)
}

/// The .app a folder holds, if it has the program in it.
fn find_app(folder: &Path, exe_name: &std::ffi::OsStr) -> Result<PathBuf, String> {
    std::fs::read_dir(folder)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e == "app") && p.join("Contents").join("MacOS").join(exe_name).is_file())
        .ok_or_else(|| crate::i18n::tr("The update has no Limoniter.app in it, so nothing was changed.").into())
}

/// Goes back to the version kept when this one was installed, keeping this one in its place, so going back can
/// itself be undone.
pub fn roll_back(install: &Install, updates: &Path, running: &str, files: &[PathBuf]) -> Result<Relaunch, String> {
    let previous = updates.join("previous");
    let kept = previous.join(install.kept_name());
    let Some(version) = kept_version(updates).filter(|_| std::fs::symlink_metadata(&kept).is_ok()) else {
        return Err(crate::i18n::tr("No previous version of Limoniter has been kept.").into());
    };
    let failed = |e: std::io::Error| crate::i18n::tf!("The previous version couldn't be put back: {e}");
    let exe_name = std::env::current_exe().ok().and_then(|e| e.file_name().map(OsString::from)).unwrap_or_else(|| "limoniter".into());
    let env = updated_env(running, &version);
    match install {
        Install::WindowsSetup { exe } => {
            // The running program can't be written over, so a copy of it does that once this window has closed.
            let helper = updates.join("limoniter-update-helper.exe");
            std::fs::copy(exe, &helper).map_err(failed)?;
            let swap = previous.join("limoniter.exe.swap");
            std::fs::copy(exe, &swap).map_err(failed)?;
            let mut steps = vec![Step::Wait(exe.clone()), Step::Copy(kept.clone(), exe.clone()), Step::Copy(swap.clone(), kept.clone())];
            steps.extend(env.into_iter().map(|(n, v)| Step::Env(n, v)));
            steps.push(Step::Start(exe.clone(), file_args(files).iter().map(|a| a.to_string_lossy().into_owned()).collect()));
            let plan = updates.join("finish-update.txt");
            std::fs::write(&plan, plan_text(&steps)).map_err(failed)?;
            write_kept_version(&previous, running).map_err(failed)?;
            Ok(Relaunch { program: helper, args: vec![FINISH.into(), plan.into_os_string()], env: Vec::new() })
        }
        Install::MacApp { app } | Install::AppImage { file: app } => {
            let swap = previous.join(format!("{}.swap", install.kept_name()));
            remove_any(&swap);
            // The kept one, taken out first so this one can be kept where it was.
            move_path(&kept, &swap).map_err(failed)?;
            if let Err(e) = swap_in(app, &swap, &kept) {
                let _ = move_path(&swap, &kept);
                return Err(failed(e));
            }
            write_kept_version(&previous, running).map_err(failed)?;
            Ok(Relaunch { program: install.program(&exe_name), args: file_args(files), env })
        }
    }
}

/// Takes away finished downloads, leaving the kept previous version.
pub fn tidy(updates: &Path) {
    for entry in std::fs::read_dir(updates).into_iter().flatten().flatten() {
        if entry.file_name() != "previous" {
            remove_any(&entry.path());
        }
    }
    let _ = std::fs::remove_file(updates.join("previous").join("limoniter.exe.swap"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // A key pair made only for these tests (tools/make-update-key.py, from a seed that's no secret). Never for real
    // releases.
    const TEST_PUBLIC: &str = "RWQBI0VniavN72GlmTltxWhfE2QdGZRUatZQ0XMx54qNtHyK0Dw8h8x+";
    const TEST_SUMS: &str = "7d6141105e0b7d347b5afa9b063343b81410f4ec567c79563ebdefb443969354  Limoniter-Setup-9.9.9.exe\n4ccf66fa2eddb519bb7d5b52a2ea29ef578bf41c74701197c977c414478013f2  Limoniter-9.9.9-x86_64.AppImage\n";
    const TEST_SIGNATURE: &str = "untrusted comment: signature from Limoniter's update key\nRWQBI0VniavN77PzcClItzHlqN25XbzHTQQt0ja4zHgLZgOuWla8FgKyCs+JFcXLN7fnGdmya075ITrByyzQU6QX0rh/OmxJugk=\ntrusted comment: timestamp:1763700000\tfile:SHA256SUMS.txt\n3laSle5NYKUSkdwFDpZe4FUMhzW1/kMyb6Q9lJ06QOINytc9R1VPl8pF22BhwRCooeE8N/nudrFNAutIOlNnBQ==\n";
    const SETUP: &[u8] = b"pretend Setup program\n";
    const APPIMAGE: &[u8] = b"pretend AppImage\n";

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("limoniter-self-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn while_the_key_is_a_placeholder_updates_are_refused_before_anything_is_fetched() {
        let placeholder = "PLACEHOLDER-make-the-key-with-tools/make-update-key.py";
        assert!(key_is_placeholder_in(placeholder) && !key_is_placeholder_in(TEST_PUBLIC));
        // The built-in key is either still the placeholder, refusing, or a real minisign key.
        if key_is_placeholder() {
            assert!(verify(TEST_SUMS.as_bytes(), TEST_SIGNATURE, PUBLIC_KEY).unwrap_err().contains("no key"));
        } else {
            assert!(parse_public_key(PUBLIC_KEY).is_ok());
        }
        let refused = verify(TEST_SUMS.as_bytes(), TEST_SIGNATURE, placeholder).unwrap_err();
        assert!(refused.contains("no key"), "{refused}");
        let asked = std::cell::Cell::new(0);
        let fetch = |_: &str, _: &mut dyn Write, _: &Progress| -> Result<(), String> {
            asked.set(asked.get() + 1);
            Ok(())
        };
        let install = Install::AppImage { file: "/x/Limoniter.AppImage".into() };
        assert!(download(&install, "9.9.9", &scratch("placeholder"), placeholder, &fetch, &Progress::default()).unwrap_err().contains("no key"));
        assert_eq!(asked.get(), 0);
    }

    #[test]
    fn a_minisign_signature_is_checked_and_any_change_refused() {
        assert_eq!(verify(TEST_SUMS.as_bytes(), TEST_SIGNATURE, TEST_PUBLIC), Ok(()));
        // The whole .pub file minisign writes works as well as its key.
        assert_eq!(verify(TEST_SUMS.as_bytes(), TEST_SIGNATURE, &format!("untrusted comment: minisign public key\n{TEST_PUBLIC}\n")), Ok(()));
        // A changed list, a changed trusted comment, a signature from another key, and rubbish are all refused.
        let changed = TEST_SUMS.replace("7d61", "7d62");
        assert!(verify(changed.as_bytes(), TEST_SIGNATURE, TEST_PUBLIC).unwrap_err().contains("doesn't match"));
        let comment = TEST_SIGNATURE.replace("file:SHA256SUMS.txt", "file:other.txt");
        assert!(verify(TEST_SUMS.as_bytes(), &comment, TEST_PUBLIC).unwrap_err().contains("trusted comment"));
        let mut other_key = crate::conversions::base64_decode(TEST_PUBLIC).unwrap();
        other_key[2] ^= 1;
        let other_key = crate::conversions::base64_encode(&other_key, false);
        assert!(verify(TEST_SUMS.as_bytes(), TEST_SIGNATURE, &other_key).unwrap_err().contains("different key"));
        assert!(verify(TEST_SUMS.as_bytes(), "not a signature", TEST_PUBLIC).is_err());
        // minisign's newer form (a BLAKE2b hash signed, "ED") isn't taken for the older.
        let prehashed = TEST_SIGNATURE.replace("RWQBI0VniavN77", "RUQBI0VniavN77");
        assert!(verify(TEST_SUMS.as_bytes(), &prehashed, TEST_PUBLIC).is_err());
    }

    #[test]
    fn the_list_of_fingerprints_is_read_as_sha256sum_writes_it() {
        assert_eq!(listed_hash(TEST_SUMS, "Limoniter-Setup-9.9.9.exe").as_deref(), Some(sha256_hex(SETUP).as_str()));
        assert_eq!(listed_hash(TEST_SUMS, "Limoniter-9.9.9-x86_64.AppImage").as_deref(), Some(sha256_hex(APPIMAGE).as_str()));
        assert_eq!(listed_hash(&format!("{} *binary.zip\n", "AB".repeat(32)), "binary.zip"), Some("ab".repeat(32)));
        assert_eq!(listed_hash(TEST_SUMS, "Limoniter-Setup-9.9.exe"), None);
        assert_eq!(listed_hash("abc  short.exe\n", "short.exe"), None);
    }

    /// A pretend GitHub: the files of release v9.9.9, and which were asked for.
    fn release(files: HashMap<String, Vec<u8>>) -> impl Fn(&str, &mut dyn Write, &Progress) -> Result<(), String> {
        move |address: &str, out: &mut dyn Write, progress: &Progress| {
            let name = address.strip_prefix(&format!("{RELEASES}/v9.9.9/")).ok_or("wrong place")?;
            let bytes = files.get(name).ok_or_else(|| format!("404 {name}"))?;
            progress.total.store(bytes.len() as u64, Ordering::Relaxed);
            copy_with_progress(&mut &bytes[..], out, progress)
        }
    }

    fn good_release() -> HashMap<String, Vec<u8>> {
        HashMap::from([
            (SUMS.to_string(), TEST_SUMS.as_bytes().to_vec()),
            (format!("{SUMS}.minisig"), TEST_SIGNATURE.as_bytes().to_vec()),
            ("Limoniter-Setup-9.9.9.exe".to_string(), SETUP.to_vec()),
            ("Limoniter-9.9.9-x86_64.AppImage".to_string(), APPIMAGE.to_vec()),
        ])
    }

    #[test]
    fn a_download_is_kept_only_if_its_fingerprint_and_signature_match() {
        let folder = scratch("download");
        let install = Install::WindowsSetup { exe: "C:/Limoniter/limoniter.exe".into() };
        let progress = Progress::default();
        let got = download(&install, "9.9.9", &folder, TEST_PUBLIC, &release(good_release()), &progress).unwrap();
        assert_eq!(got, folder.join("Limoniter-Setup-9.9.9.exe"));
        assert_eq!(std::fs::read(&got).unwrap(), SETUP);
        assert_eq!(progress.done.load(Ordering::Relaxed), SETUP.len() as u64);

        // A file that was changed on its way is thrown away, and so is what was there.
        let mut tampered = good_release();
        tampered.insert("Limoniter-Setup-9.9.9.exe".into(), b"something else".to_vec());
        std::fs::remove_file(&got).unwrap();
        let refused = download(&install, "9.9.9", &folder, TEST_PUBLIC, &release(tampered), &Progress::default()).unwrap_err();
        assert!(refused.contains("SHA-256"), "{refused}");
        assert!(!got.exists() && !folder.join("Limoniter-Setup-9.9.9.exe.part").exists());

        // So is a list whose signature doesn't match, or that has none, before the file is fetched at all.
        let mut unsigned = good_release();
        unsigned.insert(SUMS.into(), TEST_SUMS.replace("7d61", "0000").into_bytes());
        assert!(download(&install, "9.9.9", &folder, TEST_PUBLIC, &release(unsigned), &Progress::default()).unwrap_err().contains("signature"));
        let mut missing = good_release();
        missing.remove(&format!("{SUMS}.minisig"));
        assert!(download(&install, "9.9.9", &folder, TEST_PUBLIC, &release(missing), &Progress::default()).unwrap_err().contains("no signature"));
        assert!(!got.exists());

        // A kind of download the list doesn't have.
        let mac = Install::MacApp { app: "/Applications/Limoniter.app".into() };
        assert!(download(&mac, "9.9.9", &folder, TEST_PUBLIC, &release(good_release()), &Progress::default()).unwrap_err().contains("doesn't have"));

        // One already downloaded and checked isn't fetched again.
        download(&install, "9.9.9", &folder, TEST_PUBLIC, &release(good_release()), &Progress::default()).unwrap();
        let mut without = good_release();
        without.remove("Limoniter-Setup-9.9.9.exe");
        assert!(download(&install, "9.9.9", &folder, TEST_PUBLIC, &release(without), &Progress::default()).is_ok());

        // Stopping a download stops it.
        std::fs::remove_file(&got).unwrap();
        let stopped = Progress::default();
        stopped.cancel.store(true, Ordering::Relaxed);
        assert!(download(&install, "9.9.9", &folder, TEST_PUBLIC, &release(good_release()), &stopped).unwrap_err().contains("stopped"));
        assert!(!got.exists());
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn each_kind_of_copy_is_updated_its_own_way_or_only_told() {
        let folder = scratch("kinds");
        let exe = folder.join("limoniter.exe");
        std::fs::write(&exe, b"").unwrap();
        assert!(install_for(Os::Windows, &exe, None, false).is_err(), "built from source, with no uninstaller beside it");
        std::fs::write(folder.join("unins000.exe"), b"").unwrap();
        assert_eq!(install_for(Os::Windows, &exe, None, false), Ok(Install::WindowsSetup { exe: exe.clone() }));
        assert!(install_for(Os::Windows, &exe, None, true).unwrap_err().contains("portable"));
        let mac_exe = Path::new("/Applications/Limoniter.app/Contents/MacOS/limoniter");
        assert_eq!(install_for(Os::Mac, mac_exe, None, false), Ok(Install::MacApp { app: "/Applications/Limoniter.app".into() }));
        assert!(install_for(Os::Mac, Path::new("/Users/me/limoniter/target/release/limoniter"), None, false).is_err());
        let appimage = folder.join("Limoniter-0.14.4-x86_64.AppImage");
        std::fs::write(&appimage, b"").unwrap();
        assert_eq!(install_for(Os::Linux, Path::new("/tmp/.mount_Limoni/usr/bin/limoniter"), Some(&appimage), false), Ok(Install::AppImage { file: appimage.clone() }));
        assert!(install_for(Os::Linux, Path::new("/usr/bin/limoniter"), None, false).unwrap_err().contains(".deb"));
        assert!(install_for(Os::Linux, Path::new("/home/me/limoniter"), None, false).is_err());
        // Each is updated from its own download.
        assert_eq!(Install::WindowsSetup { exe: exe.clone() }.download_name("0.15.0"), "Limoniter-Setup-0.15.0.exe");
        assert_eq!(Install::MacApp { app: "/A.app".into() }.download_name("0.15.0"), "Limoniter-0.15.0-macOS.zip");
        assert_eq!(Install::AppImage { file: appimage }.download_name("0.15.0"), "Limoniter-0.15.0-x86_64.AppImage");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    #[cfg(unix)]
    fn an_appimage_is_replaced_the_old_one_kept_and_going_back_swaps_them() {
        use std::os::unix::fs::PermissionsExt;
        let folder = scratch("appimage");
        let file = folder.join("Limoniter.AppImage");
        std::fs::write(&file, b"old version").unwrap();
        let download = folder.join("download");
        std::fs::write(&download, b"new version").unwrap();
        let updates = folder.join("updates");
        let install = Install::AppImage { file: file.clone() };
        let files = vec![PathBuf::from("/work/a.s"), PathBuf::from("/work/b.txt")];
        let relaunch = super::install(&install, &download, "9.9.9", "0.14.4", &updates, &files).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"new version");
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o755);
        assert_eq!(std::fs::read(updates.join("previous").join("Limoniter.AppImage")).unwrap(), b"old version");
        assert_eq!(kept_version(&updates).as_deref(), Some("0.14.4"));
        assert_eq!(relaunch.program, file);
        assert_eq!(relaunch.args, ["--", "/work/a.s", "/work/b.txt"].map(OsString::from));
        assert!(relaunch.env.contains(&(UPDATED_FROM.into(), "0.14.4".into())) && relaunch.env.contains(&(UPDATED_TO.into(), "9.9.9".into())));
        assert!(!folder.join(".Limoniter.AppImage.update").exists());

        // Going back puts the old one in place and keeps the new one, so that can be undone too.
        let back = roll_back(&install, &updates, "9.9.9", &[]).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"old version");
        assert_eq!(std::fs::read(updates.join("previous").join("Limoniter.AppImage")).unwrap(), b"new version");
        assert_eq!(kept_version(&updates).as_deref(), Some("9.9.9"));
        assert!(back.args.is_empty());
        assert!(back.env.contains(&(UPDATED_TO.into(), "0.14.4".into())));
        roll_back(&install, &updates, "0.14.4", &[]).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"new version");

        // With nothing kept, there's nothing to go back to.
        std::fs::remove_dir_all(updates.join("previous")).unwrap();
        assert!(roll_back(&install, &updates, "9.9.9", &[]).is_err());
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    #[cfg(unix)]
    fn a_mac_app_is_swapped_for_the_new_one_with_its_links_kept() {
        let folder = scratch("mac");
        let make_app = |at: &Path, words: &[u8]| {
            let program = at.join("Contents").join("MacOS");
            std::fs::create_dir_all(&program).unwrap();
            std::fs::write(program.join("limoniter"), words).unwrap();
            std::os::unix::fs::symlink("MacOS/limoniter", at.join("Contents").join("link")).unwrap();
        };
        let app = folder.join("Applications").join("Limoniter.app");
        make_app(&app, b"old");
        let unpacked = folder.join("unpacked");
        make_app(&unpacked.join("Limoniter.app"), b"new");
        assert_eq!(find_app(&unpacked, "limoniter".as_ref()), Ok(unpacked.join("Limoniter.app")));
        assert!(find_app(&folder, "limoniter".as_ref()).is_err(), "no app there");
        let kept = folder.join("previous").join("Limoniter.app");
        std::fs::create_dir_all(kept.parent().unwrap()).unwrap();
        swap_in(&app, &unpacked.join("Limoniter.app"), &kept).unwrap();
        assert_eq!(std::fs::read(app.join("Contents/MacOS/limoniter")).unwrap(), b"new");
        assert_eq!(std::fs::read(kept.join("Contents/MacOS/limoniter")).unwrap(), b"old");
        // A copy, as when it can't simply be renamed, keeps the app's links as links.
        let copied = folder.join("copied.app");
        copy_all(&kept, &copied).unwrap();
        assert!(std::fs::symlink_metadata(copied.join("Contents/link")).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(copied.join("Contents/link")).unwrap(), b"old");
        assert_eq!(Install::MacApp { app: app.clone() }.program("limoniter".as_ref()), app.join("Contents/MacOS/limoniter"));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn windows_is_updated_by_its_setup_run_quietly_once_this_window_has_closed() {
        let folder = scratch("windows");
        let exe = folder.join("Limoniter").join("limoniter.exe");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"0.14.4's program").unwrap();
        let updates = folder.join("updates");
        let download = updates.join("9.9.9").join("Limoniter-Setup-9.9.9.exe");
        std::fs::create_dir_all(download.parent().unwrap()).unwrap();
        std::fs::write(&download, SETUP).unwrap();
        let install = Install::WindowsSetup { exe: exe.clone() };
        let relaunch = super::install(&install, &download, "9.9.9", "0.14.4", &updates, &[PathBuf::from("C:/work/a.s")]).unwrap();
        // What's started is a copy of this program, outside the folder the Setup writes to, with the plan.
        let helper = updates.join("9.9.9").join("limoniter-update-helper.exe");
        assert_eq!(relaunch.program, helper);
        assert_eq!(std::fs::read(&helper).unwrap(), b"0.14.4's program");
        assert_eq!(relaunch.args[0], FINISH);
        let plan = read_plan(&std::fs::read_to_string(&relaunch.args[1]).unwrap());
        assert_eq!(plan[0], Step::Wait(exe.clone()));
        let Step::Run(setup, args) = &plan[1] else { panic!("{plan:?}") };
        assert_eq!(setup, &download);
        assert_eq!(&args[..4], ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"]);
        assert!(plan.contains(&Step::Env(UPDATED_TO.into(), "9.9.9".into())));
        assert_eq!(plan.last(), Some(&Step::Start(exe.clone(), vec!["--".into(), "C:/work/a.s".into()])));
        assert_eq!(read_plan(&plan_text(&plan)), plan);
        // This version is kept before the Setup changes it.
        assert_eq!(std::fs::read(updates.join("previous").join("limoniter.exe")).unwrap(), b"0.14.4's program");
        assert_eq!(kept_version(&updates).as_deref(), Some("0.14.4"));

        // Going back: the kept program copied over this one once it has closed, and this one kept in its place.
        let back = roll_back(&install, &updates, "9.9.9", &[]).unwrap();
        std::fs::write(&exe, b"9.9.9's program").unwrap();
        std::fs::write(updates.join("previous").join("limoniter.exe.swap"), b"9.9.9's program").unwrap();
        let steps: Vec<Step> = read_plan(&std::fs::read_to_string(&back.args[1]).unwrap()).into_iter().filter(|s| !matches!(s, Step::Start(..) | Step::Wait(_))).collect();
        assert!(follow(&steps).is_empty());
        assert_eq!(std::fs::read(&exe).unwrap(), b"0.14.4's program");
        assert_eq!(std::fs::read(updates.join("previous").join("limoniter.exe")).unwrap(), b"9.9.9's program");
        assert_eq!(kept_version(&updates).as_deref(), Some("9.9.9"));

        // Tidying leaves only what's kept.
        tidy(&updates);
        let left: Vec<_> = std::fs::read_dir(&updates).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left, ["previous"]);
        assert!(!updates.join("previous").join("limoniter.exe.swap").exists());
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    #[cfg(unix)]
    fn a_plan_is_followed_and_what_fails_is_said_but_the_rest_still_done() {
        let folder = scratch("plan");
        let (a, b) = (folder.join("a"), folder.join("b"));
        std::fs::write(&a, b"copied").unwrap();
        let marker = folder.join("started");
        let steps = vec![
            Step::Wait(a.clone()),
            Step::Run("/bin/false".into(), Vec::new()),
            Step::Copy(a.clone(), b.clone()),
            Step::Env("LIMONITER_TEST_WORD".into(), "hello".into()),
            Step::Start("/bin/sh".into(), vec!["-c".into(), format!("echo $LIMONITER_TEST_WORD > {}", marker.display())]),
        ];
        let problems = follow(&steps);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(std::fs::read(&b).unwrap(), b"copied");
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::fs::read_to_string(&marker).unwrap_or_default().trim() != "hello" && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(std::fs::read_to_string(&marker).unwrap().trim(), "hello");
        let plan = folder.join("finish-update.txt");
        std::fs::write(&plan, plan_text(&steps[1..2])).unwrap();
        assert_eq!(finish(&plan), 1);
        assert!(folder.join("finish-update.problems.txt").exists());
        let _ = std::fs::remove_dir_all(folder);
    }
}
