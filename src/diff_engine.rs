//! Comparing: where a tree of files comes from (a folder, a Git revision, a disk image or archive, or a folder on
//! a server), comparing two trees entry by entry, copying between them, and comparing two texts line by line and
//! within lines. Nothing here draws; the compare tabs show what this finds.

use crate::remote;
use sha2::{Digest, Sha256};
use similar::{Algorithm, DiffOp, capture_diff_slices_deadline};
use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long the search for the closest match between two texts may take. After that the result is still
/// correct, but cruder.
const MOST_DIFF_TIME: Duration = Duration::from_secs(5);
/// Two modified times this close are the same time: FAT keeps them to two seconds.
const SAME_TIME: u64 = 2;

// ---------- Where a tree comes from ----------

/// Where a tree of files comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Side {
    /// A folder on this computer.
    Folder(PathBuf),
    /// A folder of a repository as it was at a revision ("HEAD", a branch, a commit). `folder` is the place in
    /// the repository, with forward slashes, empty for the top.
    Git { repo: PathBuf, revision: String, folder: String },
    /// A disk image or archive, read as the file system it holds. `folder` is a folder inside it, or empty.
    Image { file: PathBuf, folder: String },
    /// A folder on an SFTP server, signed in to with the password kept in the system's credential store, or
    /// with the site's key file. Its key has to have been trusted before.
    Sftp { site: remote::Site, folder: String },
}

impl Side {
    /// What it is, for a heading.
    pub fn describe(&self) -> String {
        match self {
            Side::Folder(path) => path.display().to_string(),
            Side::Git { repo, revision, folder } => {
                let place = if folder.is_empty() { repo.display().to_string() } else { repo.join(folder).display().to_string() };
                format!("{place} at {}", revision_words(revision))
            }
            Side::Image { file, folder } if folder.is_empty() => file.display().to_string(),
            Side::Image { file, folder } => format!("{} \u{203A} {folder}", file.display()),
            Side::Sftp { site, folder } => site.url(folder),
        }
    }

    /// The same place in a Git revision: the repository a folder or file is in, at that revision.
    pub fn git_of(path: &Path, revision: &str) -> Result<Side, String> {
        let repo = crate::git::repository_of(path).ok_or_else(|| crate::i18n::tf!("{} isn't in a Git repository.", path.display()))?;
        let folder = crate::git::relative(&repo, path).unwrap_or_default();
        Ok(Side::Git { repo, revision: revision.to_string(), folder })
    }

    /// A side from a path: a disk image or archive is read as what it holds, a URL as a server, anything else
    /// as a folder.
    pub fn from_path(path: &Path) -> Side {
        let text = path.to_string_lossy();
        if text.starts_with("sftp://")
            && let Ok((site, folder)) = remote::parse_url(&text)
        {
            return Side::Sftp { site, folder };
        }
        if path.is_file() {
            return Side::Image { file: path.to_path_buf(), folder: String::new() };
        }
        Side::Folder(path.to_path_buf())
    }

    /// Whether files can be copied into it: a folder here, or a folder on a server.
    #[cfg(test)]
    pub fn writable(&self) -> bool {
        self.refusal().is_none()
    }

    /// Why files can't be copied into it, if they can't.
    pub fn refusal(&self) -> Option<String> {
        match self {
            Side::Folder(_) | Side::Sftp { .. } => None,
            Side::Git { revision, .. } => Some(format!("{} is history kept by Git, so files can't be copied into it. Compare with the folder itself to change it.", revision_words(revision))),
            Side::Image { file, .. } => Some(format!(
                "Files can't be copied into {} from a comparison. Each kind of disk image has its own rules for names and kinds of file, and an archive is only read here; open the image in a tab of its own to add files to it, choosing how each one goes on.",
                file.file_name().map_or_else(|| file.display().to_string(), |name| name.to_string_lossy().into_owned())
            )),
        }
    }
}

/// "the last commit" for HEAD, the first eight characters of a commit's hash, or a branch's name.
pub fn revision_words(revision: &str) -> String {
    if revision == "HEAD" {
        "the last commit".to_string()
    } else if revision.len() >= 20 && revision.chars().all(|c| c.is_ascii_hexdigit()) {
        format!("commit {}", &revision[..8])
    } else {
        revision.to_string()
    }
}

/// A file or folder in a tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    /// Seconds since 1970, when the side keeps times. A Git revision and most images don't.
    pub modified: Option<u64>,
}

/// A tree of files that can be listed and read, a folder at a time.
pub trait Tree: Send + Sync {
    /// What's in a folder, given by its path from the top with forward slashes ("" for the top).
    fn list(&self, folder: &str) -> Result<Vec<Item>, String>;
    /// A file's bytes.
    fn read(&self, path: &str) -> Result<Vec<u8>, String>;
    /// A fingerprint of a file's bytes, to compare contents by.
    fn hash(&self, path: &str, cancel: &AtomicBool) -> Result<[u8; 32], String> {
        let _ = cancel;
        Ok(Sha256::digest(self.read(path)?).into())
    }
    /// Where a file is on this computer, when the tree is a folder here.
    fn local_path(&self, path: &str) -> Option<PathBuf> {
        let _ = path;
        None
    }
    /// Whether files can be written into it.
    fn can_write(&self) -> bool {
        false
    }
    /// What's at a path now, if anything, to check it hasn't changed since it was compared.
    fn stat(&self, path: &str) -> Result<Option<Meta>, String> {
        let _ = path;
        Err(READ_ONLY.into())
    }
    /// Puts a file in place of whatever is at its path, whole or not at all, with its modified time if given.
    fn write(&self, path: &str, bytes: &[u8], modified: Option<u64>) -> Result<(), String> {
        let _ = (path, bytes, modified);
        Err(READ_ONLY.into())
    }
    /// Makes a folder, and any it's in that aren't there.
    fn make_folder(&self, path: &str) -> Result<(), String> {
        let _ = path;
        Err(READ_ONLY.into())
    }
    /// Takes a file away.
    fn remove_file(&self, path: &str) -> Result<(), String> {
        let _ = path;
        Err(READ_ONLY.into())
    }
    /// Takes an empty folder away. One with anything in it is left, and that's said.
    fn remove_folder(&self, path: &str) -> Result<(), String> {
        let _ = path;
        Err(READ_ONLY.into())
    }
}

const READ_ONLY: &str = "This side can only be read.";

/// Whether a path from the top stays inside it: parts with no "..", ".", backslash or nothing at all (nor, on
/// Windows, a colon, which could name a drive), so nothing is ever written outside the side being written to.
pub fn safe_path(path: &str) -> Result<(), String> {
    let fine = !path.is_empty() && path.split('/').all(|part| !part.is_empty() && part != "." && part != ".." && !part.contains(['\\', '\0']) && !(cfg!(windows) && part.contains(':')));
    if fine { Ok(()) } else { Err(crate::i18n::tf!("\u{201C}{path}\u{201D} isn't a path inside the folder, so it was left alone.")) }
}

/// Opens a side, ready to list. A server is connected to here, so this can take a while.
pub fn open_tree(side: &Side, known: &remote::KnownHosts) -> Result<Arc<dyn Tree>, String> {
    match side {
        Side::Folder(root) => {
            if !root.is_dir() {
                return Err(crate::i18n::tf!("{} isn't a folder.", root.display()));
            }
            Ok(Arc::new(LocalTree { root: root.clone() }))
        }
        Side::Git { repo, revision, folder } => Ok(Arc::new(GitTree::open(repo, revision, folder)?)),
        Side::Image { file, folder } => Ok(Arc::new(ImageTree::open(file, folder)?)),
        Side::Sftp { site, folder } => Ok(Arc::new(SftpTree::open(site, folder, known)?)),
    }
}

/// Joins a folder's path and a name in it.
pub fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() { name.to_string() } else { format!("{folder}/{name}") }
}

