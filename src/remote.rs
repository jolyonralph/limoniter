//! Files on other computers, reached over SFTP (SSH). A remote file is fetched into a folder of copies on this
//! computer and opened from there, so everything that works on a file on disk works on it too, very large files
//! included. Saving writes the copy, then sends it back.
//!
//! The server's key is checked on every connection. The first time, its fingerprint is shown and the person
//! decides whether to trust it; after that a different key is refused, as it may mean someone is in between.
//! Passwords are kept, when asked, in the system's own store of credentials (Windows Credential Manager), never in
//! Limoniter's settings.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const CHUNK: usize = 256 * 1024;
const CREDENTIAL_SERVICE: &str = "Limoniter SFTP";

/// Where to connect, and as whom.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Site {
    pub host: String,
    pub port: u16,
    pub user: String,
    /// A private key file to sign in with, instead of a password.
    pub key_file: Option<PathBuf>,
}

impl Site {
    /// user@host:port, which names the site and its stored password.
    pub fn key(&self) -> String {
        format!("{}@{}:{}", self.user, self.host, self.port)
    }

    pub fn url(&self, path: &str) -> String {
        let port = if self.port == 22 { String::new() } else { format!(":{}", self.port) };
        format!("sftp://{}@{}{port}{}", self.user, self.host, if path.starts_with('/') { path.to_string() } else { format!("/{path}") })
    }
}

/// Reads sftp://user@host:port/path into its parts. The port is 22 unless it's given.
pub fn parse_url(url: &str) -> Result<(Site, String), String> {
    let rest = url.trim().strip_prefix("sftp://").ok_or("A remote file is written sftp://user@host/path.")?;
    let (place, path) = rest.split_once('/').map_or((rest, "/".to_string()), |(place, path)| (place, format!("/{path}")));
    let (user, host_port) = place.rsplit_once('@').ok_or("Say who to sign in as: sftp://user@host/path.")?;
    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port)) if !host.ends_with(']') || host.starts_with('[') => (host, port.parse::<u16>().map_err(|_| format!("\u{201C}{port}\u{201D} isn't a port number."))?),
        _ => (host_port, 22),
    };
    if user.is_empty() || host.is_empty() {
        return Err("Say who to sign in as and where: sftp://user@host/path.".to_string());
    }
    Ok((Site { host: host.trim_matches(|c| c == '[' || c == ']').to_string(), port, user: user.to_string(), key_file: None }, path))
}

/// How to sign in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Secret {
    Password(String),
    /// The passphrase of the site's key file, if it has one.
    KeyPassphrase(Option<String>),
}

/// A server key someone has agreed to trust: the site, and its fingerprint.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KnownHosts {
    pub keys: Vec<(String, String)>,
}

impl KnownHosts {
    fn place(site: &Site) -> String {
        format!("{}:{}", site.host.to_lowercase(), site.port)
    }

    pub fn trust(&mut self, site: &Site, fingerprint: &str) {
        let place = Self::place(site);
        self.keys.retain(|(known, _)| *known != place);
        self.keys.push((place, fingerprint.to_string()));
    }

    pub fn forget(&mut self, site: &Site) {
        let place = Self::place(site);
        self.keys.retain(|(known, _)| *known != place);
    }

    fn known(&self, site: &Site) -> Option<&str> {
        let place = Self::place(site);
        self.keys.iter().find(|(known, _)| *known == place).map(|(_, print)| print.as_str())
    }

    pub fn to_settings(&self) -> String {
        self.keys.iter().map(|(place, print)| format!("sftp_host_key={place} {print}\n")).collect()
    }

    pub fn read_settings(values: &[String]) -> Self {
        Self { keys: values.iter().filter_map(|value| value.split_once(' ')).map(|(place, print)| (place.to_string(), print.to_string())).collect() }
    }
}

/// Why a connection didn't happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// A server not met before. Its key's fingerprint, for the person to check.
    UnknownHost(String),
    /// The server's key isn't the one trusted before.
    HostKeyChanged { expected: String, found: String },
    /// The password or key wasn't accepted.
    SignIn(String),
    Failed(String),
}