fn seconds(time: std::io::Result<std::time::SystemTime>) -> Option<u64> {
    time.ok()?.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// A folder on this computer.
pub struct LocalTree {
    pub root: PathBuf,
}

impl LocalTree {
    fn place(&self, path: &str) -> PathBuf {
        path.split('/').filter(|part| !part.is_empty()).fold(self.root.clone(), |at, part| at.join(part))
    }

    /// Where a path is, to write to: checked to stay inside the folder, through no link that could lead out.
    fn place_to_write(&self, path: &str) -> Result<PathBuf, String> {
        safe_path(path)?;
        let mut at = self.root.clone();
        let parts: Vec<&str> = path.split('/').collect();
        for (n, part) in parts.iter().enumerate() {
            at.push(part);
            if std::fs::symlink_metadata(&at).is_ok_and(|meta| meta.file_type().is_symlink()) {
                let what = if n + 1 == parts.len() { "is a link" } else { "goes through a link" };
                return Err(crate::i18n::tf!("{path} {what}, which could lead outside the folder, so it was left alone."));
            }
        }
        Ok(at)
    }
}

impl Tree for LocalTree {
    fn list(&self, folder: &str) -> Result<Vec<Item>, String> {
        let place = self.place(folder);
        let mut items = Vec::new();
        for entry in std::fs::read_dir(&place).map_err(|e| crate::i18n::tf!("{} couldn't be listed: {e}", place.display()))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else { continue };
            // A link is followed to a file, but not into a folder, which could lead round in a circle.
            let meta = if kind.is_symlink() { std::fs::metadata(entry.path()) } else { entry.metadata() };
            let Ok(meta) = meta else { continue };
            if kind.is_symlink() && meta.is_dir() {
                continue;
            }
            items.push(Item { name, is_dir: meta.is_dir(), size: if meta.is_dir() { 0 } else { meta.len() }, modified: seconds(meta.modified()) });
        }
        Ok(items)
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        std::fs::read(self.place(path)).map_err(|e| crate::i18n::tf!("{path} couldn't be read: {e}"))
    }

    fn hash(&self, path: &str, cancel: &AtomicBool) -> Result<[u8; 32], String> {
        use std::io::Read;
        let mut file = std::fs::File::open(self.place(path)).map_err(|e| crate::i18n::tf!("{path} couldn't be read: {e}"))?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0u8; 256 * 1024];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(crate::i18n::tr("It was stopped.").into());
            }
            let got = file.read(&mut buffer).map_err(|e| crate::i18n::tf!("{path} couldn't be read: {e}"))?;
            if got == 0 {
                break;
            }
            hasher.update(&buffer[..got]);
        }
        Ok(hasher.finalize().into())
    }

    fn local_path(&self, path: &str) -> Option<PathBuf> {
        Some(self.place(path))
    }

    fn can_write(&self) -> bool {
        true
    }

    fn stat(&self, path: &str) -> Result<Option<Meta>, String> {
        match std::fs::symlink_metadata(self.place_to_write(path)?) {
            Ok(meta) => Ok(Some(Meta { is_dir: meta.is_dir(), size: if meta.is_dir() { 0 } else { meta.len() }, modified: seconds(meta.modified()) })),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn write(&self, path: &str, bytes: &[u8], modified: Option<u64>) -> Result<(), String> {
        let target = self.place_to_write(path)?;
        if let Some(folder) = target.parent() {
            std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
        }
        crate::codec::write_file(&target, bytes).map_err(|e| e.to_string())?;
        if let Some(time) = modified {
            let when = std::time::UNIX_EPOCH + Duration::from_secs(time);
            let _ = std::fs::OpenOptions::new().write(true).open(&target).and_then(|file| file.set_modified(when));
        }
        Ok(())
    }

    fn make_folder(&self, path: &str) -> Result<(), String> {
        std::fs::create_dir_all(self.place_to_write(path)?).map_err(|e| e.to_string())
    }

    fn remove_file(&self, path: &str) -> Result<(), String> {
        std::fs::remove_file(self.place_to_write(path)?).map_err(|e| e.to_string())
    }

    fn remove_folder(&self, path: &str) -> Result<(), String> {
        let place = self.place_to_write(path)?;
        std::fs::remove_dir(&place).map_err(|e| if std::fs::read_dir(&place).is_ok_and(|mut entries| entries.next().is_some()) { "It still has files in it that weren't compared, so it was kept.".to_string() } else { e.to_string() })
    }
}

/// A folder of a repository at a revision, listed all at once with `git ls-tree`.
pub struct GitTree {
    repo: PathBuf,
    revision: String,
    /// The folder's place in the repository, with a / after it, or empty for the top.
    prefix: String,
    folders: HashMap<String, Vec<Item>>,
}

impl GitTree {
    pub fn open(repo: &Path, revision: &str, folder: &str) -> Result<Self, String> {
        let revision = revision.trim();
        if revision.is_empty() {
            return Err(crate::i18n::tr("Say which revision: HEAD, a branch, a tag or a commit.").into());
        }
        crate::git::run(repo, &["rev-parse", "--verify", "--quiet", &format!("{revision}^{{tree}}")]).map_err(|_| crate::i18n::tf!("{revision} isn't a revision of this repository."))?;
        let folder = folder.trim_matches('/');
        let prefix = if folder.is_empty() { String::new() } else { format!("{folder}/") };
        let mut args = vec!["ls-tree", "-r", "-t", "-l", "-z", revision];
        if !prefix.is_empty() {
            args.extend(["--", prefix.as_str()]);
        }
        let listing = crate::git::run(repo, &args)?;
        Ok(Self { repo: repo.to_path_buf(), revision: revision.to_string(), folders: parse_ls_tree(&listing, &prefix), prefix })
    }
}

/// Reads `git ls-tree -r -t -l -z` into each folder's items, by the folder's path below `prefix`.
fn parse_ls_tree(listing: &str, prefix: &str) -> HashMap<String, Vec<Item>> {
    let mut folders: HashMap<String, Vec<Item>> = HashMap::new();
    for record in listing.split('\0').filter(|record| !record.is_empty()) {
        let Some((info, path)) = record.split_once('\t') else { continue };
        let mut parts = info.split_whitespace();
        let (_mode, kind, _object, size) = (parts.next(), parts.next(), parts.next(), parts.next());
        let Some(inside) = path.strip_prefix(prefix) else { continue };
        if inside.is_empty() {
            continue;
        }
        let is_dir = match kind {
            Some("tree") => true,
            Some("blob") => false,
            // A submodule is another repository, not part of this one's files.
            _ => continue,
        };
        let (parent, name) = inside.rsplit_once('/').unwrap_or(("", inside));
        let size = size.and_then(|size| size.parse().ok()).unwrap_or(0);
        folders.entry(parent.to_string()).or_default().push(Item { name: name.to_string(), is_dir, size, modified: None });
    }
    folders
}

impl Tree for GitTree {
    fn list(&self, folder: &str) -> Result<Vec<Item>, String> {
        Ok(self.folders.get(folder).cloned().unwrap_or_default())
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        crate::git::file_at(&self.repo, &self.revision, &format!("{}{path}", self.prefix)).ok_or_else(|| crate::i18n::tf!("{path} couldn't be read from {}.", revision_words(&self.revision)))
    }
}

/// A disk image or archive, read through the media readers.
pub struct ImageTree {
    volume: Box<dyn crate::media::Volume>,
    /// The folder inside the image that's the top, or None for its own top.
    top: Option<crate::media::Entry>,
    /// Every entry listed so far, by path, so a file can be read later.
    seen: Mutex<HashMap<String, crate::media::Entry>>,
}

impl ImageTree {
    pub fn open(file: &Path, folder: &str) -> Result<Self, String> {
        let source: Arc<dyn crate::media::Source> = Arc::new(crate::media::FileSource::open(file)?);
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let volume = crate::media::open(source, &name).map_err(|e| crate::i18n::tf!("{name} couldn't be read as a disk image or archive: {e}"))?;
        let mut top = None;
        for part in folder.split('/').filter(|part| !part.is_empty()) {
            let found = volume.list(top.as_ref())?.into_iter().find(|entry| entry.is_dir && entry.name == part).ok_or_else(|| crate::i18n::tf!("{name} has no folder {folder}."))?;
            top = Some(found);
        }
        Ok(Self { volume, top, seen: Mutex::new(HashMap::new()) })
    }
}

impl Tree for ImageTree {
    fn list(&self, folder: &str) -> Result<Vec<Item>, String> {
        let mut seen = self.seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = if folder.is_empty() { self.top.clone() } else { Some(seen.get(folder).cloned().ok_or_else(|| crate::i18n::tf!("{folder} hasn't been listed."))?) };
        let entries = self.volume.list(dir.as_ref())?;
        let mut items = Vec::new();
        for entry in entries {
            // A name with a slash in it would read as a folder; it's shown with the slash changed.
            let name = entry.name.replace('/', "\u{2215}");
            if name.is_empty() || name == "." || name == ".." {
                continue;
            }
            items.push(Item { name: name.clone(), is_dir: entry.is_dir, size: entry.size, modified: None });
            seen.insert(join(folder, &name), entry);
        }
        Ok(items)
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let entry = self.seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get(path).cloned().ok_or_else(|| crate::i18n::tf!("{path} hasn't been listed."))?;
        self.volume.read(&entry)
    }
}

/// What a folder comparison needs of a server: listing, fetching and sending files, and making and taking away
/// files and folders. An SFTP connection does it; tests use a pretend server.
pub trait RemoteFs: Send {
    fn home(&self) -> Result<String, String>;
    fn list(&self, folder: &str) -> Result<Vec<remote::Entry>, String>;
    /// Whether something is at a path: if so, whether it's a folder, and its size and time.
    fn look(&self, path: &str) -> Result<Option<(bool, remote::Stamp)>, String>;
    fn download(&self, path: &str, local: &Path) -> Result<(), String>;
    /// Sends a file in place of what's there, so the one on the server is whole all the time.
    fn upload(&self, local: &Path, path: &str) -> Result<(), String>;
    fn set_modified(&self, path: &str, time: u64) -> Result<(), String>;
    fn make_folder(&self, path: &str) -> Result<(), String>;
    fn remove_file(&self, path: &str) -> Result<(), String>;
    fn remove_folder(&self, path: &str) -> Result<(), String>;
}

impl RemoteFs for remote::Connection {
    fn home(&self) -> Result<String, String> {
        remote::Connection::home(self)
    }
    fn list(&self, folder: &str) -> Result<Vec<remote::Entry>, String> {
        remote::Connection::list(self, folder)
    }
    fn look(&self, path: &str) -> Result<Option<(bool, remote::Stamp)>, String> {
        remote::Connection::look(self, path)
    }
    fn download(&self, path: &str, local: &Path) -> Result<(), String> {
        remote::Connection::download(self, path, local, &AtomicU64::new(0), &AtomicBool::new(false)).map(|_| ())
    }
    fn upload(&self, local: &Path, path: &str) -> Result<(), String> {
        remote::Connection::upload(self, local, path, None, true, &AtomicU64::new(0), &AtomicBool::new(false)).map(|_| ()).map_err(|e| match e {
            remote::UploadError::ChangedOnServer => "It changed on the server.".to_string(),
            remote::UploadError::Failed(problem) => problem,
        })
    }
    fn set_modified(&self, path: &str, time: u64) -> Result<(), String> {
        remote::Connection::set_modified(self, path, time)
    }
    fn make_folder(&self, path: &str) -> Result<(), String> {
        remote::Connection::make_folder(self, path)
    }
    fn remove_file(&self, path: &str) -> Result<(), String> {
        remote::Connection::remove_file(self, path)
    }
    fn remove_folder(&self, path: &str) -> Result<(), String> {
        remote::Connection::remove_folder(self, path)
    }
}

/// A file on this computer for a moment, taken away again when it's done with.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!("limoniter-compare-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A folder on an SFTP server. Files are written to it only by sending each to a new file beside the old and
/// putting it in place, so a file there is never half written.
pub struct SftpTree {
    connection: Mutex<Box<dyn RemoteFs>>,
    root: String,
    site: remote::Site,
}

impl SftpTree {
    /// Connects with what's kept for the site: its password in the credential store, or its key file, or the
    /// SSH agent. A server whose key hasn't been trusted before is refused.
    pub fn open(site: &remote::Site, folder: &str, known: &remote::KnownHosts) -> Result<Self, String> {
        let stored = remote::stored_password(site);
        let secret = match (&site.key_file, stored) {
            (Some(_), passphrase) => remote::Secret::KeyPassphrase(passphrase),
            (None, Some(password)) => remote::Secret::Password(password),
            (None, None) => remote::Secret::KeyPassphrase(None),
        };
        let connection = remote::connect(site, &secret, known).map_err(|refused| match refused {
            remote::Refused::UnknownHost(_) => format!("Limoniter hasn't connected to {} before. Connect to it once with File, Open a remote file, to check its key and trust it, and keep its password if it needs one.", site.host),
            other => other.describe(site),
        })?;
        Self::with(Box::new(connection), site, folder)
    }

    /// A tree on a connection already made, at a folder: the home folder for "" or "~", and "~/sub" in it.
    pub fn with(connection: Box<dyn RemoteFs>, site: &remote::Site, folder: &str) -> Result<Self, String> {
        let in_home = folder.trim_start_matches('/').strip_prefix('~').filter(|rest| rest.is_empty() || rest.starts_with('/'));
        let root = match in_home {
            _ if folder.is_empty() => connection.home()?,
            Some(rest) if rest.trim_matches('/').is_empty() => connection.home()?,
            Some(rest) => remote::join(&connection.home()?, rest.trim_matches('/')),
            None => folder.to_string(),
        };
        Ok(Self { connection: Mutex::new(connection), root, site: site.clone() })
    }

    fn place(&self, path: &str) -> String {
        if path.is_empty() { self.root.clone() } else { remote::join(&self.root, path) }
    }

    fn connection(&self) -> std::sync::MutexGuard<'_, Box<dyn RemoteFs>> {
        self.connection.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn url(&self, path: &str) -> String {
        self.site.url(&self.place(path))
    }
}

impl Tree for SftpTree {
    fn list(&self, folder: &str) -> Result<Vec<Item>, String> {
        Ok(self.connection().list(&self.place(folder))?.into_iter().map(|entry| Item { name: entry.name, is_dir: entry.folder, size: entry.size, modified: Some(entry.modified) }).collect())
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let local = Scratch::new();
        self.connection().download(&self.place(path), &local.0).and_then(|()| std::fs::read(&local.0).map_err(|e| e.to_string())).map_err(|e| crate::i18n::tf!("{} couldn't be read: {e}", self.url(path)))
    }

    fn can_write(&self) -> bool {
        true
    }

    fn stat(&self, path: &str) -> Result<Option<Meta>, String> {
        safe_path(path)?;
        Ok(self.connection().look(&self.place(path))?.map(|(is_dir, stamp)| Meta { is_dir, size: if is_dir { 0 } else { stamp.size }, modified: Some(stamp.modified) }))
    }

    fn write(&self, path: &str, bytes: &[u8], modified: Option<u64>) -> Result<(), String> {
        safe_path(path)?;
        if let Some((folder, _)) = path.rsplit_once('/') {
            self.make_folder(folder)?;
        }
        let local = Scratch::new();
        std::fs::write(&local.0, bytes).map_err(|e| e.to_string())?;
        let connection = self.connection();
        let place = self.place(path);
        connection.upload(&local.0, &place).map_err(|e| crate::i18n::tf!("{} couldn't be sent: {e}", self.url(path)))?;
        if let Some(time) = modified {
            let _ = connection.set_modified(&place, time);
        }
        Ok(())
    }

    fn make_folder(&self, path: &str) -> Result<(), String> {
        safe_path(path)?;
        let connection = self.connection();
        let mut at = String::new();
        for part in path.split('/') {
            at = join(&at, part);
            match connection.look(&self.place(&at))? {
                Some((true, _)) => {}
                Some((false, _)) => return Err(crate::i18n::tf!("{} is a file, so a folder can't be made there.", self.url(&at))),
                None => connection.make_folder(&self.place(&at))?,
            }
        }
        Ok(())
    }

    fn remove_file(&self, path: &str) -> Result<(), String> {
        safe_path(path)?;
        self.connection().remove_file(&self.place(path))
    }

    fn remove_folder(&self, path: &str) -> Result<(), String> {
        safe_path(path)?;
        let connection = self.connection();
        if !connection.list(&self.place(path))?.is_empty() {
            return Err(crate::i18n::tr("It still has files in it that weren't compared, so it was kept.").into());
        }
        connection.remove_folder(&self.place(path))
    }
}

/// A tree held in memory, for tests. It can be written to when it's made `writable`.
#[cfg(test)]
#[derive(Default)]
pub struct MemoryTree {
    pub files: Mutex<BTreeMap<String, (Vec<u8>, Option<u64>)>>,
    /// Folders made that may have nothing in them.
    pub folders: Mutex<std::collections::BTreeSet<String>>,
    pub writable: bool,
}

#[cfg(test)]
impl MemoryTree {
    pub fn with(files: &[(&str, &str, Option<u64>)]) -> Self {
        Self { files: Mutex::new(files.iter().map(|(path, text, time)| (path.to_string(), (text.as_bytes().to_vec(), *time))).collect()), ..Self::default() }
    }

    pub fn writable_with(files: &[(&str, &str, Option<u64>)]) -> Self {
        Self { writable: true, ..Self::with(files) }
    }

    /// A file's text and time, for a test to check.
    pub fn get(&self, path: &str) -> Option<(String, Option<u64>)> {
        self.files.lock().unwrap().get(path).map(|(bytes, time)| (String::from_utf8_lossy(bytes).into_owned(), *time))
    }

    fn has_folder(&self, path: &str) -> bool {
        self.folders.lock().unwrap().contains(path) || self.files.lock().unwrap().keys().any(|file| file.starts_with(&format!("{path}/")))
    }

    fn writing(&self, path: &str) -> Result<(), String> {
        if !self.writable {
            return Err(READ_ONLY.into());
        }
        safe_path(path)
    }
}

#[cfg(test)]
impl Tree for MemoryTree {
    fn list(&self, folder: &str) -> Result<Vec<Item>, String> {
        let prefix = if folder.is_empty() { String::new() } else { format!("{folder}/") };
        let mut items: BTreeMap<String, Item> = BTreeMap::new();
        let dir = |name: &str| Item { name: name.to_string(), is_dir: true, size: 0, modified: None };
        for path in self.folders.lock().unwrap().iter() {
            if let Some(rest) = path.strip_prefix(&prefix) {
                let name = rest.split('/').next().unwrap_or(rest);
                items.entry(name.to_string()).or_insert_with(|| dir(name));
            }
        }
        for (path, (bytes, time)) in self.files.lock().unwrap().iter() {
            let Some(rest) = path.strip_prefix(&prefix) else { continue };
            match rest.split_once('/') {
                Some((name, _)) => {
                    items.entry(name.to_string()).or_insert_with(|| dir(name));
                }
                None => {
                    items.insert(rest.to_string(), Item { name: rest.to_string(), is_dir: false, size: bytes.len() as u64, modified: *time });
                }
            }
        }
        Ok(items.into_values().collect())
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        self.files.lock().unwrap().get(path).map(|(bytes, _)| bytes.clone()).ok_or_else(|| crate::i18n::tf!("{path} isn't there."))
    }

    fn can_write(&self) -> bool {
        self.writable
    }

    fn stat(&self, path: &str) -> Result<Option<Meta>, String> {
        if let Some((bytes, time)) = self.files.lock().unwrap().get(path) {
            return Ok(Some(Meta { is_dir: false, size: bytes.len() as u64, modified: *time }));
        }
        Ok(self.has_folder(path).then_some(Meta { is_dir: true, size: 0, modified: None }))
    }

    fn write(&self, path: &str, bytes: &[u8], modified: Option<u64>) -> Result<(), String> {
        self.writing(path)?;
        self.files.lock().unwrap().insert(path.to_string(), (bytes.to_vec(), modified));
        Ok(())
    }

    fn make_folder(&self, path: &str) -> Result<(), String> {
        self.writing(path)?;
        self.folders.lock().unwrap().insert(path.to_string());
        Ok(())
    }

    fn remove_file(&self, path: &str) -> Result<(), String> {
        self.writing(path)?;
        self.files.lock().unwrap().remove(path).map(|_| ()).ok_or_else(|| crate::i18n::tf!("{path} isn't there."))
    }

    fn remove_folder(&self, path: &str) -> Result<(), String> {
        self.writing(path)?;
        let inside = format!("{path}/");
        if self.files.lock().unwrap().keys().any(|file| file.starts_with(&inside)) || self.folders.lock().unwrap().iter().any(|folder| folder.starts_with(&inside)) {
            return Err(crate::i18n::tr("It still has files in it that weren't compared, so it was kept.").into());
        }
        self.folders.lock().unwrap().remove(path);
        Ok(())
    }
}

// ---------- Filters ----------

/// Which files take part in a folder comparison.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filters {
    /// Patterns a file has to match to be compared, separated by ; or commas. Empty takes every file.
    pub include: String,
    /// Patterns for files and folders to leave out.
    pub exclude: String,
    /// Leave out what each side's .gitignore files leave out, and the .git folder.
    pub gitignore: bool,
    /// How files are compared by name, and which are always left out.
    pub rules: crate::compare_rules::Rules,
}

/// The patterns in a list typed as "*.rs; *.toml, docs/**".
pub fn patterns(list: &str) -> Vec<String> {
    list.split([';', ',']).map(str::trim).filter(|pattern| !pattern.is_empty()).map(str::to_string).collect()
}

/// Whether a name or path matches a pattern: * is any run of characters but /, ** any run at all, ? one
/// character, and [abc] or [!a-z] one of a set.
pub fn glob_match(pattern: &str, text: &str, ignore_case: bool) -> bool {
    let fold = |s: &str| if ignore_case { s.to_lowercase() } else { s.to_string() };
    let pattern: Vec<char> = fold(pattern).chars().collect();
    let text: Vec<char> = fold(text).chars().collect();
    glob_at(&pattern, &text)
}

fn glob_at(p: &[char], t: &[char]) -> bool {
    match p.first() {
        None => t.is_empty(),
        Some('*') if p.get(1) == Some(&'*') => {
            // **/ also matches nothing at all, so a/**/b matches a/b.
            let rest = if p.get(2) == Some(&'/') { &p[3..] } else { &p[2..] };
            (0..=t.len()).any(|skip| glob_at(rest, &t[skip..])) || (p.get(2) == Some(&'/') && glob_at(&p[2..], t))
        }
        Some('*') => {
            let rest = &p[1..];
            for skip in 0..=t.len() {
                if glob_at(rest, &t[skip..]) {
                    return true;
                }
                if t.get(skip) == Some(&'/') {
                    break;
                }
            }
            false
        }
        Some('?') => t.first().is_some_and(|c| *c != '/') && glob_at(&p[1..], &t[1..]),
        Some('[') => {
            let Some(close) = p.iter().skip(2).position(|c| *c == ']').map(|at| at + 2) else {
                return t.first() == Some(&'[') && glob_at(&p[1..], &t[1..]);
            };
            let Some(c) = t.first() else { return false };
            let set = &p[1..close];
            let (negated, set) = match set.first() {
                Some('!' | '^') => (true, &set[1..]),
                _ => (false, set),
            };
            let mut found = false;
            let mut i = 0;
            while i < set.len() {
                if i + 2 < set.len() && set[i + 1] == '-' {
                    found |= (set[i]..=set[i + 2]).contains(c);
                    i += 3;
                } else {
                    found |= set[i] == *c;
                    i += 1;
                }
            }
            found != negated && glob_at(&p[close + 1..], &t[1..])
        }
        Some(c) => t.first() == Some(c) && glob_at(&p[1..], &t[1..]),
    }
}

/// Whether one of the patterns matches a file or folder: a pattern with a / in it is matched against its path
/// from the top, any other against its name.
pub fn matches_any(patterns: &[String], path: &str, name: &str) -> bool {
    patterns.iter().any(|pattern| if pattern.contains('/') { glob_match(pattern.trim_start_matches('/'), path, true) } else { glob_match(pattern, name, true) })
}

/// One line of a .gitignore file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IgnoreRule {
    /// The folder the .gitignore is in, from the top, which the pattern is relative to.
    base: String,
    pattern: String,
    negate: bool,
    dir_only: bool,
    /// Matched against the path from `base` rather than just the name.
    anchored: bool,
}

/// The rules in a .gitignore file in `base`.
pub fn parse_gitignore(base: &str, text: &str) -> Vec<IgnoreRule> {
    let mut rules = Vec::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        let line = if line.ends_with("\\ ") { line } else { line.trim_end() };
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (negate, line) = match line.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, line.strip_prefix('\\').unwrap_or(line)),
        };
        let (dir_only, line) = match line.strip_suffix('/') {
            Some(rest) => (true, rest),
            None => (false, line),
        };
        if line.is_empty() {
            continue;
        }
        let anchored = line.contains('/');
        rules.push(IgnoreRule { base: base.to_string(), pattern: line.trim_start_matches('/').to_string(), negate, dir_only, anchored });
    }
    rules
}

/// Whether the rules leave a file or folder out: the last rule that matches decides.
pub fn ignored(rules: &[IgnoreRule], path: &str, is_dir: bool) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let mut out = false;
    for rule in rules {
        if rule.dir_only && !is_dir {
            continue;
        }
        let below = if rule.base.is_empty() { Some(path) } else { path.strip_prefix(&rule.base).and_then(|rest| rest.strip_prefix('/')) };
        let Some(below) = below else { continue };
        let hit = if rule.anchored { glob_match(&rule.pattern, below, false) } else { glob_match(&rule.pattern, name, false) };
        if hit {
            out = !rule.negate;
        }
    }
    out
}

// ---------- Working in the background ----------

/// How far some work has got, for a progress bar.
#[derive(Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    phase: Mutex<String>,
}

impl Progress {
    pub fn set_phase(&self, words: impl Into<String>) {
        *self.phase.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = words.into();
    }

    pub fn phase(&self) -> String {
        self.phase.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// How much is done, from 0 to 1, or None while there's no telling.
    pub fn share(&self) -> Option<f32> {
        let total = self.total.load(Ordering::Relaxed);
        (total > 0).then(|| (self.done.load(Ordering::Relaxed) as f32 / total as f32).min(1.0))
    }
}

/// Work done on another thread, with its progress, a way to stop it, and its result once it's finished.
pub struct Job<T> {
    pub progress: Arc<Progress>,
    cancel: Arc<AtomicBool>,
    result: Arc<Mutex<Option<T>>>,
}

impl<T: Send + 'static> Job<T> {
    /// Starts the work. `finished` is called on the worker's thread when it's done, to wake the window.
    pub fn start(work: impl FnOnce(&Progress, &AtomicBool) -> T + Send + 'static, finished: impl FnOnce() + Send + 'static) -> Self {
        let (progress, cancel, result) = (Arc::new(Progress::default()), Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(None)));
        let (worker_progress, worker_cancel, worker_result) = (progress.clone(), cancel.clone(), result.clone());
        std::thread::spawn(move || {
            let outcome = work(&worker_progress, &worker_cancel);
            *worker_result.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(outcome);
            finished();
        });
        Self { progress, cancel, result }
    }

    /// The result, once.
    pub fn take(&self) -> Option<T> {
        self.result.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Waits for it to finish, for a test.
    #[cfg(test)]
    pub fn wait(&self) -> T {
        loop {
            if let Some(result) = self.take() {
                return result;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

// ---------- Comparing folders ----------

/// How files on both sides are compared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// By size and modified time. Where a side keeps no times, files of the same size are compared by contents.
    #[default]
    Quick,
    /// By contents, every time.
    Thorough,
}

/// How an entry on one side compares with the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Same,
    /// Different, with no telling which is newer.
    Different,
    /// Different, and the left one was changed more recently.
    NewerLeft,
    NewerRight,
    LeftOnly,
    RightOnly,
    /// A file on one side and a folder on the other.
    TypeDiffers,
    /// One side or the other couldn't be read.
    Unreadable,
}

impl Status {
    /// What it's called, in words, so the colour is never the only way to tell.
    pub fn words(self) -> &'static str {
        match self {
            Status::Same => "Same",
            Status::Different => "Different",
            Status::NewerLeft => "Different, left newer",
            Status::NewerRight => "Different, right newer",
            Status::LeftOnly => "Only on the left",
            Status::RightOnly => "Only on the right",
            Status::TypeDiffers => "File and folder",
            Status::Unreadable => "Couldn't be read",
        }
    }

    /// A short mark to go beside it.
    pub fn symbol(self) -> &'static str {
        match self {
            Status::Same => "=",
            Status::Different => "\u{2260}",
            Status::NewerLeft => "\u{25C0}\u{2260}",
            Status::NewerRight => "\u{2260}\u{25B6}",
            Status::LeftOnly => "\u{25C0}",
            Status::RightOnly => "\u{25B6}",
            Status::TypeDiffers => "!",
            Status::Unreadable => "?",
        }
    }
}

/// An entry as it is on one side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Meta {
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<u64>,
}

/// One line of the comparison: an entry found on either side or both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// Its path from the top, with forward slashes.
    pub path: String,
    pub name: String,
    pub depth: usize,
    pub left: Option<Meta>,
    pub right: Option<Meta>,
    pub status: Status,
    pub problem: Option<String>,
    /// The folder it's in, by row.
    pub parent: Option<usize>,
    /// The row after its last one inside it: a file's is the row after it.
    pub end: usize,
}

impl Row {
    /// A folder on at least one side, so it has rows inside it.
    pub fn is_dir(&self) -> bool {
        self.left.is_some_and(|m| m.is_dir) || self.right.is_some_and(|m| m.is_dir)
    }

    /// A file on both sides, which can be compared as files.
    pub fn is_file_pair(&self) -> bool {
        self.left.is_some_and(|m| !m.is_dir) && self.right.is_some_and(|m| !m.is_dir)
    }

    pub fn side(&self, left: bool) -> Option<Meta> {
        if left { self.left } else { self.right }
    }
}

/// How many files came out each way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub same: usize,
    pub different: usize,
    pub left_only: usize,
    pub right_only: usize,
    pub type_differs: usize,
    pub unreadable: usize,
}

impl Counts {
    pub fn describe(&self) -> String {
        let mut parts = vec![format!("{} same", self.same), format!("{} different", self.different), format!("{} only on the left", self.left_only), format!("{} only on the right", self.right_only)];
        if self.type_differs > 0 {
            parts.push(format!("{} file and folder", self.type_differs));
        }
        if self.unreadable > 0 {
            parts.push(format!("{} couldn't be read", self.unreadable));
        }
        parts.join(" \u{B7} ")
    }
}

/// Two trees compared.
#[derive(Clone)]
pub struct FolderResult {
    pub left: Side,
    pub right: Side,
    pub mode: Mode,
    pub filters: Filters,
    pub rows: Vec<Row>,
    pub left_tree: Arc<dyn Tree>,
    pub right_tree: Arc<dyn Tree>,
}

impl FolderResult {
    /// The files, counted by how they compare. Folders aren't counted.
    pub fn counts(&self) -> Counts {
        count_rows(self.rows.iter())
    }

    pub fn tree(&self, left: bool) -> &Arc<dyn Tree> {
        if left { &self.left_tree } else { &self.right_tree }
    }
}

/// Lists a whole tree with the filters, as a path for every file and folder kept.
pub fn walk(tree: &dyn Tree, filters: &Filters, progress: &Progress, cancel: &AtomicBool) -> Result<BTreeMap<String, Item>, String> {
    let include = patterns(&filters.include);
    let exclude = patterns(&filters.exclude);
    let mut found = BTreeMap::new();
    let mut folders: Vec<(String, Vec<IgnoreRule>)> = vec![(String::new(), Vec::new())];
    while let Some((folder, inherited)) = folders.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Err(crate::i18n::tr("It was stopped.").into());
        }
        let items = tree.list(&folder)?;
        let mut rules = inherited;
        if filters.gitignore
            && items.iter().any(|item| !item.is_dir && item.name == ".gitignore")
            && let Ok(bytes) = tree.read(&join(&folder, ".gitignore"))
        {
            rules.extend(parse_gitignore(&folder, &String::from_utf8_lossy(&bytes)));
        }
        for item in items {
            let path = join(&folder, &item.name);
            if matches_any(&exclude, &path, &item.name) || filters.rules.skips(&path, &item.name, item.is_dir) || (filters.gitignore && ((item.is_dir && item.name == ".git") || ignored(&rules, &path, item.is_dir))) {
                continue;
            }
            if !item.is_dir && !include.is_empty() && !matches_any(&include, &path, &item.name) {
                continue;
            }
            progress.done.fetch_add(1, Ordering::Relaxed);
            if item.is_dir {
                folders.push((path.clone(), rules.clone()));
            }
            found.insert(path, item);
        }
    }
    // With only some files taken, a folder with none of them in it is left out too.
    if !include.is_empty() {
        let mut wanted = std::collections::HashSet::new();
        for (path, item) in &found {
            if !item.is_dir {
                let mut at = path.as_str();
                while let Some((parent, _)) = at.rsplit_once('/') {
                    wanted.insert(parent.to_string());
                    at = parent;
                }
            }
        }
        found.retain(|path, item| !item.is_dir || wanted.contains(path));
    }
    Ok(found)
}

/// Puts the two listings side by side as rows, in the order they're shown: in each folder its folders first,
/// then its files, each by name. Files on both sides are left Same for `compare_files` to look at.
pub fn build_rows(left: &BTreeMap<String, Item>, right: &BTreeMap<String, Item>) -> Vec<Row> {
    let meta = |item: &Item| Meta { is_dir: item.is_dir, size: item.size, modified: item.modified };
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    for path in left.keys().chain(right.keys().filter(|path| !left.contains_key(*path))) {
        let parent = path.rsplit_once('/').map_or("", |(parent, _)| parent);
        children.entry(parent.to_string()).or_default().push(path.clone());
    }
    let is_dir = |path: &str| left.get(path).is_some_and(|item| item.is_dir) || right.get(path).is_some_and(|item| item.is_dir);
    for list in children.values_mut() {
        list.sort_by(|a, b| {
            let (an, bn) = (a.rsplit('/').next().unwrap_or(a), b.rsplit('/').next().unwrap_or(b));
            is_dir(b).cmp(&is_dir(a)).then_with(|| an.to_lowercase().cmp(&bn.to_lowercase())).then_with(|| an.cmp(bn))
        });
    }
    let mut rows = Vec::new();
    fn add(rows: &mut Vec<Row>, children: &HashMap<String, Vec<String>>, folder: &str, parent: Option<usize>, depth: usize, left: &BTreeMap<String, Item>, right: &BTreeMap<String, Item>, meta: &dyn Fn(&Item) -> Meta) {
        let Some(list) = children.get(folder) else { return };
        for path in list {
            let (l, r) = (left.get(path).map(meta), right.get(path).map(meta));
            let status = match (l, r) {
                (Some(_), None) => Status::LeftOnly,
                (None, Some(_)) => Status::RightOnly,
                (Some(a), Some(b)) if a.is_dir != b.is_dir => Status::TypeDiffers,
                _ => Status::Same,
            };
            let at = rows.len();
            rows.push(Row { path: path.clone(), name: path.rsplit('/').next().unwrap_or(path).to_string(), depth, left: l, right: r, status, problem: None, parent, end: at + 1 });
            add(rows, children, path, Some(at), depth + 1, left, right, meta);
            rows[at].end = rows.len();
        }
    }
    add(&mut rows, &children, "", None, 0, left, right, &meta);
    rows
}