impl Refused {
    pub fn describe(&self, site: &Site) -> String {
        match self {
            Refused::UnknownHost(print) => format!("This is the first time Limoniter has connected to {}. Its key's fingerprint is\n\n{print}\n\nIf that's what the server's owner says it should be, trust it and connect.", site.host),
            Refused::HostKeyChanged { expected, found } => format!(
                "The key of {} isn't the one it had before, so Limoniter hasn't connected.\n\nThis can mean the server was set up again, or that someone is between you and it, reading what's sent. Check with whoever looks after the server before trusting the new key.\n\nTrusted before: {expected}\nNow: {found}",
                site.host
            ),
            Refused::SignIn(why) => format!("{} didn't accept the sign-in. {why}", site.host),
            Refused::Failed(why) => why.clone(),
        }
    }
}

fn failed(e: impl std::fmt::Display) -> Refused {
    Refused::Failed(e.to_string())
}

/// An open SFTP connection.
pub struct Connection {
    _session: ssh2::Session,
    sftp: ssh2::Sftp,
}

/// A file or folder in a remote listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub folder: bool,
    pub size: u64,
    pub modified: u64,
}

/// What a remote file was like, to notice if someone else changes it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stamp {
    pub size: u64,
    pub modified: u64,
}

fn stamp_of(stat: &ssh2::FileStat) -> Stamp {
    Stamp { size: stat.size.unwrap_or(0), modified: stat.mtime.unwrap_or(0) }
}

/// The SHA-256 fingerprint of a server key, as OpenSSH shows it.
fn fingerprint(raw: &[u8]) -> String {
    use std::fmt::Write as _;
    let digest = Sha256::digest(raw);
    // Base64 without padding, as ssh-keygen -l prints it.
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::from("SHA256:");
    for chunk in digest.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            let _ = out.write_char(char::from(TABLE[(n >> (18 - 6 * i) & 63) as usize]));
        }
    }
    out
}

/// Connects and signs in. The server's key is checked against those trusted before; see `Refused`.
pub fn connect(site: &Site, secret: &Secret, known: &KnownHosts) -> Result<Connection, Refused> {
    let address = (site.host.as_str(), site.port).to_socket_addrs().map_err(|e| Refused::Failed(format!("{} couldn't be found: {e}", site.host)))?.next().ok_or_else(|| Refused::Failed(format!("{} couldn't be found.", site.host)))?;
    let tcp = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).map_err(|e| Refused::Failed(format!("Couldn't reach {} on port {}: {e}", site.host, site.port)))?;
    let mut session = ssh2::Session::new().map_err(failed)?;
    session.set_timeout(CONNECT_TIMEOUT.as_millis() as u32 * 2);
    session.set_tcp_stream(tcp);
    session.handshake().map_err(|e| Refused::Failed(format!("The SSH greeting with {} failed: {e}", site.host)))?;
    let (raw, _) = session.host_key().ok_or_else(|| Refused::Failed("The server didn't give its key.".to_string()))?;
    let print = fingerprint(raw);
    match known.known(site) {
        None => return Err(Refused::UnknownHost(print)),
        Some(expected) if expected != print => return Err(Refused::HostKeyChanged { expected: expected.to_string(), found: print }),
        Some(_) => {}
    }
    let signed_in = match (secret, &site.key_file) {
        (Secret::KeyPassphrase(passphrase), Some(key)) => session.userauth_pubkey_file(&site.user, None, key, passphrase.as_deref()),
        (Secret::Password(password), _) => session.userauth_password(&site.user, password),
        (Secret::KeyPassphrase(_), None) => session.userauth_agent(&site.user),
    };
    if let Err(e) = signed_in {
        return Err(Refused::SignIn(e.message().to_string()));
    }
    if !session.authenticated() {
        return Err(Refused::SignIn(String::new()));
    }
    let sftp = session.sftp().map_err(|e| Refused::Failed(format!("{} doesn't offer SFTP: {e}", site.host)))?;
    Ok(Connection { _session: session, sftp })
}

fn path(text: &str) -> &Path {
    // Remote paths are always written with /, whatever this computer uses.
    Path::new(text)
}

/// A folder and a name in it, with / between.
pub fn join(folder: &str, name: &str) -> String {
    if folder.ends_with('/') { format!("{folder}{name}") } else { format!("{folder}/{name}") }
}

/// The folder a remote path is in.
pub fn parent(of: &str) -> String {
    match of.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) | None => "/".to_string(),
        Some((folder, _)) => folder.to_string(),
    }
}

impl Connection {
    /// Where the person starts: their home folder, as the server names it.
    pub fn home(&self) -> Result<String, String> {
        self.sftp.realpath(path(".")).map(|p| p.to_string_lossy().replace('\\', "/")).map_err(|e| e.to_string())
    }

    /// What's in a folder: folders first, then files, each by name.
    pub fn list(&self, folder: &str) -> Result<Vec<Entry>, String> {
        let mut entries: Vec<Entry> = self
            .sftp
            .readdir(path(folder))
            .map_err(|e| format!("{folder} couldn't be listed: {e}"))?
            .into_iter()
            .filter_map(|(full, stat)| {
                let name = full.to_string_lossy().replace('\\', "/").rsplit('/').next()?.to_string();
                (name != "." && name != "..").then(|| Entry { name, folder: stat.is_dir(), size: stat.size.unwrap_or(0), modified: stat.mtime.unwrap_or(0) })
            })
            .collect();
        entries.sort_by(|a, b| b.folder.cmp(&a.folder).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(entries)
    }

    pub fn stamp(&self, remote: &str) -> Result<Stamp, String> {
        self.sftp.stat(path(remote)).map(|stat| stamp_of(&stat)).map_err(|e| e.to_string())
    }

    /// Copies a remote file to a local one, counting the bytes as they come.
    pub fn download(&self, remote: &str, local: &Path, done: &AtomicU64, stop: &AtomicBool) -> Result<Stamp, String> {
        let stat = self.sftp.stat(path(remote)).map_err(|e| format!("{remote} couldn't be found: {e}"))?;
        if stat.is_dir() {
            return Err(format!("{remote} is a folder."));
        }
        let mut from = self.sftp.open(path(remote)).map_err(|e| format!("{remote} couldn't be opened: {e}"))?;
        if let Some(folder) = local.parent() {
            std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        }
        let mut to = std::io::BufWriter::new(std::fs::File::create(local).map_err(|e| e.to_string())?);
        let mut buffer = vec![0u8; CHUNK];
        let mut total = 0u64;
        loop {
            if stop.load(Ordering::Relaxed) {
                return Err("It was stopped.".to_string());
            }
            let got = from.read(&mut buffer).map_err(|e| format!("Reading {remote} failed: {e}"))?;
            if got == 0 {
                break;
            }
            to.write_all(&buffer[..got]).map_err(|e| e.to_string())?;
            total += got as u64;
            done.store(total, Ordering::Relaxed);
        }
        to.flush().map_err(|e| e.to_string())?;
        Ok(stamp_of(&stat))
    }

    /// Sends a local file to the server, in place of what's there. It goes to a new file beside it first, so the
    /// file on the server is whole all the time. Unless `force`, it's refused if the remote file has changed since
    /// `expected` (the stamp it had when it was opened).
    pub fn upload(&self, local: &Path, remote: &str, expected: Option<Stamp>, force: bool, done: &AtomicU64, stop: &AtomicBool) -> Result<Stamp, UploadError> {
        let before = self.sftp.stat(path(remote)).ok();
        if !force
            && let (Some(expected), Some(now)) = (expected, before.as_ref())
            && stamp_of(now) != expected
        {
            return Err(UploadError::ChangedOnServer);
        }
        let temp = format!("{remote}.limoniter-tmp");
        let result = (|| -> Result<(), String> {
            let mut from = std::fs::File::open(local).map_err(|e| e.to_string())?;
            let mut to = self.sftp.create(path(&temp)).map_err(|e| format!("{temp} couldn't be made: {e}"))?;
            let mut buffer = vec![0u8; CHUNK];
            let mut total = 0u64;
            loop {
                if stop.load(Ordering::Relaxed) {
                    return Err("It was stopped.".to_string());
                }
                let got = from.read(&mut buffer).map_err(|e| e.to_string())?;
                if got == 0 {
                    break;
                }
                to.write_all(&buffer[..got]).map_err(|e| format!("Sending failed: {e}"))?;
                total += got as u64;
                done.store(total, Ordering::Relaxed);
            }
            to.flush().map_err(|e| e.to_string())?;
            to.fsync().ok();
            drop(to);
            // The new file keeps the permissions the old one had.
            if let Some(perm) = before.as_ref().and_then(|stat| stat.perm) {
                let _ = self.sftp.setstat(path(&temp), ssh2::FileStat { size: None, uid: None, gid: None, perm: Some(perm), atime: None, mtime: None });
            }
            self.replace(&temp, remote, before.is_some())
        })();
        if let Err(problem) = result {
            let _ = self.sftp.unlink(path(&temp));
            return Err(UploadError::Failed(problem));
        }
        self.sftp.stat(path(remote)).map(|stat| stamp_of(&stat)).map_err(|e| UploadError::Failed(e.to_string()))
    }

    /// Puts `temp` where `target` is. Many servers won't rename over a file, so the old one is moved aside first
    /// and put back if that fails.
    fn replace(&self, temp: &str, target: &str, exists: bool) -> Result<(), String> {
        let flags = ssh2::RenameFlags::OVERWRITE | ssh2::RenameFlags::ATOMIC | ssh2::RenameFlags::NATIVE;
        if !exists || self.sftp.rename(path(temp), path(target), Some(flags)).is_ok() {
            return if exists { Ok(()) } else { self.sftp.rename(path(temp), path(target), Some(flags)).map_err(|e| e.to_string()) };
        }
        let aside = format!("{target}.limoniter-old");
        let _ = self.sftp.unlink(path(&aside));
        self.sftp.rename(path(target), path(&aside), None).map_err(|e| format!("{target} couldn't be replaced: {e}"))?;
        if let Err(e) = self.sftp.rename(path(temp), path(target), None) {
            let _ = self.sftp.rename(path(&aside), path(target), None);
            return Err(format!("{target} couldn't be replaced: {e}"));
        }
        let _ = self.sftp.unlink(path(&aside));
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UploadError {
    /// Someone else has changed the file on the server since it was opened.
    ChangedOnServer,
    Failed(String),
}

// ---------- Stored passwords ----------

/// A password kept in the system's store for a site, if there is one.
pub fn stored_password(site: &Site) -> Option<String> {
    keyring::Entry::new(CREDENTIAL_SERVICE, &site.key()).ok()?.get_password().ok()
}

pub fn store_password(site: &Site, password: &str) -> Result<(), String> {
    keyring::Entry::new(CREDENTIAL_SERVICE, &site.key()).and_then(|entry| entry.set_password(password)).map_err(|e| e.to_string())
}

pub fn forget_password(site: &Site) {
    if let Ok(entry) = keyring::Entry::new(CREDENTIAL_SERVICE, &site.key()) {
        let _ = entry.delete_credential();
    }
}

// ---------- Work in the background ----------

/// What a background job is asked to do.
#[derive(Clone, Debug)]
pub enum Task {
    /// Connect, and list the home folder or the folder given.
    List(Option<String>),
    Download { remote: String, local: PathBuf },
    Upload { local: PathBuf, remote: String, expected: Option<Stamp>, force: bool },
}

#[derive(Clone, Debug)]
pub enum Outcome {
    Listed { folder: String, entries: Vec<Entry> },
    Downloaded { remote: String, local: PathBuf, stamp: Stamp },
    Uploaded { stamp: Stamp },
    Refused(Refused),
    ChangedOnServer,
    Failed(String),
}

/// A connection and one task, on a thread of its own so the window keeps working.
pub struct Job {
    pub site: Site,
    pub done: Arc<AtomicU64>,
    pub total: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Outcome>>>,
}

impl Job {
    pub fn start(site: Site, secret: Secret, known: KnownHosts, task: Task) -> Self {
        let (done, total, stop, result) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)), Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(None)));
        let worker_site = site.clone();
        let (worker_done, worker_total, worker_stop, worker_result) = (done.clone(), total.clone(), stop.clone(), result.clone());
        std::thread::spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| perform(&worker_site, &secret, &known, task, &worker_done, &worker_total, &worker_stop))).unwrap_or_else(|_| Outcome::Failed("It failed unexpectedly.".to_string()));
            *worker_result.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(outcome);
        });
        Self { site, done, total, stop, result }
    }

    pub fn take(&self) -> Option<Outcome> {
        self.result.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
    }

    pub fn share(&self) -> f32 {
        self.done.load(Ordering::Relaxed) as f32 / self.total.load(Ordering::Relaxed).max(1) as f32
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn perform(site: &Site, secret: &Secret, known: &KnownHosts, task: Task, done: &AtomicU64, total: &AtomicU64, stop: &AtomicBool) -> Outcome {
    let connection = match connect(site, secret, known) {
        Ok(connection) => connection,
        Err(refused) => return Outcome::Refused(refused),
    };
    match task {
        Task::List(folder) => {
            let folder = match folder {
                Some(folder) => folder,
                None => match connection.home() {
                    Ok(home) => home,
                    Err(problem) => return Outcome::Failed(problem),
                },
            };
            match connection.list(&folder) {
                Ok(entries) => Outcome::Listed { folder, entries },
                Err(problem) => Outcome::Failed(problem),
            }
        }
        Task::Download { remote, local } => {
            if let Ok(stamp) = connection.stamp(&remote) {
                total.store(stamp.size, Ordering::Relaxed);
            }
            match connection.download(&remote, &local, done, stop) {
                Ok(stamp) => Outcome::Downloaded { remote, local, stamp },
                Err(problem) => Outcome::Failed(problem),
            }
        }
        Task::Upload { local, remote, expected, force } => {
            total.store(std::fs::metadata(&local).map_or(0, |m| m.len()), Ordering::Relaxed);
            match connection.upload(&local, &remote, expected, force, done, stop) {
                Ok(stamp) => Outcome::Uploaded { stamp },
                Err(UploadError::ChangedOnServer) => Outcome::ChangedOnServer,
                Err(UploadError::Failed(problem)) => Outcome::Failed(problem),
            }
        }
    }
}

/// Where the copy of a remote file is kept on this computer: a folder for each site and path, so two files of the
/// same name don't meet, with the file's own name inside so its ending still says what it is.
pub fn local_copy(root: &Path, site: &Site, remote: &str) -> PathBuf {
    let digest: String = Sha256::digest(site.url(remote).as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect();
    let name = remote.rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or("file");
    root.join(digest).join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_read_and_written() {
        let (site, path) = parse_url("sftp://ann@example.org/home/ann/notes.txt").unwrap();
        assert_eq!((site.user.as_str(), site.host.as_str(), site.port, path.as_str()), ("ann", "example.org", 22, "/home/ann/notes.txt"));
        assert_eq!(site.url(&path), "sftp://ann@example.org/home/ann/notes.txt");
        let (site, path) = parse_url("sftp://bob@10.0.0.2:2222").unwrap();
        assert_eq!((site.port, path.as_str()), (2222, "/"));
        assert_eq!(site.url("/x"), "sftp://bob@10.0.0.2:2222/x");
        let (site, _) = parse_url("sftp://c@[::1]:2200/f").unwrap();
        assert_eq!((site.host.as_str(), site.port), ("::1", 2200));
        assert!(parse_url("http://x/y").is_err());
        assert!(parse_url("sftp://host/y").is_err());
        assert!(parse_url("sftp://u@h:port/y").is_err());
    }

    #[test]
    fn remote_paths_use_slashes() {
        assert_eq!(join("/home/ann", "a.txt"), "/home/ann/a.txt");
        assert_eq!(join("/", "a.txt"), "/a.txt");
        assert_eq!(parent("/home/ann/a.txt"), "/home/ann");
        assert_eq!(parent("/a.txt"), "/");
        assert_eq!(parent("/"), "/");
    }

    #[test]
    fn fingerprints_are_written_as_openssh_writes_them() {
        // ssh-keygen -lf prints the SHA-256 of the key blob in base64 without padding.
        let print = fingerprint(b"abc");
        assert_eq!(print, "SHA256:ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0");
    }

    #[test]
    fn a_trusted_key_is_kept_and_a_different_one_noticed() {
        let site = Site { host: "Example.org".into(), port: 22, user: "ann".into(), key_file: None };
        let mut known = KnownHosts::default();
        assert_eq!(known.known(&site), None);
        known.trust(&site, "SHA256:aaa");
        let again = KnownHosts::read_settings(&known.to_settings().lines().filter_map(|l| l.strip_prefix("sftp_host_key=")).map(str::to_string).collect::<Vec<_>>());
        assert_eq!(again.known(&Site { host: "example.org".into(), ..site.clone() }), Some("SHA256:aaa"));
        known.forget(&site);
        assert!(known.keys.is_empty());
    }

    #[test]
    fn copies_of_remote_files_keep_their_names_apart() {
        let site = Site { host: "h".into(), port: 22, user: "u".into(), key_file: None };
        let a = local_copy(Path::new("C:/cache"), &site, "/one/notes.txt");
        let b = local_copy(Path::new("C:/cache"), &site, "/two/notes.txt");
        assert_ne!(a, b);
        assert_eq!(a.file_name().unwrap(), "notes.txt");
    }

    /// Writes to the real credential store, so it's run by hand. It removes what it adds.
    #[test]
    #[ignore = "touches the system's credential store"]
    fn a_kept_password_comes_back_and_can_be_forgotten() {
        let site = Site { host: "limoniter-test.invalid".into(), port: 2299, user: "nobody".into(), key_file: None };
        store_password(&site, "not a real password").unwrap();
        assert_eq!(stored_password(&site).as_deref(), Some("not a real password"));
        forget_password(&site);
        assert_eq!(stored_password(&site), None);
    }

    /// Against a real server: LIMONITER_SFTP is sftp://user@host:port/folder, LIMONITER_SFTP_KEY a private key file
    /// that it accepts, and LIMONITER_SFTP_PASSWORD the password, if that's to be tried too.
    #[test]
    #[ignore = "needs an SFTP server named in LIMONITER_SFTP"]
    fn a_real_server_lists_sends_and_fetches() {
        let Ok(url) = std::env::var("LIMONITER_SFTP") else {
            return;
        };
        let (mut site, folder) = parse_url(&url).unwrap();
        let unknown = connect(&site, &Secret::Password(String::new()), &KnownHosts::default());
        let Err(Refused::UnknownHost(print)) = unknown else { panic!("a new server's key has to be agreed to first: {:?}", unknown.err()) };
        let mut known = KnownHosts::default();
        known.trust(&site, &print);
        let mut wrong = KnownHosts::default();
        wrong.trust(&site, "SHA256:not-it");
        assert!(matches!(connect(&site, &Secret::Password(String::new()), &wrong), Err(Refused::HostKeyChanged { .. })));
        if let Ok(password) = std::env::var("LIMONITER_SFTP_PASSWORD") {
            assert!(matches!(connect(&site, &Secret::Password("wrong".into()), &known), Err(Refused::SignIn(_))));
            connect(&site, &Secret::Password(password), &known).unwrap();
        }
        site.key_file = std::env::var("LIMONITER_SFTP_KEY").ok().map(PathBuf::from);
        let connection = connect(&site, &Secret::KeyPassphrase(None), &known).unwrap_or_else(|e| panic!("{}", e.describe(&site)));
        let names: Vec<String> = connection.list(&folder).unwrap().into_iter().map(|e| e.name).collect();
        eprintln!("{folder}: {names:?}");
        let local = std::env::temp_dir().join(format!("limoniter-sftp-{}.txt", std::process::id()));
        let content: Vec<u8> = (0..1_000_000u32).flat_map(|i| format!("line {i}\n").into_bytes()).collect();
        std::fs::write(&local, &content).unwrap();
        let remote = join(&folder, "limoniter-test.txt");
        let (done, stop) = (AtomicU64::new(0), AtomicBool::new(false));
        let first = connection.upload(&local, &remote, None, false, &done, &stop).unwrap();
        assert_eq!(first.size, content.len() as u64);
        // Sent again over itself, which many servers make awkward.
        std::fs::write(&local, b"replaced\n").unwrap();
        let second = connection.upload(&local, &remote, Some(first), false, &done, &stop).unwrap();
        assert_eq!(second.size, 9);
        assert_eq!(connection.upload(&local, &remote, Some(first), false, &done, &stop), Err(UploadError::ChangedOnServer), "it's changed since the first stamp");
        let back = local.with_extension("back");
        connection.download(&remote, &back, &done, &stop).unwrap();
        assert_eq!(std::fs::read(&back).unwrap(), b"replaced\n");
        assert!(connection.list(&folder).unwrap().iter().all(|e| !e.name.ends_with(".limoniter-tmp") && !e.name.ends_with(".limoniter-old")), "nothing left behind");
        connection.sftp.unlink(path(&remote)).unwrap();
        let _ = std::fs::remove_file(&local);
        let _ = std::fs::remove_file(&back);
    }
}