/// Two files that differ: which is newer, if their times say.
fn by_time(left: Meta, right: Meta) -> Status {
    match (left.modified, right.modified) {
        (Some(a), Some(b)) if a > b + SAME_TIME => Status::NewerLeft,
        (Some(a), Some(b)) if b > a + SAME_TIME => Status::NewerRight,
        _ => Status::Different,
    }
}

/// Whether a file's size and time are as they were, within the two seconds FAT keeps times to.
pub fn same_size_and_time(a: Meta, b: Meta) -> bool {
    a.size == b.size
        && match (a.modified, b.modified) {
            (Some(x), Some(y)) => x.abs_diff(y) <= SAME_TIME,
            _ => true,
        }
}

/// How two files that are on both sides compare, without reading them if the mode allows.
fn quick_status(left: Meta, right: Meta, mode: Mode) -> Option<Status> {
    let by_time = || by_time(left, right);
    if left.size != right.size {
        return Some(by_time());
    }
    if mode == Mode::Thorough {
        return None;
    }
    match (left.modified, right.modified) {
        (Some(a), Some(b)) if a.abs_diff(b) <= SAME_TIME => Some(Status::Same),
        (Some(_), Some(_)) => Some(by_time()),
        // Without times on both sides, the contents have to say.
        _ => None,
    }
}

/// Works out how each pair of files compares, reading those the mode needs read, and then each folder from
/// what's in it.
pub fn compare_files(rows: &mut [Row], left: &dyn Tree, right: &dyn Tree, mode: Mode, rules: &crate::compare_rules::Rules, progress: &Progress, cancel: &AtomicBool) -> Result<(), String> {
    use crate::compare_rules::{RuleKind, same_picture, same_text};
    let pairs: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].is_file_pair()).collect();
    progress.done.store(0, Ordering::Relaxed);
    progress.total.store(pairs.len() as u64, Ordering::Relaxed);
    // Each rule's pattern for lines to leave out, made once.
    let patterns: Vec<Option<regex::Regex>> = rules.list.iter().map(|rule| rule.ignore_regex().ok().flatten()).collect();
    for i in pairs {
        if cancel.load(Ordering::Relaxed) {
            return Err(crate::i18n::tr("It was stopped.").into());
        }
        let (Some(l), Some(r)) = (rows[i].left, rows[i].right) else { continue };
        let path = rows[i].path.clone();
        let rule = rules.list.iter().position(|rule| rule.matches(&path, &rows[i].name, false));
        let kind = rule.map(|at| rules.list[at].kind);
        let loose = rule.is_some_and(|at| rules.list[at].loosens_text());
        let quick = match kind {
            Some(RuleKind::SizeAndTime) => Some(if same_size_and_time(l, r) { Status::Same } else { by_time(l, r) }),
            // Files that differ in size may still be the same picture, or the same text with things left out.
            Some(RuleKind::Picture) => quick_status(l, r, mode).filter(|status| *status == Status::Same),
            _ if loose => quick_status(l, r, mode).filter(|status| *status == Status::Same),
            _ => quick_status(l, r, mode),
        };
        rows[i].status = match quick {
            Some(status) => status,
            None => match (left.hash(&path, cancel), right.hash(&path, cancel)) {
                (Ok(a), Ok(b)) if a == b => Status::Same,
                (Ok(_), Ok(_)) => {
                    // Read, to see whether the rule makes them the same.
                    let looser = if loose || kind == Some(RuleKind::Picture) {
                        match (left.read(&path), right.read(&path)) {
                            (Ok(a), Ok(b)) => match (kind, rule) {
                                (Some(RuleKind::Picture), _) => same_picture(&rows[i].name, &a, &b),
                                (_, Some(at)) => same_text(&a, &b, &rules.list[at], patterns[at].as_ref()),
                                _ => None,
                            },
                            _ => None,
                        }
                    } else {
                        None
                    };
                    if looser == Some(true) { Status::Same } else { by_time(l, r) }
                }
                (Err(problem), _) | (_, Err(problem)) => {
                    if cancel.load(Ordering::Relaxed) {
                        return Err(crate::i18n::tr("It was stopped.").into());
                    }
                    rows[i].problem = Some(problem);
                    Status::Unreadable
                }
            },
        };
        progress.done.fetch_add(1, Ordering::Relaxed);
    }
    settle_folders(rows);
    Ok(())
}

/// A folder on both sides is the same when everything in it is.
pub fn settle_folders(rows: &mut [Row]) {
    let mut all_same = vec![true; rows.len()];
    for i in (0..rows.len()).rev() {
        if rows[i].is_dir() && rows[i].left.is_some() && rows[i].right.is_some() && rows[i].status != Status::TypeDiffers {
            rows[i].status = if all_same[i] { Status::Same } else { Status::Different };
        }
        if rows[i].status != Status::Same
            && let Some(parent) = rows[i].parent
        {
            all_same[parent] = false;
        }
    }
}

/// Compares two sides: opens them, lists them with the filters, and compares what's on both.
pub fn compare_folders(left: &Side, right: &Side, mode: Mode, filters: &Filters, known: &remote::KnownHosts, progress: &Progress, cancel: &AtomicBool) -> Result<FolderResult, String> {
    progress.set_phase(format!("Opening {}", left.describe()));
    let left_tree = open_tree(left, known)?;
    progress.set_phase(format!("Opening {}", right.describe()));
    let right_tree = open_tree(right, known)?;
    compare_trees(left.clone(), right.clone(), left_tree, right_tree, mode, filters, progress, cancel)
}

/// Compares two trees already open.
pub fn compare_trees(left: Side, right: Side, left_tree: Arc<dyn Tree>, right_tree: Arc<dyn Tree>, mode: Mode, filters: &Filters, progress: &Progress, cancel: &AtomicBool) -> Result<FolderResult, String> {
    progress.set_phase("Listing the left side");
    let left_items = walk(&*left_tree, filters, progress, cancel)?;
    progress.set_phase("Listing the right side");
    let right_items = walk(&*right_tree, filters, progress, cancel)?;
    let mut rows = build_rows(&left_items, &right_items);
    progress.set_phase(if mode == Mode::Thorough { "Comparing contents" } else { "Comparing" });
    compare_files(&mut rows, &*left_tree, &*right_tree, mode, &filters.rules, progress, cancel)?;
    Ok(FolderResult { left, right, mode, filters: filters.clone(), rows, left_tree, right_tree })
}

/// Which rows show: those inside a collapsed folder don't, and with `hide_same` neither do rows that are the same.
pub fn visible_rows(rows: &[Row], expanded: &std::collections::HashSet<String>, hide_same: bool) -> Vec<usize> {
    let mut shown = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let row = &rows[i];
        if hide_same && row.status == Status::Same {
            i = row.end;
            continue;
        }
        shown.push(i);
        i = if row.is_dir() && !expanded.contains(&row.path) { row.end } else { i + 1 };
    }
    shown
}

// ---------- Copying between sides ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    LeftToRight,
    RightToLeft,
}

impl Direction {
    pub fn goes_right(self) -> bool {
        self == Direction::LeftToRight
    }
}

/// One file to be copied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyItem {
    pub path: String,
    /// A file there already, which is kept in the backup folder first.
    pub replaces: bool,
    pub size: u64,
    pub modified: Option<u64>,
    /// What was there when it was compared, so a file changed since isn't written over unseen.
    pub expected: Option<Meta>,
}

/// What a copy will do, to show before it's done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyPlan {
    pub direction: Direction,
    pub files: Vec<CopyItem>,
    /// Folders to make that aren't there, including empty ones.
    pub folders: Vec<String>,
    /// What can't be copied, and why.
    pub skipped: Vec<(String, String)>,
    /// Where the files go, in words.
    pub target: String,
}

impl CopyPlan {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty() && self.folders.is_empty()
    }

    pub fn replaced(&self) -> usize {
        self.files.iter().filter(|item| item.replaces).count()
    }
}

/// Whether files can be written into one side of a comparison, or why not.
pub fn check_writable(result: &FolderResult, left: bool) -> Result<(), String> {
    let side = if left { &result.left } else { &result.right };
    if let Some(refusal) = side.refusal() {
        return Err(refusal);
    }
    if result.tree(left).can_write() { Ok(()) } else { Err(crate::i18n::tf!("Files can't be copied into {}.", side.describe())) }
}

/// Works out what copying the selected rows one way would do. A selected folder brings everything inside it
/// that isn't already the same. Nothing is ever deleted.
pub fn plan_copy(result: &FolderResult, selected: &[usize], direction: Direction) -> Result<CopyPlan, String> {
    let from_left = direction.goes_right();
    check_writable(result, !from_left)?;
    let target = if from_left { &result.right } else { &result.left }.describe();
    let mut wanted: Vec<usize> = Vec::new();
    for &i in selected {
        let Some(row) = result.rows.get(i) else { continue };
        wanted.extend(i..row.end);
    }
    wanted.sort_unstable();
    wanted.dedup();
    let mut plan = CopyPlan { direction, files: Vec::new(), folders: Vec::new(), skipped: Vec::new(), target };
    for i in wanted {
        let row = &result.rows[i];
        let (Some(source), there) = (row.side(from_left), row.side(!from_left)) else { continue };
        if row.status == Status::Same {
            continue;
        }
        match (source.is_dir, there) {
            (true, None) => plan.folders.push(row.path.clone()),
            (true, Some(m)) if !m.is_dir => plan.skipped.push((row.path.clone(), "A file of that name is in the way of the folder.".into())),
            (true, Some(_)) => {}
            (false, Some(m)) if m.is_dir => plan.skipped.push((row.path.clone(), "A folder of that name is in the way of the file.".into())),
            (false, there) => {
                if row.status == Status::Unreadable {
                    plan.skipped.push((row.path.clone(), row.problem.clone().unwrap_or_else(|| "It couldn't be read.".into())));
                    continue;
                }
                plan.files.push(CopyItem { path: row.path.clone(), replaces: there.is_some(), size: source.size, modified: source.modified, expected: there });
            }
        }
    }
    Ok(plan)
}

/// What a copy did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CopyReport {
    pub copied: usize,
    pub folders_made: usize,
    pub backed_up: usize,
    pub failed: Vec<(String, String)>,
    /// Where the files replaced were kept, if any were.
    pub backup_folder: Option<PathBuf>,
}

impl CopyReport {
    pub fn describe(&self) -> String {
        let mut said = format!("Copied {} file{}", self.copied, if self.copied == 1 { "" } else { "s" });
        if self.folders_made > 0 {
            said.push_str(&format!(" and made {} folder{}", self.folders_made, if self.folders_made == 1 { "" } else { "s" }));
        }
        said.push('.');
        if let Some(folder) = &self.backup_folder {
            said.push_str(&format!("\n\nThe {} file{} replaced {} kept in {}.", self.backed_up, if self.backed_up == 1 { "" } else { "s" }, if self.backed_up == 1 { "is" } else { "are" }, folder.display()));
        }
        if !self.failed.is_empty() {
            said.push_str(&format!("\n\n{} couldn't be copied:", self.failed.len()));
            for (path, why) in self.failed.iter().take(12) {
                said.push_str(&format!("\n{path}: {why}"));
            }
        }
        said
    }
}

fn local_place(root: &Path, path: &str) -> PathBuf {
    path.split('/').filter(|part| !part.is_empty()).fold(root.to_path_buf(), |at, part| at.join(part))
}

/// Keeps a copy of a file on a side, under its path in `backup_root` on this computer, before it's replaced or
/// taken away. If that can't be done, the file mustn't be touched.
pub fn keep_backup(tree: &dyn Tree, path: &str, backup_root: &Path) -> Result<(), String> {
    safe_path(path)?;
    let kept = local_place(backup_root, path);
    if let Some(folder) = kept.parent() {
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    }
    match tree.local_path(path) {
        Some(here) => std::fs::copy(here, &kept).map(|_| ()).map_err(|e| e.to_string()),
        None => tree.read(path).and_then(|bytes| crate::codec::write_file(&kept, &bytes).map_err(|e| e.to_string())),
    }
}

/// Whether what's at a path is still what was there when it was compared: the same file by size and time, or
/// nothing, as expected.
pub fn still_as_compared(tree: &dyn Tree, path: &str, expected: Option<Meta>) -> Result<(), String> {
    match (tree.stat(path)?, expected) {
        (None, None) => Ok(()),
        (Some(now), Some(then)) if now.is_dir == then.is_dir && (now.is_dir || same_size_and_time(now, then)) => Ok(()),
        (Some(_), None) => Err(crate::i18n::tr("Something has been put there since the comparison, so it was left alone. Compare again first.").into()),
        (None, Some(_)) => Err(crate::i18n::tr("It has gone since the comparison. Compare again first.").into()),
        (Some(_), Some(_)) => Err(crate::i18n::tr("It has changed since the comparison, so it was left alone. Compare again first.").into()),
    }
}

/// Copies one file from one side to the other, keeping its modified time.
pub fn put_file(source: &dyn Tree, target: &dyn Tree, path: &str, modified: Option<u64>) -> Result<(), String> {
    safe_path(path)?;
    let bytes = source.read(path)?;
    target.write(path, &bytes, modified)
}

/// Does a copy into `target`. A file about to be replaced is first copied into `backup_root`, under its path;
/// if that fails, or the file has changed since it was compared, the file is left alone. A copied file keeps the
/// modified time it had.
pub fn run_copy(plan: &CopyPlan, source: &dyn Tree, target: &dyn Tree, backup_root: &Path, progress: &Progress, cancel: &AtomicBool) -> CopyReport {
    let mut report = CopyReport::default();
    progress.total.store(plan.files.len() as u64 + plan.folders.len() as u64, Ordering::Relaxed);
    for folder in &plan.folders {
        match safe_path(folder).and_then(|()| target.make_folder(folder)) {
            Ok(()) => report.folders_made += 1,
            Err(e) => report.failed.push((folder.clone(), e)),
        }
        progress.done.fetch_add(1, Ordering::Relaxed);
    }
    for item in &plan.files {
        if cancel.load(Ordering::Relaxed) {
            report.failed.push((item.path.clone(), "It was stopped before this file.".into()));
            continue;
        }
        progress.set_phase(format!("Copying {}", item.path));
        let checked = safe_path(&item.path).and_then(|()| still_as_compared(target, &item.path, item.expected));
        if let Err(problem) = checked {
            report.failed.push((item.path.clone(), problem));
            progress.done.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        if item.replaces {
            if let Err(e) = keep_backup(target, &item.path, backup_root) {
                report.failed.push((item.path.clone(), format!("It couldn't be backed up first ({e}), so it was left as it was.")));
                progress.done.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            report.backed_up += 1;
            report.backup_folder = Some(backup_root.to_path_buf());
        }
        match put_file(source, target, &item.path, item.modified) {
            Ok(()) => report.copied += 1,
            Err(e) => report.failed.push((item.path.clone(), e)),
        }
        progress.done.fetch_add(1, Ordering::Relaxed);
    }
    report
}

/// A folder for this copy's backups, named for when it is, under the folder they're all kept in. A name already
/// taken gets a number after it.
pub fn backup_folder(under: &Path) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y-%m-%d %H-%M-%S").to_string();
    let mut folder = under.join(&stamp);
    let mut attempt = 1;
    while folder.exists() {
        attempt += 1;
        folder = under.join(format!("{stamp} ({attempt})"));
    }
    folder
}

// ---------- Comparing text ----------

/// What's left out of a comparison of text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextOptions {
    /// Spaces and tabs count for nothing: "a  b" is the same as "ab".
    pub ignore_whitespace: bool,
    pub ignore_case: bool,
    /// A line ending in CR LF is the same as one ending in LF.
    pub ignore_line_endings: bool,
}

/// What a line is compared by, with what's to be left out taken out. `ending` is the line's ending, compared too
/// unless line endings are left out.
pub fn line_key<E: std::hash::Hash>(line: &str, ending: Option<E>, options: &TextOptions) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    let line = if options.ignore_line_endings { line.trim_end_matches('\r') } else { line };
    for c in line.chars() {
        if options.ignore_whitespace && c.is_whitespace() {
            continue;
        }
        if options.ignore_case {
            for lower in c.to_lowercase() {
                lower.hash(&mut hasher);
            }
        } else {
            c.hash(&mut hasher);
        }
    }
    if !options.ignore_line_endings
        && let Some(ending) = ending
    {
        0xFFu8.hash(&mut hasher);
        ending.hash(&mut hasher);
    }
    hasher.finish()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChunkKind {
    Same,
    /// Lines on both sides, different.
    Changed,
    LeftOnly,
    RightOnly,
}

/// A stretch of lines that are all alike, or one change: lines on the left that became lines on the right.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub kind: ChunkKind,
    pub left: Range<usize>,
    pub right: Range<usize>,
}

/// Compares two lists of lines, each boiled down to its key, into stretches. A deletion next to an insertion is
/// one change.
pub fn diff_keys<T: std::hash::Hash + Eq + Ord>(left: &[T], right: &[T]) -> Vec<Chunk> {
    let deadline = Some(Instant::now() + MOST_DIFF_TIME);
    let mut out: Vec<Chunk> = Vec::new();
    for op in capture_diff_slices_deadline(Algorithm::Myers, left, right, deadline) {
        let (same, l, r) = match op {
            DiffOp::Equal { old_index, new_index, len } => (true, old_index..old_index + len, new_index..new_index + len),
            DiffOp::Delete { old_index, old_len, new_index } => (false, old_index..old_index + old_len, new_index..new_index),
            DiffOp::Insert { old_index, new_index, new_len } => (false, old_index..old_index, new_index..new_index + new_len),
            DiffOp::Replace { old_index, old_len, new_index, new_len } => (false, old_index..old_index + old_len, new_index..new_index + new_len),
        };
        if l.is_empty() && r.is_empty() {
            continue;
        }
        if !same && let Some(last) = out.last_mut().filter(|last| last.kind != ChunkKind::Same) {
            last.left.end = l.end;
            last.right.end = r.end;
            last.kind = change_kind(&last.left, &last.right);
            continue;
        }
        let kind = if same { ChunkKind::Same } else { change_kind(&l, &r) };
        out.push(Chunk { kind, left: l, right: r });
    }
    out
}

fn change_kind(left: &Range<usize>, right: &Range<usize>) -> ChunkKind {
    match (left.is_empty(), right.is_empty()) {
        (false, false) => ChunkKind::Changed,
        (false, true) => ChunkKind::LeftOnly,
        _ => ChunkKind::RightOnly,
    }
}

/// Compares two texts' lines, with no endings to compare.
pub fn diff_lines<S: AsRef<str>, T: AsRef<str>>(left: &[S], right: &[T], options: &TextOptions) -> Vec<Chunk> {
    let keys = |lines: &[&str]| lines.iter().map(|line| line_key::<u8>(line, None, options)).collect::<Vec<_>>();
    let l: Vec<&str> = left.iter().map(AsRef::as_ref).collect();
    let r: Vec<&str> = right.iter().map(AsRef::as_ref).collect();
    diff_keys(&keys(&l), &keys(&r))
}

/// The changes only, in order.
pub fn changes(chunks: &[Chunk]) -> impl Iterator<Item = (usize, &Chunk)> {
    chunks.iter().enumerate().filter(|(_, chunk)| chunk.kind != ChunkKind::Same)
}

/// Where the line on the other side is that goes with a line on this one, for keeping the two in step as they
/// scroll: the same place in a stretch that's alike, or the start of a change.
pub fn matching_line(chunks: &[Chunk], line: usize, from_left: bool) -> usize {
    for chunk in chunks {
        let (here, there) = if from_left { (&chunk.left, &chunk.right) } else { (&chunk.right, &chunk.left) };
        if line < here.end || (here.is_empty() && line == here.start) {
            if here.is_empty() {
                return there.start;
            }
            let into = line - here.start;
            return if chunk.kind == ChunkKind::Same { there.start + into } else { there.start + into.min(there.len().saturating_sub(1)) };
        }
    }
    chunks.last().map_or(line, |chunk| {
        let (here, there) = if from_left { (&chunk.left, &chunk.right) } else { (&chunk.right, &chunk.left) };
        there.end + line.saturating_sub(here.end)
    })
}

/// The parts of two lines that differ, as ranges of characters, by words or by single characters. Whitespace
/// and case are left out as the options say.
pub fn inline_differences(left: &str, right: &str, by_chars: bool, options: &TextOptions) -> (Vec<(usize, usize)>, Vec<(usize, usize)>) {
    let tokens = |text: &str| -> Vec<(usize, usize, String)> {
        let chars: Vec<char> = text.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            let start = i;
            let class = |c: char| if c.is_alphanumeric() || c == '_' { 0 } else if c.is_whitespace() { 1 } else { 2 };
            if by_chars || class(chars[i]) == 2 {
                i += 1;
            } else {
                let kind = class(chars[i]);
                while i < chars.len() && class(chars[i]) == kind {
                    i += 1;
                }
            }
            let token: String = chars[start..i].iter().collect();
            if options.ignore_whitespace && token.chars().all(char::is_whitespace) {
                continue;
            }
            out.push((start, i, if options.ignore_case { token.to_lowercase() } else { token }));
        }
        out
    };
    let (a, b) = (tokens(left), tokens(right));
    let ka: Vec<&str> = a.iter().map(|t| t.2.as_str()).collect();
    let kb: Vec<&str> = b.iter().map(|t| t.2.as_str()).collect();
    let mut left_spans: Vec<(usize, usize)> = Vec::new();
    let mut right_spans: Vec<(usize, usize)> = Vec::new();
    let push = |spans: &mut Vec<(usize, usize)>, tokens: &[(usize, usize, String)], range: Range<usize>| {
        if range.is_empty() {
            return;
        }
        let (start, end) = (tokens[range.start].0, tokens[range.end - 1].1);
        match spans.last_mut() {
            Some(last) if last.1 >= start => last.1 = end.max(last.1),
            _ => spans.push((start, end)),
        }
    };
    for chunk in diff_keys(&ka, &kb) {
        if chunk.kind != ChunkKind::Same {
            push(&mut left_spans, &a, chunk.left);
            push(&mut right_spans, &b, chunk.right);
        }
    }
    (left_spans, right_spans)
}

/// How a line is marked in a text view of a comparison.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineMark {
    pub kind: ChunkKind,
    /// The parts that differ, as ranges of characters.
    pub spans: Vec<(usize, usize)>,
}

/// The marks for one side's text view.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Marks {
    /// By line; None for a line that's the same.
    pub lines: Vec<Option<LineMark>>,
    /// Lines before which the other side has lines this side hasn't: a line drawn across there. The line count
    /// itself means at the end.
    pub gaps: Vec<usize>,
    /// The lines of the difference gone to last.
    pub current: Option<Range<usize>>,
}

impl Marks {
    pub fn line(&self, line: usize) -> Option<&LineMark> {
        self.lines.get(line).and_then(Option::as_ref)
    }

    pub fn gap_before(&self, line: usize) -> bool {
        self.gaps.binary_search(&line).is_ok()
    }
}

/// Changes longer than this aren't compared within their lines, as it would take too long and say little.
const MOST_INLINE_LINES: usize = 400;
const MOST_INLINE_CHARS: usize = 4000;

/// The marks for both sides' views, with the parts of changed lines that differ.
pub fn marks_for<S: AsRef<str>, T: AsRef<str>>(chunks: &[Chunk], left: &[S], right: &[T], by_chars: bool, options: &TextOptions) -> (Marks, Marks) {
    let mut l = Marks { lines: vec![None; left.len()], ..Marks::default() };
    let mut r = Marks { lines: vec![None; right.len()], ..Marks::default() };
    for chunk in chunks {
        match chunk.kind {
            ChunkKind::Same => {}
            ChunkKind::LeftOnly => {
                for line in chunk.left.clone() {
                    l.lines[line] = Some(LineMark { kind: ChunkKind::LeftOnly, spans: Vec::new() });
                }
                r.gaps.push(chunk.right.start);
            }
            ChunkKind::RightOnly => {
                for line in chunk.right.clone() {
                    r.lines[line] = Some(LineMark { kind: ChunkKind::RightOnly, spans: Vec::new() });
                }
                l.gaps.push(chunk.left.start);
            }
            ChunkKind::Changed => {
                let small = chunk.left.len().max(chunk.right.len()) <= MOST_INLINE_LINES;
                for (n, line) in chunk.left.clone().enumerate() {
                    let other = chunk.right.start + n;
                    let spans = match right.get(other).filter(|_| small && other < chunk.right.end) {
                        Some(theirs) if left[line].as_ref().len() <= MOST_INLINE_CHARS && theirs.as_ref().len() <= MOST_INLINE_CHARS => inline_differences(left[line].as_ref(), theirs.as_ref(), by_chars, options).0,
                        _ => Vec::new(),
                    };
                    l.lines[line] = Some(LineMark { kind: ChunkKind::Changed, spans });
                }
                for (n, line) in chunk.right.clone().enumerate() {
                    let other = chunk.left.start + n;
                    let spans = match left.get(other).filter(|_| small && other < chunk.left.end) {
                        Some(ours) if right[line].as_ref().len() <= MOST_INLINE_CHARS && ours.as_ref().len() <= MOST_INLINE_CHARS => inline_differences(ours.as_ref(), right[line].as_ref(), by_chars, options).1,
                        _ => Vec::new(),
                    };
                    r.lines[line] = Some(LineMark { kind: ChunkKind::Changed, spans });
                }
            }
        }
    }
    (l, r)
}

// ---------- Patches and reports ----------

/// A text's lines from its bytes, and whether it ends with a line ending; None if it looks binary.
pub fn text_lines(bytes: &[u8]) -> Option<(Vec<String>, bool)> {
    if crate::codec::looks_binary(bytes) {
        return None;
    }
    let text = String::from_utf8_lossy(bytes);
    let final_newline = text.ends_with('\n');
    let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    if final_newline || text.is_empty() {
        lines.pop();
    }
    Some((lines, final_newline))
}

/// A unified diff of two texts, as patch and git apply read it, with three lines of context. Empty if they're
/// the same.
pub fn unified_diff<S: AsRef<str>, T: AsRef<str>>(left_name: &str, right_name: &str, left: &[S], right: &[T], left_final: bool, right_final: bool, chunks: &[Chunk]) -> String {
    const CONTEXT: usize = 3;
    if chunks.iter().all(|chunk| chunk.kind == ChunkKind::Same) && left_final == right_final {
        return String::new();
    }
    // The last line's ending counts as part of it, so a missing one makes the last line differ.
    let mut chunks = chunks.to_vec();
    if left_final != right_final && !left.is_empty() && !right.is_empty() {
        let (ll, rl) = (left.len() - 1, right.len() - 1);
        if let Some(last) = chunks.last_mut()
            && last.kind == ChunkKind::Same
            && last.left.end == left.len()
            && last.right.end == right.len()
        {
            last.left.end -= 1;
            last.right.end -= 1;
            let empty = last.left.is_empty();
            if empty {
                chunks.pop();
            }
            chunks.push(Chunk { kind: ChunkKind::Changed, left: ll..ll + 1, right: rl..rl + 1 });
        }
    }
    let mut out = format!("--- {left_name}\n+++ {right_name}\n");
    let changed: Vec<usize> = chunks.iter().enumerate().filter(|(_, c)| c.kind != ChunkKind::Same).map(|(i, _)| i).collect();
    let mut i = 0;
    while i < changed.len() {
        // Changes close enough together share a hunk.
        let mut last = i;
        while last + 1 < changed.len() && (changed[last] + 1..changed[last + 1]).all(|between| chunks[between].left.len() <= CONTEXT * 2) {
            last += 1;
        }
        let first = &chunks[changed[i]];
        let end = &chunks[changed[last]];
        let left_start = first.left.start.saturating_sub(CONTEXT);
        let right_start = first.right.start.saturating_sub(CONTEXT);
        let left_end = (end.left.end + CONTEXT).min(left.len());
        let right_end = (end.right.end + CONTEXT).min(right.len());
        let count = |start: usize, end: usize| if end - start == 1 { format!("{}", start + 1) } else if end == start { format!("{start},0") } else { format!("{},{}", start + 1, end - start) };
        out.push_str(&format!("@@ -{} +{} @@\n", count(left_start, left_end), count(right_start, right_end)));
        let line_out = |out: &mut String, mark: char, text: &str, missing_newline: bool| {
            out.push(mark);
            out.push_str(text);
            out.push('\n');
            if missing_newline {
                out.push_str("\\ No newline at end of file\n");
            }
        };
        let (mut l, mut r) = (left_start, right_start);
        for chunk in &chunks[..=changed[last]] {
            if chunk.left.end <= left_start && chunk.right.end <= right_start {
                continue;
            }
            if chunk.kind == ChunkKind::Same {
                while l < chunk.left.end && l < left_end {
                    line_out(&mut out, ' ', left[l].as_ref(), l + 1 == left.len() && !left_final);
                    l += 1;
                    r += 1;
                }
            } else {
                for at in chunk.left.clone() {
                    line_out(&mut out, '-', left[at].as_ref(), at + 1 == left.len() && !left_final);
                }
                for at in chunk.right.clone() {
                    line_out(&mut out, '+', right[at].as_ref(), at + 1 == right.len() && !right_final);
                }
                l = chunk.left.end;
                r = chunk.right.end;
            }
        }
        while l < left_end && r < right_end {
            line_out(&mut out, ' ', left[l].as_ref(), l + 1 == left.len() && !left_final);
            l += 1;
            r += 1;
        }
        i = last + 1;
    }
    out
}

/// A patch of everything that differs between two folders: each text file's changes, files only on one side
/// as added or taken away, and a line for each binary file that differs.
pub fn folder_patch(result: &FolderResult, cancel: &AtomicBool) -> Result<String, String> {
    let mut out = String::new();
    for row in &result.rows {
        if cancel.load(Ordering::Relaxed) {
            return Err(crate::i18n::tr("It was stopped.").into());
        }
        if row.is_dir() || row.status == Status::Same || row.status == Status::Unreadable {
            continue;
        }
        let read = |left: bool| -> Result<Option<Vec<u8>>, String> { if row.side(left).is_some() { result.tree(left).read(&row.path).map(Some) } else { Ok(None) } };
        let (a, b) = (read(true)?, read(false)?);
        let left_name = if a.is_some() { format!("a/{}", row.path) } else { "/dev/null".to_string() };
        let right_name = if b.is_some() { format!("b/{}", row.path) } else { "/dev/null".to_string() };
        let empty = Vec::new();
        match (text_lines(a.as_ref().unwrap_or(&empty)), text_lines(b.as_ref().unwrap_or(&empty))) {
            (Some((l, lf)), Some((r, rf))) => {
                let chunks = diff_lines(&l, &r, &TextOptions::default());
                let (lf, rf) = (lf || a.is_none(), rf || b.is_none());
                out.push_str(&unified_diff(&left_name, &right_name, &l, &r, lf, rf, &chunks));
            }
            _ => out.push_str(&format!("Binary files {left_name} and {right_name} differ\n")),
        }
    }
    Ok(out)
}

fn time_words(time: Option<u64>) -> String {
    time.and_then(|secs| chrono::DateTime::from_timestamp(secs as i64, 0)).map_or_else(String::new, |when| when.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string())
}

fn size_words(meta: Option<Meta>) -> String {
    match meta {
        Some(m) if m.is_dir => "folder".to_string(),
        Some(m) => m.size.to_string(),
        None => String::new(),
    }
}

/// The rows a report lists: every one, or only those that differ.
fn report_rows(result: &FolderResult, hide_same: bool) -> impl Iterator<Item = &Row> {
    result.rows.iter().filter(move |row| !(hide_same && row.status == Status::Same))
}

/// A plain text report of a folder comparison, a line for each entry.
pub fn report_text(result: &FolderResult, hide_same: bool) -> String {
    let mut out = String::from("Folder comparison\n");
    out.push_str(&format!("Left:  {}\nRight: {}\n", result.left.describe(), result.right.describe()));
    out.push_str(&format!("Compared by {}\n", if result.mode == Mode::Thorough { "contents" } else { "size and modified time" }));
    if !result.filters.include.trim().is_empty() {
        out.push_str(&format!("Only: {}\n", result.filters.include.trim()));
    }
    if !result.filters.exclude.trim().is_empty() {
        out.push_str(&format!("Leaving out: {}\n", result.filters.exclude.trim()));
    }
    if result.filters.gitignore {
        out.push_str("Leaving out what .gitignore files do\n");
    }
    out.push_str(&format!("{}\n\n", result.counts().describe()));
    let width = report_rows(result, hide_same).map(|row| row.path.chars().count() + if row.is_dir() { 1 } else { 0 }).max().unwrap_or(4).max(4);
    out.push_str(&format!("{:<24}  {:<width$}  {:>12}  {:<19}  {:>12}  {:<19}\n", "Status", "Path", "Left size", "Left modified", "Right size", "Right modified"));
    for row in report_rows(result, hide_same) {
        let path = if row.is_dir() { format!("{}/", row.path) } else { row.path.clone() };
        out.push_str(&format!(
            "{:<24}  {:<width$}  {:>12}  {:<19}  {:>12}  {:<19}\n",
            row.status.words(),
            path,
            size_words(row.left),
            time_words(row.left.and_then(|m| m.modified)),
            size_words(row.right),
            time_words(row.right.and_then(|m| m.modified))
        ));
    }
    out
}

pub fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// An HTML report of a folder comparison: a table, with each entry's status in words as well as colour.
pub fn report_html(result: &FolderResult, hide_same: bool) -> String {
    let class = |status: Status| match status {
        Status::Same => "same",
        Status::Different | Status::NewerLeft | Status::NewerRight => "different",
        Status::LeftOnly => "left",
        Status::RightOnly => "right",
        Status::TypeDiffers | Status::Unreadable => "problem",
    };
    let mut out = String::from(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>Folder comparison</title>\n<style>\nbody{font-family:system-ui,sans-serif;margin:24px;color:#222}\ntable{border-collapse:collapse;font-size:14px}\nth,td{padding:3px 10px;text-align:left;border-bottom:1px solid #ddd}\ntd.n{text-align:right;font-variant-numeric:tabular-nums}\ntr.different td.s{background:#fff1b8}\ntr.left td.s{background:#fbd5d5}\ntr.right td.s{background:#cdeedd}\ntr.problem td.s{background:#e8d5f5}\n</style>\n</head>\n<body>\n<h1>Folder comparison</h1>\n",
    );
    out.push_str(&format!("<p>Left: {}<br>Right: {}<br>Compared by {}</p>\n", html_escape(&result.left.describe()), html_escape(&result.right.describe()), if result.mode == Mode::Thorough { "contents" } else { "size and modified time" }));
    out.push_str(&format!("<p>{}</p>\n", html_escape(&result.counts().describe())));
    out.push_str("<table>\n<thead><tr><th>Status</th><th>Path</th><th>Left size</th><th>Left modified</th><th>Right size</th><th>Right modified</th></tr></thead>\n<tbody>\n");
    for row in report_rows(result, hide_same) {
        let path = if row.is_dir() { format!("{}/", row.path) } else { row.path.clone() };
        out.push_str(&format!(
            "<tr class=\"{}\"><td class=\"s\">{}</td><td>{}</td><td class=\"n\">{}</td><td>{}</td><td class=\"n\">{}</td><td>{}</td></tr>\n",
            class(row.status),
            html_escape(row.status.words()),
            html_escape(&path),
            size_words(row.left),
            time_words(row.left.and_then(|m| m.modified)),
            size_words(row.right),
            time_words(row.right.and_then(|m| m.modified))
        ));
    }
    out.push_str("</tbody>\n</table>\n</body>\n</html>\n");
    out
}

/// Files counted by how they compare, among some rows.
fn count_rows<'a>(rows: impl Iterator<Item = &'a Row>) -> Counts {
    let mut counts = Counts::default();
    for row in rows {
        if row.is_dir() && row.status != Status::TypeDiffers {
            continue;
        }
        match row.status {
            Status::Same => counts.same += 1,
            Status::Different | Status::NewerLeft | Status::NewerRight => counts.different += 1,
            Status::LeftOnly => counts.left_only += 1,
            Status::RightOnly => counts.right_only += 1,
            Status::TypeDiffers => counts.type_differs += 1,
            Status::Unreadable => counts.unreadable += 1,
        }
    }
    counts
}

/// A report of the counts only: for the whole comparison, then for each folder at the top. Plain text, or an
/// HTML page.
pub fn report_summary(result: &FolderResult, html: bool) -> String {
    let by = if result.mode == Mode::Thorough { "contents" } else { "size and modified time" };
    let mut folders: Vec<(String, Counts)> = Vec::new();
    let top_files = count_rows(result.rows.iter().filter(|row| row.parent.is_none() && !row.is_dir()));
    if top_files != Counts::default() {
        folders.push(("(files at the top)".to_string(), top_files));
    }
    for (i, row) in result.rows.iter().enumerate() {
        if row.parent.is_none() && row.is_dir() {
            folders.push((format!("{}/", row.path), count_rows(result.rows[i..row.end].iter())));
        }
    }
    if html {
        let mut out = String::from("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>Folder comparison summary</title>\n<style>\nbody{font-family:system-ui,sans-serif;margin:24px;color:#222}\ntable{border-collapse:collapse;font-size:14px}\nth,td{padding:3px 10px;text-align:left;border-bottom:1px solid #ddd}\ntd.n{text-align:right;font-variant-numeric:tabular-nums}\n</style>\n</head>\n<body>\n<h1>Folder comparison summary</h1>\n");
        out.push_str(&format!("<p>Left: {}<br>Right: {}<br>Compared by {by}</p>\n<p>{}</p>\n", html_escape(&result.left.describe()), html_escape(&result.right.describe()), html_escape(&result.counts().describe())));
        out.push_str("<table>\n<thead><tr><th>Folder</th><th>Same</th><th>Different</th><th>Only on the left</th><th>Only on the right</th><th>Other</th></tr></thead>\n<tbody>\n");
        for (name, c) in &folders {
            out.push_str(&format!("<tr><td>{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td><td class=\"n\">{}</td></tr>\n", html_escape(name), c.same, c.different, c.left_only, c.right_only, c.type_differs + c.unreadable));
        }
        out.push_str("</tbody>\n</table>\n</body>\n</html>\n");
        return out;
    }
    let mut out = String::from("Folder comparison summary\n");
    out.push_str(&format!("Left:  {}\nRight: {}\nCompared by {by}\n{}\n\n", result.left.describe(), result.right.describe(), result.counts().describe()));
    let width = folders.iter().map(|(name, _)| name.chars().count()).max().unwrap_or(6).max(6);
    out.push_str(&format!("{:<width$}  {:>8}  {:>9}  {:>16}  {:>17}  {:>6}\n", "Folder", "Same", "Different", "Only on the left", "Only on the right", "Other"));
    for (name, c) in &folders {
        out.push_str(&format!("{:<width$}  {:>8}  {:>9}  {:>16}  {:>17}  {:>6}\n", name, c.same, c.different, c.left_only, c.right_only, c.type_differs + c.unreadable));
    }
    out
}

/// A field for a CSV file: quoted when it has a comma, quote or line break in it, with quotes doubled.
pub fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) { format!("\"{}\"", text.replace('"', "\"\"")) } else { text.to_string() }
}

/// The comparison as a CSV file for a spreadsheet: a row for each entry, with its status, path, whether it's a
/// folder, and each side's size and modified time.
pub fn report_csv(result: &FolderResult, hide_same: bool) -> String {
    let mut out = String::from("Status,Path,Kind,Left size,Left modified,Right size,Right modified\r\n");
    let size = |meta: Option<Meta>| meta.filter(|m| !m.is_dir).map_or_else(String::new, |m| m.size.to_string());
    for row in report_rows(result, hide_same) {
        let fields = [row.status.words().to_string(), row.path.clone(), if row.is_dir() { "Folder".into() } else { "File".into() }, size(row.left), time_words(row.left.and_then(|m| m.modified)), size(row.right), time_words(row.right.and_then(|m| m.modified))];
        out.push_str(&fields.iter().map(|field| csv_field(field)).collect::<Vec<_>>().join(","));
        out.push_str("\r\n");
    }
    out
}

// ---------- Bytes and pictures ----------

/// The stretches of offsets where two lots of bytes differ, compared place by place. What one has past the
/// other's end is a difference too.
pub fn byte_differences(a: &[u8], b: &[u8]) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    let common = a.len().min(b.len());
    let mut i = 0;
    while i < common {
        if a[i] != b[i] {
            let start = i;
            while i < common && a[i] != b[i] {
                i += 1;
            }
            out.push(start..i);
        } else {
            i += 1;
        }
    }
    if a.len() != b.len() {
        match out.last_mut() {
            Some(last) if last.end == common => last.end = a.len().max(b.len()),
            _ => out.push(common..a.len().max(b.len())),
        }
    }
    out
}

/// A picture of where two pictures differ: the left one faded to grey, with each pixel that differs in a strong
/// colour. Pictures of different sizes are compared where they overlap, and the rest counts as different.
/// Returns its width, height, pixels (four bytes each) and how many pixels differ.
pub fn pixel_difference(aw: usize, ah: usize, a: &[u8], bw: usize, bh: usize, b: &[u8]) -> (usize, usize, Vec<u8>, usize) {
    let (w, h) = (aw.max(bw), ah.max(bh));
    let mut out = vec![0u8; w * h * 4];
    let mut differing = 0;
    for y in 0..h {
        for x in 0..w {
            let pa = (x < aw && y < ah).then(|| &a[(y * aw + x) * 4..(y * aw + x) * 4 + 4]);
            let pb = (x < bw && y < bh).then(|| &b[(y * bw + x) * 4..(y * bw + x) * 4 + 4]);
            let at = (y * w + x) * 4;
            match (pa, pb) {
                (Some(p), Some(q)) if p == q => {
                    let grey = ((u32::from(p[0]) * 30 + u32::from(p[1]) * 59 + u32::from(p[2]) * 11) / 100) as u8;
                    let faded = 128 + grey / 2;
                    out[at..at + 4].copy_from_slice(&[faded, faded, faded, 255]);
                }
                _ => {
                    differing += 1;
                    out[at..at + 4].copy_from_slice(&[230, 0, 160, 255]);
                }
            }
        }
    }
    (w, h, out, differing)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(files: &[(&str, &str, Option<u64>)]) -> Arc<dyn Tree> {
        Arc::new(MemoryTree::with(files))
    }

    fn compared(left: &[(&str, &str, Option<u64>)], right: &[(&str, &str, Option<u64>)], mode: Mode, filters: &Filters) -> FolderResult {
        let side = || Side::Folder(PathBuf::from("memory"));
        compare_trees(side(), side(), tree(left), tree(right), mode, filters, &Progress::default(), &AtomicBool::new(false)).unwrap()
    }

    fn statuses(result: &FolderResult) -> Vec<(String, Status)> {
        result.rows.iter().map(|row| (row.path.clone(), row.status)).collect()
    }

    fn status_of(result: &FolderResult, path: &str) -> Status {
        result.rows.iter().find(|row| row.path == path).unwrap_or_else(|| panic!("no row for {path}")).status
    }

    #[test]
    fn each_kind_of_difference_is_told_apart() {
        let left = [("same.txt", "x", Some(100)), ("changed.txt", "aaa", Some(100)), ("newer-left.txt", "a", Some(500)), ("newer-right.txt", "a", Some(100)), ("left.txt", "l", Some(1)), ("both", "file", Some(1))];
        let right = [("same.txt", "x", Some(101)), ("changed.txt", "bb", Some(100)), ("newer-left.txt", "b", Some(100)), ("newer-right.txt", "b", Some(900)), ("right.txt", "r", Some(1)), ("both/inside.txt", "i", Some(1))];
        let result = compared(&left, &right, Mode::Quick, &Filters::default());
        assert_eq!(status_of(&result, "same.txt"), Status::Same, "a second apart is the same time");
        assert_eq!(status_of(&result, "changed.txt"), Status::Different, "sizes differ, times the same");
        assert_eq!(status_of(&result, "newer-left.txt"), Status::NewerLeft);
        assert_eq!(status_of(&result, "newer-right.txt"), Status::NewerRight);
        assert_eq!(status_of(&result, "left.txt"), Status::LeftOnly);
        assert_eq!(status_of(&result, "right.txt"), Status::RightOnly);
        assert_eq!(status_of(&result, "both"), Status::TypeDiffers);
        assert_eq!(status_of(&result, "both/inside.txt"), Status::RightOnly);
        let counts = result.counts();
        assert_eq!((counts.same, counts.different, counts.left_only, counts.right_only, counts.type_differs), (1, 3, 1, 2, 1));
        assert!(counts.describe().starts_with("1 same \u{B7} 3 different"));
    }

    #[test]
    fn quick_trusts_times_and_thorough_reads_the_contents() {
        let left = [("a.txt", "one", Some(100)), ("b.txt", "two", Some(100))];
        let right = [("a.txt", "one", Some(900)), ("b.txt", "TWO", Some(100))];
        let quick = compared(&left, &right, Mode::Quick, &Filters::default());
        assert_eq!(status_of(&quick, "a.txt"), Status::NewerRight, "same size, a later time");
        assert_eq!(status_of(&quick, "b.txt"), Status::Same, "same size and time");
        let thorough = compared(&left, &right, Mode::Thorough, &Filters::default());
        assert_eq!(status_of(&thorough, "a.txt"), Status::Same, "the same bytes");
        assert_eq!(status_of(&thorough, "b.txt"), Status::Different);
    }

    #[test]
    fn without_times_quick_compares_contents_of_files_the_same_size() {
        let result = compared(&[("a", "abc", None), ("b", "abc", None)], &[("a", "abc", Some(5)), ("b", "xyz", None)], Mode::Quick, &Filters::default());
        assert_eq!(status_of(&result, "a"), Status::Same);
        assert_eq!(status_of(&result, "b"), Status::Different);
    }

    #[test]
    fn folders_follow_what_is_in_them_and_come_before_files() {
        let left = [("z.txt", "1", None), ("src/a.rs", "a", None), ("src/b.rs", "b", None), ("docs/x.md", "x", None)];
        let right = [("z.txt", "1", None), ("src/a.rs", "a", None), ("src/b.rs", "B", None), ("docs/x.md", "x", None)];
        let result = compared(&left, &right, Mode::Thorough, &Filters::default());
        let paths: Vec<String> = result.rows.iter().map(|row| row.path.clone()).collect();
        assert_eq!(paths, ["docs", "docs/x.md", "src", "src/a.rs", "src/b.rs", "z.txt"]);
        assert_eq!(status_of(&result, "docs"), Status::Same);
        assert_eq!(status_of(&result, "src"), Status::Different);
        let src = &result.rows[2];
        assert_eq!((src.depth, src.end), (0, 5));
        assert_eq!(result.rows[3].parent, Some(2));
    }

    #[test]
    fn filters_take_and_leave_out_files_and_folders() {
        let files = [("a.rs", "1", None), ("b.txt", "1", None), ("target/out.rs", "1", None), ("src/c.rs", "1", None), ("notes/n.txt", "1", None)];
        let only_rust = Filters { include: "*.rs".into(), exclude: "target".into(), ..Filters::default() };
        let result = compared(&files, &files, Mode::Quick, &only_rust);
        let paths: Vec<String> = result.rows.iter().map(|row| row.path.clone()).collect();
        assert_eq!(paths, ["src", "src/c.rs", "a.rs"], "folders with nothing taken in them go too");
        let by_path = Filters { exclude: "src/*.rs; *.TXT".into(), ..Filters::default() };
        let paths: Vec<String> = compared(&files, &files, Mode::Quick, &by_path).rows.iter().map(|row| row.path.clone()).collect();
        assert_eq!(paths, ["notes", "src", "target", "target/out.rs", "a.rs"], "patterns ignore case");
    }

    #[test]
    fn gitignore_files_are_followed_when_asked() {
        let files = [(".gitignore", "target/\n*.log\n!keep.log\n/top.txt\n", None), ("a.log", "", None), ("keep.log", "", None), ("top.txt", "", None), ("sub/top.txt", "", None), ("target/x", "", None), (".git/HEAD", "", None), ("sub/.gitignore", "*.tmp\n", None), ("sub/x.tmp", "", None), ("y.tmp", "", None)];
        let honoured = compared(&files, &files, Mode::Quick, &Filters { gitignore: true, ..Filters::default() });
        let paths: Vec<String> = honoured.rows.iter().map(|row| row.path.clone()).collect();
        assert_eq!(paths, ["sub", "sub/.gitignore", "sub/top.txt", ".gitignore", "keep.log", "y.tmp"]);
        let all = compared(&files, &files, Mode::Quick, &Filters::default());
        assert_eq!(all.rows.len(), 13);
    }

    #[test]
    fn globs_match_as_expected() {
        assert!(glob_match("*.rs", "main.rs", false));
        assert!(!glob_match("*.rs", "src/main.rs", false), "* stops at a slash");
        assert!(glob_match("src/**/*.rs", "src/a/b/main.rs", false));
        assert!(glob_match("src/**/*.rs", "src/main.rs", false));
        assert!(glob_match("**/x", "x", false));
        assert!(glob_match("file?.[ch]", "file1.c", false));
        assert!(!glob_match("file?.[!ch]", "file1.c", false));
        assert!(glob_match("[a-c]*", "beta", false));
        assert!(glob_match("*.TXT", "a.txt", true));
        assert!(!glob_match("*.TXT", "a.txt", false));
        assert_eq!(patterns(" *.rs ;*.toml,, docs/** "), ["*.rs", "*.toml", "docs/**"]);
    }

    #[test]
    fn rows_can_be_collapsed_and_the_same_hidden() {
        let left = [("d/a", "1", None), ("d/b", "1", None), ("e/c", "1", None)];
        let right = [("d/a", "1", None), ("d/b", "2", None), ("e/c", "1", None)];
        let result = compared(&left, &right, Mode::Thorough, &Filters::default());
        let mut open = std::collections::HashSet::new();
        assert_eq!(visible_rows(&result.rows, &open, false), [0, 3]);
        open.insert("d".to_string());
        open.insert("e".to_string());
        assert_eq!(visible_rows(&result.rows, &open, false), [0, 1, 2, 3, 4]);
        assert_eq!(visible_rows(&result.rows, &open, true), [0, 2], "only the folder with a difference, and the difference");
    }

    #[test]
    fn a_copy_is_planned_before_anything_is_done_and_only_into_a_folder() {
        let dir = std::env::temp_dir().join(format!("limoniter-copy-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("right/sub")).unwrap();
        std::fs::write(dir.join("right/changed.txt"), "old").unwrap();
        std::fs::write(dir.join("right/same.txt"), "same").unwrap();
        std::fs::write(dir.join("right/only-right.txt"), "mine").unwrap();
        let left = MemoryTree::with(&[("changed.txt", "new!", None), ("same.txt", "same", None), ("new.txt", "fresh", Some(1_000_000)), ("folder/inner.txt", "in", None), ("sub/deep.txt", "deep", None)]);
        let left_side = Side::Folder(PathBuf::from("memory"));
        let right_side = Side::Folder(dir.join("right"));
        let right_tree = open_tree(&right_side, &remote::KnownHosts::default()).unwrap();
        let mut result = compare_trees(left_side, right_side, Arc::new(left), right_tree, Mode::Thorough, &Filters::default(), &Progress::default(), &AtomicBool::new(false)).unwrap();
        let everything: Vec<usize> = (0..result.rows.len()).filter(|&i| result.rows[i].parent.is_none()).collect();
        let plan = plan_copy(&result, &everything, Direction::LeftToRight).unwrap();
        let files: Vec<(&str, bool)> = plan.files.iter().map(|item| (item.path.as_str(), item.replaces)).collect();
        assert_eq!(files, [("folder/inner.txt", false), ("sub/deep.txt", false), ("changed.txt", true), ("new.txt", false)], "nothing that's the same, and nothing deleted");
        assert_eq!(plan.folders, ["folder"]);
        assert_eq!(plan.replaced(), 1);
        assert!(dir.join("right/only-right.txt").exists());
        // The other way, the left side isn't a folder that can be written to.
        assert!(plan_copy(&result, &everything, Direction::RightToLeft).is_err());
        // Doing it backs up what's replaced, and keeps the time a file had.
        let backups = dir.join("backups");
        let report = run_copy(&plan, &*result.left_tree, &*result.right_tree, &backups, &Progress::default(), &AtomicBool::new(false));
        assert_eq!((report.copied, report.backed_up, report.folders_made), (4, 1, 1), "{report:?}");
        assert_eq!(std::fs::read_to_string(dir.join("right/changed.txt")).unwrap(), "new!");
        assert_eq!(std::fs::read_to_string(backups.join("changed.txt")).unwrap(), "old");
        assert_eq!(std::fs::read_to_string(dir.join("right/folder/inner.txt")).unwrap(), "in");
        let time = std::fs::metadata(dir.join("right/new.txt")).unwrap().modified().unwrap();
        assert_eq!(time.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(), 1_000_000);
        assert!(report.describe().contains("kept in"));
        // A row that's only on the target side has nothing to copy.
        let only_right = result.rows.iter().position(|row| row.path == "only-right.txt").unwrap();
        result.rows[only_right].status = Status::RightOnly;
        assert!(plan_copy(&result, &[only_right], Direction::LeftToRight).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_local_folder_is_listed_with_its_times() {
        let dir = std::env::temp_dir().join(format!("limoniter-local-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("inner")).unwrap();
        std::fs::write(dir.join("inner/file.txt"), "hello").unwrap();
        let tree = open_tree(&Side::Folder(dir.clone()), &remote::KnownHosts::default()).unwrap();
        let top = tree.list("").unwrap();
        assert_eq!(top.len(), 1);
        assert!(top[0].is_dir);
        let inner = tree.list("inner").unwrap();
        assert_eq!((inner[0].name.as_str(), inner[0].size), ("file.txt", 5));
        assert!(inner[0].modified.is_some());
        assert_eq!(tree.read("inner/file.txt").unwrap(), b"hello");
        assert_eq!(tree.hash("inner/file.txt", &AtomicBool::new(false)).unwrap(), <[u8; 32]>::from(Sha256::digest(b"hello")));
        assert!(open_tree(&Side::Folder(dir.join("missing")), &remote::KnownHosts::default()).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_listings_are_read_into_folders() {
        let listing = "040000 tree 1111\t-\tsrc\x00100644 blob 2222      12\tsrc/main.rs\x00100644 blob 3333       7\tREADME\x00160000 commit 4444       -\tvendor\x00";
        let folders = parse_ls_tree(&listing.replace("\t-\t", "       -\t"), "");
        let top = &folders[""];
        assert_eq!(top.iter().map(|i| (i.name.as_str(), i.is_dir)).collect::<Vec<_>>(), [("src", true), ("README", false)], "a submodule is left out");
        assert_eq!(folders["src"][0].size, 12);
        let below = parse_ls_tree("100644 blob 2222      12\tapp/src/main.rs\x00040000 tree 1111       -\tapp/src\x00", "app/");
        assert_eq!(below[""][0].name, "src");
        assert_eq!(below["src"][0].name, "main.rs");
    }

    #[test]
    fn a_git_revision_is_a_side_like_any_other() {
        if crate::git::program().is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("limoniter-git-side-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.txt"), "one\n").unwrap();
        crate::git::init(&dir).unwrap();
        let run = |args: &[&str]| crate::git::run(&dir, args).unwrap();
        run(&["-c", "user.name=T", "-c", "user.email=t@example.com", "add", "."]);
        run(&["-c", "user.name=T", "-c", "user.email=t@example.com", "commit", "-q", "-m", "first"]);
        std::fs::write(dir.join("src/a.txt"), "two\n").unwrap();
        std::fs::write(dir.join("src/b.txt"), "new\n").unwrap();
        let committed = Side::git_of(&dir.join("src"), "HEAD").unwrap();
        assert!(matches!(&committed, Side::Git { folder, .. } if folder == "src"));
        assert!(committed.describe().ends_with("at the last commit"));
        let result = compare_folders(&Side::Folder(dir.join("src")), &committed, Mode::Thorough, &Filters::default(), &remote::KnownHosts::default(), &Progress::default(), &AtomicBool::new(false)).unwrap();
        assert_eq!(statuses(&result), [("a.txt".to_string(), Status::Different), ("b.txt".to_string(), Status::LeftOnly)]);
        assert_eq!(result.right_tree.read("a.txt").unwrap(), b"one\n");
        assert!(GitTree::open(&dir, "no-such-branch", "").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_archive_is_a_side_too() {
        // A zip with one stored file, made by hand.
        fn zip_with(name: &str, data: &[u8]) -> Vec<u8> {
            let crc = {
                let mut crc = 0xFFFF_FFFFu32;
                for &b in data {
                    crc ^= u32::from(b);
                    for _ in 0..8 {
                        crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
                    }
                }
                !crc
            };
            let mut out = Vec::new();
            let local = |out: &mut Vec<u8>| {
                out.extend(b"PK\x03\x04");
                out.extend([20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
                out.extend(crc.to_le_bytes());
                out.extend((data.len() as u32).to_le_bytes());
                out.extend((data.len() as u32).to_le_bytes());
                out.extend((name.len() as u16).to_le_bytes());
                out.extend([0, 0]);
                out.extend(name.as_bytes());
            };
            local(&mut out);
            out.extend(data);
            let central = out.len();
            out.extend(b"PK\x01\x02");
            out.extend([20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            out.extend(crc.to_le_bytes());
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((name.len() as u16).to_le_bytes());
            out.extend([0u8; 12]);
            out.extend(0u32.to_le_bytes());
            out.extend(name.as_bytes());
            let size = out.len() - central;
            out.extend(b"PK\x05\x06");
            out.extend([0, 0, 0, 0, 1, 0, 1, 0]);
            out.extend((size as u32).to_le_bytes());
            out.extend((central as u32).to_le_bytes());
            out.extend([0, 0]);
            out
        }
        let dir = std::env::temp_dir().join(format!("limoniter-zip-side-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("folder")).unwrap();
        std::fs::write(dir.join("folder/notes.txt"), "hello").unwrap();
        std::fs::write(dir.join("pack.zip"), zip_with("notes.txt", b"hellO")).unwrap();
        let side = Side::from_path(&dir.join("pack.zip"));
        assert!(matches!(side, Side::Image { .. }));
        let result = compare_folders(&Side::Folder(dir.join("folder")), &side, Mode::Quick, &Filters::default(), &remote::KnownHosts::default(), &Progress::default(), &AtomicBool::new(false)).unwrap();
        assert_eq!(statuses(&result), [("notes.txt".to_string(), Status::Different)], "same size, no times, so read and compared");
        assert_eq!(result.right_tree.read("notes.txt").unwrap(), b"hellO");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_comparison_runs_in_the_background_and_can_be_stopped() {
        let dir = std::env::temp_dir().join(format!("limoniter-folder-job-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::create_dir_all(dir.join("b")).unwrap();
        std::fs::write(dir.join("a/x"), "1").unwrap();
        let (left, right) = (Side::Folder(dir.join("a")), Side::Folder(dir.join("b")));
        let job = Job::start(move |progress, cancel| compare_folders(&left, &right, Mode::Quick, &Filters::default(), &remote::KnownHosts::default(), progress, cancel), || {});
        let result = job.wait().unwrap();
        assert_eq!(statuses(&result), [("x".to_string(), Status::LeftOnly)]);
        let (left, right) = (Side::Folder(dir.join("a")), Side::Folder(dir.join("b")));
        let stopped = Job::start(
            move |progress, cancel| {
                cancel.store(true, Ordering::Relaxed);
                compare_folders(&left, &right, Mode::Quick, &Filters::default(), &remote::KnownHosts::default(), progress, cancel)
            },
            || {},
        );
        assert!(stopped.wait().is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn text_options_leave_out_what_they_say() {
        let plain = TextOptions::default();
        assert_ne!(line_key::<u8>("a b", None, &plain), line_key::<u8>("ab", None, &plain));
        let spaces = TextOptions { ignore_whitespace: true, ..plain };
        assert_eq!(line_key::<u8>("a \tb ", None, &spaces), line_key::<u8>("ab", None, &spaces));
        let case = TextOptions { ignore_case: true, ..plain };
        assert_eq!(line_key::<u8>("ABC", None, &case), line_key::<u8>("abc", None, &case));
        assert_ne!(line_key("x", Some(1u8), &plain), line_key("x", Some(2u8), &plain), "endings count unless left out");
        let endings = TextOptions { ignore_line_endings: true, ..plain };
        assert_eq!(line_key("x", Some(1u8), &endings), line_key("x", Some(2u8), &endings));
        assert_eq!(line_key::<u8>("x\r", None, &endings), line_key::<u8>("x", None, &endings));
    }

    #[test]
    fn lines_are_compared_into_stretches_with_changes_joined() {
        let chunks = diff_lines(&["a", "b", "c", "d"], &["a", "x", "c", "d", "e"], &TextOptions::default());
        let kinds: Vec<ChunkKind> = chunks.iter().map(|c| c.kind).collect();
        assert_eq!(kinds, [ChunkKind::Same, ChunkKind::Changed, ChunkKind::Same, ChunkKind::RightOnly]);
        assert_eq!(chunks[1].left, 1..2);
        assert_eq!(chunks[3].right, 4..5);
        assert_eq!(changes(&chunks).count(), 2);
        let ignoring = diff_lines(&["A  b"], &["a b"], &TextOptions { ignore_case: true, ignore_whitespace: true, ..TextOptions::default() });
        assert_eq!(ignoring.len(), 1);
        assert_eq!(ignoring[0].kind, ChunkKind::Same);
    }

    #[test]
    fn scrolling_finds_the_matching_line() {
        let chunks = diff_lines(&["a", "b", "c", "d"], &["a", "x", "y", "z", "c", "d"], &TextOptions::default());
        assert_eq!(matching_line(&chunks, 0, true), 0);
        assert_eq!(matching_line(&chunks, 2, true), 4);
        assert_eq!(matching_line(&chunks, 3, true), 5);
        assert_eq!(matching_line(&chunks, 5, false), 3);
        assert_eq!(matching_line(&chunks, 2, false), 1, "inside a change, its first line on the other side");
        assert_eq!(matching_line(&chunks, 9, true), 11, "past the end keeps going");
    }

    #[test]
    fn inline_differences_go_by_words_or_characters() {
        let (l, r) = inline_differences("let total = a + b;", "let sum = a + c;", false, &TextOptions::default());
        assert_eq!(l, [(4, 9), (16, 17)]);
        assert_eq!(r, [(4, 7), (14, 15)]);
        let (l, r) = inline_differences("colour", "color", true, &TextOptions::default());
        assert_eq!((l, r), (vec![(4, 5)], vec![]));
        let (l, r) = inline_differences("A  B", "a b", false, &TextOptions { ignore_case: true, ignore_whitespace: true, ..TextOptions::default() });
        assert!(l.is_empty() && r.is_empty());
    }

    #[test]
    fn marks_show_each_side_what_differs() {
        let left = ["same", "old word", "gone", "end"];
        let right = ["same", "new word", "end", "added"];
        let chunks = diff_lines(&left, &right, &TextOptions::default());
        let (l, r) = marks_for(&chunks, &left, &right, false, &TextOptions::default());
        assert!(l.line(0).is_none());
        assert_eq!(l.line(1).unwrap().kind, ChunkKind::Changed);
        assert_eq!(l.line(1).unwrap().spans, [(0, 3)]);
        assert_eq!(r.line(1).unwrap().spans, [(0, 3)]);
        assert_eq!(l.line(2).unwrap().kind, ChunkKind::Changed, "gone is paired as part of the change");
        assert_eq!(r.line(3).unwrap().kind, ChunkKind::RightOnly);
        assert!(l.gap_before(4), "the left has nothing where the right added a line");
        assert!(!r.gap_before(2));
    }

    #[test]
    fn a_unified_diff_is_what_patch_reads() {
        let left = ["one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"];
        let right = ["one", "TWO", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven"];
        let chunks = diff_lines(&left, &right, &TextOptions::default());
        let diff = unified_diff("a/x.txt", "b/x.txt", &left, &right, true, true, &chunks);
        assert_eq!(
            diff,
            "--- a/x.txt\n+++ b/x.txt\n@@ -1,5 +1,5 @@\n one\n-two\n+TWO\n three\n four\n five\n@@ -8,3 +8,4 @@\n eight\n nine\n ten\n+eleven\n"
        );
        assert_eq!(unified_diff("a", "b", &left, &left, true, true, &diff_lines(&left, &left, &TextOptions::default())), "", "nothing for the same text");
        let close = diff_lines(&["a", "b", "c", "d"], &["A", "b", "c", "D"], &TextOptions::default());
        let joined = unified_diff("l", "r", &["a", "b", "c", "d"], &["A", "b", "c", "D"], true, true, &close);
        assert_eq!(joined.matches("@@").count(), 2, "changes close together share a hunk: {joined}");
        let ending = unified_diff("l", "r", &["a", "b"], &["a", "b"], true, false, &diff_lines(&["a", "b"], &["a", "b"], &TextOptions::default()));
        assert_eq!(ending, "--- l\n+++ r\n@@ -1,2 +1,2 @@\n a\n-b\n+b\n\\ No newline at end of file\n");
        let added = unified_diff("/dev/null", "b/new", &Vec::<String>::new(), &["x"], true, true, &diff_lines(&Vec::<String>::new(), &["x"], &TextOptions::default()));
        assert_eq!(added, "--- /dev/null\n+++ b/new\n@@ -0,0 +1 @@\n+x\n");
    }

    #[test]
    fn a_patch_and_reports_cover_a_whole_folder() {
        let left = [("a.txt", "1\n2\n", None), ("gone.txt", "bye\n", None), ("bin.dat", "\0\x01", None), ("same.txt", "s\n", None)];
        let right = [("a.txt", "1\n3\n", None), ("new.txt", "hi\n", None), ("bin.dat", "\0\x02", None), ("same.txt", "s\n", None)];
        let result = compared(&left, &right, Mode::Thorough, &Filters::default());
        let patch = folder_patch(&result, &AtomicBool::new(false)).unwrap();
        assert!(patch.contains("--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n 1\n-2\n+3\n"), "{patch}");
        assert!(patch.contains("--- a/gone.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-bye\n"), "{patch}");
        assert!(patch.contains("--- /dev/null\n+++ b/new.txt\n"), "{patch}");
        assert!(patch.contains("Binary files a/bin.dat and b/bin.dat differ"));
        assert!(!patch.contains("same.txt"));
        let text = report_text(&result, true);
        assert!(text.contains("Only on the left") && text.contains("gone.txt"));
        assert!(!text.contains("same.txt"), "the same are left out when asked");
        assert!(report_text(&result, false).contains("same.txt"));
        let html = report_html(&result, false);
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<td class=\"s\">Only on the right</td><td>new.txt</td>"));
        assert_eq!(html_escape("<a & \"b\">"), "&lt;a &amp; &quot;b&quot;&gt;");
    }

    #[test]
    fn a_summary_counts_by_folder_and_a_csv_has_a_row_for_each() {
        let left = [("a.txt", "1", Some(1)), ("docs/x.txt", "x", Some(1)), ("docs/y.txt", "y", Some(1)), ("odd, \"name\".txt", "q", Some(1))];
        let right = [("a.txt", "1", Some(1)), ("docs/x.txt", "changed", Some(1)), ("docs/z.txt", "z", Some(1))];
        let result = compared(&left, &right, Mode::Quick, &Filters::default());
        let summary = report_summary(&result, false);
        let docs = summary.lines().find(|line| line.starts_with("docs/")).unwrap();
        let numbers: Vec<&str> = docs.split_whitespace().skip(1).collect();
        assert_eq!(numbers, ["0", "1", "1", "1", "0"], "{summary}");
        let top = summary.lines().find(|line| line.starts_with("(files at the top)")).unwrap();
        assert!(top.split_whitespace().any(|n| n == "1"));
        assert!(!summary.contains("y.txt"), "no entries, only counts");
        assert!(report_summary(&result, true).contains("<td>docs/</td>"));
        let csv = report_csv(&result, false);
        let rows: Vec<&str> = csv.split("\r\n").filter(|row| !row.is_empty()).collect();
        assert_eq!(rows[0], "Status,Path,Kind,Left size,Left modified,Right size,Right modified");
        assert_eq!(rows.len(), 1 + result.rows.len());
        assert!(csv.contains("Only on the left,\"odd, \"\"name\"\".txt\",File,1,"), "{csv}");
        assert!(csv.contains("Different,docs,Folder,,,,"));
        assert_eq!(report_csv(&result, true).split("\r\n").filter(|row| !row.is_empty()).count(), 1 + result.rows.iter().filter(|row| row.status != Status::Same).count());
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("line\nbreak"), "\"line\nbreak\"");
    }

    #[test]
    fn text_is_told_from_binary_for_a_patch() {
        assert_eq!(text_lines(b"a\nb\n"), Some((vec!["a".to_string(), "b".to_string()], true)));
        assert_eq!(text_lines(b"a\nb"), Some((vec!["a".to_string(), "b".to_string()], false)));
        assert_eq!(text_lines(b""), Some((Vec::new(), false)));
        assert_eq!(text_lines(b"\0\0\0"), None);
    }

    #[test]
    fn bytes_are_compared_place_by_place() {
        assert_eq!(byte_differences(b"abcdef", b"abXdYf"), [2..3, 4..5]);
        assert_eq!(byte_differences(b"abc", b"abcde"), vec![Range { start: 3, end: 5 }]);
        assert_eq!(byte_differences(b"abX", b"abcde"), vec![Range { start: 2, end: 5 }], "a difference running into the extra joins it");
        assert!(byte_differences(b"same", b"same").is_empty());
    }

    #[test]
    fn pictures_show_where_they_differ() {
        let a = [10u8, 20, 30, 255, 0, 0, 0, 255];
        let b = [10u8, 20, 30, 255, 9, 9, 9, 255];
        let (w, h, pixels, differing) = pixel_difference(2, 1, &a, 2, 1, &b);
        assert_eq!((w, h, differing), (2, 1, 1));
        assert_eq!(&pixels[4..8], &[230, 0, 160, 255]);
        assert_eq!(pixels[0], pixels[1], "the same pixel is grey");
        let (w, h, _, differing) = pixel_difference(2, 1, &a, 1, 1, &a[..4]);
        assert_eq!((w, h, differing), (2, 1, 1), "what one hasn't counts as different");
    }

    #[test]
    fn sides_describe_themselves() {
        assert_eq!(revision_words("HEAD"), "the last commit");
        assert_eq!(revision_words("0123456789abcdef0123456789abcdef01234567"), "commit 01234567");
        assert_eq!(revision_words("main"), "main");
        let site = remote::Site { host: "example.com".into(), port: 22, user: "me".into(), key_file: None };
        assert_eq!(Side::Sftp { site, folder: "/srv".into() }.describe(), "sftp://me@example.com/srv");
        assert!(matches!(Side::from_path(Path::new("sftp://me@example.com/srv")), Side::Sftp { .. }));
        assert!(Side::Folder(PathBuf::from("x")).writable());
        assert!(!Side::Image { file: PathBuf::from("x.zip"), folder: String::new() }.writable());
    }
}
